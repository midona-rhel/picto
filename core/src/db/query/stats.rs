//! Count and stats queries.

use rusqlite::Connection;

/// System scope counts (top-level entities only, excludes collection members).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScopeCounts {
    pub all_active: i64,
    pub inbox: i64,
    pub trash: i64,
    pub uncategorized: i64,
    pub untagged: i64,
}

/// Get system scope counts. Excludes collection members from all counts.
pub fn get_scope_counts(conn: &Connection) -> rusqlite::Result<ScopeCounts> {
    let count = |status: i64| -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT COUNT(*) FROM media_entity WHERE status = ?1 AND parent_collection_entity_id IS NULL",
            [status],
            |row| row.get(0),
        )
    };

    let all_active = count(1)?;
    let inbox = count(0)?;
    let trash = count(2)?;

    let uncategorized: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity me
         WHERE me.status = 1
           AND me.parent_collection_entity_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = me.entity_id)
           AND NOT EXISTS (
               SELECT 1 FROM media_entity child
               WHERE child.parent_collection_entity_id = me.entity_id
                 AND EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = child.entity_id)
           )",
        [],
        |row| row.get(0),
    )?;

    let untagged: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity me
         WHERE me.status = 1
           AND me.parent_collection_entity_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = me.entity_id)
           AND NOT EXISTS (
               SELECT 1 FROM media_entity child
               WHERE child.parent_collection_entity_id = me.entity_id
                 AND EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = child.entity_id)
           )",
        [],
        |row| row.get(0),
    )?;

    Ok(ScopeCounts {
        all_active,
        inbox,
        trash,
        uncategorized,
        untagged,
    })
}

/// Count total entities (top-level only).
pub fn count_total(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM media_entity WHERE parent_collection_entity_id IS NULL",
        [],
        |row| row.get(0),
    )
}
