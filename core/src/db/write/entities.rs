//! Entity write operations — insert, update status/metadata, delete.
//! All writes target authoritative tables only.

use rusqlite::{params, Connection};

use crate::db::types::{EntityChange, ExpansionMode, StatusChange};

/// Insert a new single entity + its backing file record.
/// Returns the assigned entity_id.
pub fn insert_single(
    conn: &Connection,
    entity_hash: &str,
    file_id: i64,
    name: Option<&str>,
    status: i64,
    date_created: &str,
    date_added: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO media_entity (entity_hash, entity_kind, status, name, date_created, date_added, date_modified)
         VALUES (?1, 'single', ?2, ?3, ?4, ?5, ?5)",
        params![entity_hash, status, name, date_created, date_added],
    )?;
    let entity_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO single_media_entity (entity_id, file_id) VALUES (?1, ?2)",
        params![entity_id, file_id],
    )?;

    Ok(entity_id)
}

/// Insert a new collection entity (no members yet).
/// Returns the assigned entity_id.
pub fn insert_collection(
    conn: &Connection,
    entity_hash: &str,
    name: &str,
    date_created: &str,
    date_added: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO media_entity (entity_hash, entity_kind, status, name, member_count, total_size_bytes, date_created, date_added, date_modified)
         VALUES (?1, 'collection', 0, ?2, 0, 0, ?3, ?4, ?4)",
        params![entity_hash, name, date_created, date_added],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Update entity status. Expands to collection members per the expansion mode.
/// Returns a StatusChange with all affected entity IDs/hashes.
pub fn set_entity_status(
    conn: &Connection,
    entity_ids: &[i64],
    status: i64,
    expansion: ExpansionMode,
    now: &str,
) -> rusqlite::Result<StatusChange> {
    let expanded = expand_ids(conn, entity_ids, expansion)?;
    let mut change = StatusChange {
        new_status: status,
        ..Default::default()
    };

    for eid in &expanded {
        conn.execute(
            "UPDATE media_entity SET status = ?1, date_modified = ?2 WHERE entity_id = ?3",
            params![status, now, eid],
        )?;
        let hash: String = conn.query_row(
            "SELECT entity_hash FROM media_entity WHERE entity_id = ?1",
            [eid],
            |row| row.get(0),
        )?;
        change.entity_ids.push(*eid);
        change.entity_hashes.push(hash);
    }

    Ok(change)
}

/// Update user-authored metadata fields on entities.
pub fn patch_entity_metadata(
    conn: &Connection,
    entity_ids: &[i64],
    name: Option<&str>,
    rating: Option<Option<i64>>,
    notes: Option<Option<&str>>,
    source_urls_json: Option<&str>,
    now: &str,
    expansion: ExpansionMode,
) -> rusqlite::Result<EntityChange> {
    let expanded = expand_ids(conn, entity_ids, expansion)?;
    let mut change = EntityChange::default();

    for eid in &expanded {
        let mut sets = vec!["date_modified = ?1"];
        let mut idx = 2u32;

        // Build dynamic SET clause based on which fields are provided
        if name.is_some() { sets.push("name = ?2"); idx = 3; }
        if rating.is_some() { sets.push(if idx == 2 { "rating = ?2" } else { "rating = ?3" }); }
        if notes.is_some() { sets.push("notes = ?4"); }
        if source_urls_json.is_some() { sets.push("source_urls_json = ?5"); }

        // For simplicity, update all provided fields in one statement
        if let Some(n) = name {
            conn.execute(
                "UPDATE media_entity SET name = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![n, now, eid],
            )?;
        }
        if let Some(r) = rating {
            conn.execute(
                "UPDATE media_entity SET rating = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![r, now, eid],
            )?;
        }
        if let Some(n) = notes {
            conn.execute(
                "UPDATE media_entity SET notes = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![n, now, eid],
            )?;
        }
        if let Some(u) = source_urls_json {
            conn.execute(
                "UPDATE media_entity SET source_urls_json = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![u, now, eid],
            )?;
        }

        let hash: String = conn.query_row(
            "SELECT entity_hash FROM media_entity WHERE entity_id = ?1",
            [eid],
            |row| row.get(0),
        )?;
        change.entity_ids.push(*eid);
        change.entity_hashes.push(hash);
    }

    Ok(change)
}

pub fn set_entity_date_created(
    conn: &Connection,
    entity_id: i64,
    date_created: &str,
    now: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE media_entity
         SET date_created = ?1,
             date_modified = ?2
         WHERE entity_id = ?3",
        params![date_created, now, entity_id],
    )?;
    Ok(())
}

