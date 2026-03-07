//! Collection entity storage.
//!
//! Collections are first-class media entities (`kind='collection'`) and are no
//! longer folder-backed placeholders.

use std::collections::{BTreeSet, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::compilers::CompilerEvent;
use crate::sqlite::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionRecord {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub image_count: i64,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionMimeCount {
    pub mime: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionSummary {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub image_count: i64,
    pub total_size_bytes: i64,
    pub mime_breakdown: Vec<CollectionMimeCount>,
    pub source_urls: Vec<String>,
    pub rating: Option<i64>,
}

fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in tags {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        if seen.insert(t.to_string()) {
            out.push(t.to_string());
        }
    }
    out
}

fn normalize_urls(urls: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in urls {
        let u = raw.trim();
        if u.is_empty() {
            continue;
        }
        if seen.insert(u.to_string()) {
            out.push(u.to_string());
        }
    }
    out
}

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
            let t = s.trim();
            if t.is_empty() {
                Vec::new()
            } else {
                vec![t.to_string()]
            }
        }
        _ => Vec::new(),
    }
}

fn ensure_collection_folder_replacements(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<()> {
    let mut folder_stmt = conn.prepare_cached(
        "SELECT fe.folder_id, MIN(fe.position_rank) AS min_rank
         FROM folder_entity fe
         JOIN media_entity me_member ON me_member.entity_id = fe.entity_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1
         GROUP BY fe.folder_id",
    )?;
    let rows = folder_stmt.query_map([collection_id], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
    })?;
    let folder_rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;

    // Remove stale folder links where the collection no longer has any member
    // in the folder.
    if folder_rows.is_empty() {
        conn.execute(
            "DELETE FROM folder_entity WHERE entity_id = ?1",
            [collection_id],
        )?;
        return Ok(());
    }
    let keep_folder_ids: Vec<i64> = folder_rows
        .iter()
        .map(|(folder_id, _)| *folder_id)
        .collect();
    let placeholders = (0..keep_folder_ids.len())
        .map(|i| format!("?{}", i + 2))
        .collect::<Vec<_>>()
        .join(",");
    let delete_sql = format!(
        "DELETE FROM folder_entity
         WHERE entity_id = ?1
           AND folder_id NOT IN ({})",
        placeholders
    );
    let mut delete_params: Vec<rusqlite::types::Value> =
        Vec::with_capacity(keep_folder_ids.len() + 1);
    delete_params.push(rusqlite::types::Value::Integer(collection_id));
    for folder_id in &keep_folder_ids {
        delete_params.push(rusqlite::types::Value::Integer(*folder_id));
    }
    conn.execute(
        &delete_sql,
        rusqlite::params_from_iter(delete_params.iter()),
    )?;

    for (folder_id, min_rank) in folder_rows {
        let existing_rank: Option<i64> = conn
            .query_row(
                "SELECT position_rank FROM folder_entity WHERE folder_id = ?1 AND entity_id = ?2",
                params![folder_id, collection_id],
                |row| row.get(0),
            )
            .optional()?;
        match existing_rank {
            Some(rank) if rank <= min_rank => {}
            Some(_) => {
                conn.execute(
                    "UPDATE folder_entity SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
                    params![min_rank, folder_id, collection_id],
                )?;
            }
            None => {
                conn.execute(
                    "INSERT INTO folder_entity (folder_id, entity_id, position_rank) VALUES (?1, ?2, ?3)",
                    params![folder_id, collection_id, min_rank],
                )?;
            }
        }
    }

    Ok(())
}

