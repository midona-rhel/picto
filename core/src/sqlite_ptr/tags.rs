//! PTR tag lookup and batch resolution.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

use super::PtrSqliteDatabase;

/// PBI-031: Counter for detected compact varint decode corruption events.
pub static COMPACT_CORRUPTION_COUNT: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtrTag {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtrTagRecord {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub file_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtrTagRelation {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtrResolvedTag {
    pub raw_ns: String,
    pub raw_st: String,
    pub display_ns: String,
    pub display_st: String,
}

// ─── Standalone functions ───

pub fn get_or_create_tag(
    conn: &Connection,
    namespace: &str,
    subtag: &str,
) -> rusqlite::Result<i64> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT tag_id FROM ptr_tag WHERE namespace = ?1 AND subtag = ?2",
            params![namespace, subtag],
            |row| row.get(0),
        )
        .optional()?;

    if let Some(id) = existing {
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO ptr_tag (namespace, subtag) VALUES (?1, ?2)",
        params![namespace, subtag],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn lookup_tags_for_hash(
    conn: &Connection,
    hash: &str,
) -> rusqlite::Result<Vec<PtrResolvedTag>> {
    let hash_blob = super::hash_to_blob(hash);
    let stub_id: Option<i64> = conn
        .query_row(
            "SELECT file_stub_id FROM ptr_file_stub WHERE hash = ?1",
            [&hash_blob],
            |row| row.get(0),
        )
        .optional()?;

    let stub_id = match stub_id {
        Some(id) => id,
        None => return Ok(Vec::new()),
    };

    let mut stmt = conn.prepare_cached(
        "SELECT t.namespace, t.subtag,
                COALESCE(td.display_ns, t.namespace),
                COALESCE(td.display_st, t.subtag)
         FROM ptr_file_tag ft
         JOIN ptr_tag t ON t.tag_id = ft.tag_id
         LEFT JOIN ptr_tag_display td ON td.tag_id = t.tag_id
         WHERE ft.file_stub_id = ?1",
    )?;

    let rows = stmt.query_map([stub_id], |row| {
        Ok(PtrResolvedTag {
            raw_ns: row.get(0)?,
            raw_st: row.get(1)?,
            display_ns: row.get(2)?,
            display_st: row.get(3)?,
        })
    })?;
    let mut resolved: Vec<PtrResolvedTag> = rows.collect::<Result<Vec<_>, _>>()?;
    if resolved.is_empty() {
        resolved = lookup_tags_for_hash_compact(conn, hash)?;
    }
    Ok(resolved)
}

pub fn batch_lookup_tags(
    conn: &Connection,
    hashes: &[String],
) -> rusqlite::Result<Vec<(String, Vec<PtrResolvedTag>)>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }

    let hash_blobs: Vec<Vec<u8>> = hashes.iter().map(|h| super::hash_to_blob(h)).collect();

    let placeholders = std::iter::repeat_n("?", hashes.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT fs.hash, t.namespace, t.subtag,
                COALESCE(td.display_ns, t.namespace),
                COALESCE(td.display_st, t.subtag)
         FROM ptr_file_stub fs
         JOIN ptr_file_tag ft ON ft.file_stub_id = fs.file_stub_id
         JOIN ptr_tag t ON t.tag_id = ft.tag_id
         LEFT JOIN ptr_tag_display td ON td.tag_id = t.tag_id
         WHERE fs.hash IN ({})
         ORDER BY fs.hash",
        placeholders
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(hash_blobs.iter()), |row| {
        Ok((
            super::blob_to_hash(&row.get::<_, Vec<u8>>(0)?),
            PtrResolvedTag {
                raw_ns: row.get(1)?,
                raw_st: row.get(2)?,
                display_ns: row.get(3)?,
                display_st: row.get(4)?,
            },
        ))
    })?;

    // Group results by hash
    let mut map: std::collections::HashMap<String, Vec<PtrResolvedTag>> =
        std::collections::HashMap::new();
    for row in rows {
        let (hash, tag) = row?;
        map.entry(hash).or_default().push(tag);
    }

    // Compact index fallback for missing hashes.
    for hash in hashes {
        if map.contains_key(hash) {
            continue;
        }
        let tags = lookup_tags_for_hash_compact(conn, hash)?;
        if !tags.is_empty() {
            map.insert(hash.clone(), tags);
        }
    }

    Ok(map.into_iter().collect())
}

fn lookup_tags_for_hash_compact(
    conn: &Connection,
    hash: &str,
) -> rusqlite::Result<Vec<PtrResolvedTag>> {
    let hash_blob = super::hash_to_blob(hash);
    let encoded: Option<Vec<u8>> = conn
        .query_row(
            "SELECT p.tag_ids_blob
             FROM ptr_compact_hash h
             JOIN ptr_compact_posting p ON p.service_hash_id = h.service_hash_id
             WHERE h.hash = ?1",
            [&hash_blob],
            |row| row.get(0),
        )
        .optional()?;
    let Some(blob) = encoded else {
        return Ok(Vec::new());
    };

    // PBI-031: Fail fast on corrupt compact blobs instead of silent partial decode.
    let tag_ids = match decode_delta_varint(&blob) {
        Ok(ids) => ids,
        Err(reason) => {
            COMPACT_CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                hash = hash,
                reason = reason,
                "ptr_compact varint corruption; returning empty"
            );
            return Ok(Vec::new());
        }
    };
    if tag_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut by_id: std::collections::HashMap<i64, PtrResolvedTag> =
        std::collections::HashMap::with_capacity(tag_ids.len());
    for chunk in tag_ids.chunks(512) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT t.service_tag_id,
                    t.namespace,
                    t.subtag,
                    COALESCE(td.display_ns, t.namespace),
                    COALESCE(td.display_st, t.subtag)
             FROM ptr_compact_tag t
             LEFT JOIN ptr_tag_display td ON td.tag_id = t.service_tag_id
             WHERE t.service_tag_id IN ({})",
            placeholders
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                PtrResolvedTag {
                    raw_ns: row.get(1)?,
                    raw_st: row.get(2)?,
                    display_ns: row.get(3)?,
                    display_st: row.get(4)?,
                },
            ))
        })?;
        for row in rows {
            let (id, tag) = row?;
            by_id.insert(id, tag);
        }
    }

    let mut out = Vec::with_capacity(tag_ids.len());
    for id in tag_ids {
        if let Some(tag) = by_id.remove(&id) {
            out.push(tag);
        }
    }
    Ok(out)
}

