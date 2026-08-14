//! Remote op application — materializes peer-device ops into local truth
//! tables (split from db/mod.rs).

use super::*;

// ── Remote op application ────────────────────────────────────────
// Materializes ops from peer devices into local truth tables. A missing
// prerequisite parks the containing segment; it is never treated as applied.

pub(super) enum RemoteOpOutcome {
    Applied,
    Pending(String),
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

fn entity_ids_for_hashes(conn: &Connection, hashes: &[String]) -> rusqlite::Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(hashes.len());
    for hash in hashes {
        if let Some(id) = entity_id_by_hash(conn, hash)? {
            ids.push(id);
        }
    }
    Ok(ids)
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
        | "collection_split"
        | "entity_tags_added"
        | "entity_tags_removed"
        | "collection_renamed"
        | "collection_members_added"
        | "collection_members_removed"
        | "collection_members_reordered" => {
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
        "collection_members_added" | "collection_members_removed" => payload_strings(p, "members"),
        "collection_members_reordered" => payload_strings(p, "order"),
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
    if let Some(reason) = missing_prerequisite(conn, op)? {
        return Ok(RemoteOpOutcome::Pending(reason));
    }
    let key = op.entity_key.as_str();
    let p = &op.payload;
    let now = chrono::Utc::now().to_rfc3339();
    match op.op_type.as_str() {
        "entity_created" => {
            if entity_id_by_hash(conn, key)?.is_some() {
                return Ok(RemoteOpOutcome::Applied); // content-addressed: already materialized
            }
            conn.execute(
                "DELETE FROM media_file WHERE file_hash = ?1
                 AND file_id NOT IN (SELECT file_id FROM single_media_entity)",
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
            let entity_id = write::entities::insert_single(
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
                    types::ExpansionMode::EntityOnly,
                )?;
            }
            let tags = payload_strings(p, "tags");
            if !tags.is_empty() {
                write::tags::add_tags(
                    conn,
                    &[entity_id],
                    &tags,
                    payload_mask(p, "tag_provenance", types::TAG_PROVENANCE_MANUAL),
                    types::ExpansionMode::EntityOnly,
                )?;
            }
            // Derivatives (thumbnail, phash, colors) queue when the blob
            // lands; the blob itself arrives via blob sync or source refetch.
        }
        "entity_status_changed" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                if let Some(status) = p.get("status").and_then(|v| v.as_i64()) {
                    write::entities::set_entity_status(
                        conn,
                        &[id],
                        status,
                        types::ExpansionMode::EntityOnly,
                        &now,
                    )?;
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
                    types::ExpansionMode::EntityOnly,
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
        "collection_split" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                write::collections::split_collection(conn, id)?;
            }
        }
        "entity_tags_added" | "entity_tags_removed" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                let tags = payload_strings(p, "tags");
                if tags.is_empty() {
                    return Ok(RemoteOpOutcome::Applied);
                }
                if op.op_type == "entity_tags_added" {
                    write::tags::add_tags(
                        conn,
                        &[id],
                        &tags,
                        payload_mask(p, "provenance", types::TAG_PROVENANCE_MANUAL),
                        types::ExpansionMode::EntityOnly,
                    )?;
                } else {
                    write::tags::remove_tags(conn, &[id], &tags, types::ExpansionMode::EntityOnly)?;
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
                    return Ok(RemoteOpOutcome::Applied);
                }
                if op.op_type == "folder_members_added" {
                    write::folders::add_members(conn, id, &ids, types::ExpansionMode::EntityOnly)?;
                } else {
                    write::folders::remove_members(
                        conn,
                        id,
                        &ids,
                        types::ExpansionMode::EntityOnly,
                    )?;
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
                    payload_str(p, "sort_field"),
                    payload_str(p, "sort_order"),
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
        "collection_created" => {
            if entity_id_by_hash(conn, key)?.is_none() {
                write::entities::insert_collection(
                    conn,
                    key,
                    payload_str(p, "name").unwrap_or("Collection"),
                    payload_str(p, "date_created").unwrap_or(&now),
                    &now,
                )?;
            }
        }
        "collection_renamed" => {
            if let (Some(id), Some(name)) = (entity_id_by_hash(conn, key)?, payload_str(p, "name"))
            {
                write::collections::update_collection_name(conn, id, name, &now)?;
            }
        }
        "collection_members_added" | "collection_members_removed" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                let ids = entity_ids_for_hashes(conn, &payload_strings(p, "members"))?;
                if ids.is_empty() {
                    return Ok(RemoteOpOutcome::Applied);
                }
                if op.op_type == "collection_members_added" {
                    write::collections::add_members(conn, id, &ids)?;
                } else {
                    write::collections::remove_members(conn, id, &ids)?;
                }
            }
        }
        "collection_members_reordered" => {
            if let Some(id) = entity_id_by_hash(conn, key)? {
                let ids = entity_ids_for_hashes(conn, &payload_strings(p, "order"))?;
                if !ids.is_empty() {
                    write::collections::reorder_members(conn, id, &ids)?;
                }
            }
        }
        "duplicate_decided" => {
            if let Some((hash_a, hash_b)) = key.split_once('|') {
                let file_of = |hash: &str| -> rusqlite::Result<Option<i64>> {
                    conn.query_row(
                        "SELECT sme.file_id FROM media_entity me
                         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
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
    Ok(RemoteOpOutcome::Applied)
}