pub(crate) fn sync_collection_aggregate_metadata(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<()> {
    // 1) Mirror member tags to collection-level entity tags.
    conn.execute(
        "DELETE FROM entity_tag_raw WHERE entity_id = ?1",
        [collection_id],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO entity_tag_raw (entity_id, tag_id, source)
         SELECT ?1, etr.tag_id, 'local'
         FROM media_entity me_member
         JOIN entity_tag_raw etr ON etr.entity_id = me_member.entity_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1
         GROUP BY etr.tag_id",
        [collection_id],
    )?;

    // 2) Keep collection tag chips in sync with mirrored member tags.
    conn.execute(
        "DELETE FROM collection_tag WHERE collection_entity_id = ?1",
        [collection_id],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO collection_tag (collection_entity_id, tag)
         SELECT
            ?1,
            CASE
                WHEN COALESCE(t.namespace, '') = '' THEN t.subtag
                ELSE t.namespace || ':' || t.subtag
            END
         FROM media_entity me_member
         JOIN entity_tag_raw etr ON etr.entity_id = me_member.entity_id
         JOIN tag t ON t.tag_id = etr.tag_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1
         GROUP BY t.namespace, t.subtag",
        [collection_id],
    )?;

    // 3) Merge source URLs from all member files.
    let mut url_stmt = conn.prepare_cached(
        "SELECT f.source_urls_json
         FROM media_entity me_member
         JOIN entity_file ef ON ef.entity_id = me_member.entity_id
         JOIN file f ON f.file_id = ef.file_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1
           AND f.source_urls_json IS NOT NULL",
    )?;
    let url_rows = url_stmt.query_map([collection_id], |row| row.get::<_, Option<String>>(0))?;
    let mut merged_urls = BTreeSet::new();
    for row in url_rows {
        if let Some(raw) = row? {
            for url in parse_source_urls_json(&raw) {
                merged_urls.insert(url);
            }
        }
    }
    conn.execute(
        "DELETE FROM collection_source_url WHERE collection_entity_id = ?1",
        [collection_id],
    )?;
    if !merged_urls.is_empty() {
        let mut insert_url = conn.prepare_cached(
            "INSERT OR IGNORE INTO collection_source_url (collection_entity_id, url) VALUES (?1, ?2)",
        )?;
        for url in merged_urls {
            insert_url.execute(params![collection_id, url])?;
        }
    }

    // 4) Rating = max member rating (if any rating exists).
    let merged_rating: Option<i64> = conn.query_row(
        "SELECT MAX(f.rating)
         FROM media_entity me_member
         JOIN entity_file ef ON ef.entity_id = me_member.entity_id
         JOIN file f ON f.file_id = ef.file_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1",
        [collection_id],
        |row| row.get(0),
    )?;

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE media_entity
         SET rating = ?1,
             status = COALESCE((
                 SELECT MIN(me_member.status)
                 FROM media_entity me_member
                 WHERE me_member.kind = 'single'
                   AND me_member.parent_collection_id = ?3
             ), status),
             updated_at = ?2
         WHERE entity_id = ?3 AND kind = 'collection'",
        params![merged_rating, now, collection_id],
    )?;

    // 5) Ensure collection appears anywhere its members already lived (folder replacement semantics).
    ensure_collection_folder_replacements(conn, collection_id)?;

    // 6) Populate denormalized cover/count/size cache for fast grid queries.
    conn.execute(
        "UPDATE media_entity
         SET cover_file_id = (
             SELECT ef2.file_id
             FROM media_entity me_member
             JOIN entity_file ef2 ON ef2.entity_id = me_member.entity_id
             WHERE me_member.kind = 'single'
               AND me_member.parent_collection_id = ?1
             ORDER BY COALESCE(me_member.collection_ordinal, 9223372036854775807) ASC,
                      me_member.entity_id ASC
             LIMIT 1
         ),
         cached_item_count = (
             SELECT COUNT(*)
             FROM media_entity me_member
             WHERE me_member.kind = 'single'
               AND me_member.parent_collection_id = ?1
         ),
         cached_total_size_bytes = (
             SELECT COALESCE(SUM(f2.size), 0)
             FROM media_entity me_member
             JOIN entity_file ef2 ON ef2.entity_id = me_member.entity_id
             JOIN file f2 ON f2.file_id = ef2.file_id
             WHERE me_member.kind = 'single'
               AND me_member.parent_collection_id = ?1
         )
         WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
    )?;

    // Keep tag.file_count consistent after mirrored collection tagging changes.
    crate::tags::db::rebuild_tag_counts(conn)?;

    Ok(())
}

pub fn create_collection(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    tags: &[String],
) -> rusqlite::Result<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let description = description.unwrap_or("");
    conn.execute(
        "INSERT INTO media_entity (kind, name, description, status, created_at, updated_at)
         VALUES ('collection', ?1, ?2, 1, ?3, ?3)",
        params![name, description, now],
    )?;
    let collection_id = conn.last_insert_rowid();

    let tags = normalize_tags(tags);
    if !tags.is_empty() {
        let mut stmt = conn.prepare_cached(
            "INSERT OR IGNORE INTO collection_tag (collection_entity_id, tag)
             VALUES (?1, ?2)",
        )?;
        for tag in tags {
            stmt.execute(params![collection_id, tag])?;
        }
    }

    Ok(collection_id)
}