/// PBI-031: Returns `Err` on malformed bytes instead of silently returning partial results.
fn decode_delta_varint(bytes: &[u8]) -> Result<Vec<i64>, &'static str> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    let mut prev = 0u64;
    while idx < bytes.len() {
        let mut shift = 0u32;
        let mut value = 0u64;
        loop {
            if idx >= bytes.len() {
                return Err("truncated varint");
            }
            let b = bytes[idx];
            idx += 1;
            value |= ((b & 0x7f) as u64) << shift;
            if (b & 0x80) == 0 {
                break;
            }
            shift += 7;
            if shift > 63 {
                return Err("varint overflow");
            }
        }
        prev = prev.saturating_add(value);
        out.push(prev as i64);
    }
    Ok(out)
}

pub fn search_tags(conn: &Connection, query: &str, limit: i64) -> rusqlite::Result<Vec<PtrTag>> {
    // Use FTS5 if available, fall back to LIKE for compatibility.
    let fts_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='ptr_tag_fts'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if fts_exists {
        // FTS5 prefix search: append * for prefix matching
        let fts_query = format!("\"{}\"*", query.replace('"', "\"\""));
        let mut stmt = conn.prepare_cached(
            "SELECT t.tag_id, t.namespace, t.subtag
             FROM ptr_tag_fts fts
             JOIN ptr_tag t ON t.tag_id = fts.rowid
             WHERE ptr_tag_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts_query, limit], |row| {
            Ok(PtrTag {
                tag_id: row.get(0)?,
                namespace: row.get(1)?,
                subtag: row.get(2)?,
            })
        })?;
        return rows.collect();
    }

    // Fallback: LIKE scan
    let pattern = format!("%{query}%");
    let mut stmt = conn.prepare_cached(
        "SELECT tag_id, namespace, subtag FROM ptr_tag
         WHERE subtag LIKE ?1 OR namespace LIKE ?1
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![pattern, limit], |row| {
        Ok(PtrTag {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
        })
    })?;
    rows.collect()
}

/// Rebuild the FTS5 index from the ptr_tag table. Call after sync.
pub fn rebuild_fts_index(conn: &Connection) -> rusqlite::Result<()> {
    // Check if FTS5 table exists
    let fts_exists: bool = conn
        .query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='ptr_tag_fts'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if !fts_exists {
        return Ok(());
    }

    // Clear and repopulate
    conn.execute("DELETE FROM ptr_tag_fts", [])?;
    conn.execute_batch(
        "INSERT INTO ptr_tag_fts(rowid, combined_tag)
         SELECT tag_id, namespace || ':' || subtag FROM ptr_tag",
    )?;
    Ok(())
}

