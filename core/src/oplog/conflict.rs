//! Durable per-field conflict clocks for sync.
//!
//! A clock row is the last accepted total-order key for one independent piece
//! of truth. Remote operations are filtered before their normal writers run;
//! stale fields are consumed without being allowed to overwrite newer truth.

use rusqlite::{params, Connection, OptionalExtension};

use super::OpRecord;

const DELETE_FIELD: &str = "__delete__";
const CREATE_FIELD: &str = "__create__";
const HIGH_WATER_KIND: &str = "__hlc__";
const HIGH_WATER_KEY: &str = "__global__";
const HIGH_WATER_FIELD: &str = "high_water";

#[derive(Debug, Clone)]
struct SlotSpec {
    target_kind: String,
    target_key: String,
    field_key: String,
}

fn is_create(op_type: &str) -> bool {
    matches!(
        op_type,
        "entity_created"
            | "entity_recreated"
            | "folder_created"
            | "smart_folder_created"
            | "subscription_created"
            | "subscription_query_created"
    )
}

fn is_recreate(op_type: &str) -> bool {
    op_type == "entity_recreated"
}

fn create_is_blocked(
    op: &OpRecord,
    current_create: &Option<(String, String)>,
    delete: &Option<(String, String)>,
) -> bool {
    !newer(&op.hlc, &op.device_id, current_create.clone())
        || (!is_recreate(&op.op_type) && (current_create.is_some() || delete.is_some()))
        || (is_recreate(&op.op_type) && !newer(&op.hlc, &op.device_id, delete.clone()))
}

fn is_delete(op_type: &str) -> bool {
    matches!(
        op_type,
        "entity_deleted"
            | "folder_deleted"
            | "smart_folder_deleted"
            | "subscription_deleted"
            | "subscription_query_deleted"
    )
}

fn target_kind(op_type: &str) -> Option<&'static str> {
    match op_type {
        "entity_created"
        | "entity_recreated"
        | "entity_status_changed"
        | "entity_updated"
        | "entity_deleted"
        | "entity_tags_added"
        | "entity_tags_removed" => Some("entity"),
        "folder_created"
        | "folder_updated"
        | "folder_moved"
        | "folder_deleted"
        | "folder_members_added"
        | "folder_members_removed" => Some("folder"),
        "smart_folder_created"
        | "smart_folder_updated"
        | "smart_folder_moved"
        | "smart_folder_deleted" => Some("smart_folder"),
        "subscription_created" | "subscription_updated" | "subscription_deleted" => {
            Some("subscription")
        }
        "subscription_query_created"
        | "subscription_query_updated"
        | "subscription_query_deleted" => Some("subscription_query"),
        "duplicate_decided" => Some("duplicate"),
        // Structural tag operations deliberately remain on their existing
        // live path in this slice.
        _ => None,
    }
}

fn object_fields(payload: &serde_json::Value, skip: &[&str]) -> Vec<String> {
    payload
        .as_object()
        .into_iter()
        .flat_map(|object| object.keys())
        .filter(|key| !skip.contains(&key.as_str()))
        .cloned()
        .collect()
}

fn array_strings(payload: &serde_json::Value, field: &str) -> Vec<String> {
    payload
        .get(field)
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flat_map(|values| values.iter())
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect()
}