pub fn update_collection(
    conn: &Connection,
    collection_id: i64,
    name: Option<&str>,
    description: Option<&str>,
    tags: Option<&[String]>,
    source_urls: Option<&[String]>,
) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM media_entity
         WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE media_entity
         SET name = COALESCE(?1, name),
             description = COALESCE(?2, description),
             updated_at = ?3
         WHERE entity_id = ?4 AND kind = 'collection'",
        params![name, description, now, collection_id],
    )?;

    if let Some(tags) = tags {
        conn.execute(
            "DELETE FROM collection_tag WHERE collection_entity_id = ?1",
            [collection_id],
        )?;
        let tags = normalize_tags(tags);
        if !tags.is_empty() {
            let mut stmt = conn.prepare_cached(
                "INSERT OR IGNORE INTO collection_tag (collection_entity_id, tag)
                 VALUES (?1, ?2)",
            )?;
            for tag in tags {
                stmt.execute(params![collection_id, tag])?;
            }
        }
    }

    if let Some(source_urls) = source_urls {
        conn.execute(
            "DELETE FROM collection_source_url WHERE collection_entity_id = ?1",
            [collection_id],
        )?;
        let urls = normalize_urls(source_urls);
        if !urls.is_empty() {
            let mut stmt = conn.prepare_cached(
                "INSERT OR IGNORE INTO collection_source_url (collection_entity_id, url)
                 VALUES (?1, ?2)",
            )?;
            for url in urls {
                stmt.execute(params![collection_id, url])?;
            }
        }
    }

    Ok(())
}

pub fn set_collection_rating(
    conn: &Connection,
    collection_id: i64,
    rating: Option<i64>,
) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM media_entity
         WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE media_entity
         SET rating = ?1,
             updated_at = ?2
         WHERE entity_id = ?3 AND kind = 'collection'",
        params![rating, now, collection_id],
    )?;
    Ok(())
}

pub fn delete_collection(conn: &Connection, collection_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE media_entity
         SET parent_collection_id = NULL,
             collection_ordinal = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE parent_collection_id = ?1",
        [collection_id],
    )?;
    conn.execute(
        "DELETE FROM media_entity WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
    )?;
    Ok(())
}

pub fn list_collections(conn: &Connection) -> rusqlite::Result<Vec<CollectionRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT
             me.entity_id,
             COALESCE(me.name, ''),
             COALESCE(me.description, ''),
             me.created_at,
             me.updated_at,
             me.cached_item_count,
             (SELECT f.hash FROM file f WHERE f.file_id = me.cover_file_id) AS cover_hash
         FROM media_entity me
         WHERE me.kind = 'collection'
         ORDER BY COALESCE(me.updated_at, me.created_at) DESC, me.entity_id DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let cover_hash: Option<String> = row.get(6)?;
        Ok(CollectionRecord {
            id,
            name: row.get(1)?,
            description: row.get(2)?,
            tags: Vec::new(),
            image_count: row.get(5)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
            thumbnail_url: cover_hash.map(|h| format!("media://localhost/thumb/{h}.jpg")),
        })
    })?;

    let mut collections: Vec<CollectionRecord> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    if collections.is_empty() {
        return Ok(collections);
    }

    let mut tag_stmt = conn.prepare_cached(
        "SELECT tag FROM collection_tag
         WHERE collection_entity_id = ?1
         ORDER BY tag COLLATE NOCASE",
    )?;
    for collection in &mut collections {
        let tags = tag_stmt.query_map([collection.id], |row| row.get::<_, String>(0))?;
        collection.tags = tags.collect::<rusqlite::Result<Vec<_>>>()?;
    }

    Ok(collections)
}

pub fn list_collection_member_file_ids(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare_cached(
        "SELECT ef.file_id
         FROM media_entity me_member
         JOIN entity_file ef ON ef.entity_id = me_member.entity_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1
         ORDER BY COALESCE(me_member.collection_ordinal, 9223372036854775807) ASC, me_member.entity_id ASC",
    )?;
    let rows = stmt.query_map([collection_id], |row| row.get::<_, i64>(0))?;
    rows.collect()
}

