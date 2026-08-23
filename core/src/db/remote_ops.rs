//! Remote op application — materializes peer-device ops into local truth
//! tables (split from db/mod.rs).

use super::*;
use std::collections::BTreeSet;

// ── Remote op application ────────────────────────────────────────
// Materializes ops from peer devices into local truth tables. A missing
// prerequisite parks the containing segment; it is never treated as applied.

pub(super) enum RemoteOpOutcome {
    Applied(RemoteProjectionImpact),
    Ignored,
    Pending(String),
}

#[derive(Default)]
pub(super) struct RemoteProjectionImpact {
    pub deleted_entity_ids: Vec<i64>,
    pub dirty_tag_ids: Vec<i64>,
    pub rebuild_status: bool,
    pub rebuild_tag_derivatives: bool,
    pub rebuild_all_smart_folders: bool,
    pub dirty_smart_folder_ids: Vec<i64>,
    pub rebuild_sidebar: bool,
    pub rebuild_folder_sizes: bool,
}

impl RemoteProjectionImpact {
    pub(super) fn merge(&mut self, other: Self) {
        self.deleted_entity_ids.extend(other.deleted_entity_ids);
        self.dirty_tag_ids.extend(other.dirty_tag_ids);
        self.rebuild_status |= other.rebuild_status;
        self.rebuild_tag_derivatives |= other.rebuild_tag_derivatives;
        self.rebuild_all_smart_folders |= other.rebuild_all_smart_folders;
        self.dirty_smart_folder_ids
            .extend(other.dirty_smart_folder_ids);
        self.rebuild_sidebar |= other.rebuild_sidebar;
        self.rebuild_folder_sizes |= other.rebuild_folder_sizes;
    }

    pub(super) fn into_compiler_plan(mut self) -> crate::db::projection::compiler::CompilerPlan {
        let dedup = |values: &mut Vec<i64>| {
            let deduped = values.iter().copied().collect::<BTreeSet<_>>();
            *values = deduped.into_iter().collect();
        };
        dedup(&mut self.deleted_entity_ids);
        dedup(&mut self.dirty_tag_ids);
        dedup(&mut self.dirty_smart_folder_ids);
        crate::db::projection::compiler::CompilerPlan {
            rebuild_status: self.rebuild_status,
            dirty_tag_ids: self.dirty_tag_ids,
            rebuild_tag_derivatives: self.rebuild_tag_derivatives,
            rebuild_all_smart_folders: self.rebuild_all_smart_folders,
            dirty_smart_folder_ids: self.dirty_smart_folder_ids,
            rebuild_sidebar: self.rebuild_sidebar,
            rebuild_folder_sizes: self.rebuild_folder_sizes,
            ..Default::default()
        }
    }
}

#[derive(Default)]
struct RemoteProjectionSnapshot {
    entity_ids: Vec<i64>,
    tag_ids: Vec<i64>,
    smart_folder_ids: Vec<i64>,
}

fn entity_id_by_hash(conn: &Connection, hash: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
        [hash],
        |row| row.get(0),
    )
    .optional()
}

fn folder_id_by_uuid(conn: &Connection, uuid: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT folder_id FROM folder WHERE uuid = ?1",
        [uuid],
        |row| row.get(0),
    )
    .optional()
}

fn smart_folder_id_by_uuid(conn: &Connection, uuid: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT smart_folder_id FROM smart_folder WHERE uuid = ?1",
        [uuid],
        |row| row.get(0),
    )
    .optional()
}

fn subscription_id_by_uuid(conn: &Connection, uuid: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT subscription_id FROM subscription WHERE uuid = ?1",
        [uuid],
        |row| row.get(0),
    )
    .optional()
}

fn subscription_query_id_by_uuid(conn: &Connection, uuid: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT query_id FROM subscription_query WHERE uuid = ?1",
        [uuid],
        |row| row.get(0),
    )
    .optional()
}

fn target_was_deleted(conn: &Connection, kind: &str, key: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sync_conflict_clock
             WHERE target_kind = ?1 AND target_key = ?2 AND field_key = '__delete__'
         )",
        rusqlite::params![kind, key],
        |row| row.get(0),
    )
}

fn split_tag_key(key: &str) -> (&str, &str) {
    match key.find(':') {
        Some(idx) => (&key[..idx], &key[idx + 1..]),
        None => ("", key),
    }
}

fn tag_id_by_key(conn: &Connection, key: &str) -> rusqlite::Result<Option<i64>> {
    let (ns, st) = split_tag_key(key);
    conn.query_row(
        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
        rusqlite::params![ns, st],
        |row| row.get(0),
    )
    .optional()
}

fn get_or_create_tag_by_key(conn: &Connection, key: &str) -> rusqlite::Result<i64> {
    if let Some(id) = tag_id_by_key(conn, key)? {
        return Ok(id);
    }
    let (ns, st) = split_tag_key(key);
    conn.execute(
        "INSERT OR IGNORE INTO tag (namespace, subtag) VALUES (?1, ?2)",
        rusqlite::params![ns, st],
    )?;
    tag_id_by_key(conn, key)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

fn payload_str<'a>(p: &'a serde_json::Value, field: &str) -> Option<&'a str> {
    p.get(field).and_then(|v| v.as_str())
}