pub fn get_tag_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM ptr_tag", [], |row| row.get(0))
}

pub fn get_file_stub_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM ptr_file_stub", [], |row| row.get(0))
}

pub fn get_mapping_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM ptr_file_tag", [], |row| row.get(0))
}

// ─── Browse / paginate functions ───

pub fn get_namespace_summary(conn: &Connection) -> rusqlite::Result<Vec<(String, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT namespace, COUNT(*) as cnt FROM ptr_tag GROUP BY namespace ORDER BY cnt DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    rows.collect()
}

pub fn get_tags_paginated(
    conn: &Connection,
    namespace: Option<&str>,
    search: Option<&str>,
    cursor: Option<&str>,
    limit: i64,
) -> rusqlite::Result<Vec<PtrTagRecord>> {
    // With search: use FTS5
    if let Some(q) = search {
        if !q.is_empty() {
            let fts_query = format!("\"{}\"*", q.replace('"', ""));
            let (sql, use_ns) = match namespace {
                Some(_ns) => (
                    "SELECT t.tag_id, t.namespace, t.subtag, COALESCE(tc.file_count, 0)
                     FROM ptr_tag_fts fts
                     JOIN ptr_tag t ON t.tag_id = fts.rowid
                     LEFT JOIN ptr_tag_count tc ON tc.tag_id = t.tag_id
                     WHERE ptr_tag_fts MATCH ?1 AND t.namespace = ?2
                     ORDER BY t.subtag ASC, t.tag_id ASC
                     LIMIT ?3"
                        .to_string(),
                    true,
                ),
                None => (
                    "SELECT t.tag_id, t.namespace, t.subtag, COALESCE(tc.file_count, 0)
                     FROM ptr_tag_fts fts
                     JOIN ptr_tag t ON t.tag_id = fts.rowid
                     LEFT JOIN ptr_tag_count tc ON tc.tag_id = t.tag_id
                     WHERE ptr_tag_fts MATCH ?1
                     ORDER BY t.subtag ASC, t.tag_id ASC
                     LIMIT ?2"
                        .to_string(),
                    false,
                ),
            };
            let mut stmt = conn.prepare_cached(&sql)?;
            let map_row = |row: &rusqlite::Row| {
                Ok(PtrTagRecord {
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

    // Without search: keyset pagination
    let (cursor_subtag, cursor_id) = if let Some(c) = cursor {
        if let Some(pos) = c.find('\0') {
            (Some(c[..pos].to_string()), c[pos + 1..].parse::<i64>().ok())
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let has_cursor = cursor_subtag.is_some() && cursor_id.is_some();
    let has_ns = namespace.is_some();

    let sql = match (has_ns, has_cursor) {
        (true, true) => {
            "SELECT t.tag_id, t.namespace, t.subtag, COALESCE(tc.file_count, 0)
             FROM ptr_tag t
             LEFT JOIN ptr_tag_count tc ON tc.tag_id = t.tag_id
             WHERE t.namespace = ?1 AND (t.subtag, t.tag_id) > (?2, ?3)
             ORDER BY t.subtag ASC, t.tag_id ASC
             LIMIT ?4"
        }
        (true, false) => {
            "SELECT t.tag_id, t.namespace, t.subtag, COALESCE(tc.file_count, 0)
             FROM ptr_tag t
             LEFT JOIN ptr_tag_count tc ON tc.tag_id = t.tag_id
             WHERE t.namespace = ?1
             ORDER BY t.subtag ASC, t.tag_id ASC
             LIMIT ?2"
        }
        (false, true) => {
            "SELECT t.tag_id, t.namespace, t.subtag, COALESCE(tc.file_count, 0)
             FROM ptr_tag t
             LEFT JOIN ptr_tag_count tc ON tc.tag_id = t.tag_id
             WHERE (t.subtag, t.tag_id) > (?1, ?2)
             ORDER BY t.subtag ASC, t.tag_id ASC
             LIMIT ?3"
        }
        (false, false) => {
            "SELECT t.tag_id, t.namespace, t.subtag, COALESCE(tc.file_count, 0)
             FROM ptr_tag t
             LEFT JOIN ptr_tag_count tc ON tc.tag_id = t.tag_id
             ORDER BY t.subtag ASC, t.tag_id ASC
             LIMIT ?1"
        }
    };

    let mut stmt = conn.prepare_cached(sql)?;
    let map_row = |row: &rusqlite::Row| {
        Ok(PtrTagRecord {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            file_count: row.get(3)?,
        })
    };

    let rows: Vec<PtrTagRecord> = match (has_ns, has_cursor) {
        (true, true) => stmt
            .query_map(
                params![
                    namespace.unwrap(),
                    cursor_subtag.as_ref().unwrap(),
                    cursor_id.unwrap(),
                    limit
                ],
                map_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (true, false) => stmt
            .query_map(params![namespace.unwrap(), limit], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (false, true) => stmt
            .query_map(
                params![cursor_subtag.as_ref().unwrap(), cursor_id.unwrap(), limit],
                map_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        (false, false) => stmt
            .query_map(params![limit], map_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    };

    Ok(rows)
}

pub fn get_tag_siblings(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<PtrTagRelation>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.tag_id, t.namespace, t.subtag, 'to' as direction
         FROM ptr_tag_sibling s
         JOIN ptr_tag t ON t.tag_id = s.to_tag_id
         WHERE s.from_tag_id = ?1
         UNION
         SELECT t.tag_id, t.namespace, t.subtag, 'from' as direction
         FROM ptr_tag_sibling s
         JOIN ptr_tag t ON t.tag_id = s.from_tag_id
         WHERE s.to_tag_id = ?1",
    )?;
    let rows = stmt.query_map(params![tag_id], |row| {
        Ok(PtrTagRelation {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            relation: row.get(3)?,
        })
    })?;
    rows.collect()
}

pub fn get_tag_parents(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<PtrTagRelation>> {
    let mut stmt = conn.prepare_cached(
        "SELECT t.tag_id, t.namespace, t.subtag, 'parent' as relation
         FROM ptr_tag_parent p
         JOIN ptr_tag t ON t.tag_id = p.parent_tag_id
         WHERE p.child_tag_id = ?1
         UNION ALL
         SELECT t.tag_id, t.namespace, t.subtag, 'child' as relation
         FROM ptr_tag_parent p
         JOIN ptr_tag t ON t.tag_id = p.child_tag_id
         WHERE p.parent_tag_id = ?1",
    )?;
    let rows = stmt.query_map(params![tag_id], |row| {
        Ok(PtrTagRelation {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            relation: row.get(3)?,
        })
    })?;
    rows.collect()
}

// ─── High-level methods ───

impl PtrSqliteDatabase {
    pub async fn lookup_tags_for_hash(&self, hash: &str) -> Result<Vec<PtrResolvedTag>, String> {
        let h = hash.to_string();
        self.with_read_conn(move |conn| lookup_tags_for_hash(conn, &h))
            .await
    }

    pub async fn search_tags(&self, query: &str, limit: i64) -> Result<Vec<PtrTag>, String> {
        let q = query.to_string();
        self.with_read_conn(move |conn| search_tags(conn, &q, limit))
            .await
    }

    pub async fn rebuild_fts_index(&self) -> Result<(), String> {
        self.with_conn(rebuild_fts_index).await
    }

    pub async fn get_namespace_summary(&self) -> Result<Vec<(String, i64)>, String> {
        self.with_read_conn(get_namespace_summary).await
    }

    pub async fn get_tags_paginated(
        &self,
        namespace: Option<String>,
        search: Option<String>,
        cursor: Option<String>,
        limit: i64,
    ) -> Result<Vec<PtrTagRecord>, String> {
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

    pub async fn get_tag_siblings(&self, tag_id: i64) -> Result<Vec<PtrTagRelation>, String> {
        self.with_read_conn(move |conn| get_tag_siblings(conn, tag_id))
            .await
    }

    pub async fn get_tag_parents(&self, tag_id: i64) -> Result<Vec<PtrTagRelation>, String> {
        self.with_read_conn(move |conn| get_tag_parents(conn, tag_id))
            .await
    }

    pub async fn get_stats(&self) -> Result<PtrStats, String> {
        self.with_read_conn(|conn| {
            let sibling_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM ptr_tag_sibling", [], |row| row.get(0))
                .unwrap_or(0);
            let parent_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM ptr_tag_parent", [], |row| row.get(0))
                .unwrap_or(0);
            let sync_position: i64 = super::sync::get_cursor(conn).unwrap_or(-1);
            Ok(PtrStats {
                tag_count: get_tag_count(conn)?,
                file_stub_count: get_file_stub_count(conn)?,
                mapping_count: get_mapping_count(conn)?,
                sibling_count,
                parent_count,
                sync_position,
            })
        })
        .await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtrStats {
    pub tag_count: i64,
    pub file_stub_count: i64,
    pub mapping_count: i64,
    pub sibling_count: i64,
    pub parent_count: i64,
    pub sync_position: i64,
}
