//! Media-entity writes. One entity owns exactly one physical media file.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::types::{EntityChange, StatusChange};

pub fn insert_entity(
    conn: &Connection,
    entity_hash: &str,
    file_id: i64,
    name: Option<&str>,
    status: i64,
    date_created: &str,
    date_added: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO media_entity (
             entity_hash, file_id, status, name, date_created, date_added, date_modified
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![entity_hash, file_id, status, name, date_created, date_added],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn set_entity_status(
    conn: &Connection,
    entity_ids: &[i64],
    status: i64,
    now: &str,
) -> rusqlite::Result<StatusChange> {
    let mut change = StatusChange {
        new_status: status,
        ..Default::default()
    };

    for entity_id in entity_ids {
        let entity_hash: Option<String> = conn
            .query_row(
                "SELECT entity_hash FROM media_entity WHERE entity_id = ?1",
                [entity_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(entity_hash) = entity_hash else {
            continue;
        };
        conn.execute(
            "UPDATE media_entity SET status = ?1, date_modified = ?2 WHERE entity_id = ?3",
            params![status, now, entity_id],
        )?;
        change.entity_ids.push(*entity_id);
        change.entity_hashes.push(entity_hash);
    }

    Ok(change)
}

pub fn patch_entity_metadata(
    conn: &Connection,
    entity_ids: &[i64],
    name: Option<&str>,
    rating: Option<Option<i64>>,
    notes: Option<Option<&str>>,
    source_urls_json: Option<&str>,
    now: &str,
) -> rusqlite::Result<EntityChange> {
    let mut change = EntityChange::default();

    for entity_id in entity_ids {
        let entity_hash: Option<String> = conn
            .query_row(
                "SELECT entity_hash FROM media_entity WHERE entity_id = ?1",
                [entity_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(entity_hash) = entity_hash else {
            continue;
        };
        if let Some(value) = name {
            conn.execute(
                "UPDATE media_entity SET name = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![value, now, entity_id],
            )?;
        }
        if let Some(value) = rating {
            conn.execute(
                "UPDATE media_entity SET rating = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![value, now, entity_id],
            )?;
        }
        if let Some(value) = notes {
            conn.execute(
                "UPDATE media_entity SET notes = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![value, now, entity_id],
            )?;
        }
        if let Some(value) = source_urls_json {
            conn.execute(
                "UPDATE media_entity SET source_urls_json = ?1, date_modified = ?2 WHERE entity_id = ?3",
                params![value, now, entity_id],
            )?;
        }
        change.entity_ids.push(*entity_id);
        change.entity_hashes.push(entity_hash);
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
         SET date_created = ?1, date_modified = ?2
         WHERE entity_id = ?3",
        params![date_created, now, entity_id],
    )?;
    Ok(())
}

pub fn delete_entities(conn: &Connection, entity_ids: &[i64]) -> rusqlite::Result<EntityChange> {
    let mut change = EntityChange::default();

    for entity_id in entity_ids {
        let identity: Option<(String, i64, String)> = conn
            .query_row(
                "SELECT me.entity_hash, me.file_id, mf.file_hash
                 FROM media_entity me
                 JOIN media_file mf ON mf.file_id = me.file_id
                 WHERE me.entity_id = ?1",
                [entity_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        let Some((entity_hash, file_id, file_hash)) = identity else {
            continue;
        };

        conn.execute(
            "UPDATE subscription_post_member
             SET entity_id = NULL, status = 'deleted', updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE entity_id = ?1",
            [entity_id],
        )?;
        conn.execute(
            "DELETE FROM duplicate WHERE file_id_a = ?1 OR file_id_b = ?1",
            [file_id],
        )?;
        conn.execute("DELETE FROM media_entity WHERE entity_id = ?1", [entity_id])?;
        conn.execute("DELETE FROM media_file WHERE file_id = ?1", [file_id])?;

        change.entity_ids.push(*entity_id);
        change.entity_hashes.push(entity_hash);
        change.freed_file_hashes.push(file_hash);
    }

    change.freed_file_hashes.sort();
    change.freed_file_hashes.dedup();
    Ok(change)
}