fn slots_for(op: &OpRecord) -> Vec<SlotSpec> {
    let Some(kind) = target_kind(&op.op_type) else {
        return Vec::new();
    };
    let key = if op.op_type == "duplicate_decided" {
        super::normalized_pair_key(&op.entity_key)
    } else {
        op.entity_key.clone()
    };
    let mut slots = Vec::new();
    if is_create(&op.op_type) {
        slots.push(SlotSpec {
            target_kind: kind.to_owned(),
            target_key: key.clone(),
            field_key: CREATE_FIELD.to_owned(),
        });
    }
    if is_delete(&op.op_type) {
        slots.push(SlotSpec {
            target_kind: kind.to_owned(),
            target_key: key,
            field_key: DELETE_FIELD.to_owned(),
        });
        return slots;
    }

    let fields = match op.op_type.as_str() {
        "entity_updated" | "folder_updated" | "smart_folder_updated" | "subscription_updated" => {
            object_fields(&op.payload, &[])
        }
        "entity_status_changed" => vec!["status".to_owned()],
        "entity_tags_added" | "entity_tags_removed" => array_strings(&op.payload, "tags")
            .into_iter()
            .map(|tag| format!("tag:{tag}"))
            .collect(),
        "folder_created" | "folder_moved" | "smart_folder_created" | "smart_folder_moved" => {
            let mut fields = object_fields(&op.payload, &["kind"]);
            if op.op_type.ends_with("_moved") && !fields.iter().any(|f| f == "parent") {
                fields.push("parent".to_owned());
            }
            fields
        }
        "folder_members_added" | "folder_members_removed" => array_strings(&op.payload, "entities")
            .into_iter()
            .map(|entity| format!("member:{entity}"))
            .collect(),
        "duplicate_decided" => vec!["decision".to_owned()],
        "entity_created" | "entity_recreated" => {
            let mut fields = object_fields(&op.payload, &["kind", "tags"]);
            fields.extend(
                array_strings(&op.payload, "tags")
                    .into_iter()
                    .map(|tag| format!("tag:{tag}")),
            );
            fields
        }
        "subscription_query_updated" => object_fields(&op.payload, &["subscription_uuid"]),
        "subscription_created" | "subscription_query_created" => object_fields(&op.payload, &[]),
        _ => Vec::new(),
    };
    for field in fields {
        slots.push(SlotSpec {
            target_kind: kind.to_owned(),
            target_key: key.clone(),
            field_key: field,
        });
    }
    slots
}

fn newer(incoming_hlc: &str, incoming_device: &str, current: Option<(String, String)>) -> bool {
    let Some((current_hlc, current_device)) = current else {
        return true;
    };
    (incoming_hlc, incoming_device) > (current_hlc.as_str(), current_device.as_str())
}

fn current_slot(conn: &Connection, slot: &SlotSpec) -> rusqlite::Result<Option<(String, String)>> {
    conn.prepare_cached(
        "SELECT hlc, device_id FROM sync_conflict_clock
         WHERE target_kind = ?1 AND target_key = ?2 AND field_key = ?3",
    )?
    .query_row(
        params![slot.target_kind, slot.target_key, slot.field_key],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .optional()
}

fn put_slot(conn: &Connection, slot: &SlotSpec, op: &OpRecord) -> rusqlite::Result<()> {
    conn.prepare_cached(
        "INSERT INTO sync_conflict_clock
            (target_kind, target_key, field_key, hlc, device_id)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(target_kind, target_key, field_key) DO UPDATE SET
            hlc = excluded.hlc,
            device_id = excluded.device_id
         WHERE (excluded.hlc > sync_conflict_clock.hlc)
            OR (excluded.hlc = sync_conflict_clock.hlc
                AND excluded.device_id > sync_conflict_clock.device_id)",
    )?
    .execute(params![
        slot.target_kind,
        slot.target_key,
        slot.field_key,
        op.hlc,
        op.device_id,
    ])?;
    Ok(())
}

fn put_high_water(conn: &Connection, op: &OpRecord) -> rusqlite::Result<()> {
    put_slot(
        conn,
        &SlotSpec {
            target_kind: HIGH_WATER_KIND.to_owned(),
            target_key: HIGH_WATER_KEY.to_owned(),
            field_key: HIGH_WATER_FIELD.to_owned(),
        },
        op,
    )
}

fn reset_generation_slots(conn: &Connection, slot: &SlotSpec) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM sync_conflict_clock
         WHERE target_kind = ?1 AND target_key = ?2 AND field_key != ?3",
        params![slot.target_kind, slot.target_key, DELETE_FIELD],
    )?;
    Ok(())
}

