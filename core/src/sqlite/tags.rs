//! Tag CRUD, file tagging, search (FTS5), sibling/parent operations.

use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use super::bitmaps::BitmapKey;
use super::compilers::CompilerEvent;
use super::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRecord {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub file_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagRelation {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub relation: String, // "to"/"from" for siblings, "parent"/"child" for parents
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagDisplay {
    pub namespace: String,
    pub subtag: String,
    pub display_ns: String,
    pub display_st: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTagInfo {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub source: String,
    pub display_ns: Option<String>,
    pub display_st: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamespaceNormalizationStats {
    pub tags_rewritten: i64,
    pub tags_merged: i64,
    pub affected_files: i64,
}

pub fn get_or_create_tag(
    conn: &Connection,
    namespace: &str,
    subtag: &str,
) -> rusqlite::Result<i64> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
            params![namespace, subtag],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)",
        params![namespace, subtag],
    )?;
    let tag_id = conn.last_insert_rowid();

    conn.execute(
        "INSERT INTO tag_fts (rowid, namespace, subtag) VALUES (?1, ?2, ?3)",
        params![tag_id, namespace, subtag],
    )?;

    Ok(tag_id)
}

pub fn tag_entity(
    conn: &Connection,
    entity_id: i64,
    tag_id: i64,
    source: &str,
) -> rusqlite::Result<bool> {
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO entity_tag_raw (entity_id, tag_id, source) VALUES (?1, ?2, ?3)",
        params![entity_id, tag_id, source],
    )?;
    if inserted > 0 {
        conn.execute(
            "UPDATE tag SET file_count = file_count + 1 WHERE tag_id = ?1",
            [tag_id],
        )?;
    }
    Ok(inserted > 0)
}

pub fn untag_entity(conn: &Connection, entity_id: i64, tag_id: i64) -> rusqlite::Result<bool> {
    let removed = conn.execute(
        "DELETE FROM entity_tag_raw WHERE entity_id = ?1 AND tag_id = ?2",
        params![entity_id, tag_id],
    )?;
    if removed > 0 {
        conn.execute(
            "UPDATE tag SET file_count = MAX(0, file_count - 1) WHERE tag_id = ?1",
            [tag_id],
        )?;
    }
    Ok(removed > 0)
}