/// Delete entities. For collections, also deletes member singles.
/// Returns the deleted entity IDs and hashes.
pub fn delete_entities(
    conn: &Connection,
    entity_ids: &[i64],
) -> rusqlite::Result<EntityChange> {
    let mut change = EntityChange::default();

    for eid in entity_ids {
        // Collect hash before deletion
        let hash: Option<String> = conn
            .query_row(
                "SELECT entity_hash FROM media_entity WHERE entity_id = ?1",
                [eid],
                |row| row.get(0),
            )
            .ok();

        let kind: Option<String> = conn
            .query_row(
                "SELECT entity_kind FROM media_entity WHERE entity_id = ?1",
                [eid],
                |row| row.get(0),
            )
            .ok();

        // If collection, delete all members first
        if kind.as_deref() == Some("collection") {
            let mut stmt = conn.prepare(
                "SELECT entity_id FROM media_entity WHERE parent_collection_entity_id = ?1",
            )?;
            let member_ids: Vec<i64> = stmt
                .query_map([eid], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;

            for mid in &member_ids {
                let mhash: String = conn.query_row(
                    "SELECT entity_hash FROM media_entity WHERE entity_id = ?1",
                    [mid],
                    |row| row.get(0),
                )?;
                conn.execute("DELETE FROM single_media_entity WHERE entity_id = ?1", [mid])?;
                conn.execute("DELETE FROM entity_tag WHERE entity_id = ?1", [mid])?;
                conn.execute("DELETE FROM folder_member WHERE entity_id = ?1", [mid])?;
                conn.execute("DELETE FROM media_entity WHERE entity_id = ?1", [mid])?;
                change.entity_ids.push(*mid);
                change.entity_hashes.push(mhash);
            }
        }

        // Delete the entity itself
        conn.execute("DELETE FROM single_media_entity WHERE entity_id = ?1", [eid])?;
        conn.execute("DELETE FROM entity_tag WHERE entity_id = ?1", [eid])?;
        conn.execute("DELETE FROM folder_member WHERE entity_id = ?1", [eid])?;
        conn.execute("DELETE FROM media_entity WHERE entity_id = ?1", [eid])?;

        if let Some(h) = hash {
            change.entity_ids.push(*eid);
            change.entity_hashes.push(h);
        }
    }

    Ok(change)
}

/// Expand entity IDs according to the expansion mode.
/// EntityOnly: return as-is.
/// DescendantsOnly: return member singles of any collections in the list.
/// EntityAndDescendants: return the original IDs plus member singles.
pub fn expand_ids(
    conn: &Connection,
    entity_ids: &[i64],
    mode: ExpansionMode,
) -> rusqlite::Result<Vec<i64>> {
    match mode {
        ExpansionMode::EntityOnly => Ok(entity_ids.to_vec()),
        ExpansionMode::DescendantsOnly => {
            let mut result = Vec::new();
            for eid in entity_ids {
                let kind: String = conn.query_row(
                    "SELECT entity_kind FROM media_entity WHERE entity_id = ?1",
                    [eid],
                    |row| row.get(0),
                )?;
                if kind == "collection" {
                    let mut stmt = conn.prepare(
                        "SELECT entity_id FROM media_entity WHERE parent_collection_entity_id = ?1 ORDER BY collection_ordinal",
                    )?;
                    let members: Vec<i64> = stmt
                        .query_map([eid], |row| row.get(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    result.extend(members);
                }
            }
            Ok(result)
        }
        ExpansionMode::EntityAndDescendants => {
            let mut result = Vec::new();
            for eid in entity_ids {
                result.push(*eid);
                let kind: String = conn.query_row(
                    "SELECT entity_kind FROM media_entity WHERE entity_id = ?1",
                    [eid],
                    |row| row.get(0),
                )?;
                if kind == "collection" {
                    let mut stmt = conn.prepare(
                        "SELECT entity_id FROM media_entity WHERE parent_collection_entity_id = ?1 ORDER BY collection_ordinal",
                    )?;
                    let members: Vec<i64> = stmt
                        .query_map([eid], |row| row.get(0))?
                        .collect::<rusqlite::Result<Vec<_>>>()?;
                    result.extend(members);
                }
            }
            Ok(result)
        }
    }
}