pub fn get_collection_summary(
    conn: &Connection,
    collection_id: i64,
) -> rusqlite::Result<CollectionSummary> {
    let (id, name, description, image_count, total_size_bytes, rating): (
        i64,
        String,
        String,
        i64,
        i64,
        Option<i64>,
    ) = conn.query_row(
        "SELECT
             me.entity_id,
             COALESCE(me.name, ''),
             COALESCE(me.description, ''),
             me.cached_item_count,
             me.cached_total_size_bytes,
             me.rating
         FROM media_entity me
         WHERE me.entity_id = ?1 AND me.kind = 'collection'
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

    let mut tag_stmt = conn.prepare_cached(
        "SELECT tag FROM collection_tag
         WHERE collection_entity_id = ?1
         ORDER BY tag COLLATE NOCASE",
    )?;
    let tags = tag_stmt
        .query_map([collection_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut mime_stmt = conn.prepare_cached(
        "SELECT f.mime, COUNT(*) AS cnt
         FROM media_entity me_member
         JOIN entity_file ef ON ef.entity_id = me_member.entity_id
         JOIN file f ON f.file_id = ef.file_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1
         GROUP BY f.mime
         ORDER BY cnt DESC, f.mime ASC",
    )?;
    let mime_breakdown = mime_stmt
        .query_map([collection_id], |row| {
            Ok(CollectionMimeCount {
                mime: row.get(0)?,
                count: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut source_stmt = conn.prepare_cached(
        "SELECT url FROM collection_source_url
         WHERE collection_entity_id = ?1
         ORDER BY url COLLATE NOCASE",
    )?;
    let source_urls = source_stmt
        .query_map([collection_id], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(CollectionSummary {
        id,
        name,
        description,
        tags,
        image_count,
        total_size_bytes,
        mime_breakdown,
        source_urls,
        rating,
    })
}

pub fn add_collection_member(
    conn: &Connection,
    collection_id: i64,
    member_entity_id: i64,
    ordinal: Option<i64>,
) -> rusqlite::Result<()> {
    let previous_parent: Option<i64> = conn
        .query_row(
            "SELECT parent_collection_id FROM media_entity WHERE entity_id = ?1",
            [member_entity_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();

    let max_ordinal: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(collection_ordinal), 0)
             FROM media_entity
             WHERE parent_collection_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let ord = ordinal.unwrap_or(max_ordinal + 1);
    let changed = conn.execute(
        "UPDATE media_entity
         SET parent_collection_id = ?1,
             collection_ordinal = ?2,
             updated_at = CURRENT_TIMESTAMP
         WHERE entity_id = ?3
           AND kind = 'single'
           AND entity_id != ?1",
        params![collection_id, ord, member_entity_id],
    )?;
    if changed == 0 {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }
    sync_collection_aggregate_metadata(conn, collection_id)?;
    if let Some(prev) = previous_parent {
        if prev != collection_id {
            sync_collection_aggregate_metadata(conn, prev)?;
        }
    }
    Ok(())
}

pub fn add_collection_members_by_hashes(
    conn: &Connection,
    collection_id: i64,
    hashes: &[String],
) -> rusqlite::Result<usize> {
    if hashes.is_empty() {
        return Ok(0);
    }

    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM media_entity
         WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    let max_ordinal: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(collection_ordinal), 0)
             FROM media_entity
             WHERE parent_collection_id = ?1",
            [collection_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);

    let mut resolve_stmt = conn.prepare_cached(
        "SELECT ef.entity_id, me.parent_collection_id
         FROM file f
         JOIN entity_file ef ON ef.file_id = f.file_id
         JOIN media_entity me ON me.entity_id = ef.entity_id
         WHERE f.hash = ?1
         LIMIT 1",
    )?;
    let mut assign_stmt = conn.prepare_cached(
        "UPDATE media_entity
         SET parent_collection_id = ?1,
             collection_ordinal = ?2,
             updated_at = ?3
         WHERE entity_id = ?4
           AND kind = 'single'
           AND entity_id != ?1",
    )?;

    let mut next_ordinal = max_ordinal + 1;
    let mut added = 0usize;
    let now = chrono::Utc::now().to_rfc3339();
    let mut seen_members = std::collections::HashSet::<i64>::new();
    let mut touched_previous_collections = std::collections::HashSet::<i64>::new();
    for hash in hashes {
        let resolved: Option<(i64, Option<i64>)> = resolve_stmt
            .query_row([hash], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        let Some((member_entity_id, current_parent)) = resolved else {
            continue;
        };
        if current_parent == Some(collection_id) || !seen_members.insert(member_entity_id) {
            continue;
        }
        let changed =
            assign_stmt.execute(params![collection_id, next_ordinal, now, member_entity_id])?;
        if changed > 0 {
            next_ordinal += 1;
            added += changed;
            if let Some(prev) = current_parent {
                if prev != collection_id {
                    touched_previous_collections.insert(prev);
                }
            }
        }
    }

    if added > 0 {
        sync_collection_aggregate_metadata(conn, collection_id)?;
        for prev_collection_id in touched_previous_collections {
            sync_collection_aggregate_metadata(conn, prev_collection_id)?;
        }
    }

    Ok(added)
}

pub fn remove_collection_members_by_hashes(
    conn: &Connection,
    collection_id: i64,
    hashes: &[String],
) -> rusqlite::Result<usize> {
    if hashes.is_empty() {
        return Ok(0);
    }

    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM media_entity
         WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    let mut resolve_stmt = conn.prepare_cached(
        "SELECT ef.entity_id
         FROM file f
         JOIN entity_file ef ON ef.file_id = f.file_id
         JOIN media_entity me ON me.entity_id = ef.entity_id
         WHERE f.hash = ?1 AND me.parent_collection_id = ?2
         LIMIT 1",
    )?;
    let mut detach_stmt = conn.prepare_cached(
        "UPDATE media_entity
         SET parent_collection_id = NULL,
             collection_ordinal = NULL,
             updated_at = CURRENT_TIMESTAMP
         WHERE entity_id = ?1",
    )?;

    let mut removed = 0usize;
    let mut seen = std::collections::HashSet::<i64>::new();
    for hash in hashes {
        let resolved: Option<i64> = resolve_stmt
            .query_row(params![hash, collection_id], |row| row.get(0))
            .optional()?;
        let Some(member_entity_id) = resolved else {
            continue;
        };
        if !seen.insert(member_entity_id) {
            continue;
        }
        let changed = detach_stmt.execute([member_entity_id])?;
        removed += changed;
    }

    if removed > 0 {
        sync_collection_aggregate_metadata(conn, collection_id)?;
    }

    Ok(removed)
}

/// Repoint an entity's file reference to a different file.
/// Used during duplicate resolution: when a collection member's file is a duplicate,
/// the entity keeps its place in the collection but references the winner's file instead.
/// Returns the old file_id that was replaced.
pub fn repoint_entity_to_file(
    conn: &Connection,
    entity_id: i64,
    new_file_id: i64,
) -> rusqlite::Result<Option<i64>> {
    let old_file_id: Option<i64> = conn
        .query_row(
            "SELECT file_id FROM entity_file WHERE entity_id = ?1",
            [entity_id],
            |row| row.get(0),
        )
        .optional()?;

    conn.execute(
        "UPDATE entity_file SET file_id = ?1 WHERE entity_id = ?2",
        params![new_file_id, entity_id],
    )?;

    // If this entity is in a collection, re-sync aggregate metadata
    // (cover hash may have changed).
    let parent: Option<i64> = conn
        .query_row(
            "SELECT parent_collection_id FROM media_entity WHERE entity_id = ?1",
            [entity_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    if let Some(parent_id) = parent {
        sync_collection_aggregate_metadata(conn, parent_id)?;
    }

    Ok(old_file_id)
}

pub fn set_collection_source_urls(
    conn: &Connection,
    collection_id: i64,
    source_urls: &[String],
) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM media_entity
         WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    conn.execute(
        "DELETE FROM collection_source_url WHERE collection_entity_id = ?1",
        [collection_id],
    )?;
    let urls = normalize_urls(source_urls);
    if !urls.is_empty() {
        let mut stmt = conn.prepare_cached(
            "INSERT OR IGNORE INTO collection_source_url (collection_entity_id, url)
             VALUES (?1, ?2)",
        )?;
        for url in urls {
            stmt.execute(params![collection_id, url])?;
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE media_entity SET updated_at = ?1
         WHERE entity_id = ?2 AND kind = 'collection'",
        params![now, collection_id],
    )?;
    Ok(())
}

pub fn reorder_collection_members_by_hashes(
    conn: &Connection,
    collection_id: i64,
    ordered_hashes: &[String],
) -> rusqlite::Result<()> {
    let exists: bool = conn.query_row(
        "SELECT COUNT(*) > 0 FROM media_entity
         WHERE entity_id = ?1 AND kind = 'collection'",
        [collection_id],
        |row| row.get(0),
    )?;
    if !exists {
        return Err(rusqlite::Error::QueryReturnedNoRows);
    }

    // Current member order.
    let mut current_stmt = conn.prepare_cached(
        "SELECT me_member.entity_id, f.hash
         FROM media_entity me_member
         JOIN entity_file ef ON ef.entity_id = me_member.entity_id
         JOIN file f ON f.file_id = ef.file_id
         WHERE me_member.kind = 'single'
           AND me_member.parent_collection_id = ?1
         ORDER BY COALESCE(me_member.collection_ordinal, 9223372036854775807) ASC, me_member.entity_id ASC",
    )?;
    let current_rows = current_stmt
        .query_map([collection_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    if current_rows.is_empty() {
        return Ok(());
    }

    let mut by_hash = std::collections::HashMap::<String, i64>::new();
    let mut current_hash_order = Vec::with_capacity(current_rows.len());
    for (entity_id, hash) in current_rows {
        by_hash.insert(hash.clone(), entity_id);
        current_hash_order.push(hash);
    }

    let mut seen = HashSet::<String>::new();
    let mut final_order_hashes = Vec::<String>::with_capacity(current_hash_order.len());
    for hash in ordered_hashes {
        if by_hash.contains_key(hash) && seen.insert(hash.clone()) {
            final_order_hashes.push(hash.clone());
        }
    }
    for hash in current_hash_order {
        if seen.insert(hash.clone()) {
            final_order_hashes.push(hash);
        }
    }

    let mut update_stmt = conn.prepare_cached(
        "UPDATE media_entity
         SET collection_ordinal = ?1,
             updated_at = CURRENT_TIMESTAMP
         WHERE parent_collection_id = ?2 AND entity_id = ?3",
    )?;
    for (idx, hash) in final_order_hashes.iter().enumerate() {
        if let Some(member_entity_id) = by_hash.get(hash) {
            update_stmt.execute(params![idx as i64 + 1, collection_id, member_entity_id])?;
        }
    }

    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE media_entity SET updated_at = ?1
         WHERE entity_id = ?2 AND kind = 'collection'",
        params![now, collection_id],
    )?;

    // Reordering may change which member is first → update cover + cached metadata.
    sync_collection_aggregate_metadata(conn, collection_id)?;

    Ok(())
}

impl SqliteDatabase {
    pub async fn list_collections(&self) -> Result<Vec<CollectionRecord>, String> {
        self.with_read_conn(list_collections).await
    }

    pub async fn create_collection(
        &self,
        name: &str,
        description: Option<&str>,
        tags: &[String],
    ) -> Result<i64, String> {
        let n = name.to_string();
        let d = description.map(|v| v.to_string());
        let t = tags.to_vec();
        self.with_conn(move |conn| create_collection(conn, &n, d.as_deref(), &t))
            .await
    }

    pub async fn update_collection(
        &self,
        collection_id: i64,
        name: Option<&str>,
        description: Option<&str>,
        tags: Option<&[String]>,
        source_urls: Option<&[String]>,
    ) -> Result<(), String> {
        let n = name.map(|v| v.to_string());
        let d = description.map(|v| v.to_string());
        let t = tags.map(|v| v.to_vec());
        let s = source_urls.map(|v| v.to_vec());
        self.with_conn(move |conn| {
            update_collection(
                conn,
                collection_id,
                n.as_deref(),
                d.as_deref(),
                t.as_deref(),
                s.as_deref(),
            )
        })
        .await
    }

    pub async fn delete_collection(&self, collection_id: i64) -> Result<(), String> {
        self.with_conn(move |conn| delete_collection(conn, collection_id))
            .await
    }

    pub async fn set_collection_rating(
        &self,
        collection_id: i64,
        rating: Option<i64>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| set_collection_rating(conn, collection_id, rating))
            .await
    }

    pub async fn add_collection_member(
        &self,
        collection_id: i64,
        member_entity_id: i64,
        ordinal: Option<i64>,
    ) -> Result<(), String> {
        self.with_conn(move |conn| {
            add_collection_member(conn, collection_id, member_entity_id, ordinal)
        })
        .await
    }

    pub async fn add_collection_members_by_hashes(
        &self,
        collection_id: i64,
        hashes: &[String],
    ) -> Result<usize, String> {
        let hs = hashes.to_vec();
        let added = self
            .with_conn(move |conn| add_collection_members_by_hashes(conn, collection_id, &hs))
            .await?;
        if added > 0 {
            // Ensure folder bitmaps include the collection tile in folders where
            // member entities already existed.
            let folder_ids = self
                .with_read_conn(move |conn| {
                    let mut stmt = conn.prepare_cached(
                        "SELECT DISTINCT folder_id FROM folder_entity WHERE entity_id = ?1",
                    )?;
                    let rows = stmt.query_map([collection_id], |row| row.get::<_, i64>(0))?;
                    rows.collect::<rusqlite::Result<Vec<_>>>()
                })
                .await?;
            for folder_id in folder_ids {
                self.bitmaps
                    .insert(&BitmapKey::Folder(folder_id), collection_id as u32);
            }

            // Rebuild status/tag/sidebar/smart-folder derived artifacts so
            // counts and visibility reflect collection-member replacement.
            self.emit_compiler_event(CompilerEvent::StatusBatchChanged);
            self.emit_compiler_event(CompilerEvent::TagGraphChanged);
            self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id: 0 });
        }
        Ok(added)
    }

    pub async fn remove_collection_members_by_hashes(
        &self,
        collection_id: i64,
        hashes: &[String],
    ) -> Result<usize, String> {
        let hs = hashes.to_vec();
        let removed = self
            .with_conn(move |conn| remove_collection_members_by_hashes(conn, collection_id, &hs))
            .await?;
        if removed > 0 {
            self.emit_compiler_event(CompilerEvent::StatusBatchChanged);
            self.emit_compiler_event(CompilerEvent::TagGraphChanged);
            self.emit_compiler_event(CompilerEvent::FolderChanged { folder_id: 0 });
        }
        Ok(removed)
    }

    pub async fn set_collection_source_urls(
        &self,
        collection_id: i64,
        source_urls: &[String],
    ) -> Result<(), String> {
        let urls = source_urls.to_vec();
        self.with_conn(move |conn| set_collection_source_urls(conn, collection_id, &urls))
            .await
    }

    pub async fn reorder_collection_members_by_hashes(
        &self,
        collection_id: i64,
        ordered_hashes: &[String],
    ) -> Result<(), String> {
        let hashes = ordered_hashes.to_vec();
        self.with_conn(move |conn| {
            reorder_collection_members_by_hashes(conn, collection_id, &hashes)
        })
        .await
    }

    pub async fn list_collection_member_file_ids(
        &self,
        collection_id: i64,
    ) -> Result<Vec<i64>, String> {
        self.with_read_conn(move |conn| list_collection_member_file_ids(conn, collection_id))
            .await
    }

    pub async fn get_collection_summary(
        &self,
        collection_id: i64,
    ) -> Result<CollectionSummary, String> {
        self.with_read_conn(move |conn| get_collection_summary(conn, collection_id))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::files::{insert_file, NewFile};

    #[test]
    fn collection_crud_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        crate::sqlite::schema::apply_pragmas(&conn).unwrap();
        crate::sqlite::schema::init_schema(&conn).unwrap();

        let id = create_collection(
            &conn,
            "My Collection",
            Some("desc"),
            &["tag1".into(), "tag2".into(), "tag1".into()],
        )
        .unwrap();
        assert!(id > 0);

        let rows = list_collections(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "My Collection");
        assert_eq!(rows[0].description, "desc");
        assert_eq!(rows[0].tags, vec!["tag1".to_string(), "tag2".to_string()]);
        assert_eq!(rows[0].image_count, 0);
        assert!(rows[0].thumbnail_url.is_none());

        update_collection(
            &conn,
            id,
            Some("Renamed"),
            Some("updated"),
            Some(&["tag3".into(), "tag4".into()]),
            None,
        )
        .unwrap();
        let rows = list_collections(&conn).unwrap();
        assert_eq!(rows[0].name, "Renamed");
        assert_eq!(rows[0].description, "updated");
        assert_eq!(rows[0].tags, vec!["tag3".to_string(), "tag4".to_string()]);

        set_collection_rating(&conn, id, Some(4)).unwrap();
        let summary = get_collection_summary(&conn, id).unwrap();
        assert_eq!(summary.rating, Some(4));

        set_collection_rating(&conn, id, None).unwrap();
        let summary = get_collection_summary(&conn, id).unwrap();
        assert_eq!(summary.rating, None);

        delete_collection(&conn, id).unwrap();
        let rows = list_collections(&conn).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn add_collection_members_by_hashes_roundtrip() {
        let conn = Connection::open_in_memory().unwrap();
        crate::sqlite::schema::apply_pragmas(&conn).unwrap();
        crate::sqlite::schema::init_schema(&conn).unwrap();

        let now = chrono::Utc::now().to_rfc3339();
        let mk_file = |hash: &str, status: i64| NewFile {
            hash: hash.to_string(),
            name: Some(hash.to_string()),
            size: 1234,
            mime: "image/jpeg".to_string(),
            width: Some(100),
            height: Some(80),
            duration_ms: None,
            num_frames: None,
            has_audio: false,
            blurhash: None,
            status,
            imported_at: now.clone(),
            notes: None,
            source_urls_json: None,
            dominant_color_hex: None,
            dominant_palette_blob: None,
        };

        let file_a_id = insert_file(&conn, &mk_file("hash_a", 0)).unwrap();
        let file_b_id = insert_file(&conn, &mk_file("hash_b", 1)).unwrap();

        // Seed member metadata that should be merged into the collection entity.
        conn.execute(
            "UPDATE file SET rating = 3, source_urls_json = '[\"https://source/a\"]' WHERE file_id = ?1",
            [file_a_id],
        )
        .unwrap();
        conn.execute(
            "UPDATE file SET rating = 5, source_urls_json = '[\"https://source/b\", \"https://source/a\"]' WHERE file_id = ?1",
            [file_b_id],
        )
        .unwrap();

        let tag_a = crate::tags::db::get_or_create_tag(&conn, "artist", "alice").unwrap();
        let tag_b = crate::tags::db::get_or_create_tag(&conn, "", "landscape").unwrap();
        crate::tags::db::tag_entity(&conn, file_a_id, tag_a, "local").unwrap();
        crate::tags::db::tag_entity(&conn, file_b_id, tag_b, "local").unwrap();

        let folder_id = crate::folders::db::create_folder(
            &conn,
            &crate::folders::db::NewFolder {
                name: "F".to_string(),
                parent_id: None,
                icon: None,
                color: None,
                auto_tags: Vec::new(),
            },
        )
        .unwrap();
        crate::folders::db::add_entity_to_folder(&conn, folder_id, file_a_id).unwrap();

        let collection_id = create_collection(&conn, "C", None, &[]).unwrap();
        let added = add_collection_members_by_hashes(
            &conn,
            collection_id,
            &[
                "hash_a".to_string(),
                "hash_b".to_string(),
                "missing".to_string(),
                "hash_a".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(added, 2);

        let rows = list_collections(&conn).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].image_count, 2);
        assert_eq!(
            rows[0].thumbnail_url.as_deref(),
            Some("media://localhost/thumb/hash_a.jpg")
        );

        let summary = get_collection_summary(&conn, collection_id).unwrap();
        assert_eq!(summary.id, collection_id);
        assert_eq!(summary.image_count, 2);
        assert_eq!(summary.total_size_bytes, 2468);
        assert_eq!(summary.rating, Some(5));
        assert_eq!(summary.mime_breakdown.len(), 1);
        assert_eq!(summary.mime_breakdown[0].mime, "image/jpeg");
        assert_eq!(summary.mime_breakdown[0].count, 2);
        assert_eq!(
            summary.tags,
            vec!["artist:alice".to_string(), "landscape".to_string()]
        );
        assert_eq!(
            summary.source_urls,
            vec![
                "https://source/a".to_string(),
                "https://source/b".to_string()
            ]
        );
        let collection_status: i64 = conn
            .query_row(
                "SELECT status FROM media_entity WHERE entity_id = ?1",
                [collection_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(collection_status, 0);
        let has_collection_in_folder: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM folder_entity WHERE folder_id = ?1 AND entity_id = ?2",
                params![folder_id, collection_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(has_collection_in_folder, 1);

        set_collection_source_urls(
            &conn,
            collection_id,
            &[
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
                "https://example.com/a".to_string(),
            ],
        )
        .unwrap();
        let summary = get_collection_summary(&conn, collection_id).unwrap();
        assert_eq!(summary.source_urls.len(), 2);
        assert_eq!(summary.source_urls[0], "https://example.com/a");
        assert_eq!(summary.source_urls[1], "https://example.com/b");

        reorder_collection_members_by_hashes(
            &conn,
            collection_id,
            &["hash_b".to_string(), "hash_a".to_string()],
        )
        .unwrap();
        let ordered_file_ids = list_collection_member_file_ids(&conn, collection_id).unwrap();
        assert_eq!(ordered_file_ids.len(), 2);
    }
}