fn delete_marker(conn: &Connection, slot: &SlotSpec) -> rusqlite::Result<Option<(String, String)>> {
    current_slot(
        conn,
        &SlotSpec {
            target_kind: slot.target_kind.clone(),
            target_key: slot.target_key.clone(),
            field_key: DELETE_FIELD.to_owned(),
        },
    )
}

fn filter_payload(op: &OpRecord, accepted: &[SlotSpec]) -> serde_json::Value {
    let accepted: std::collections::HashSet<&str> = accepted
        .iter()
        .map(|slot| slot.field_key.as_str())
        .collect();
    let mut payload = op.payload.clone();
    match op.op_type.as_str() {
        "entity_updated"
        | "folder_updated"
        | "smart_folder_updated"
        | "subscription_updated"
        | "subscription_query_updated" => {
            if let Some(object) = payload.as_object_mut() {
                object.retain(|field, _| {
                    accepted.contains(field.as_str())
                        || (op.op_type == "subscription_query_updated"
                            && field == "subscription_uuid")
                });
            }
        }
        "entity_status_changed" => {
            if !accepted.contains("status") {
                payload = serde_json::json!({});
            }
        }
        "entity_tags_added" | "entity_tags_removed" => {
            let tags: Vec<_> = array_strings(&payload, "tags")
                .into_iter()
                .filter(|tag| accepted.contains(format!("tag:{tag}").as_str()))
                .collect();
            if let Some(object) = payload.as_object_mut() {
                object.insert("tags".to_owned(), serde_json::json!(tags));
            }
        }
        "folder_members_added" | "folder_members_removed" => {
            let entities: Vec<_> = array_strings(&payload, "entities")
                .into_iter()
                .filter(|entity| accepted.contains(format!("member:{entity}").as_str()))
                .collect();
            if let Some(object) = payload.as_object_mut() {
                object.insert("entities".to_owned(), serde_json::json!(entities));
            }
        }
        _ => {}
    }
    payload
}

fn parse_hlc(hlc: &str) -> Option<(u64, u64)> {
    let (wall, counter) = hlc.split_once('-')?;
    Some((
        u64::from_str_radix(wall, 16).ok()?,
        u64::from_str_radix(counter, 16).ok()?,
    ))
}

fn increment_hlc(hlc: &str) -> Option<String> {
    let (mut wall, mut counter) = parse_hlc(hlc)?;
    counter += 1;
    if counter > 0xffff {
        wall += 1;
        counter = 0;
    }
    Some(format!("{wall:013x}-{counter:04x}"))
}

pub(crate) fn next_durable_hlc(conn: &Connection) -> rusqlite::Result<String> {
    let durable: Option<String> = conn
        .query_row(
            "SELECT hlc FROM sync_conflict_clock
             WHERE target_kind = ?1 AND target_key = ?2 AND field_key = ?3",
            params![HIGH_WATER_KIND, HIGH_WATER_KEY, HIGH_WATER_FIELD],
            |row| row.get(0),
        )
        .optional()?;
    let candidate = super::next_hlc();
    match durable {
        Some(durable) if candidate <= durable => {
            increment_hlc(&durable).ok_or_else(|| rusqlite::Error::InvalidQuery)
        }
        _ => Ok(candidate),
    }
}