fn payload_strings(p: &serde_json::Value, field: &str) -> Vec<String> {
    p.get(field)
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn payload_mask(p: &serde_json::Value, field: &str, default: u64) -> u64 {
    payload_str(p, field)
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn payload_i64(p: &serde_json::Value, field: &str, default: i64) -> i64 {
    p.get(field)
        .and_then(|value| value.as_i64())
        .unwrap_or(default)
}

fn payload_bool(p: &serde_json::Value, field: &str, default: bool) -> bool {
    p.get(field)
        .and_then(|value| value.as_bool())
        .unwrap_or(default)
}

fn entity_ids_for_hashes(conn: &Connection, hashes: &[String]) -> rusqlite::Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(hashes.len());
    for hash in hashes {
        if let Some(id) = entity_id_by_hash(conn, hash)? {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn tag_ids_for_entities(conn: &Connection, entity_ids: &[i64]) -> rusqlite::Result<Vec<i64>> {
    let mut tag_ids = Vec::new();
    for entity_id in entity_ids {
        let ids = conn
            .prepare_cached("SELECT tag_id FROM entity_tag WHERE entity_id = ?1")?
            .query_map([entity_id], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        tag_ids.extend(ids);
    }
    tag_ids.sort_unstable();
    tag_ids.dedup();
    Ok(tag_ids)
}

fn smart_folder_scope_ids(conn: &Connection, uuid: &str) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "WITH RECURSIVE descendants(smart_folder_id) AS (
             SELECT smart_folder_id FROM smart_folder WHERE uuid = ?1
             UNION ALL
             SELECT child.smart_folder_id
             FROM smart_folder child
             JOIN descendants parent ON child.parent_id = parent.smart_folder_id
         )
         SELECT smart_folder_id FROM descendants",
    )?;
    let mut ids = stmt
        .query_map([uuid], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.sort_unstable();
    ids.dedup();
    Ok(ids)
}

fn projection_snapshot(
    conn: &Connection,
    op: &crate::oplog::OpRecord,
) -> rusqlite::Result<RemoteProjectionSnapshot> {
    let p = &op.payload;
    let mut entity_hashes = Vec::new();
    let mut tag_keys = Vec::new();
    match op.op_type.as_str() {
        "entity_created"
        | "entity_recreated"
        | "entity_status_changed"
        | "entity_updated"
        | "entity_deleted"
        | "entity_tags_added"
        | "entity_tags_removed" => entity_hashes.push(op.entity_key.clone()),
        _ => {}
    }
    if matches!(
        op.op_type.as_str(),
        "tag_renamed" | "tag_merged" | "tag_deleted" | "tag_alias_set" | "tag_implication_set"
    ) {
        tag_keys.push(op.entity_key.clone());
        for field in ["to", "into", "parent"] {
            if let Some(value) = payload_str(p, field) {
                tag_keys.push(value.to_owned());
            }
        }
    }
    let entity_ids = entity_ids_for_hashes(conn, &entity_hashes)?;
    let mut tag_ids = tag_ids_for_entities(conn, &entity_ids)?;
    for key in tag_keys {
        if let Some(tag_id) = tag_id_by_key(conn, &key)? {
            tag_ids.push(tag_id);
        }
    }
    tag_ids.sort_unstable();
    tag_ids.dedup();
    let smart_folder_ids = if op.op_type.starts_with("smart_folder_") {
        smart_folder_scope_ids(conn, &op.entity_key)?
    } else {
        Vec::new()
    };
    Ok(RemoteProjectionSnapshot {
        entity_ids,
        tag_ids,
        smart_folder_ids,
    })
}

fn applied_impact(
    conn: &Connection,
    op: &crate::oplog::OpRecord,
    before: RemoteProjectionSnapshot,
) -> rusqlite::Result<RemoteOpOutcome> {
    let after = projection_snapshot(conn, op)?;
    let deletes = matches!(op.op_type.as_str(), "entity_deleted" | "entity_recreated");
    let deleted_entity_ids = if deletes {
        let mut deleted = Vec::new();
        let mut exists =
            conn.prepare_cached("SELECT EXISTS(SELECT 1 FROM media_entity WHERE entity_id = ?1)")?;
        for entity_id in &before.entity_ids {
            if !exists.query_row([entity_id], |row| row.get::<_, bool>(0))? {
                deleted.push(*entity_id);
            }
        }
        deleted
    } else {
        Vec::new()
    };
    let tags_changed = matches!(
        op.op_type.as_str(),
        "entity_created"
            | "entity_recreated"
            | "entity_deleted"
            | "entity_tags_added"
            | "entity_tags_removed"
            | "tag_renamed"
            | "tag_merged"
            | "tag_deleted"
            | "tag_alias_set"
            | "tag_implication_set"
    );
    let mut dirty_tag_ids = if tags_changed {
        before.tag_ids
    } else {
        Vec::new()
    };
    if tags_changed {
        dirty_tag_ids.extend(after.tag_ids);
        dirty_tag_ids.sort_unstable();
        dirty_tag_ids.dedup();
    }
    let rebuild_status = matches!(
        op.op_type.as_str(),
        "entity_created" | "entity_recreated" | "entity_deleted" | "entity_status_changed"
    );
    let rebuild_all_smart_folders = matches!(
        op.op_type.as_str(),
        "entity_updated"
            | "entity_tags_added"
            | "entity_tags_removed"
            | "tag_renamed"
            | "tag_merged"
            | "tag_deleted"
            | "tag_alias_set"
            | "tag_implication_set"
    );
    let rebuild_tag_derivatives = (matches!(
        op.op_type.as_str(),
        "entity_created" | "entity_recreated" | "entity_tags_added" | "entity_tags_removed"
    ) && !payload_strings(&op.payload, "tags").is_empty())
        || matches!(
            op.op_type.as_str(),
            "tag_renamed" | "tag_merged" | "tag_deleted" | "tag_alias_set" | "tag_implication_set"
        );
    // Metadata changes can alter smart-folder membership and therefore the
    // sidebar counts derived from those smart-folder bitmaps.
    let rebuild_sidebar = !op.op_type.starts_with("subscription_");
    let rebuild_folder_sizes = matches!(
        op.op_type.as_str(),
        "entity_recreated"
            | "entity_deleted"
            | "folder_members_added"
            | "folder_members_removed"
            | "duplicate_decided"
    );
    Ok(RemoteOpOutcome::Applied(RemoteProjectionImpact {
        deleted_entity_ids,
        dirty_tag_ids,
        rebuild_status,
        rebuild_tag_derivatives,
        rebuild_all_smart_folders,
        dirty_smart_folder_ids: before
            .smart_folder_ids
            .into_iter()
            .chain(after.smart_folder_ids)
            .collect(),
        rebuild_sidebar,
        rebuild_folder_sizes,
    }))
}

fn first_missing_entity(conn: &Connection, hashes: &[String]) -> rusqlite::Result<Option<String>> {
    for hash in hashes {
        if entity_id_by_hash(conn, hash)?.is_none() {
            return Ok(Some(hash.clone()));
        }
    }
    Ok(None)
}

fn missing_prerequisite(
    conn: &Connection,
    op: &crate::oplog::OpRecord,
) -> rusqlite::Result<Option<String>> {
    let key = op.entity_key.as_str();
    let p = &op.payload;
    let missing = |kind: &str, value: &str| Some(format!("missing {kind} {value}"));
    match op.op_type.as_str() {
        "entity_status_changed"
        | "entity_updated"
        | "entity_tags_added"
        | "entity_tags_removed" => {
            if entity_id_by_hash(conn, key)?.is_none() {
                return Ok(missing("entity", key));
            }
        }
        "tag_renamed" => {
            if tag_id_by_key(conn, key)?.is_none() {
                let target_exists = payload_str(p, "to")
                    .map(|target| tag_id_by_key(conn, target))
                    .transpose()?
                    .flatten()
                    .is_some();
                if !target_exists {
                    return Ok(missing("tag", key));
                }
            }
        }
        "tag_merged" => {
            if tag_id_by_key(conn, key)?.is_none() {
                let target_exists = payload_str(p, "into")
                    .map(|target| tag_id_by_key(conn, target))
                    .transpose()?
                    .flatten()
                    .is_some();
                if !target_exists {
                    return Ok(missing("tag", key));
                }
            }
        }
        "folder_created" => {
            if let Some(parent) = payload_str(p, "parent") {
                if folder_id_by_uuid(conn, parent)?.is_none() {
                    return Ok(missing("parent folder", parent));
                }
            }
        }
        "folder_updated" | "folder_moved" | "folder_members_added" | "folder_members_removed" => {
            if folder_id_by_uuid(conn, key)?.is_none() {
                return Ok(missing("folder", key));
            }
            if op.op_type == "folder_moved" {
                if let Some(parent) = payload_str(p, "parent") {
                    if folder_id_by_uuid(conn, parent)?.is_none() {
                        return Ok(missing("parent folder", parent));
                    }
                }
            }
        }
        "smart_folder_created" => {
            if let Some(parent) = payload_str(p, "parent") {
                if smart_folder_id_by_uuid(conn, parent)?.is_none() {
                    return Ok(missing("parent smart folder", parent));
                }
            }
        }
        "smart_folder_updated" | "smart_folder_moved" => {
            if smart_folder_id_by_uuid(conn, key)?.is_none() {
                return Ok(missing("smart folder", key));
            }
            if op.op_type == "smart_folder_moved" {
                if let Some(parent) = payload_str(p, "parent") {
                    if smart_folder_id_by_uuid(conn, parent)?.is_none() {
                        return Ok(missing("parent smart folder", parent));
                    }
                }
            }
        }
        "subscription_created" | "subscription_updated" => {
            if op.op_type == "subscription_updated" && subscription_id_by_uuid(conn, key)?.is_none()
            {
                return Ok(missing("subscription", key));
            }
        }
        "subscription_query_created" | "subscription_query_updated" => {
            let subscription_uuid = payload_str(p, "subscription_uuid").ok_or_else(|| {
                rusqlite::Error::InvalidParameterName(format!(
                    "{} requires subscription_uuid",
                    op.op_type
                ))
            })?;
            let parent_deleted = target_was_deleted(conn, "subscription", subscription_uuid)?;
            if op.op_type == "subscription_query_updated"
                && subscription_query_id_by_uuid(conn, key)?.is_none()
            {
                return Ok(if parent_deleted {
                    None
                } else {
                    missing("subscription query", key)
                });
            }
            if subscription_id_by_uuid(conn, subscription_uuid)?.is_none() && !parent_deleted {
                return Ok(missing("subscription", subscription_uuid));
            }
        }
        "duplicate_decided" => {
            if let Some((hash_a, hash_b)) = key.split_once('|') {
                if let Some(hash) = first_missing_entity(conn, &[hash_a.into(), hash_b.into()])? {
                    return Ok(missing("duplicate entity", &hash));
                }
            }
        }
        _ => {}
    }

    let referenced = match op.op_type.as_str() {
        "folder_members_added" | "folder_members_removed" => payload_strings(p, "entities"),
        _ => Vec::new(),
    };
    if let Some(hash) = first_missing_entity(conn, &referenced)? {
        return Ok(missing("referenced entity", &hash));
    }
    Ok(None)
}

pub(super) fn apply_remote_op(
    conn: &Connection,
    op: &crate::oplog::OpRecord,
) -> rusqlite::Result<RemoteOpOutcome> {
    if !crate::oplog::is_supported_op_type(&op.op_type) {
        return Err(rusqlite::Error::InvalidParameterName(format!(
            "unsupported remote operation: {}",
            op.op_type
        )));
    }
    let Some(op) = crate::oplog::conflict::accept_remote_op(conn, op)? else {
        return Ok(RemoteOpOutcome::Ignored);
    };
    if let Some(reason) = missing_prerequisite(conn, &op)? {
        return Ok(RemoteOpOutcome::Pending(reason));
    }
    let before = projection_snapshot(conn, &op)?;
    let key = op.entity_key.as_str();
    let p = &op.payload;
    let now = chrono::Utc::now().to_rfc3339();
    match op.op_type.as_str() {
        "entity_created" | "entity_recreated" => {
            let existing_id = entity_id_by_hash(conn, key)?;
            if op.op_type == "entity_created" && existing_id.is_some() {
                // Content-addressed: already materialized. The accepted op is
                // still settled through the same impact path.
                return applied_impact(conn, &op, before);
            }
            if let Some(entity_id) = existing_id {
                // A recreate is a new metadata generation for the same content
                // hash. Reset the old entity even when cross-device delivery
                // brings this op in before its older delete.
                write::entities::delete_entities(conn, &[entity_id])?;
            }
            conn.execute(
                "DELETE FROM media_file WHERE file_hash = ?1
                 AND file_id NOT IN (SELECT file_id FROM media_entity)",
                [key],
            )?;
            let file_id = write::files::insert_file(
                conn,
                key,
                payload_str(p, "mime").unwrap_or("application/octet-stream"),
                p.get("size").and_then(|v| v.as_i64()).unwrap_or(0),
                p.get("width").and_then(|v| v.as_i64()),
                p.get("height").and_then(|v| v.as_i64()),
                p.get("duration_ms").and_then(|v| v.as_i64()),
                p.get("frame_count").and_then(|v| v.as_i64()),
                p.get("has_audio")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
                &now,
            )?;
            let entity_id = write::entities::insert_entity(
                conn,
                key,
                file_id,
                payload_str(p, "name"),
                p.get("status").and_then(|v| v.as_i64()).unwrap_or(0),
                payload_str(p, "date_created").unwrap_or(&now),
                &now,
            )?;
            let source_urls = payload_strings(p, "source_urls");
            let source_urls_json = if source_urls.is_empty() {
                None
            } else {
                serde_json::to_string(&source_urls).ok()
            };
            if payload_str(p, "notes").is_some() || source_urls_json.is_some() {
                write::entities::patch_entity_metadata(
                    conn,
                    &[entity_id],
                    None,
                    None,
                    payload_str(p, "notes").map(Some),
                    source_urls_json.as_deref(),
                    &now,
                )?;
            }
            let tags = payload_strings(p, "tags");
            if !tags.is_empty() {
                write::tags::add_tags(
                    conn,
                    &[entity_id],
                    &tags,
                    payload_mask(p, "tag_provenance", types::TAG_PROVENANCE_MANUAL),
                )?;
            }
            let work_types = crate::media_analysis::derivative_work_types_for_target(
                payload_str(p, "mime").unwrap_or("application/octet-stream"),
                p.get("frame_count").and_then(|value| value.as_i64()),
                true,
            );
            insert_deferred_work_rows(conn, key, &work_types)?;
        }
        "entity_status_changed" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                if let Some(status) = p.get("status").and_then(|v| v.as_i64()) {
                    write::entities::set_entity_status(conn, &[id], status, &now)?;
                }
            }
        }
        "entity_updated" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                write::entities::patch_entity_metadata(
                    conn,
                    &[id],
                    payload_str(p, "name"),
                    p.get("rating").map(|v| v.as_i64()),
                    p.get("notes").map(|v| v.as_str()),
                    p.get("source_urls")
                        .and_then(|v| serde_json::to_string(v).ok())
                        .as_deref(),
                    &now,
                )?;
                if let Some(created) = payload_str(p, "date_created") {
                    write::entities::set_entity_date_created(conn, id, created, &now)?;
                }
            }
        }
        "entity_deleted" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                write::entities::delete_entities(conn, &[id])?;
            }
        }
        "entity_tags_added" | "entity_tags_removed" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                let tags = payload_strings(p, "tags");
                if tags.is_empty() {
                    return applied_impact(conn, &op, before);
                }
                if op.op_type == "entity_tags_added" {
                    write::tags::add_tags(
                        conn,
                        &[id],
                        &tags,
                        payload_mask(p, "provenance", types::TAG_PROVENANCE_MANUAL),
                    )?;
                } else {
                    write::tags::remove_tags(conn, &[id], &tags)?;
                }
            }
        }
        "tag_renamed" => {
            if let (Some(id), Some(to)) = (tag_id_by_key(conn, key)?, payload_str(p, "to")) {
                write::tags::rename_tag(conn, id, to)?;
            }
        }
        "tag_merged" => {
            if let (Some(from), Some(into_key)) =
                (tag_id_by_key(conn, key)?, payload_str(p, "into"))
            {
                let into = get_or_create_tag_by_key(conn, into_key)?;
                if from != into {
                    write::tags::merge_tags(conn, from, into)?;
                }
            }
        }
        "tag_deleted" => {
            if let Some(id) = tag_id_by_key(conn, key)? {
                write::tags::delete_tag(conn, id)?;
            }
        }
        "tag_alias_set" => {
            let from = get_or_create_tag_by_key(conn, key)?;
            let to = match payload_str(p, "to") {
                Some(to_key) => Some(get_or_create_tag_by_key(conn, to_key)?),
                None => None,
            };
            write::tags::manage_alias(conn, from, to)?;
        }
        "tag_implication_set" => {
            if let (Some(parent_key), Some(add)) = (
                payload_str(p, "parent"),
                p.get("add").and_then(|v| v.as_bool()),
            ) {
                let child = get_or_create_tag_by_key(conn, key)?;
                let parent = get_or_create_tag_by_key(conn, parent_key)?;
                write::tags::manage_implication(conn, child, parent, add)?;
            }
        }
        "folder_created" => {
            if folder_id_by_uuid(conn, key)?.is_none() {
                let parent_id = match payload_str(p, "parent") {
                    Some(parent_uuid) => folder_id_by_uuid(conn, parent_uuid)?,
                    None => None,
                };
                conn.execute(
                    "INSERT INTO folder (name, parent_id, icon, color, uuid, date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    rusqlite::params![
                        payload_str(p, "name").unwrap_or("Folder"),
                        parent_id,
                        payload_str(p, "icon"),
                        payload_str(p, "color"),
                        key,
                        now,
                    ],
                )?;
            }
        }
        "folder_updated" => {
            if let Some(id) = folder_id_by_uuid(conn, key)? {
                let patch = types::FolderPatch {
                    name: payload_str(p, "name").map(|s| s.to_string()),
                    icon: payload_str(p, "icon").map(|s| s.to_string()),
                    color: payload_str(p, "color").map(|s| s.to_string()),
                    notes: payload_str(p, "notes").map(|s| s.to_string()),
                    ..Default::default()
                };
                write::folders::update_folder(conn, id, &patch, &now)?;
            }
        }
        "folder_moved" => {
            if let Some(id) = folder_id_by_uuid(conn, key)? {
                let parent_id = match payload_str(p, "parent") {
                    Some(parent_uuid) => folder_id_by_uuid(conn, parent_uuid)?,
                    None => None,
                };
                write::folders::move_folder(conn, id, parent_id, &now)?;
            }
        }
        "folder_deleted" => {
            if let Some(id) = folder_id_by_uuid(conn, key)? {
                let _ = write::folders::delete_folder(conn, id)?;
            }
        }
        "folder_members_added" | "folder_members_removed" => {
            if let Some(id) = folder_id_by_uuid(conn, key)? {
                let ids = entity_ids_for_hashes(conn, &payload_strings(p, "entities"))?;
                if ids.is_empty() {
                    return applied_impact(conn, &op, before);
                }
                if op.op_type == "folder_members_added" {
                    write::folders::add_members(conn, id, &ids)?;
                } else {
                    write::folders::remove_members(conn, id, &ids)?;
                }
            }
        }
        "smart_folder_created" => {
            if smart_folder_id_by_uuid(conn, key)?.is_none() {
                let parent_id = match payload_str(p, "parent") {
                    Some(parent_uuid) => smart_folder_id_by_uuid(conn, parent_uuid)?,
                    None => None,
                };
                conn.execute(
                    "INSERT INTO smart_folder (name, parent_id, predicate_json, icon, color, notes, uuid, date_added, date_modified)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                    rusqlite::params![
                        payload_str(p, "name").unwrap_or("Smart folder"),
                        parent_id,
                        payload_str(p, "predicate").unwrap_or("{}"),
                        payload_str(p, "icon"),
                        payload_str(p, "color"),
                        payload_str(p, "notes"),
                        key,
                        now,
                    ],
                )?;
            }
        }
        "smart_folder_updated" => {
            if let Some(id) = smart_folder_id_by_uuid(conn, key)? {
                write::smart_folders::update_smart_folder(
                    conn,
                    id,
                    payload_str(p, "name"),
                    payload_str(p, "predicate"),
                    payload_str(p, "icon"),
                    payload_str(p, "color"),
                    payload_str(p, "notes"),
                    &now,
                )?;
            }
        }
        "smart_folder_moved" => {
            if let Some(id) = smart_folder_id_by_uuid(conn, key)? {
                let parent_id = match payload_str(p, "parent") {
                    Some(parent_uuid) => smart_folder_id_by_uuid(conn, parent_uuid)?,
                    None => None,
                };
                write::smart_folders::move_smart_folder(conn, id, parent_id, &now)?;
            }
        }
        "smart_folder_deleted" => {
            if let Some(id) = smart_folder_id_by_uuid(conn, key)? {
                write::smart_folders::delete_smart_folder(conn, id)?;
            }
        }
        "subscription_created" => {
            if subscription_id_by_uuid(conn, key)?.is_none() {
                conn.execute(
                    "INSERT INTO subscription (
                         name, schedule, paused, initial_post_limit,
                         periodic_post_limit, uuid, date_added
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![
                        payload_str(p, "name").unwrap_or("Subscription"),
                        payload_str(p, "schedule").unwrap_or("manual"),
                        payload_bool(p, "paused", false),
                        payload_i64(p, "initial_post_limit", 100),
                        payload_i64(p, "periodic_post_limit", 100),
                        key,
                        payload_str(p, "date_added").unwrap_or(&now),
                    ],
                )?;
            }
        }
        "subscription_updated" => {
            if let Some(id) = subscription_id_by_uuid(conn, key)? {
                if let Some(value) = payload_str(p, "name") {
                    conn.execute(
                        "UPDATE subscription SET name = ?1 WHERE subscription_id = ?2",
                        rusqlite::params![value, id],
                    )?;
                }
                if let Some(value) = payload_str(p, "schedule") {
                    conn.execute(
                        "UPDATE subscription SET schedule = ?1 WHERE subscription_id = ?2",
                        rusqlite::params![value, id],
                    )?;
                }
                if let Some(value) = p.get("paused").and_then(|value| value.as_bool()) {
                    conn.execute(
                        "UPDATE subscription SET paused = ?1 WHERE subscription_id = ?2",
                        rusqlite::params![value, id],
                    )?;
                }
                if let Some(value) = p.get("initial_post_limit").and_then(|value| value.as_i64()) {
                    conn.execute("UPDATE subscription SET initial_post_limit = ?1 WHERE subscription_id = ?2", rusqlite::params![value, id])?;
                }
                if let Some(value) = p
                    .get("periodic_post_limit")
                    .and_then(|value| value.as_i64())
                {
                    conn.execute("UPDATE subscription SET periodic_post_limit = ?1 WHERE subscription_id = ?2", rusqlite::params![value, id])?;
                }
            }
        }
        "subscription_deleted" => {
            if let Some(id) = subscription_id_by_uuid(conn, key)? {
                conn.execute("DELETE FROM subscription WHERE subscription_id = ?1", [id])?;
            }
        }
        "subscription_query_created" => {
            if subscription_query_id_by_uuid(conn, key)?.is_none() {
                if let Some(subscription_id) = payload_str(p, "subscription_uuid")
                    .map(|uuid| subscription_id_by_uuid(conn, uuid))
                    .transpose()?
                    .flatten()
                {
                    conn.execute(
                        "INSERT INTO subscription_query (
                             subscription_id, site_id, query_kind, query_text,
                             display_name, notes, paused, uuid
                         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                        rusqlite::params![
                            subscription_id,
                            payload_str(p, "site_id").unwrap_or(""),
                            payload_str(p, "query_kind").unwrap_or(""),
                            payload_str(p, "query_text").unwrap_or(""),
                            p.get("display_name").and_then(|value| value.as_str()),
                            p.get("notes").and_then(|value| value.as_str()),
                            payload_bool(p, "paused", false),
                            key,
                        ],
                    )?;
                }
            }
        }
        "subscription_query_updated" => {
            if let Some(id) = subscription_query_id_by_uuid(conn, key)? {
                for (field, column) in [
                    ("site_id", "site_id"),
                    ("query_kind", "query_kind"),
                    ("query_text", "query_text"),
                ] {
                    if let Some(value) = payload_str(p, field) {
                        conn.execute(
                            &format!(
                                "UPDATE subscription_query SET {column} = ?1 WHERE query_id = ?2"
                            ),
                            rusqlite::params![value, id],
                        )?;
                    }
                }
                for field in ["display_name", "notes"] {
                    if p.get(field).is_some() {
                        let value = p.get(field).and_then(|value| value.as_str());
                        conn.execute(
                            &format!(
                                "UPDATE subscription_query SET {field} = ?1 WHERE query_id = ?2"
                            ),
                            rusqlite::params![value, id],
                        )?;
                    }
                }
                if let Some(value) = p.get("paused").and_then(|value| value.as_bool()) {
                    conn.execute(
                        "UPDATE subscription_query SET paused = ?1 WHERE query_id = ?2",
                        rusqlite::params![value, id],
                    )?;
                }
            }
        }
        "subscription_query_deleted" => {
            if let Some(id) = subscription_query_id_by_uuid(conn, key)? {
                conn.execute("DELETE FROM subscription_query WHERE query_id = ?1", [id])?;
            }
        }
        "duplicate_decided" => {
            if let Some((hash_a, hash_b)) = key.split_once('|') {
                let file_of = |hash: &str| -> rusqlite::Result<Option<i64>> {
                    conn.query_row(
                        "SELECT me.file_id FROM media_entity me
                         WHERE me.entity_hash = ?1",
                        [hash],
                        |row| row.get(0),
                    )
                    .optional()
                };
                if let (Some(fa), Some(fb)) = (file_of(hash_a)?, file_of(hash_b)?) {
                    let status = match payload_str(p, "action") {
                        Some("not_duplicate") => "ignored_false_positive",
                        Some("keep_both") => "dismissed_keep_both",
                        _ => "resolved",
                    };
                    let winner_file = match payload_str(p, "winner") {
                        Some(w) => file_of(w)?,
                        None => None,
                    };
                    let loser_file = match payload_str(p, "loser") {
                        Some(l) => file_of(l)?,
                        None => None,
                    };
                    conn.execute(
                        "UPDATE duplicate SET status = ?1, decision_at = datetime('now'),
                             decision_source = 'sync', decision_reason = 'Decision synced from another device',
                             winner_file_id = ?2, loser_file_id = ?3
                         WHERE (file_id_a = ?4 AND file_id_b = ?5) OR (file_id_a = ?5 AND file_id_b = ?4)",
                        rusqlite::params![status, winner_file, loser_file, fa, fb],
                    )?;
                }
            }
        }
        other => {
            return Err(rusqlite::Error::InvalidParameterName(format!(
                "unknown remote op type {other}; update required"
            )));
        }
    }
    applied_impact(conn, &op, before)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::projection::bitmaps::BitmapKey;
    use crate::db::projection::compiler::CompilerPlan;
    use crate::db::LibraryDatabase;
    use crate::oplog::OpRecord;
    use roaring::RoaringBitmap;
    use rusqlite::params;
    use tempfile::TempDir;

    fn db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(crate::db::core::schema::LIBRARY_DDL)
            .unwrap();
        conn
    }

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

    fn entity(conn: &Connection, id: i64, hash: &str, status: i64) {
        conn.execute(
            "INSERT INTO media_file
             (file_id, file_hash, mime_type, size_bytes, date_added)
             VALUES (?1, ?2, 'image/jpeg', 1, '2026-01-01')",
            params![id, hash],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO media_entity
             (entity_id, entity_hash, file_id, status, date_created, date_added, date_modified)
             VALUES (?1, ?2, ?1, ?3, '2026-01-01', '2026-01-01', '2026-01-01')",
            params![id, hash, status],
        )
        .unwrap();
    }

    fn folder(conn: &Connection, id: i64, uuid: &str) {
        conn.execute(
            "INSERT INTO folder (folder_id, name, uuid, date_added, date_modified)
             VALUES (?1, 'Folder', ?2, '2026-01-01', '2026-01-01')",
            params![id, uuid],
        )
        .unwrap();
    }

    fn consumed(conn: &Connection, op: OpRecord) {
        assert!(matches!(
            apply_remote_op(conn, &op).unwrap(),
            RemoteOpOutcome::Applied(_) | RemoteOpOutcome::Ignored
        ));
    }

    fn library_with_entity() -> (TempDir, LibraryDatabase) {
        let root = TempDir::new().unwrap();
        let db = LibraryDatabase::open_with_device_id(root.path(), "remote-test".into()).unwrap();
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO media_file
                 (file_id, file_hash, mime_type, size_bytes, date_added)
                 VALUES (1, 'h1', 'image/jpeg', 1, '2026-01-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_entity
                 (entity_id, entity_hash, file_id, status, date_created, date_added, date_modified)
                 VALUES (1, 'h1', 1, 1, '2026-01-01', '2026-01-01', '2026-01-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'existing')",
                [],
            )?;
            conn.execute(
                "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source)
                 VALUES (1, 1, 1, 'local')",
                [],
            )?;
            conn.execute(
                "INSERT INTO folder (folder_id, name, uuid, date_added, date_modified)
                 VALUES (1, 'Folder', 'folder-1', '2026-01-01', '2026-01-01')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db.run_compiler(CompilerPlan {
            rebuild_status: true,
            rebuild_all_tags: true,
            rebuild_all_smart_folders: true,
            rebuild_sidebar: true,
            ..Default::default()
        });
        (root, db)
    }

    #[test]
    fn stale_remote_operation_is_consumed_without_reporting_a_write() {
        let conn = db();
        entity(&conn, 1, "h1", 1);
        consumed(
            &conn,
            op(
                "0000000000002-0000",
                "b",
                "entity_status_changed",
                "h1",
                serde_json::json!({"status": 3}),
            ),
        );
        assert!(matches!(
            apply_remote_op(
                &conn,
                &op(
                    "0000000000001-0000",
                    "a",
                    "entity_status_changed",
                    "h1",
                    serde_json::json!({"status": 2}),
                ),
            )
            .unwrap(),
            RemoteOpOutcome::Ignored
        ));
    }

    #[test]
    fn remote_metadata_and_folder_updates_preserve_unrelated_tag_projection() {
        let (_root, db) = library_with_entity();
        let sentinel = RoaringBitmap::from_iter([99_u32]);
        db.bitmaps.set(BitmapKey::Tag(1), sentinel.clone());
        db.bitmaps.set(BitmapKey::Tagged, sentinel.clone());
        assert_eq!(db.bitmaps.get(&BitmapKey::Tag(1)), sentinel);

        let applied = db
            .apply_remote_ops(
                &[
                    op(
                        "0000000000001-0000",
                        "peer",
                        "entity_updated",
                        "h1",
                        serde_json::json!({"rating": 4}),
                    ),
                    op(
                        "0000000000002-0000",
                        "peer",
                        "folder_updated",
                        "folder-1",
                        serde_json::json!({"name": "Renamed"}),
                    ),
                ],
                &[],
            )
            .unwrap()
            .unwrap();

        assert_eq!(applied, vec![0, 1]);
        assert_eq!(db.bitmaps.get(&BitmapKey::Tag(1)), sentinel);
        assert_eq!(db.bitmaps.get(&BitmapKey::Tagged), sentinel);
    }

    #[test]
    fn remote_status_settles_status_projection_without_touching_tag_truth() {
        let (_root, db) = library_with_entity();
        db.apply_remote_ops(
            &[op(
                "0000000000001-0000",
                "peer",
                "entity_status_changed",
                "h1",
                serde_json::json!({"status": 0}),
            )],
            &[],
        )
        .unwrap()
        .unwrap();

        assert_eq!(db.bitmaps.get(&BitmapKey::Status(1)), RoaringBitmap::new());
        assert_eq!(
            db.bitmaps.get(&BitmapKey::Status(0)),
            RoaringBitmap::from_iter([1_u32])
        );
        assert_eq!(
            db.bitmaps.get(&BitmapKey::Tag(1)),
            RoaringBitmap::from_iter([1_u32])
        );
    }

    #[test]
    fn remote_tag_change_rebuilds_tagged_and_direct_tag_bitmaps() {
        let (_root, db) = library_with_entity();
        db.apply_remote_ops(
            &[op(
                "0000000000001-0000",
                "peer",
                "entity_tags_added",
                "h1",
                serde_json::json!({"tags": ["general:new"]}),
            )],
            &[],
        )
        .unwrap()
        .unwrap();

        let new_tag_id = db
            .with_read(|conn| tag_id_by_key(conn, "general:new"))
            .unwrap()
            .unwrap();
        assert_eq!(
            db.bitmaps.get(&BitmapKey::Tag(new_tag_id)),
            RoaringBitmap::from_iter([1_u32])
        );
        assert_eq!(
            db.bitmaps.get(&BitmapKey::Tagged),
            RoaringBitmap::from_iter([1_u32])
        );
    }

    #[test]
    fn remote_delete_removes_entity_from_every_relevant_bitmap() {
        let (_root, db) = library_with_entity();
        db.apply_remote_ops(
            &[op(
                "0000000000001-0000",
                "peer",
                "entity_deleted",
                "h1",
                serde_json::json!({}),
            )],
            &[],
        )
        .unwrap()
        .unwrap();

        assert_eq!(db.bitmaps.get(&BitmapKey::Status(1)), RoaringBitmap::new());
        assert_eq!(db.bitmaps.get(&BitmapKey::Tag(1)), RoaringBitmap::new());
        assert_eq!(db.bitmaps.get(&BitmapKey::Tagged), RoaringBitmap::new());
        assert_eq!(db.count_media_files().unwrap(), 0);
        assert_eq!(
            db.with_read(|conn| entity_id_by_hash(conn, "h1")).unwrap(),
            None
        );
    }

    #[test]
    fn removed_operations_fail_as_unknown_remote_ops() {
        let conn = db();
        let result = apply_remote_op(
            &conn,
            &op(
                "0000000000001-0000",
                "peer",
                "removed_operation",
                "removed-entity",
                serde_json::json!({}),
            ),
        );
        let Err(error) = result else {
            panic!("unsupported operation must not be applied");
        };

        assert!(error
            .to_string()
            .contains("unsupported remote operation: removed_operation"));
    }

    #[test]
    fn remote_tag_add_refreshes_existing_implications() {
        let (_root, db) = library_with_entity();
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO tag (tag_id, namespace, subtag) VALUES
                 (2, 'general', 'child'), (3, 'general', 'parent')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tag_implication (child_tag_id, parent_tag_id) VALUES (2, 3)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db.run_compiler(CompilerPlan {
            rebuild_tag_derivatives: true,
            ..Default::default()
        });

        db.apply_remote_ops(
            &[op(
                "0000000000001-0000",
                "peer",
                "entity_tags_added",
                "h1",
                serde_json::json!({"tags": ["general:child"]}),
            )],
            &[],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            db.bitmaps.get(&BitmapKey::EffectiveTag(3)),
            RoaringBitmap::from_iter([1_u32])
        );
        assert_eq!(
            db.with_read(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM entity_tag_implied
                     WHERE entity_id = 1 AND tag_id = 3",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap(),
            1
        );
    }

    #[test]
    fn remote_create_with_tags_refreshes_existing_implications() {
        let root = TempDir::new().unwrap();
        let db =
            LibraryDatabase::open_with_device_id(root.path(), "remote-create-test".into()).unwrap();
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO tag (tag_id, namespace, subtag) VALUES
                 (1, 'general', 'child'), (2, 'general', 'parent')",
                [],
            )?;
            conn.execute(
                "INSERT INTO tag_implication (child_tag_id, parent_tag_id) VALUES (1, 2)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        db.run_compiler(CompilerPlan {
            rebuild_tag_derivatives: true,
            ..Default::default()
        });

        db.apply_remote_ops(
            &[op(
                "0000000000001-0000",
                "peer",
                "entity_created",
                "created-with-tags",
                serde_json::json!({
                    "mime": "image/jpeg",
                    "size": 1,
                    "status": 1,
                    "tags": ["general:child"]
                }),
            )],
            &[],
        )
        .unwrap()
        .unwrap();

        let entity_id = db
            .with_read(|conn| entity_id_by_hash(conn, "created-with-tags"))
            .unwrap()
            .unwrap();
        assert!(db
            .bitmaps
            .get(&BitmapKey::EffectiveTag(2))
            .contains(entity_id as u32));
    }

    #[test]
    fn late_status_and_metadata_do_not_overwrite_newer_fields() {
        let conn = db();
        entity(&conn, 1, "h1", 1);
        consumed(
            &conn,
            op(
                "0000000000002-0000",
                "b",
                "entity_status_changed",
                "h1",
                serde_json::json!({"status": 3}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000001-0000",
                "a",
                "entity_status_changed",
                "h1",
                serde_json::json!({"status": 2}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000004-0000",
                "b",
                "entity_updated",
                "h1",
                serde_json::json!({"rating": 5}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000003-0000",
                "a",
                "entity_updated",
                "h1",
                serde_json::json!({"rating": 1, "notes": "kept"}),
            ),
        );
        let row: (i64, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT status, rating, notes FROM media_entity WHERE entity_hash = 'h1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(row, (3, Some(5), Some("kept".to_owned())));
    }

    #[test]
    fn late_tag_payloads_keep_newer_tag_truth_and_merge_new_tags() {
        let conn = db();
        entity(&conn, 1, "h1", 1);
        consumed(
            &conn,
            op(
                "0000000000002-0000",
                "b",
                "entity_tags_added",
                "h1",
                serde_json::json!({"tags":["general:cat"]}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000001-0000",
                "a",
                "entity_tags_added",
                "h1",
                serde_json::json!({"tags":["general:cat", "general:bird"]}),
            ),
        );
        let tags: Vec<String> = conn
            .prepare(
                "SELECT t.namespace || ':' || t.subtag FROM entity_tag et
                 JOIN tag t ON t.tag_id = et.tag_id
                 WHERE et.entity_id = 1 ORDER BY 1",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(tags, vec!["general:bird", "general:cat"]);
    }

    #[test]
    fn late_membership_payloads_merge_independent_members() {
        let conn = db();
        entity(&conn, 1, "h1", 1);
        entity(&conn, 2, "h2", 1);
        folder(&conn, 1, "folder-1");
        consumed(
            &conn,
            op(
                "0000000000002-0000",
                "b",
                "folder_members_added",
                "folder-1",
                serde_json::json!({"entities":["h1"]}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000001-0000",
                "a",
                "folder_members_added",
                "folder-1",
                serde_json::json!({"entities":["h1", "h2"]}),
            ),
        );
        let members: Vec<String> = conn
            .prepare(
                "SELECT me.entity_hash FROM folder_member fm
                 JOIN media_entity me ON me.entity_id = fm.entity_id
                 ORDER BY me.entity_hash",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(members, vec!["h1", "h2"]);
    }

    #[test]
    fn missing_delete_is_consumed_and_explicit_recreate_materializes() {
        let conn = db();
        consumed(
            &conn,
            op(
                "0000000000002-0000",
                "b",
                "entity_deleted",
                "h1",
                serde_json::json!({}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000003-0000",
                "c",
                "entity_updated",
                "h1",
                serde_json::json!({"rating": 1}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000004-0000",
                "c",
                "entity_recreated",
                "h1",
                serde_json::json!({"mime":"image/jpeg","size":1,"status":1}),
            ),
        );
        let rating: Option<i64> = conn
            .query_row(
                "SELECT rating FROM media_entity WHERE entity_hash = 'h1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(rating, None);
        let delete_clock: Option<String> = conn
            .query_row(
                "SELECT field_key FROM sync_conflict_clock
                 WHERE target_kind = 'entity' AND target_key = 'h1' AND field_key = '__create__'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap();
        assert_eq!(delete_clock.as_deref(), Some("__create__"));
    }

    #[test]
    fn recreate_before_older_delete_resets_metadata_and_survives_late_delete() {
        let conn = db();
        consumed(
            &conn,
            op(
                "0000000000001-0000",
                "a",
                "entity_created",
                "h1",
                serde_json::json!({"mime":"image/jpeg","size":1,"status":1,"name":"old"}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000002-0000",
                "a",
                "entity_updated",
                "h1",
                serde_json::json!({"rating":5}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000004-0000",
                "b",
                "entity_recreated",
                "h1",
                serde_json::json!({"mime":"image/png","size":2,"status":0,"name":"new"}),
            ),
        );
        consumed(
            &conn,
            op(
                "0000000000003-0000",
                "a",
                "entity_deleted",
                "h1",
                serde_json::json!({}),
            ),
        );

        let (name, rating, mime): (Option<String>, Option<i64>, String) = conn
            .query_row(
                "SELECT me.name, me.rating, mf.mime_type
                 FROM media_entity me
                 JOIN media_file mf ON mf.file_id = me.file_id
                 WHERE me.entity_hash = 'h1'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(name.as_deref(), Some("new"));
        assert_eq!(rating, None);
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn remote_subscription_definitions_materialize_without_runtime_state() {
        let conn = db();
        let operations = [
            op(
                "0000000000001-0000",
                "peer",
                "subscription_created",
                "subscription-uuid",
                serde_json::json!({
                    "name":"Daily artists",
                    "schedule":"daily",
                    "paused":false,
                    "initial_post_limit":200,
                    "periodic_post_limit":50,
                    "date_added":"2026-01-02"
                }),
            ),
            op(
                "0000000000003-0000",
                "peer",
                "subscription_query_created",
                "query-uuid",
                serde_json::json!({
                    "subscription_uuid":"subscription-uuid",
                    "site_id":"gelbooru",
                    "query_kind":"tag_search",
                    "query_text":"one_girl",
                    "display_name":"One girl",
                    "notes":null,
                    "paused":false
                }),
            ),
        ];
        for operation in operations {
            let outcome = apply_remote_op(&conn, &operation).unwrap();
            let RemoteOpOutcome::Applied(impact) = outcome else {
                panic!("definition operation was not applied");
            };
            assert!(!impact.rebuild_sidebar);
            assert!(impact.into_compiler_plan().is_empty());
        }

        let (name, schedule): (String, String) = conn
            .query_row(
                "SELECT s.name, s.schedule
                 FROM subscription s
                 WHERE s.uuid = 'subscription-uuid'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(name, "Daily artists");
        assert_eq!(schedule, "daily");

        conn.execute(
            "UPDATE subscription_query
             SET files_found = 17, resume_cursor = 'local-cursor'
             WHERE uuid = 'query-uuid'",
            [],
        )
        .unwrap();
        consumed(
            &conn,
            op(
                "0000000000004-0000",
                "peer",
                "subscription_query_updated",
                "query-uuid",
                serde_json::json!({
                    "subscription_uuid":"subscription-uuid",
                    "notes":"portable note",
                    "paused":true
                }),
            ),
        );
        let (notes, paused, files_found, cursor): (Option<String>, bool, i64, Option<String>) =
            conn.query_row(
                "SELECT notes, paused, files_found, resume_cursor
                 FROM subscription_query WHERE uuid = 'query-uuid'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(notes.as_deref(), Some("portable note"));
        assert!(paused);
        assert_eq!(files_found, 17);
        assert_eq!(cursor.as_deref(), Some("local-cursor"));
    }

    #[test]
    fn removed_subscription_group_remote_operations_are_rejected() {
        let conn = db();
        for operation in [
            "subscription_group_created",
            "subscription_group_updated",
            "subscription_group_deleted",
        ] {
            let result = apply_remote_op(
                &conn,
                &op(
                    "0000000000001-0000",
                    "peer",
                    operation,
                    "group-uuid",
                    serde_json::json!({}),
                ),
            );
            match result {
                Err(error) => assert!(error.to_string().contains("unsupported remote operation")),
                Ok(_) => panic!("removed subscription-group operations must be rejected"),
            }
        }
    }
}
