//! Canonical collection queries.

use std::collections::{BTreeSet, HashSet};

use rusqlite::{Connection, OptionalExtension};
use serde_json::Value as JsonValue;

use crate::db::types::{CollectionMimeCount, CollectionRecord, CollectionSummary};

fn parse_source_urls_json(raw: &str) -> Vec<String> {
    let parsed = serde_json::from_str::<JsonValue>(raw).ok();
    let Some(value) = parsed else {
        return Vec::new();
    };
    match value {
        JsonValue::Array(items) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect(),
        JsonValue::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        }
        _ => Vec::new(),
    }
}

fn list_member_tag_strings(conn: &Connection, collection_id: i64) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT CASE
             WHEN t.namespace = '' THEN t.subtag
             ELSE t.namespace || ':' || t.subtag
         END AS raw_tag
         FROM media_entity me
         JOIN entity_tag et ON et.entity_id = me.entity_id
         JOIN tag t ON t.tag_id = et.tag_id
         WHERE me.parent_collection_entity_id = ?1
         ORDER BY raw_tag COLLATE NOCASE ASC",
    )?;
    let rows = stmt.query_map([collection_id], |row| row.get::<_, String>(0))?;
    rows.collect()
}

pub fn list_collection_member_hash_rows(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT me.entity_id, me.entity_hash
         FROM media_entity me
         WHERE me.entity_kind = 'single'
           AND me.parent_collection_entity_id = ?1
         ORDER BY COALESCE(me.collection_ordinal, 9223372036854775807) ASC,
                  me.entity_id ASC",
    )?;
    let rows = stmt.query_map([collection_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}

pub fn list_collection_member_hashes(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<Vec<String>> {
    Ok(list_collection_member_hash_rows(conn, collection_id)?
        .into_iter()
        .map(|(_, hash)| hash)
        .collect())
}

pub fn get_collection_hash(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT entity_hash
         FROM media_entity
         WHERE entity_id = ?1 AND entity_kind = 'collection'",
        [collection_id],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_collection_folder_ids(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT fm.folder_id
         FROM folder_member fm
         WHERE fm.entity_id = ?1
            OR fm.entity_id IN (
                SELECT entity_id
                FROM media_entity
                WHERE parent_collection_entity_id = ?1
            )
         ORDER BY fm.folder_id ASC",
    )?;
    let rows = stmt.query_map([collection_id], |row| row.get::<_, i64>(0))?;
    rows.collect()
}

pub fn get_folder_ids_for_entities(
    conn: &Connection,
    entity_ids: &[i64],
) -> rusqlite::Result<Vec<i64>> {
    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=entity_ids.len())
        .map(|idx| format!("?{idx}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT DISTINCT folder_id
         FROM folder_member
         WHERE entity_id IN ({placeholders})
         ORDER BY folder_id ASC"
    );
    let params: Vec<&dyn rusqlite::ToSql> = entity_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, i64>(0))?;
    rows.collect()
}

pub fn list_collections(conn: &Connection) -> rusqlite::Result<Vec<CollectionRecord>> {
    let mut stmt = conn.prepare(
        "SELECT
             me.entity_id,
             COALESCE(me.name, ''),
             me.date_created,
             me.date_modified,
             COALESCE(me.member_count, 0),
             pm.entity_hash
         FROM media_entity me
         LEFT JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
         WHERE me.entity_kind = 'collection'
         ORDER BY COALESCE(me.date_modified, me.date_created) DESC, me.entity_id DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(CollectionRecord {
            id: row.get(0)?,
            name: row.get(1)?,
            tags: Vec::new(),
            image_count: row.get(4)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            thumbnail_url: row
                .get::<_, Option<String>>(5)?
                .map(|hash| format!("media://localhost/thumb/{hash}.jpg")),
        })
    })?;

    let mut collections = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for collection in &mut collections {
        collection.tags = list_member_tag_strings(conn, collection.id)?;
    }
    Ok(collections)
}

pub fn get_collection_summary(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<CollectionSummary> {
    let (id, name, image_count, total_size_bytes, created_at, updated_at): (
        i64,
        String,
        i64,
        i64,
        Option<String>,
        Option<String>,
    ) = conn.query_row(
        "SELECT
             me.entity_id,
             COALESCE(me.name, ''),
             COALESCE(me.member_count, 0),
             COALESCE(me.total_size_bytes, 0),
             me.date_created,
             me.date_modified
         FROM media_entity me
         WHERE me.entity_id = ?1 AND me.entity_kind = 'collection'
         LIMIT 1",
        [collection_id],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    )?;

    let tags = list_member_tag_strings(conn, collection_id)?;

    let mut mime_stmt = conn.prepare(
        "SELECT mf.mime_type, COUNT(*) AS cnt
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         WHERE me.parent_collection_entity_id = ?1
         GROUP BY mf.mime_type
         ORDER BY cnt DESC, mf.mime_type ASC",
    )?;
    let mime_breakdown = mime_stmt
        .query_map([collection_id], |row| {
            Ok(CollectionMimeCount {
                mime: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut source_stmt = conn.prepare(
        "SELECT me.source_urls_json
         FROM media_entity me
         WHERE me.parent_collection_entity_id = ?1
           AND me.source_urls_json IS NOT NULL",
    )?;
    let source_rows = source_stmt.query_map([collection_id], |row| row.get::<_, String>(0))?;
    let mut source_urls = BTreeSet::new();
    for row in source_rows {
        for url in parse_source_urls_json(&row?) {
            source_urls.insert(url);
        }
    }

    let rating: Option<i64> = conn.query_row(
        "SELECT MAX(rating)
         FROM media_entity
         WHERE parent_collection_entity_id = ?1",
        [collection_id],
        |row| row.get(0),
    )?;

    let notes = {
        let mut stmt = conn.prepare(
            "SELECT notes
             FROM media_entity
             WHERE parent_collection_entity_id = ?1
               AND notes IS NOT NULL
               AND TRIM(notes) != ''",
        )?;
        let rows = stmt.query_map([collection_id], |row| row.get::<_, String>(0))?;
        let mut seen = HashSet::new();
        let mut values = Vec::new();
        for row in rows {
            let note = row?;
            let trimmed = note.trim();
            if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                values.push(trimmed.to_string());
            }
        }
        if values.is_empty() {
            None
        } else {
            Some(values.join("\n\n"))
        }
    };

    let imported_at: Option<String> = conn.query_row(
        "SELECT MIN(date_added)
         FROM media_entity
         WHERE parent_collection_entity_id = ?1",
        [collection_id],
        |row| row.get(0),
    )?;

    Ok(CollectionSummary {
        id,
        name,
        tags,
        image_count,
        total_size_bytes,
        mime_breakdown,
        source_urls: source_urls.into_iter().collect(),
        rating,
        created_at,
        updated_at,
        imported_at,
        notes,
    })
}