pub(crate) fn local_op_type(
    conn: &Connection,
    requested: &str,
    entity_key: &str,
) -> rusqlite::Result<String> {
    if requested != "entity_created" {
        return Ok(requested.to_owned());
    }
    let was_deleted = conn
        .query_row(
            "SELECT 1 FROM sync_conflict_clock
             WHERE target_kind = 'entity' AND target_key = ?1 AND field_key = ?2",
            params![entity_key, DELETE_FIELD],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    Ok(if was_deleted {
        "entity_recreated".to_owned()
    } else {
        requested.to_owned()
    })
}

pub(crate) fn record_local_op(conn: &Connection, op: &OpRecord) -> rusqlite::Result<()> {
    let slots = slots_for(op);
    if is_create(&op.op_type) {
        if let Some(slot) = slots.first() {
            reset_generation_slots(conn, slot)?;
        }
    }
    for slot in &slots {
        put_slot(conn, slot, op)?;
    }
    put_high_water(conn, op)
}

/// Read-only preflight used before downloading a create's blob. Returning
/// true is stable: later clocks can only make the operation more stale.
pub(crate) fn remote_create_is_blocked(conn: &Connection, op: &OpRecord) -> rusqlite::Result<bool> {
    if !is_create(&op.op_type) {
        return Ok(false);
    }
    let slots = slots_for(op);
    let Some(first) = slots.first() else {
        return Ok(false);
    };
    let delete = delete_marker(conn, first)?;
    let current_create = current_slot(
        conn,
        &SlotSpec {
            target_kind: first.target_kind.clone(),
            target_key: first.target_key.clone(),
            field_key: CREATE_FIELD.to_owned(),
        },
    )?;
    Ok(create_is_blocked(op, &current_create, &delete))
}

/// Return a remote op with stale fields removed, or no op when the complete
/// op is stale/blocked by a hard-delete tombstone. Clock writes happen before
/// the normal remote writer and roll back with it if the writer parks/fails.
pub(crate) fn accept_remote_op(
    conn: &Connection,
    op: &OpRecord,
) -> rusqlite::Result<Option<OpRecord>> {
    let slots = slots_for(op);
    if slots.is_empty() {
        put_high_water(conn, op)?;
        return Ok(Some(op.clone()));
    }
    let first = &slots[0];
    let delete = delete_marker(conn, first)?;
    let current_create = current_slot(
        conn,
        &SlotSpec {
            target_kind: first.target_kind.clone(),
            target_key: first.target_key.clone(),
            field_key: CREATE_FIELD.to_owned(),
        },
    )?;
    if is_delete(op.op_type.as_str()) {
        if !newer(&op.hlc, &op.device_id, delete.clone())
            || !newer(&op.hlc, &op.device_id, current_create.clone())
        {
            put_high_water(conn, op)?;
            return Ok(None);
        }
        put_slot(conn, first, op)?;
        put_high_water(conn, op)?;
        return Ok(Some(op.clone()));
    }
    if is_create(op.op_type.as_str()) {
        let create_slot = &slots[0];
        if create_is_blocked(op, &current_create, &delete) {
            put_high_water(conn, op)?;
            return Ok(None);
        }
        reset_generation_slots(conn, create_slot)?;
        for slot in &slots {
            put_slot(conn, slot, op)?;
        }
        put_high_water(conn, op)?;
        return Ok(Some(op.clone()));
    }

    // A partial mutation cannot resurrect a deleted target. After an explicit
    // recreate it must also be newer than that create, otherwise it belongs
    // to the previous generation and is consumed as stale.
    if delete
        .as_ref()
        .is_some_and(|(hlc, device)| newer(hlc, device, current_create.clone()))
        || current_create.as_ref().is_some_and(|(hlc, device)| {
            !newer(&op.hlc, &op.device_id, Some((hlc.clone(), device.clone())))
        })
    {
        put_high_water(conn, op)?;
        return Ok(None);
    }

    let mut accepted = Vec::new();
    for slot in &slots {
        if newer(&op.hlc, &op.device_id, current_slot(conn, slot)?) {
            accepted.push(slot.clone());
        }
    }
    if accepted.is_empty() {
        put_high_water(conn, op)?;
        return Ok(None);
    }
    for slot in &accepted {
        put_slot(conn, slot, op)?;
    }
    put_high_water(conn, op)?;
    let mut filtered = op.clone();
    filtered.payload = filter_payload(op, &accepted);
    Ok(Some(filtered))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(
        hlc: &str,
        device: &str,
        op_type: &str,
        key: &str,
        payload: serde_json::Value,
    ) -> OpRecord {
        OpRecord {
            op_version: 1,
            op_type: op_type.to_owned(),
            entity_key: key.to_owned(),
            payload,
            hlc: hlc.to_owned(),
            device_id: device.to_owned(),
        }
    }

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::core::schema::LIBRARY_DDL)
            .unwrap();
        conn
    }

    #[test]
    fn remote_fields_are_independent_and_tag_payloads_are_filtered() {
        let conn = db();
        let newer = op(
            "0000000000002-0000",
            "b",
            "entity_tags_added",
            "h",
            serde_json::json!({"tags":["general:cat","general:dog"]}),
        );
        let older = op(
            "0000000000001-0000",
            "a",
            "entity_tags_added",
            "h",
            serde_json::json!({"tags":["general:cat","general:bird"]}),
        );
        assert_eq!(slots_for(&newer).len(), 2);
        assert!(accept_remote_op(&conn, &newer).unwrap().is_some());
        let filtered = accept_remote_op(&conn, &older).unwrap().unwrap();
        assert_eq!(
            filtered.payload["tags"],
            serde_json::json!(["general:bird"])
        );
    }

    #[test]
    fn durable_hlc_advances_past_remote_clock() {
        let conn = db();
        conn.execute(
            "INSERT INTO sync_conflict_clock
             (target_kind,target_key,field_key,hlc,device_id)
             VALUES ('__hlc__','__global__','high_water','ffffffffffff0-ffff','remote')",
            [],
        )
        .unwrap();
        let next = next_durable_hlc(&conn).unwrap();
        assert!(next.as_str() > "ffffffffffff0-ffff");
    }

    #[test]
    fn local_record_uses_one_durable_hlc_for_outbox_and_slots() {
        let conn = db();
        conn.execute(
            "INSERT INTO sync_conflict_clock
             (target_kind,target_key,field_key,hlc,device_id)
             VALUES ('__hlc__','__global__','high_water','0000000000002-0000','remote')",
            [],
        )
        .unwrap();
        super::super::record_op(
            &conn,
            "local",
            "entity_status_changed",
            "h",
            &serde_json::json!({"status": 1}),
        )
        .unwrap();
        let outbox_hlc: String = conn
            .query_row("SELECT hlc FROM op_outbox", [], |row| row.get(0))
            .unwrap();
        let slot_hlc: String = conn
            .query_row(
                "SELECT hlc FROM sync_conflict_clock
                 WHERE target_kind = 'entity' AND target_key = 'h' AND field_key = 'status'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(outbox_hlc, slot_hlc);
        assert!(outbox_hlc.as_str() > "0000000000002-0000");
    }

    #[test]
    fn older_delete_loses_to_newer_create_and_duplicate_create_keeps_field_clocks() {
        let conn = db();
        let create = op(
            "0000000000004-0000",
            "new",
            "entity_created",
            "h",
            serde_json::json!({"name":"created"}),
        );
        assert!(accept_remote_op(&conn, &create).unwrap().is_some());
        let update = op(
            "0000000000005-0000",
            "new",
            "entity_updated",
            "h",
            serde_json::json!({"rating":5}),
        );
        assert!(accept_remote_op(&conn, &update).unwrap().is_some());

        let duplicate_create = op(
            "0000000000006-0000",
            "other",
            "entity_created",
            "h",
            serde_json::json!({"name":"duplicate import"}),
        );
        assert!(accept_remote_op(&conn, &duplicate_create)
            .unwrap()
            .is_none());
        let rating_clock: String = conn
            .query_row(
                "SELECT hlc FROM sync_conflict_clock
                 WHERE target_kind = 'entity' AND target_key = 'h' AND field_key = 'rating'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rating_clock, "0000000000005-0000");
        let create_clock: String = conn
            .query_row(
                "SELECT hlc FROM sync_conflict_clock
                 WHERE target_kind = 'entity' AND target_key = 'h' AND field_key = '__create__'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(create_clock, "0000000000004-0000");

        let old_delete = op(
            "0000000000003-0000",
            "old",
            "entity_deleted",
            "h",
            serde_json::json!({}),
        );
        assert!(accept_remote_op(&conn, &old_delete).unwrap().is_none());
    }

    #[test]
    fn recreate_rejects_edits_from_the_previous_generation() {
        let conn = db();
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000001-0000",
                "a",
                "entity_created",
                "h",
                serde_json::json!({"name":"first"}),
            ),
        )
        .unwrap()
        .is_some());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000004-0000",
                "a",
                "entity_deleted",
                "h",
                serde_json::json!({}),
            ),
        )
        .unwrap()
        .is_some());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000006-0000",
                "b",
                "entity_recreated",
                "h",
                serde_json::json!({"name":"second"}),
            ),
        )
        .unwrap()
        .is_some());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000005-0000",
                "old",
                "entity_updated",
                "h",
                serde_json::json!({"rating":1}),
            ),
        )
        .unwrap()
        .is_none());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000007-0000",
                "new",
                "entity_updated",
                "h",
                serde_json::json!({"rating":5}),
            ),
        )
        .unwrap()
        .is_some());
    }

    #[test]
    fn local_create_becomes_recreate_only_after_a_durable_delete() {
        let conn = db();
        assert_eq!(
            local_op_type(&conn, "entity_created", "h").unwrap(),
            "entity_created"
        );

        record_local_op(
            &conn,
            &op(
                "0000000000001-0000",
                "a",
                "entity_deleted",
                "h",
                serde_json::json!({}),
            ),
        )
        .unwrap();

        assert_eq!(
            local_op_type(&conn, "entity_created", "h").unwrap(),
            "entity_recreated"
        );
        assert_eq!(
            local_op_type(&conn, "folder_created", "h").unwrap(),
            "folder_created"
        );
    }

    #[test]
    fn recreate_wins_when_it_arrives_before_its_older_delete() {
        let conn = db();
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000001-0000",
                "a",
                "entity_created",
                "h",
                serde_json::json!({"name":"first"}),
            ),
        )
        .unwrap()
        .is_some());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000003-0000",
                "b",
                "entity_recreated",
                "h",
                serde_json::json!({"name":"second"}),
            ),
        )
        .unwrap()
        .is_some());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000002-0000",
                "a",
                "entity_deleted",
                "h",
                serde_json::json!({}),
            ),
        )
        .unwrap()
        .is_none());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000004-0000",
                "c",
                "entity_created",
                "h",
                serde_json::json!({"name":"duplicate"}),
            ),
        )
        .unwrap()
        .is_none());

        let create_clock: String = conn
            .query_row(
                "SELECT hlc FROM sync_conflict_clock
                 WHERE target_kind = 'entity' AND target_key = 'h' AND field_key = '__create__'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(create_clock, "0000000000003-0000");
    }

    #[test]
    fn subscription_definition_updates_filter_stale_fields_independently() {
        let conn = db();
        let newer = op(
            "0000000000002-0000",
            "b",
            "subscription_updated",
            "subscription-1",
            serde_json::json!({"name":"new","schedule":"daily"}),
        );
        let older = op(
            "0000000000001-0000",
            "a",
            "subscription_updated",
            "subscription-1",
            serde_json::json!({"name":"old","paused":true}),
        );

        assert!(accept_remote_op(&conn, &newer).unwrap().is_some());
        let filtered = accept_remote_op(&conn, &older).unwrap().unwrap();
        assert_eq!(filtered.payload, serde_json::json!({"paused":true}));
    }

    #[test]
    fn subscription_definition_create_cannot_cross_a_tombstone() {
        let conn = db();
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000002-0000",
                "a",
                "subscription_deleted",
                "subscription-1",
                serde_json::json!({}),
            ),
        )
        .unwrap()
        .is_some());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000003-0000",
                "b",
                "subscription_created",
                "subscription-1",
                serde_json::json!({"name":"stale"}),
            ),
        )
        .unwrap()
        .is_none());
        assert!(accept_remote_op(
            &conn,
            &op(
                "0000000000004-0000",
                "c",
                "subscription_updated",
                "subscription-1",
                serde_json::json!({"name":"resurrection"}),
            ),
        )
        .unwrap()
        .is_none());
    }
}
