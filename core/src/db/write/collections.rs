//! Collection write operations — membership, reorder, split, aggregates.
//! Collection membership is encoded on media_entity via parent_collection_entity_id.

use rusqlite::{params, Connection};

use crate::db::types::CollectionMembershipChange;

pub fn create_collection(conn: &Connection, name: &str, now: &str) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO media_entity (
             entity_hash, entity_kind, status, name, member_count, total_size_bytes,
             date_created, date_added, date_modified
         ) VALUES (
             lower(hex(randomblob(32))), 'collection', 1, ?1, 0, 0, ?2, ?2, ?2
         )",
        params![name, now],
    )?;
    let entity_id = conn.last_insert_rowid();

    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(format!("collection:{entity_id}").as_bytes());
    let entity_hash = hex::encode(hasher.finalize());
    conn.execute(
        "UPDATE media_entity SET entity_hash = ?1 WHERE entity_id = ?2",
        params![entity_hash, entity_id],
    )?;

    Ok(entity_id)
}

pub fn update_collection_name(
    conn: &Connection,
    collection_id: i64,
    name: &str,
    now: &str,
) -> rusqlite::Result<()> {
    let changed = conn.execute(
        "UPDATE media_entity
         SET name = ?1, date_modified = ?2
         WHERE entity_id = ?3 AND entity_kind = 'collection'",
        params![name, now, collection_id],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    Ok(())
}

pub fn delete_collection(conn: &Connection, collection_id: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT entity_id
         FROM media_entity
         WHERE parent_collection_entity_id = ?1
         ORDER BY COALESCE(collection_ordinal, 9223372036854775807) ASC, entity_id ASC",
    )?;
    let member_ids = stmt
        .query_map([collection_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    conn.execute(
        "UPDATE media_entity
         SET parent_collection_entity_id = NULL,
             collection_ordinal = NULL
         WHERE parent_collection_entity_id = ?1",
        [collection_id],
    )?;
    conn.execute(
        "DELETE FROM media_entity
         WHERE entity_id = ?1 AND entity_kind = 'collection'",
        [collection_id],
    )?;

    Ok(member_ids)
}

/// Add single entities as members of a collection.
/// Sets parent_collection_entity_id and collection_ordinal on each member.
/// Updates collection aggregates transactionally.
pub fn add_members(
    conn: &Connection,
    collection_id: i64,
    member_entity_ids: &[i64],
) -> rusqlite::Result<CollectionMembershipChange> {
    let mut previous_collections = Vec::new();
    let mut previous_stmt = conn.prepare(
        "SELECT parent_collection_entity_id
         FROM media_entity
         WHERE entity_id = ?1",
    )?;
    // Determine next ordinal
    let max_ordinal: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(collection_ordinal), 0) FROM media_entity WHERE parent_collection_entity_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (i, eid) in member_entity_ids.iter().enumerate() {
        if let Some(previous_collection_id) = previous_stmt
            .query_row([eid], |row| row.get::<_, Option<i64>>(0))
            .ok()
            .flatten()
        {
            if previous_collection_id != collection_id && !previous_collections.contains(&previous_collection_id) {
                previous_collections.push(previous_collection_id);
            }
        }
        conn.execute(
            "UPDATE media_entity SET parent_collection_entity_id = ?1, collection_ordinal = ?2 WHERE entity_id = ?3 AND entity_kind = 'single'",
            params![collection_id, max_ordinal + 1 + i as i64, eid],
        )?;
    }

    sync_aggregates(conn, collection_id)?;
    for previous_collection_id in previous_collections {
        sync_aggregates(conn, previous_collection_id)?;
    }

    Ok(CollectionMembershipChange {
        collection_id,
        added: member_entity_ids.to_vec(),
        removed: Vec::new(),
    })
}

/// Remove single entities from a collection.
/// Clears parent_collection_entity_id and collection_ordinal.
/// If the collection becomes empty, deletes it.
pub fn remove_members(
    conn: &Connection,
    collection_id: i64,
    member_entity_ids: &[i64],
) -> rusqlite::Result<CollectionMembershipChange> {
    for eid in member_entity_ids {
        conn.execute(
            "UPDATE media_entity SET parent_collection_entity_id = NULL, collection_ordinal = NULL WHERE entity_id = ?1 AND parent_collection_entity_id = ?2",
            params![eid, collection_id],
        )?;
    }

    // Check if collection is now empty — if so, delete it
    let remaining: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity WHERE parent_collection_entity_id = ?1",
        [collection_id],
        |row| row.get(0),
    )?;

    if remaining == 0 {
        conn.execute("DELETE FROM media_entity WHERE entity_id = ?1", [collection_id])?;
    } else {
        sync_aggregates(conn, collection_id)?;
    }

    Ok(CollectionMembershipChange {
        collection_id,
        added: Vec::new(),
        removed: member_entity_ids.to_vec(),
    })
}