pub fn get_entity_tags(conn: &Connection, entity_id: i64) -> rusqlite::Result<Vec<FileTagInfo>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.tag_id, t.namespace, t.subtag, etr.source,
                td.display_ns, td.display_st
         FROM entity_tag_raw etr
         JOIN tag t ON t.tag_id = etr.tag_id
         LEFT JOIN tag_display td ON td.tag_id = t.tag_id
         WHERE etr.entity_id = ?1
         ORDER BY t.namespace, t.subtag",
    )?;
    let rows = stmt.query_map([entity_id], |row| {
        Ok(FileTagInfo {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            source: row.get(3)?,
            display_ns: row.get(4)?,
            display_st: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn get_entities_tags(
    conn: &Connection,
    entity_ids: &[i64],
) -> rusqlite::Result<HashMap<i64, Vec<FileTagInfo>>> {
    if entity_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = std::iter::repeat_n("?", entity_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT etr.entity_id,
                t.tag_id, t.namespace, t.subtag, etr.source,
                td.display_ns, td.display_st
         FROM entity_tag_raw etr
         JOIN tag t ON t.tag_id = etr.tag_id
         LEFT JOIN tag_display td ON td.tag_id = t.tag_id
         WHERE etr.entity_id IN ({})
         ORDER BY etr.entity_id, t.namespace, t.subtag",
        placeholders
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(entity_ids.iter()), |row| {
        Ok((
            row.get::<_, i64>(0)?,
            FileTagInfo {
                tag_id: row.get(1)?,
                namespace: row.get(2)?,
                subtag: row.get(3)?,
                source: row.get(4)?,
                display_ns: row.get(5)?,
                display_st: row.get(6)?,
            },
        ))
    })?;

    let mut out: HashMap<i64, Vec<FileTagInfo>> = HashMap::new();
    for row in rows {
        let (entity_id, tag) = row?;
        out.entry(entity_id).or_default().push(tag);
    }
    Ok(out)
}

pub fn get_entity_implied_tags(
    conn: &Connection,
    entity_id: i64,
) -> rusqlite::Result<Vec<FileTagInfo>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.tag_id, t.namespace, t.subtag, 'implied' as source,
                td.display_ns, td.display_st
         FROM entity_tag_implied eti
         JOIN tag t ON t.tag_id = eti.tag_id
         LEFT JOIN tag_display td ON td.tag_id = t.tag_id
         WHERE eti.entity_id = ?1
         ORDER BY t.namespace, t.subtag",
    )?;
    let rows = stmt.query_map([entity_id], |row| {
        Ok(FileTagInfo {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            source: row.get(3)?,
            display_ns: row.get(4)?,
            display_st: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn search_tags(conn: &Connection, query: &str, limit: i64) -> rusqlite::Result<Vec<TagRecord>> {
    if query.is_empty() {
        // Return top tags by file_count
        let mut stmt = conn.prepare_cached(
            "SELECT tag_id, namespace, subtag, file_count FROM tag
             WHERE file_count > 0 ORDER BY file_count DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |row| {
            Ok(TagRecord {
                tag_id: row.get(0)?,
                namespace: row.get(1)?,
                subtag: row.get(2)?,
                file_count: row.get(3)?,
            })
        })?;
        return rows.collect();
    }

    // FTS5 search
    let fts_query = format!("{}*", query.replace('"', ""));
    let mut stmt = conn.prepare_cached(
        "SELECT t.tag_id, t.namespace, t.subtag, t.file_count
         FROM tag_fts fts
         JOIN tag t ON t.tag_id = fts.rowid
         WHERE tag_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![fts_query, limit], |row| {
        Ok(TagRecord {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            file_count: row.get(3)?,
        })
    })?;
    rows.collect()
}

// PBI-038: Paged tag query — supports offset for incremental loading.
pub fn search_tags_paged(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<TagRecord>> {
    if query.is_empty() {
        let mut stmt = conn.prepare_cached(
            "SELECT tag_id, namespace, subtag, file_count FROM tag
             WHERE file_count > 0 ORDER BY file_count DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(params![limit, offset], |row| {
            Ok(TagRecord {
                tag_id: row.get(0)?,
                namespace: row.get(1)?,
                subtag: row.get(2)?,
                file_count: row.get(3)?,
            })
        })?;
        return rows.collect();
    }

    let fts_query = format!("{}*", query.replace('"', ""));
    let mut stmt = conn.prepare_cached(
        "SELECT t.tag_id, t.namespace, t.subtag, t.file_count
         FROM tag_fts fts
         JOIN tag t ON t.tag_id = fts.rowid
         WHERE tag_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2 OFFSET ?3",
    )?;
    let rows = stmt.query_map(params![fts_query, limit, offset], |row| {
        Ok(TagRecord {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            file_count: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn get_all_tags_with_counts(conn: &Connection) -> rusqlite::Result<Vec<TagRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT tag_id, namespace, subtag, file_count FROM tag WHERE file_count > 0
         ORDER BY file_count DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(TagRecord {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            file_count: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn find_tag(conn: &Connection, namespace: &str, subtag: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
        params![namespace, subtag],
        |row| row.get(0),
    )
    .optional()
}

/// Add a tag sibling relationship.
pub fn add_sibling(
    conn: &Connection,
    from_tag_id: i64,
    to_tag_id: i64,
    source: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO tag_sibling (from_tag_id, to_tag_id, source)
         VALUES (?1, ?2, ?3)",
        params![from_tag_id, to_tag_id, source],
    )?;
    Ok(())
}

/// Remove a tag sibling relationship.
pub fn remove_sibling(conn: &Connection, from_tag_id: i64, source: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM tag_sibling WHERE from_tag_id = ?1 AND source = ?2",
        params![from_tag_id, source],
    )?;
    Ok(())
}

/// Add a tag parent relationship.
pub fn add_parent(
    conn: &Connection,
    child_tag_id: i64,
    parent_tag_id: i64,
    source: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO tag_parent (child_tag_id, parent_tag_id, source)
         VALUES (?1, ?2, ?3)",
        params![child_tag_id, parent_tag_id, source],
    )?;
    Ok(())
}

/// Remove a tag parent relationship.
pub fn remove_parent(
    conn: &Connection,
    child_tag_id: i64,
    parent_tag_id: i64,
    source: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM tag_parent WHERE child_tag_id = ?1 AND parent_tag_id = ?2 AND source = ?3",
        params![child_tag_id, parent_tag_id, source],
    )?;
    Ok(())
}

/// Get all siblings for a given tag (both directions).
pub fn get_siblings_for_tag(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<TagRelation>> {
    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, 'to' as direction
           FROM tag_sibling ts JOIN tag t ON ts.to_tag_id = t.tag_id
          WHERE ts.from_tag_id = ?1
         UNION
         SELECT t.tag_id, t.namespace, t.subtag, 'from' as direction
           FROM tag_sibling ts JOIN tag t ON ts.from_tag_id = t.tag_id
          WHERE ts.to_tag_id = ?1",
    )?;
    let results: Vec<TagRelation> = stmt
        .query_map(params![tag_id], |row| {
            Ok(TagRelation {
                tag_id: row.get(0)?,
                namespace: row.get(1)?,
                subtag: row.get(2)?,
                relation: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(results)
}

/// Get all parents and children for a given tag.
pub fn get_parents_for_tag(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<TagRelation>> {
    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, 'parent' as relation
           FROM tag_parent tp JOIN tag t ON tp.parent_tag_id = t.tag_id
          WHERE tp.child_tag_id = ?1
         UNION
         SELECT t.tag_id, t.namespace, t.subtag, 'child' as relation
           FROM tag_parent tp JOIN tag t ON tp.child_tag_id = t.tag_id
          WHERE tp.parent_tag_id = ?1",
    )?;
    let results: Vec<TagRelation> = stmt
        .query_map(params![tag_id], |row| {
            Ok(TagRelation {
                tag_id: row.get(0)?,
                namespace: row.get(1)?,
                subtag: row.get(2)?,
                relation: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(results)
}

/// Rebuild tag counts from entity_tag_raw (maintenance).
pub fn rebuild_tag_counts(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "UPDATE tag SET file_count = (
            SELECT COUNT(*) FROM entity_tag_raw WHERE entity_tag_raw.tag_id = tag.tag_id
        )",
    )?;
    Ok(())
}

/// Paginated tag query for the tag manager.
/// Supports optional namespace filter, FTS5 search, and keyset pagination.
pub fn get_tags_paginated(
    conn: &Connection,
    namespace: Option<&str>,
    search: Option<&str>,
    cursor: Option<&str>,
    limit: i64,
) -> rusqlite::Result<Vec<TagRecord>> {
    // With search: use FTS5
    if let Some(q) = search {
        if !q.is_empty() {
            let fts_query = format!("{}*", q.replace('"', ""));
            let (sql, use_ns) = match namespace {
                Some(_ns) => (
                    "SELECT t.tag_id, t.namespace, t.subtag, t.file_count
                     FROM tag_fts fts
                     JOIN tag t ON t.tag_id = fts.rowid
                     WHERE tag_fts MATCH ?1 AND t.file_count > 0 AND t.namespace = ?2
                     ORDER BY t.subtag ASC, t.tag_id ASC
                     LIMIT ?3"
                        .to_string(),
                    true,
                ),
                None => (
                    "SELECT t.tag_id, t.namespace, t.subtag, t.file_count
                     FROM tag_fts fts
                     JOIN tag t ON t.tag_id = fts.rowid
                     WHERE tag_fts MATCH ?1 AND t.file_count > 0
                     ORDER BY t.subtag ASC, t.tag_id ASC
                     LIMIT ?2"
                        .to_string(),
                    false,
                ),
            };
            let mut stmt = conn.prepare(&sql)?;
            let map_row = |row: &rusqlite::Row| -> rusqlite::Result<TagRecord> {
                Ok(TagRecord {
                    tag_id: row.get(0)?,
                    namespace: row.get(1)?,
                    subtag: row.get(2)?,
                    file_count: row.get(3)?,
                })
            };
            return if use_ns {
                stmt.query_map(params![fts_query, namespace.unwrap(), limit], map_row)?
                    .collect()
            } else {
                stmt.query_map(params![fts_query, limit], map_row)?
                    .collect()
            };
        }
    }

    // No search: plain query with keyset pagination
    let (cursor_subtag, cursor_id) = if let Some(c) = cursor {
        if let Some(sep) = c.find('\0') {
            let st = &c[..sep];
            let id: i64 = c[sep + 1..].parse().unwrap_or(0);
            (Some(st.to_string()), Some(id))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let has_cursor = cursor_subtag.is_some();
    let has_ns = namespace.is_some();

    let sql = match (has_ns, has_cursor) {
        (false, false) => "SELECT tag_id, namespace, subtag, file_count FROM tag
             WHERE file_count > 0
             ORDER BY subtag ASC, tag_id ASC LIMIT ?1"
            .to_string(),
        (false, true) => "SELECT tag_id, namespace, subtag, file_count FROM tag
             WHERE file_count > 0 AND (subtag, tag_id) > (?1, ?2)
             ORDER BY subtag ASC, tag_id ASC LIMIT ?3"
            .to_string(),
        (true, false) => "SELECT tag_id, namespace, subtag, file_count FROM tag
             WHERE file_count > 0 AND namespace = ?1
             ORDER BY subtag ASC, tag_id ASC LIMIT ?2"
            .to_string(),
        (true, true) => "SELECT tag_id, namespace, subtag, file_count FROM tag
             WHERE file_count > 0 AND namespace = ?1 AND (subtag, tag_id) > (?2, ?3)
             ORDER BY subtag ASC, tag_id ASC LIMIT ?4"
            .to_string(),
    };

    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<TagRecord> = match (has_ns, has_cursor) {
        (false, false) => stmt
            .query_map(params![limit], |row| {
                Ok(TagRecord {
                    tag_id: row.get(0)?,
                    namespace: row.get(1)?,
                    subtag: row.get(2)?,
                    file_count: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (false, true) => stmt
            .query_map(
                params![cursor_subtag.as_deref().unwrap(), cursor_id.unwrap(), limit],
                |row| {
                    Ok(TagRecord {
                        tag_id: row.get(0)?,
                        namespace: row.get(1)?,
                        subtag: row.get(2)?,
                        file_count: row.get(3)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (true, false) => stmt
            .query_map(params![namespace.unwrap(), limit], |row| {
                Ok(TagRecord {
                    tag_id: row.get(0)?,
                    namespace: row.get(1)?,
                    subtag: row.get(2)?,
                    file_count: row.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (true, true) => stmt
            .query_map(
                params![
                    namespace.unwrap(),
                    cursor_subtag.as_deref().unwrap(),
                    cursor_id.unwrap(),
                    limit
                ],
                |row| {
                    Ok(TagRecord {
                        tag_id: row.get(0)?,
                        namespace: row.get(1)?,
                        subtag: row.get(2)?,
                        file_count: row.get(3)?,
                    })
                },
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

/// Returns (namespace, tag_count) for all namespaces with active tags.
pub fn get_namespace_summary(conn: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT namespace, COUNT(*) as cnt FROM tag WHERE file_count > 0 GROUP BY namespace ORDER BY cnt DESC"
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.collect()
}

/// Rename a tag. If the target (namespace, subtag) already exists, merge into it.
pub fn rename_tag(
    conn: &Connection,
    tag_id: i64,
    new_tag_str: &str,
) -> rusqlite::Result<(Vec<i64>, Option<i64>)> {
    let (new_ns, new_st) = parse_tag_string(new_tag_str);

    // Check if target already exists
    let existing: Option<i64> = conn
        .query_row(
            "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
            params![new_ns, new_st],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(target_id) = existing {
        if target_id == tag_id {
            // Same tag, no-op
            return Ok((Vec::new(), None));
        }
        // Merge: move file mappings from old to target
        let mut stmt = conn.prepare("SELECT entity_id FROM entity_tag_raw WHERE tag_id = ?1")?;
        let entity_ids: Vec<i64> = stmt
            .query_map(params![tag_id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        conn.execute(
            "UPDATE OR IGNORE entity_tag_raw SET tag_id = ?1 WHERE tag_id = ?2",
            params![target_id, tag_id],
        )?;
        conn.execute(
            "DELETE FROM entity_tag_raw WHERE tag_id = ?1",
            params![tag_id],
        )?;
        // Recount both
        conn.execute(
            "UPDATE tag SET file_count = (SELECT COUNT(*) FROM entity_tag_raw WHERE tag_id = ?1) WHERE tag_id = ?1",
            params![target_id],
        )?;
        // Clean up old tag
        conn.execute(
            "DELETE FROM tag_sibling WHERE from_tag_id = ?1 OR to_tag_id = ?1",
            params![tag_id],
        )?;
        conn.execute(
            "DELETE FROM tag_parent WHERE child_tag_id = ?1 OR parent_tag_id = ?1",
            params![tag_id],
        )?;
        conn.execute(
            "DELETE FROM entity_tag_implied WHERE tag_id = ?1",
            params![tag_id],
        )?;
        conn.execute("DELETE FROM tag_fts WHERE rowid = ?1", params![tag_id])?;
        conn.execute("DELETE FROM tag WHERE tag_id = ?1", params![tag_id])?;
        Ok((entity_ids, Some(target_id)))
    } else {
        // Simple rename
        conn.execute(
            "UPDATE tag SET namespace = ?1, subtag = ?2 WHERE tag_id = ?3",
            params![new_ns, new_st, tag_id],
        )?;
        conn.execute("DELETE FROM tag_fts WHERE rowid = ?1", params![tag_id])?;
        conn.execute(
            "INSERT INTO tag_fts (rowid, namespace, subtag) VALUES (?1, ?2, ?3)",
            params![tag_id, new_ns, new_st],
        )?;
        Ok((Vec::new(), None))
    }
}

/// Delete a tag and all its associations.
/// Returns the list of affected entity_ids.
pub fn delete_tag(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<i64>> {
    // Collect affected entity_ids
    let mut stmt = conn.prepare("SELECT entity_id FROM entity_tag_raw WHERE tag_id = ?1")?;
    let entity_ids: Vec<i64> = stmt
        .query_map(params![tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    conn.execute(
        "DELETE FROM entity_tag_raw WHERE tag_id = ?1",
        params![tag_id],
    )?;
    conn.execute(
        "DELETE FROM entity_tag_implied WHERE tag_id = ?1",
        params![tag_id],
    )?;
    conn.execute(
        "DELETE FROM tag_sibling WHERE from_tag_id = ?1 OR to_tag_id = ?1",
        params![tag_id],
    )?;
    conn.execute(
        "DELETE FROM tag_parent WHERE child_tag_id = ?1 OR parent_tag_id = ?1",
        params![tag_id],
    )?;
    conn.execute("DELETE FROM tag_fts WHERE rowid = ?1", params![tag_id])?;
    conn.execute("DELETE FROM tag WHERE tag_id = ?1", params![tag_id])?;

    Ok(entity_ids)
}

/// Rewrites disallowed namespaced tags into unnamespaced literal tags.
/// Example: `http://x` stored as namespace=`http`, subtag=`//x` becomes
/// namespace=``, subtag=`http://x`.
pub fn normalize_disallowed_namespaces(
    conn: &Connection,
) -> rusqlite::Result<NamespaceNormalizationStats> {
    let mut stmt = conn.prepare(
        "SELECT tag_id, namespace, subtag FROM tag WHERE namespace <> '' ORDER BY tag_id ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    let mut candidates: Vec<(i64, String, String)> = Vec::new();
    for row in rows {
        let (tag_id, namespace, subtag) = row?;
        if !crate::tags::is_ingest_namespace_allowed(&namespace) {
            candidates.push((tag_id, namespace, subtag));
        }
    }

    let mut tags_rewritten = 0_i64;
    let mut tags_merged = 0_i64;
    let mut affected_files: HashSet<i64> = HashSet::new();
    for (tag_id, namespace, subtag) in candidates {
        let target_literal = format!("{}:{}", namespace, subtag);
        let new_tag = crate::tags::combine_tag("", &target_literal);
        let (file_ids, merged_into) = rename_tag(conn, tag_id, &new_tag)?;
        tags_rewritten += 1;
        if merged_into.is_some() {
            tags_merged += 1;
        }
        for fid in file_ids {
            affected_files.insert(fid);
        }
    }

    Ok(NamespaceNormalizationStats {
        tags_rewritten,
        tags_merged,
        affected_files: affected_files.len() as i64,
    })
}

/// Parse "namespace:subtag" string into (namespace, subtag).
/// Delegates splitting to `crate::tags::split_tag` so namespace validation
/// (rejecting emoticon prefixes like `>` in `>:(`) is applied consistently.
pub fn parse_tag_string(tag_str: &str) -> (String, String) {
    let (ns, st) = crate::tags::split_tag(tag_str);
    (ns.trim().to_lowercase(), st.trim().to_lowercase())
}

impl SqliteDatabase {
    pub async fn get_or_create_tag(&self, namespace: &str, subtag: &str) -> Result<i64, String> {
        let ns = namespace.to_string();
        let st = subtag.to_string();
        self.with_conn(move |conn| get_or_create_tag(conn, &ns, &st))
            .await
    }

    pub async fn tag_entity(
        &self,
        hash: &str,
        namespace: &str,
        subtag: &str,
        source: &str,
    ) -> Result<bool, String> {
        let entity_id = self.resolve_hash(hash).await?;
        let ns = namespace.to_string();
        let st = subtag.to_string();
        let src = source.to_string();
        let tag_id = self
            .with_conn(move |conn| {
                let tag_id = get_or_create_tag(conn, &ns, &st)?;
                tag_entity(conn, entity_id, tag_id, &src)?;
                Ok(tag_id)
            })
            .await?;

        // Update tag bitmap
        self.bitmaps
            .insert(&BitmapKey::Tag(tag_id), entity_id as u32);
        self.emit_compiler_event(CompilerEvent::FileTagsChanged { file_id: entity_id });
        self.emit_compiler_event(CompilerEvent::TagChanged { tag_id });
        Ok(true)
    }

    pub async fn untag_entity(
        &self,
        hash: &str,
        namespace: &str,
        subtag: &str,
    ) -> Result<bool, String> {
        let entity_id = self.resolve_hash(hash).await?;
        let ns = namespace.to_string();
        let st = subtag.to_string();
        let result = self
            .with_conn(move |conn| {
                let tag_id = find_tag(conn, &ns, &st)?;
                if let Some(tid) = tag_id {
                    let removed = untag_entity(conn, entity_id, tid)?;
                    Ok(Some((tid, removed)))
                } else {
                    Ok(None)
                }
            })
            .await?;

        if let Some((tag_id, true)) = result {
            self.bitmaps
                .remove(&BitmapKey::Tag(tag_id), entity_id as u32);
            self.emit_compiler_event(CompilerEvent::FileTagsChanged { file_id: entity_id });
            self.emit_compiler_event(CompilerEvent::TagChanged { tag_id });
        }

        Ok(result.map(|(_, r)| r).unwrap_or(false))
    }

    pub async fn get_entity_tags(&self, hash: &str) -> Result<Vec<FileTagInfo>, String> {
        let entity_id = self.resolve_hash(hash).await?;
        self.with_read_conn(move |conn| get_entity_tags(conn, entity_id))
            .await
    }

    pub async fn search_tags(&self, query: &str, limit: i64) -> Result<Vec<TagRecord>, String> {
        let q = query.to_string();
        self.with_read_conn(move |conn| search_tags(conn, &q, limit))
            .await
    }

    // PBI-038: Paged tag search.
    pub async fn search_tags_paged(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<TagRecord>, String> {
        let q = query.to_string();
        self.with_read_conn(move |conn| search_tags_paged(conn, &q, limit, offset))
            .await
    }

    pub async fn get_all_tags_with_counts(&self) -> Result<Vec<TagRecord>, String> {
        self.with_read_conn(get_all_tags_with_counts).await
    }

    /// Add tags by string ("namespace:subtag") to a file.
    pub async fn add_tags_by_strings(
        &self,
        hash: &str,
        tag_strings: &[String],
    ) -> Result<(), String> {
        for tag_str in tag_strings {
            let (ns, st) = parse_tag_string(tag_str);
            self.tag_entity(hash, &ns, &st, "local").await?;
        }
        Ok(())
    }

    /// Remove tags by string ("namespace:subtag") from a file.
    pub async fn remove_tags_by_strings(
        &self,
        hash: &str,
        tag_strings: &[String],
    ) -> Result<(), String> {
        for tag_str in tag_strings {
            let (ns, st) = parse_tag_string(tag_str);
            self.untag_entity(hash, &ns, &st).await?;
        }
        Ok(())
    }

    /// Batch add tags to multiple files.
    pub async fn add_tags_batch(
        &self,
        hashes: &[String],
        tag_strings: &[String],
    ) -> Result<(), String> {
        for hash in hashes {
            self.add_tags_by_strings(hash, tag_strings).await?;
        }
        Ok(())
    }

    /// Batch remove tags from multiple files.
    pub async fn remove_tags_batch(
        &self,
        hashes: &[String],
        tag_strings: &[String],
    ) -> Result<(), String> {
        for hash in hashes {
            self.remove_tags_by_strings(hash, tag_strings).await?;
        }
        Ok(())
    }

    /// Find files by tags using bitmap intersection.
    pub async fn find_files_by_tags(
        &self,
        tag_strings: &[String],
        match_all: bool,
    ) -> Result<Vec<String>, String> {
        if tag_strings.is_empty() {
            return Ok(Vec::new());
        }

        // Resolve tag strings to tag_ids
        let mut tag_ids = Vec::new();
        for ts in tag_strings {
            let (ns, st) = parse_tag_string(ts);
            let ns_c = ns.clone();
            let st_c = st.clone();
            if let Some(tid) = self
                .with_read_conn(move |conn| find_tag(conn, &ns_c, &st_c))
                .await?
            {
                tag_ids.push(tid);
            } else if match_all {
                // If match_all and a tag doesn't exist, result is empty
                return Ok(Vec::new());
            }
        }

        if tag_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Use bitmap ops
        let all_active = self.bitmaps.get(&BitmapKey::AllActive);

        let result = if match_all {
            let mut result = self.bitmaps.get(&BitmapKey::EffectiveTag(tag_ids[0]));
            for &tid in &tag_ids[1..] {
                result &= &self.bitmaps.get(&BitmapKey::EffectiveTag(tid));
            }
            result &= &all_active;
            result
        } else {
            let mut result = roaring::RoaringBitmap::new();
            for &tid in &tag_ids {
                result |= &self.bitmaps.get(&BitmapKey::EffectiveTag(tid));
            }
            result &= &all_active;
            result
        };

        // Convert file_ids to hashes
        let file_ids: Vec<i64> = result.iter().map(|id| id as i64).collect();
        let resolved = self.resolve_ids_batch(&file_ids).await?;
        let hashes: Vec<String> = resolved.into_iter().map(|(_, h)| h).collect();

        Ok(hashes)
    }

    pub async fn add_sibling(
        &self,
        from_ns: &str,
        from_st: &str,
        to_ns: &str,
        to_st: &str,
        source: &str,
    ) -> Result<(), String> {
        let fns = from_ns.to_string();
        let fst = from_st.to_string();
        let tns = to_ns.to_string();
        let tst = to_st.to_string();
        let src = source.to_string();
        self.with_conn(move |conn| {
            let from_id = get_or_create_tag(conn, &fns, &fst)?;
            let to_id = get_or_create_tag(conn, &tns, &tst)?;
            add_sibling(conn, from_id, to_id, &src)
        })
        .await?;
        self.emit_compiler_event(CompilerEvent::TagGraphChanged);
        Ok(())
    }

    pub async fn remove_sibling(
        &self,
        from_ns: &str,
        from_st: &str,
        source: &str,
    ) -> Result<(), String> {
        let fns = from_ns.to_string();
        let fst = from_st.to_string();
        let src = source.to_string();
        self.with_conn(move |conn| {
            if let Some(from_id) = find_tag(conn, &fns, &fst)? {
                remove_sibling(conn, from_id, &src)?;
            }
            Ok(())
        })
        .await?;
        self.emit_compiler_event(CompilerEvent::TagGraphChanged);
        Ok(())
    }

    pub async fn add_parent(
        &self,
        child_ns: &str,
        child_st: &str,
        parent_ns: &str,
        parent_st: &str,
        source: &str,
    ) -> Result<(), String> {
        let cns = child_ns.to_string();
        let cst = child_st.to_string();
        let pns = parent_ns.to_string();
        let pst = parent_st.to_string();
        let src = source.to_string();
        self.with_conn(move |conn| {
            let child_id = get_or_create_tag(conn, &cns, &cst)?;
            let parent_id = get_or_create_tag(conn, &pns, &pst)?;
            add_parent(conn, child_id, parent_id, &src)
        })
        .await?;
        self.emit_compiler_event(CompilerEvent::TagGraphChanged);
        Ok(())
    }

    pub async fn remove_parent(
        &self,
        child_ns: &str,
        child_st: &str,
        parent_ns: &str,
        parent_st: &str,
        source: &str,
    ) -> Result<(), String> {
        let cns = child_ns.to_string();
        let cst = child_st.to_string();
        let pns = parent_ns.to_string();
        let pst = parent_st.to_string();
        let src = source.to_string();
        self.with_conn(move |conn| {
            if let (Some(child_id), Some(parent_id)) =
                (find_tag(conn, &cns, &cst)?, find_tag(conn, &pns, &pst)?)
            {
                remove_parent(conn, child_id, parent_id, &src)?;
            }
            Ok(())
        })
        .await?;
        self.emit_compiler_event(CompilerEvent::TagGraphChanged);
        Ok(())
    }

    pub async fn get_siblings_for_tag(&self, tag_id: i64) -> Result<Vec<TagRelation>, String> {
        self.with_read_conn(move |conn| get_siblings_for_tag(conn, tag_id))
            .await
    }

    pub async fn get_parents_for_tag(&self, tag_id: i64) -> Result<Vec<TagRelation>, String> {
        self.with_read_conn(move |conn| get_parents_for_tag(conn, tag_id))
            .await
    }

    pub async fn rebuild_tag_counts(&self) -> Result<(), String> {
        self.with_conn(rebuild_tag_counts).await
    }

    pub async fn get_tags_paginated(
        &self,
        namespace: Option<String>,
        search: Option<String>,
        cursor: Option<String>,
        limit: i64,
    ) -> Result<Vec<TagRecord>, String> {
        self.with_read_conn(move |conn| {
            get_tags_paginated(
                conn,
                namespace.as_deref(),
                search.as_deref(),
                cursor.as_deref(),
                limit,
            )
        })
        .await
    }

    pub async fn get_namespace_summary(&self) -> Result<Vec<(String, i64)>, String> {
        self.with_read_conn(get_namespace_summary).await
    }

    pub async fn rename_tag_by_id(
        &self,
        tag_id: i64,
        new_tag_str: &str,
    ) -> Result<(Vec<i64>, Option<i64>), String> {
        let new_str = new_tag_str.to_string();
        let (affected_entity_ids, merged_into) = self
            .with_conn(move |conn| rename_tag(conn, tag_id, &new_str))
            .await?;

        self.emit_compiler_event(CompilerEvent::TagChanged { tag_id });
        if let Some(target_id) = merged_into {
            self.emit_compiler_event(CompilerEvent::TagChanged { tag_id: target_id });
        }
        for &eid in &affected_entity_ids {
            self.emit_compiler_event(CompilerEvent::FileTagsChanged { file_id: eid });
        }
        Ok((affected_entity_ids, merged_into))
    }

    pub async fn delete_tag_by_id(&self, tag_id: i64) -> Result<Vec<i64>, String> {
        let affected_entity_ids = self.with_conn(move |conn| delete_tag(conn, tag_id)).await?;

        self.emit_compiler_event(CompilerEvent::TagChanged { tag_id });
        for &eid in &affected_entity_ids {
            self.emit_compiler_event(CompilerEvent::FileTagsChanged { file_id: eid });
        }
        Ok(affected_entity_ids)
    }

    pub async fn normalize_disallowed_namespaces(
        &self,
    ) -> Result<NamespaceNormalizationStats, String> {
        let stats = self.with_conn(normalize_disallowed_namespaces).await?;
        if stats.tags_rewritten > 0 {
            self.emit_compiler_event(CompilerEvent::TagGraphChanged);
        }
        Ok(stats)
    }

    /// Batch add tags to entities by entity_ids (bypasses hash resolution).
    pub async fn add_tags_batch_by_entity_ids(
        &self,
        entity_ids: Vec<i64>,
        tag_strings: Vec<String>,
        source: String,
    ) -> Result<(), String> {
        if entity_ids.is_empty() || tag_strings.is_empty() {
            return Ok(());
        }

        // Parse tag strings upfront
        let parsed: Vec<(String, String)> =
            tag_strings.iter().map(|s| parse_tag_string(s)).collect();

        let bitmaps = self.bitmaps.clone();
        let compiler_tx = self.compiler_tx.clone();

        self.with_conn(move |conn| {
            for (ns, st) in &parsed {
                let tag_id = get_or_create_tag(conn, ns, st)?;

                // Multi-row INSERT OR IGNORE for all entity_ids at once
                let chunk_size = 500; // SQLite variable limit / 3 params per row
                let mut total_inserted = 0usize;
                for chunk in entity_ids.chunks(chunk_size) {
                    let placeholders = std::iter::repeat_n("(?,?,?)", chunk.len())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!(
                        "INSERT OR IGNORE INTO entity_tag_raw (entity_id, tag_id, source) VALUES {}",
                        placeholders
                    );
                    let mut flat_params: Vec<Box<dyn rusqlite::types::ToSql>> =
                        Vec::with_capacity(chunk.len() * 3);
                    for &eid in chunk {
                        flat_params.push(Box::new(eid));
                        flat_params.push(Box::new(tag_id));
                        flat_params.push(Box::new(source.clone()));
                    }
                    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                        flat_params.iter().map(|p| p.as_ref()).collect();
                    conn.execute(&sql, param_refs.as_slice())?;
                    total_inserted += conn.changes() as usize;
                }

                if total_inserted > 0 {
                    conn.execute(
                        "UPDATE tag SET file_count = file_count + ?1 WHERE tag_id = ?2",
                        params![total_inserted as i64, tag_id],
                    )?;
                    // Update bitmaps for all entity_ids (conservative: add all, bitmap dedup is free)
                    for &eid in &entity_ids {
                        bitmaps.insert(&BitmapKey::Tag(tag_id), eid as u32);
                    }
                }
                let _ = compiler_tx.send(CompilerEvent::TagChanged { tag_id });
            }
            for &eid in &entity_ids {
                let _ = compiler_tx.send(CompilerEvent::FileTagsChanged { file_id: eid });
            }
            Ok(())
        })
        .await
    }

    /// Batch remove tags from entities by entity_ids (bypasses hash resolution).
    pub async fn remove_tags_batch_by_entity_ids(
        &self,
        entity_ids: Vec<i64>,
        tag_strings: Vec<String>,
    ) -> Result<(), String> {
        if entity_ids.is_empty() || tag_strings.is_empty() {
            return Ok(());
        }

        let parsed: Vec<(String, String)> =
            tag_strings.iter().map(|s| parse_tag_string(s)).collect();

        let bitmaps = self.bitmaps.clone();
        let compiler_tx = self.compiler_tx.clone();

        self.with_conn(move |conn| {
            for (ns, st) in &parsed {
                if let Some(tag_id) = find_tag(conn, ns, st)? {
                    // Chunked batch DELETE instead of per-row DELETE.
                    let mut total_removed = 0i64;
                    for chunk in entity_ids.chunks(500) {
                        let placeholders: String = (0..chunk.len())
                            .map(|i| format!("?{}", i + 2))
                            .collect::<Vec<_>>()
                            .join(",");
                        let sql = format!(
                            "DELETE FROM entity_tag_raw WHERE tag_id = ?1 AND entity_id IN ({placeholders})"
                        );
                        let mut param_values: Vec<rusqlite::types::Value> =
                            Vec::with_capacity(chunk.len() + 1);
                        param_values.push(rusqlite::types::Value::Integer(tag_id));
                        for &eid in chunk {
                            param_values.push(rusqlite::types::Value::Integer(eid));
                        }
                        let removed = conn.execute(
                            &sql,
                            rusqlite::params_from_iter(param_values.iter()),
                        )?;
                        total_removed += removed as i64;
                    }
                    // Single count update per tag instead of per-row.
                    if total_removed > 0 {
                        conn.execute(
                            "UPDATE tag SET file_count = MAX(0, file_count - ?1) WHERE tag_id = ?2",
                            params![total_removed, tag_id],
                        )?;
                    }
                    // Bitmap removals are per-element (Roaring API constraint).
                    for &eid in &entity_ids {
                        bitmaps.remove(&BitmapKey::Tag(tag_id), eid as u32);
                    }
                    let _ = compiler_tx.send(CompilerEvent::TagChanged { tag_id });
                }
            }
            for &eid in &entity_ids {
                let _ = compiler_tx.send(CompilerEvent::FileTagsChanged { file_id: eid });
            }
            Ok(())
        })
        .await
    }
}
