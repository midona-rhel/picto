//! Collection write operations — membership, reorder, split, aggregates.
//! Collection membership is encoded on media_entity via parent_collection_entity_id.

use rusqlite::{params, Connection};

use crate::db::types::CollectionMembershipChange;

/// Add single entities as members of a collection.
/// Sets parent_collection_entity_id and collection_ordinal on each member.
/// Updates collection aggregates transactionally.
pub fn add_members(
    conn: &Connection,
    collection_id: i64,
    member_entity_ids: &[i64],
) -> rusqlite::Result<CollectionMembershipChange> {
    // Determine next ordinal
    let max_ordinal: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(collection_ordinal), 0) FROM media_entity WHERE parent_collection_entity_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    for (i, eid) in member_entity_ids.iter().enumerate() {
        conn.execute(
            "UPDATE media_entity SET parent_collection_entity_id = ?1, collection_ordinal = ?2 WHERE entity_id = ?3 AND entity_kind = 'single'",
            params![collection_id, max_ordinal + 1 + i as i64, eid],
        )?;
    }

    sync_aggregates(conn, collection_id)?;

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

    conn.execute(
        "UPDATE media_entity SET member_count = ?1, total_size_bytes = ?2, primary_member_entity_id = ?3 WHERE entity_id = ?4",
        params![member_count, total_size, primary, collection_id],
    )?;

    Ok(())
}
