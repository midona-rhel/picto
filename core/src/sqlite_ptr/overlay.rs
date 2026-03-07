//! Compiled PTR overlay — hash → fully resolved PTR tags.
//!
//! No live graph queries at read time. The overlay is rebuilt
//! by the PTR overlay compiler after each sync.

use super::tags::PtrResolvedTag;
use super::PtrSqliteDatabase;
use rusqlite::{params, params_from_iter, Connection};
use std::sync::atomic::{AtomicU64, Ordering};

/// PBI-031: Counter for detected overlay JSON corruption events.
pub static OVERLAY_CORRUPTION_COUNT: AtomicU64 = AtomicU64::new(0);

// ─── Standalone functions ───

/// Get pre-compiled overlay for a hash.
pub fn get_overlay(conn: &Connection, hash: &str) -> rusqlite::Result<Option<Vec<PtrResolvedTag>>> {
    let hash_blob = super::hash_to_blob(hash);
    let json: Option<String> = conn
        .query_row(
            "SELECT resolved_json FROM ptr_overlay WHERE hash = ?1",
            [&hash_blob],
            |row| row.get(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;

    match json {
        Some(j) => match serde_json::from_str::<Vec<PtrResolvedTag>>(&j) {
            Ok(tags) => Ok(Some(tags)),
            Err(e) => {
                // PBI-031: Corrupt overlay — quarantine by deleting row.
                OVERLAY_CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    hash = hash,
                    error = %e,
                    "ptr_overlay JSON corruption; quarantining row"
                );
                let _ = conn.execute("DELETE FROM ptr_overlay WHERE hash = ?1", [&hash_blob]);
                Ok(None)
            }
        },
        None => Ok(None),
    }
}

/// Batch get overlays for multiple hashes.
pub fn batch_get_overlay(
    conn: &Connection,
    hashes: &[String],
) -> rusqlite::Result<Vec<(String, Vec<PtrResolvedTag>)>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }

    let hash_blobs: Vec<Vec<u8>> = hashes.iter().map(|h| super::hash_to_blob(h)).collect();

    // SQLite max parameter count is high enough for our bounded API batch sizes (<= 200).
    let placeholders = std::iter::repeat_n("?", hashes.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT hash, resolved_json
         FROM ptr_overlay
         WHERE hash IN ({})",
        placeholders
    );

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(hash_blobs.iter()), |row| {
        let hash = super::blob_to_hash(&row.get::<_, Vec<u8>>(0)?);
        let json: String = row.get(1)?;
        Ok((hash, json))
    })?;

    // PBI-031: Parse JSON outside query_map so we can log and skip corrupt entries.
    let mut results = Vec::new();
    for row in rows {
        let (hash, json) = row?;
        match serde_json::from_str::<Vec<PtrResolvedTag>>(&json) {
            Ok(tags) => results.push((hash, tags)),
            Err(e) => {
                OVERLAY_CORRUPTION_COUNT.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    hash = hash,
                    error = %e,
                    "ptr_overlay JSON corruption in batch; skipping entry"
                );
            }
        }
    }
    Ok(results)
}

/// SQL to rebuild the tag display cache (sibling resolution).
const REBUILD_DISPLAY_SQL: &str =
    "INSERT OR REPLACE INTO ptr_tag_display (tag_id, display_ns, display_st)
     SELECT t.tag_id,
            COALESCE(sibling_target.namespace, t.namespace),
            COALESCE(sibling_target.subtag, t.subtag)
     FROM ptr_tag t
     LEFT JOIN ptr_tag_sibling ts ON ts.from_tag_id = t.tag_id
     LEFT JOIN ptr_tag sibling_target ON sibling_target.tag_id = ts.to_tag_id";

/// SQL to resolve tags for a single file stub (using display cache).
const RESOLVE_TAGS_SQL: &str = "SELECT t.namespace, t.subtag,
            COALESCE(td.display_ns, t.namespace),
            COALESCE(td.display_st, t.subtag)
     FROM ptr_file_tag ft
     JOIN ptr_tag t ON t.tag_id = ft.tag_id
     LEFT JOIN ptr_tag_display td ON td.tag_id = t.tag_id
     WHERE ft.file_stub_id = ?1";

