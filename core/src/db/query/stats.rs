//! Count and stats queries.

use rusqlite::Connection;

/// System scope counts (top-level entities only, excludes collection members).
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScopeCounts {
    pub active: i64,
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

    let active = count(1)?;
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
        active,
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

pub fn count_media_files(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM media_file", [], |row| row.get(0))
}

pub fn aggregate_file_stats(conn: &Connection) -> rusqlite::Result<crate::db::types::FileStats> {
    let mut inbox: i64 = 0;
    let mut active: i64 = 0;
    let mut trash: i64 = 0;
    let mut total_size: i64 = 0;

    let mut stmt = conn.prepare(
        "SELECT me.status, COUNT(*), COALESCE(SUM(mf.size_bytes), 0)
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         GROUP BY me.status",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    for row in rows {
        let (status, count, size_sum) = row?;
        match status {
            0 => {
                inbox = count;
                total_size += size_sum;
            }
            1 => {
                active = count;
                total_size += size_sum;
            }
            2 => trash = count,
            _ => {}
        }
    }

    Ok(crate::db::types::FileStats {
        total: inbox + active,
        inbox,
        active,
        trash,
        total_size,
    })
}

pub fn aggregate_media_type_breakdown(
    conn: &Connection,
) -> rusqlite::Result<crate::db::types::MediaTypeBreakdown> {
    let mut stmt = conn.prepare(
        "SELECT
            CASE
                WHEN mf.mime_type LIKE 'image/%' THEN 'image'
                WHEN mf.mime_type LIKE 'video/%' THEN 'video'
                WHEN mf.mime_type LIKE 'audio/%' THEN 'audio'
                ELSE 'other'
            END AS category,
            COUNT(*),
            COALESCE(SUM(mf.size_bytes), 0)
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         WHERE me.status IN (0, 1)
         GROUP BY category",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    })?;

    let mut breakdown = crate::db::types::MediaTypeBreakdown {
        images: 0,
        images_size: 0,
        videos: 0,
        videos_size: 0,
        audio: 0,
        audio_size: 0,
        other: 0,
        other_size: 0,
    };

    for row in rows {
        let (category, count, size) = row?;
        match category.as_str() {
            "image" => {
                breakdown.images = count;
                breakdown.images_size = size;
            }
            "video" => {
                breakdown.videos = count;
                breakdown.videos_size = size;
            }
            "audio" => {
                breakdown.audio = count;
                breakdown.audio_size = size;
            }
            _ => {
                breakdown.other = count;
                breakdown.other_size = size;
            }
        }
    }

    Ok(breakdown)
}
