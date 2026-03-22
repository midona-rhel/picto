//! Shared schema helpers used by migrations, reconciliation, and tests.

use rusqlite::Connection;
/// Repair legacy/corrupt states where a `collection` entity is still linked in
/// `entity_file`. Collections must never own a direct file row.
///
/// Strategy:
/// - move/recreate the linked file as a `single` member under the collection
/// - remove the illegal `entity_file` link from the collection entity
/// - re-sync collection aggregate metadata (cover/count/size/tag mirror)
pub(super) fn repair_collection_entity_file_links(conn: &Connection) -> rusqlite::Result<()> {
    if !table_exists(conn, "media_entity")?
        || !table_exists(conn, "entity_file")?
        || !table_exists(conn, "file")?
    {
        return Ok(());
    }

    #[derive(Debug)]
    struct BadCollectionFileLink {
        collection_id: i64,
        file_id: i64,
        file_name: Option<String>,
        file_status: i64,
        file_rating: Option<i64>,
        imported_at: Option<String>,
    }

    let mut stmt = conn.prepare(
        "SELECT c.entity_id, ef.file_id, f.name, f.status, f.rating, f.imported_at
         FROM media_entity c
         JOIN entity_file ef ON ef.entity_id = c.entity_id
         JOIN file f ON f.file_id = ef.file_id
         WHERE c.kind = 'collection'
         ORDER BY c.entity_id",
    )?;
    let bad_links: Vec<BadCollectionFileLink> = stmt
        .query_map([], |row| {
            Ok(BadCollectionFileLink {
                collection_id: row.get(0)?,
                file_id: row.get(1)?,
                file_name: row.get(2)?,
                file_status: row.get(3)?,
                file_rating: row.get(4)?,
                imported_at: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if bad_links.is_empty() {
        return Ok(());
    }

    let mut repaired_count = 0usize;
    for link in bad_links {
        let max_ordinal: i64 = conn.query_row(
            "SELECT COALESCE(MAX(collection_ordinal), 0)
             FROM media_entity
             WHERE parent_collection_id = ?1",
            [link.collection_id],
            |row| row.get(0),
        )?;
        let next_ordinal = max_ordinal + 1;

        let existing_single = conn.query_row(
            "SELECT me.entity_id, me.parent_collection_id
             FROM entity_file ef
             JOIN media_entity me ON me.entity_id = ef.entity_id
             WHERE ef.file_id = ?1
               AND me.kind = 'single'
             LIMIT 1",
            [link.file_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?)),
        );

        match existing_single {
            Ok((single_entity_id, parent_collection_id)) => {
                if parent_collection_id.is_none() {
                    conn.execute(
                        "UPDATE media_entity
                         SET parent_collection_id = ?1,
                             collection_ordinal = ?2,
                             updated_at = CURRENT_TIMESTAMP
                         WHERE entity_id = ?3
                           AND kind = 'single'",
                        rusqlite::params![link.collection_id, next_ordinal, single_entity_id],
                    )?;
                }
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                conn.execute(
                    "INSERT INTO media_entity (
                         kind, name, description, status, rating, created_at, updated_at,
                         parent_collection_id, collection_ordinal
                     ) VALUES (
                         'single', ?1, '', ?2, ?3, COALESCE(?4, CURRENT_TIMESTAMP), CURRENT_TIMESTAMP,
                         ?5, ?6
                     )",
                    rusqlite::params![
                        link.file_name,
                        link.file_status,
                        link.file_rating,
                        link.imported_at,
                        link.collection_id,
                        next_ordinal
                    ],
                )?;
                let new_entity_id = conn.last_insert_rowid();
                conn.execute(
                    "INSERT OR IGNORE INTO entity_file (entity_id, file_id) VALUES (?1, ?2)",
                    rusqlite::params![new_entity_id, link.file_id],
                )?;
            }
            Err(e) => return Err(e),
        }

        conn.execute(
            "DELETE FROM entity_file WHERE entity_id = ?1",
            [link.collection_id],
        )?;
        crate::folders::collections_db::sync_collection_aggregate_metadata(
            conn,
            link.collection_id,
        )?;
        repaired_count += 1;
    }

    if repaired_count > 0 {
        tracing::warn!(
            repaired_count,
            "Repaired collection rows with illegal direct file links"
        );
    }

    Ok(())
}

/// Check if a table has a specific column using PRAGMA table_info.
pub(super) fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", table))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
        [table],
        |row| row.get(0),
    )
}

pub(super) fn seed_manifest(conn: &Connection) -> rusqlite::Result<()> {
    let keys = [
        "global",
        "files",
        "tags",
        "tag_graph",
        "sidebar",
        "smart_folders",
        "bitmaps",
    ];
    let mut stmt =
        conn.prepare_cached("INSERT OR IGNORE INTO manifest (key, epoch) VALUES (?1, 0)")?;
    for key in &keys {
        stmt.execute([key])?;
    }
    Ok(())
}

pub(super) fn seed_artifact_manifest(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO artifact_manifest_meta (id, manifest_epoch, updated_at)
         VALUES (1, 0, CURRENT_TIMESTAMP)",
        [],
    )?;

    let current_epoch: i64 = conn.query_row(
        "SELECT manifest_epoch FROM artifact_manifest_meta WHERE id = 1",
        [],
        |row| row.get(0),
    )?;

    let mut stmt = conn.prepare_cached(
        "INSERT OR IGNORE INTO artifact_manifest_entry
            (manifest_epoch, artifact_name, artifact_version, built_from_truth_seq, payload_json)
         VALUES (?1, ?2, 0, 0, ?3)",
    )?;

    let artifacts = [
        "global",
        "files",
        "tags",
        "tag_graph",
        "sidebar",
        "smart_folders",
        "bitmaps",
    ];
    for artifact in &artifacts {
        let payload_json = if *artifact == "bitmaps" {
            r#"{"active_file":"bitmaps.bin"}"#
        } else {
            "{}"
        };
        stmt.execute(rusqlite::params![current_epoch, artifact, payload_json])?;
    }
    Ok(())
}