/// Reorder members within a collection. Updates collection_ordinal values.
pub fn reorder_members(
    conn: &Connection,
    collection_id: i64,
    ordered_entity_ids: &[i64],
) -> rusqlite::Result<()> {
    for (i, eid) in ordered_entity_ids.iter().enumerate() {
        conn.execute(
            "UPDATE media_entity SET collection_ordinal = ?1 WHERE entity_id = ?2 AND parent_collection_entity_id = ?3",
            params![i as i64 + 1, eid, collection_id],
        )?;
    }
    // Update primary_member_entity_id to the new first member
    if let Some(&first) = ordered_entity_ids.first() {
        conn.execute(
            "UPDATE media_entity SET primary_member_entity_id = ?1 WHERE entity_id = ?2",
            params![first, collection_id],
        )?;
    }
    sync_aggregates(conn, collection_id)?;
    Ok(())
}

/// Split a collection: remove all members and delete the collection entity.
/// Returns the freed member entity IDs.
pub fn split_collection(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT entity_id FROM media_entity WHERE parent_collection_entity_id = ?1 ORDER BY collection_ordinal",
    )?;
    let member_ids: Vec<i64> = stmt
        .query_map([collection_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Free all members
    conn.execute(
        "UPDATE media_entity SET parent_collection_entity_id = NULL, collection_ordinal = NULL WHERE parent_collection_entity_id = ?1",
        [collection_id],
    )?;

    // Delete the collection entity
    conn.execute("DELETE FROM media_entity WHERE entity_id = ?1", [collection_id])?;

    Ok(member_ids)
}

/// Recompute collection aggregate fields from member data.
pub fn sync_aggregates(conn: &Connection, collection_id: i64) -> rusqlite::Result<()> {
    // member_count
    let member_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity WHERE parent_collection_entity_id = ?1",
        [collection_id],
        |row| row.get(0),
    )?;

    // total_size_bytes (sum of backing file sizes)
    let total_size: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(mf.size_bytes), 0)
             FROM media_entity me
             JOIN single_media_entity sme ON sme.entity_id = me.entity_id
             JOIN media_file mf ON mf.file_id = sme.file_id
             WHERE me.parent_collection_entity_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    // primary_member_entity_id (first by ordinal)
    let primary: Option<i64> = conn
        .query_row(
            "SELECT entity_id FROM media_entity WHERE parent_collection_entity_id = ?1 ORDER BY collection_ordinal ASC LIMIT 1",
            [collection_id],
            |row| row.get(0),
        )
        .ok();

    let rating: Option<i64> = conn
        .query_row(
            "SELECT MAX(rating)
             FROM media_entity
             WHERE parent_collection_entity_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .unwrap_or(None);

    let status: i64 = conn.query_row(
        "SELECT CASE
             WHEN EXISTS(
                 SELECT 1 FROM media_entity
                 WHERE parent_collection_entity_id = ?1 AND status = 1
             ) THEN 1
             WHEN EXISTS(
                 SELECT 1 FROM media_entity
                 WHERE parent_collection_entity_id = ?1 AND status = 0
             ) THEN 0
             WHEN EXISTS(
                 SELECT 1 FROM media_entity
                 WHERE parent_collection_entity_id = ?1 AND status = 2
             ) THEN 2
             ELSE 1
         END",
        [collection_id],
        |row| row.get(0),
    )?;

    let date_created: Option<String> = conn
        .query_row(
            "SELECT MIN(date_created)
             FROM media_entity
             WHERE parent_collection_entity_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .unwrap_or(None);

    let date_modified: Option<String> = conn
        .query_row(
            "SELECT MAX(date_modified)
             FROM media_entity
             WHERE parent_collection_entity_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .unwrap_or(None);

    conn.execute(
        "UPDATE media_entity
         SET member_count = ?1,
             total_size_bytes = ?2,
             primary_member_entity_id = ?3,
             rating = ?4,
             status = ?5,
             date_created = COALESCE(?6, date_created),
             date_modified = COALESCE(?7, date_modified)
         WHERE entity_id = ?8",
        params![
            member_count,
            total_size,
            primary,
            rating,
            status,
            date_created,
            date_modified,
            collection_id
        ],
    )?;

    Ok(())
}