/// Rebuild the entire overlay from raw PTR data.
/// Full rebuild — use only for manual maintenance or initial import.
pub fn rebuild_overlay(conn: &mut Connection, epoch: i64) -> rusqlite::Result<usize> {
    let tx = conn.transaction()?;

    // Clear old overlay + negative cache
    tx.execute("DELETE FROM ptr_overlay", [])?;
    tx.execute("DELETE FROM ptr_negative_cache", [])?;

    // Rebuild display cache (sibling resolution)
    tx.execute("DELETE FROM ptr_tag_display", [])?;
    tx.execute_batch(REBUILD_DISPLAY_SQL)?;

    // Build overlay: for each file stub, resolve all tags
    let mut count = 0;
    let mut stub_stmt = tx.prepare("SELECT file_stub_id, hash FROM ptr_file_stub")?;
    let stubs: Vec<(i64, Vec<u8>)> = stub_stmt
        .query_map([], |row| Ok((row.get(0)?, row.get::<_, Vec<u8>>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stub_stmt);

    let mut tag_stmt = tx.prepare(RESOLVE_TAGS_SQL)?;
    let mut insert_stmt = tx.prepare(
        "INSERT OR REPLACE INTO ptr_overlay (hash, resolved_json, epoch) VALUES (?1, ?2, ?3)",
    )?;

    for (stub_id, hash_blob) in &stubs {
        let tags: Vec<PtrResolvedTag> = tag_stmt
            .query_map([stub_id], |row| {
                Ok(PtrResolvedTag {
                    raw_ns: row.get(0)?,
                    raw_st: row.get(1)?,
                    display_ns: row.get(2)?,
                    display_st: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        if !tags.is_empty() {
            let json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
            insert_stmt.execute(params![hash_blob, json, epoch])?;
            count += 1;
        }
    }
    drop(tag_stmt);
    drop(insert_stmt);

    // Rebuild tag counts
    tx.execute("DELETE FROM ptr_tag_count", [])?;
    tx.execute_batch(
        "INSERT INTO ptr_tag_count (tag_id, file_count)
         SELECT tag_id, COUNT(*) FROM ptr_file_tag GROUP BY tag_id",
    )?;

    tx.commit()?;
    Ok(count)
}

/// Incremental overlay rebuild — only for the given hash hex strings.
/// Rebuilds display cache fully (cheap), then updates overlay + counts
/// only for the affected file stubs.
pub fn rebuild_overlay_for_hashes(
    conn: &mut Connection,
    hashes: &[String],
    epoch: i64,
) -> rusqlite::Result<usize> {
    if hashes.is_empty() {
        return Ok(0);
    }

    let tx = conn.transaction()?;

    // Rebuild display cache fully — it's global (sibling changes affect all tags)
    tx.execute("DELETE FROM ptr_tag_display", [])?;
    tx.execute_batch(REBUILD_DISPLAY_SQL)?;

    // PBI-029: Batch-join approach — insert affected hashes into a temp table
    // and join against it, avoiding per-hash lookups.
    let hash_blobs: Vec<Vec<u8>> = hashes.iter().map(|h| super::hash_to_blob(h)).collect();

    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _batch_hash (hash BLOB PRIMARY KEY) WITHOUT ROWID;
         DELETE FROM _batch_hash;",
    )?;
    {
        let mut insert_batch =
            tx.prepare("INSERT OR IGNORE INTO _batch_hash (hash) VALUES (?1)")?;
        for blob in &hash_blobs {
            insert_batch.execute([blob])?;
        }
    }

    // Batch-resolve all affected stubs in one query
    let mut tag_stmt = tx.prepare(RESOLVE_TAGS_SQL)?;
    let mut upsert_stmt = tx.prepare(
        "INSERT OR REPLACE INTO ptr_overlay (hash, resolved_json, epoch) VALUES (?1, ?2, ?3)",
    )?;

    let mut stubs: Vec<(i64, Vec<u8>)> = Vec::new();
    {
        let mut batch_stub_stmt = tx.prepare(
            "SELECT fs.file_stub_id, fs.hash
             FROM ptr_file_stub fs
             JOIN _batch_hash bh ON bh.hash = fs.hash",
        )?;
        let rows = batch_stub_stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Vec<u8>>(1)?))
        })?;
        for row in rows {
            stubs.push(row?);
        }
    }

    // Remove overlays for hashes NOT in ptr_file_stub
    tx.execute_batch(
        "DELETE FROM ptr_overlay WHERE hash IN (
             SELECT bh.hash FROM _batch_hash bh
             LEFT JOIN ptr_file_stub fs ON fs.hash = bh.hash
             WHERE fs.file_stub_id IS NULL
         )",
    )?;

    let mut count = 0;
    for (stub_id, blob) in &stubs {
        let tags: Vec<PtrResolvedTag> = tag_stmt
            .query_map([stub_id], |row| {
                Ok(PtrResolvedTag {
                    raw_ns: row.get(0)?,
                    raw_st: row.get(1)?,
                    display_ns: row.get(2)?,
                    display_st: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        if tags.is_empty() {
            tx.execute("DELETE FROM ptr_overlay WHERE hash = ?1", [blob])?;
        } else {
            let json = serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into());
            upsert_stmt.execute(params![blob, json, epoch])?;
            count += 1;
        }
    }
    drop(tag_stmt);
    drop(upsert_stmt);

    // Batch-remove stale negative cache entries
    tx.execute_batch(
        "DELETE FROM ptr_negative_cache WHERE hash IN (SELECT hash FROM _batch_hash)",
    )?;

    // PBI-029: Incremental tag count update — only recompute counts for affected tag_ids.
    // Collect affected tag_ids from file_stubs that were just rebuilt.
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _affected_tags (tag_id INTEGER PRIMARY KEY)",
    )?;
    tx.execute_batch(
        "INSERT OR IGNORE INTO _affected_tags (tag_id)
         SELECT DISTINCT ft.tag_id FROM ptr_file_tag ft
         JOIN ptr_file_stub fs ON fs.file_stub_id = ft.file_stub_id
         JOIN _batch_hash bh ON bh.hash = fs.hash",
    )?;
    tx.execute_batch("DROP TABLE IF EXISTS _batch_hash;")?;

    // Delete stale counts for affected tags, then reinsert current counts.
    tx.execute_batch(
        "DELETE FROM ptr_tag_count WHERE tag_id IN (SELECT tag_id FROM _affected_tags)",
    )?;
    tx.execute_batch(
        "INSERT INTO ptr_tag_count (tag_id, file_count)
         SELECT ft.tag_id, COUNT(*) FROM ptr_file_tag ft
         WHERE ft.tag_id IN (SELECT tag_id FROM _affected_tags)
         GROUP BY ft.tag_id",
    )?;
    tx.execute_batch("DROP TABLE IF EXISTS _affected_tags;")?;

    tx.commit()?;
    Ok(count)
}

// ─── High-level methods ───

impl PtrSqliteDatabase {
    pub async fn get_overlay(&self, hash: &str) -> Result<Option<Vec<PtrResolvedTag>>, String> {
        let h = hash.to_string();
        self.with_read_conn(move |conn| {
            if let Some(existing) = get_overlay(conn, &h)? {
                return Ok(Some(existing));
            }
            let fallback = super::tags::lookup_tags_for_hash(conn, &h)?;
            if fallback.is_empty() {
                Ok(None)
            } else {
                Ok(Some(fallback))
            }
        })
        .await
    }

    pub async fn batch_get_overlay(
        &self,
        hashes: Vec<String>,
    ) -> Result<Vec<(String, Vec<PtrResolvedTag>)>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }

        // Check in-memory cache first
        let cache = self.overlay_cache();
        let cache_read = cache.read().await;
        let mut results: Vec<(String, Vec<PtrResolvedTag>)> = Vec::new();
        let mut misses: Vec<String> = Vec::new();
        for h in &hashes {
            if let Some(tags) = cache_read.get(h) {
                results.push((h.clone(), tags.clone()));
            } else {
                misses.push(h.clone());
            }
        }
        drop(cache_read);

        if misses.is_empty() {
            return Ok(results);
        }

        // Query DB for misses
        let db_results = self
            .with_read_conn(move |conn| batch_get_overlay(conn, &misses))
            .await?;

        // Compact/raw fallback for hashes missing overlay rows.
        let mut db_map: std::collections::HashMap<String, Vec<PtrResolvedTag>> =
            db_results.into_iter().collect();
        let unresolved: Vec<String> = hashes
            .iter()
            .filter(|h| !db_map.contains_key(*h))
            .cloned()
            .collect();
        if !unresolved.is_empty() {
            let fallback_rows = self
                .with_read_conn(move |conn| super::tags::batch_lookup_tags(conn, &unresolved))
                .await?;
            for (h, tags) in fallback_rows {
                if !tags.is_empty() {
                    db_map.insert(h, tags);
                }
            }
        }
        let db_results: Vec<(String, Vec<PtrResolvedTag>)> = db_map.into_iter().collect();

        // Populate cache with DB hits (evict-all when full)
        if !db_results.is_empty() {
            let mut cache_write = cache.write().await;
            if cache_write.len() + db_results.len() > super::OVERLAY_CACHE_MAX {
                cache_write.clear();
            }
            for (h, tags) in &db_results {
                cache_write.insert(h.clone(), tags.clone());
            }
        }

        results.extend(db_results);
        Ok(results)
    }

    pub async fn rebuild_overlay(&self, epoch: i64) -> Result<usize, String> {
        self.with_conn_mut(move |conn| rebuild_overlay(conn, epoch))
            .await
    }

    pub async fn rebuild_overlay_for_hashes(
        &self,
        hashes: Vec<String>,
        epoch: i64,
    ) -> Result<usize, String> {
        self.with_conn_mut(move |conn| rebuild_overlay_for_hashes(conn, &hashes, epoch))
            .await
    }
}
