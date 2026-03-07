//! PTR sync definition persistence and cursor management.

use std::collections::HashMap;
use std::time::Instant;

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use tracing::info;

use super::PtrSqliteDatabase;

/// Bind parameters per statement — rusqlite `bundled` SQLite has
/// MAX_VARIABLE_NUMBER=32766; larger values reduce statement count for bulk ops.
const MAX_PARAMS: usize = 16_000;
const DEF_RESOLVE_BATCH_IDS: usize = 16_000;
const STAGED_MAPPING_THRESHOLD: usize = 20_000;
/// Mid-sized deletes are faster with indexed prepared DELETEs.
const STAGED_MAPPING_DELETE_THRESHOLD: usize = 5_000;
const STAGED_RELATION_THRESHOLD: usize = 10_000;
const SLOW_PTR_TX_WARN_MS: f64 = 500.0;

pub fn get_cursor(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT last_index FROM ptr_cursor WHERE id = 1",
        [],
        |row| row.get(0),
    )
}

pub fn set_cursor(conn: &Connection, last_index: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE ptr_cursor SET last_index = ?1 WHERE id = 1",
        [last_index],
    )?;
    Ok(())
}

pub fn upsert_tag_def(conn: &Connection, def_id: i64, tag_string: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO ptr_tag_def (def_id, tag_string) VALUES (?1, ?2)",
        params![def_id, tag_string],
    )?;
    Ok(())
}

pub fn upsert_hash_def(conn: &Connection, def_id: i64, hash_hex: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO ptr_hash_def (def_id, hash_hex) VALUES (?1, ?2)",
        params![def_id, hash_hex],
    )?;
    Ok(())
}

fn multi_insert_blobs(tx: &Transaction, prefix: &str, blobs: &[Vec<u8>]) -> rusqlite::Result<()> {
    for chunk in blobs.chunks(MAX_PARAMS) {
        let placeholders = vec!["(?)"; chunk.len()].join(", ");
        let sql = format!("{prefix} {placeholders}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|b| b as &dyn rusqlite::types::ToSql)
            .collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}

fn multi_insert_i64_values(tx: &Transaction, prefix: &str, values: &[i64]) -> rusqlite::Result<()> {
    for chunk in values.chunks(MAX_PARAMS) {
        let placeholders = vec!["(?)"; chunk.len()].join(", ");
        let sql = format!("{prefix} {placeholders}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}

fn multi_insert_text_pairs(
    tx: &Transaction,
    prefix: &str,
    pairs: &[(String, String)],
) -> rusqlite::Result<()> {
    let chunk_size = MAX_PARAMS / 2;
    for chunk in pairs.chunks(chunk_size) {
        let placeholders = vec!["(?, ?)"; chunk.len()].join(", ");
        let sql = format!("{prefix} {placeholders}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .flat_map(|(a, b)| {
                [
                    a as &dyn rusqlite::types::ToSql,
                    b as &dyn rusqlite::types::ToSql,
                ]
            })
            .collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}

fn multi_insert_i64_pairs(
    tx: &Transaction,
    prefix: &str,
    pairs: &[(i64, i64)],
) -> rusqlite::Result<()> {
    let chunk_size = MAX_PARAMS / 2;
    for chunk in pairs.chunks(chunk_size) {
        let placeholders = vec!["(?, ?)"; chunk.len()].join(", ");
        let sql = format!("{prefix} {placeholders}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .flat_map(|(a, b)| {
                [
                    a as &dyn rusqlite::types::ToSql,
                    b as &dyn rusqlite::types::ToSql,
                ]
            })
            .collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}

fn multi_insert_i64_blob_pairs(
    tx: &Transaction,
    prefix: &str,
    rows: &[(i64, Vec<u8>)],
) -> rusqlite::Result<()> {
    let chunk_size = MAX_PARAMS / 2;
    for chunk in rows.chunks(chunk_size) {
        let placeholders = vec!["(?, ?)"; chunk.len()].join(", ");
        let sql = format!("{prefix} {placeholders}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .flat_map(|(id, blob)| {
                [
                    id as &dyn rusqlite::types::ToSql,
                    blob as &dyn rusqlite::types::ToSql,
                ]
            })
            .collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}

fn multi_insert_i64_text_triples(
    tx: &Transaction,
    prefix: &str,
    rows: &[(i64, String, String)],
) -> rusqlite::Result<()> {
    let chunk_size = MAX_PARAMS / 3;
    for chunk in rows.chunks(chunk_size) {
        let placeholders = vec!["(?, ?, ?)"; chunk.len()].join(", ");
        let sql = format!("{prefix} {placeholders}");
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = chunk
            .iter()
            .flat_map(|(id, a, b)| {
                [
                    id as &dyn rusqlite::types::ToSql,
                    a as &dyn rusqlite::types::ToSql,
                    b as &dyn rusqlite::types::ToSql,
                ]
            })
            .collect();
        tx.execute(&sql, param_refs.as_slice())?;
    }
    Ok(())
}

fn stage_insert_file_tag(tx: &Transaction, pairs: &[(i64, i64)]) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _stage_file_tag_add (
            file_stub_id INTEGER NOT NULL,
            tag_id       INTEGER NOT NULL,
            PRIMARY KEY (file_stub_id, tag_id)
         ) WITHOUT ROWID",
    )?;
    tx.execute_batch("DELETE FROM _stage_file_tag_add")?;
    multi_insert_i64_pairs(
        tx,
        "INSERT OR REPLACE INTO _stage_file_tag_add (file_stub_id, tag_id) VALUES",
        pairs,
    )?;
    tx.execute_batch(
        "INSERT OR IGNORE INTO ptr_file_tag (file_stub_id, tag_id)
         SELECT file_stub_id, tag_id FROM _stage_file_tag_add",
    )?;
    Ok(())
}

fn stage_delete_file_tag(tx: &Transaction, pairs: &[(i64, i64)]) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _stage_file_tag_del (
            file_stub_id INTEGER NOT NULL,
            tag_id       INTEGER NOT NULL,
            PRIMARY KEY (file_stub_id, tag_id)
         ) WITHOUT ROWID",
    )?;
    tx.execute_batch("DELETE FROM _stage_file_tag_del")?;
    multi_insert_i64_pairs(
        tx,
        "INSERT OR REPLACE INTO _stage_file_tag_del (file_stub_id, tag_id) VALUES",
        pairs,
    )?;
    tx.execute_batch(
        "DELETE FROM ptr_file_tag
         WHERE (file_stub_id, tag_id) IN (
             SELECT file_stub_id, tag_id
             FROM _stage_file_tag_del
         )",
    )?;
    Ok(())
}

fn stage_upsert_relation_pairs(
    tx: &Transaction,
    temp_table: &str,
    target_table: &str,
    col_a: &str,
    col_b: &str,
    pairs: &[(i64, i64)],
) -> rusqlite::Result<()> {
    tx.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {temp_table} (
            {col_a} INTEGER NOT NULL,
            {col_b} INTEGER NOT NULL,
            PRIMARY KEY ({col_a}, {col_b})
         ) WITHOUT ROWID"
    ))?;
    tx.execute_batch(&format!("DELETE FROM {temp_table}"))?;
    multi_insert_i64_pairs(
        tx,
        &format!("INSERT OR REPLACE INTO {temp_table} ({col_a}, {col_b}) VALUES"),
        pairs,
    )?;
    tx.execute_batch(&format!(
        "INSERT OR REPLACE INTO {target_table} ({col_a}, {col_b})
         SELECT {col_a}, {col_b} FROM {temp_table}"
    ))?;
    Ok(())
}

fn stage_delete_relation_pairs(
    tx: &Transaction,
    temp_table: &str,
    target_table: &str,
    col_a: &str,
    col_b: &str,
    pairs: &[(i64, i64)],
) -> rusqlite::Result<()> {
    tx.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {temp_table} (
            {col_a} INTEGER NOT NULL,
            {col_b} INTEGER NOT NULL,
            PRIMARY KEY ({col_a}, {col_b})
         ) WITHOUT ROWID"
    ))?;
    tx.execute_batch(&format!("DELETE FROM {temp_table}"))?;
    multi_insert_i64_pairs(
        tx,
        &format!("INSERT OR REPLACE INTO {temp_table} ({col_a}, {col_b}) VALUES"),
        pairs,
    )?;
    tx.execute_batch(&format!(
        "DELETE FROM {target_table}
         WHERE EXISTS (
             SELECT 1 FROM {temp_table} s
             WHERE s.{col_a} = {target_table}.{col_a}
               AND s.{col_b} = {target_table}.{col_b}
         )"
    ))?;
    Ok(())
}

/// Ensure all hashes and tags exist, then return string->ID lookup maps.
/// Called once per chunk so content processing uses cheap integer inserts.
pub fn ensure_and_resolve(
    conn: &mut Connection,
    hashes: &[String],         // unique hash hex strings
    tags: &[(String, String)], // unique (namespace, subtag) pairs
) -> rusqlite::Result<(HashMap<String, i64>, HashMap<(String, String), i64>)> {
    let tx = conn.transaction()?;

    if !hashes.is_empty() {
        let blobs: Vec<Vec<u8>> = hashes.iter().map(|h| super::hash_to_blob(h)).collect();
        multi_insert_blobs(
            &tx,
            "INSERT OR IGNORE INTO ptr_file_stub (hash) VALUES",
            &blobs,
        )?;
    }

    if !tags.is_empty() {
        multi_insert_text_pairs(
            &tx,
            "INSERT OR IGNORE INTO ptr_tag (namespace, subtag) VALUES",
            tags,
        )?;
    }

    let stub_map: HashMap<String, i64> = if hashes.is_empty() {
        HashMap::new()
    } else {
        let blobs: Vec<Vec<u8>> = hashes.iter().map(|h| super::hash_to_blob(h)).collect();
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _batch_hash (hash BLOB PRIMARY KEY) WITHOUT ROWID",
        )?;
        tx.execute_batch("DELETE FROM _batch_hash")?;
        multi_insert_blobs(
            &tx,
            "INSERT OR IGNORE INTO _batch_hash (hash) VALUES",
            &blobs,
        )?;
        let mut map = HashMap::with_capacity(hashes.len());
        let mut stmt = tx.prepare(
            "SELECT fs.file_stub_id, fs.hash FROM ptr_file_stub fs \
             INNER JOIN _batch_hash bh ON fs.hash = bh.hash",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, Vec<u8>>(1)?, row.get::<_, i64>(0)?))
        })?;
        for row in rows {
            let (blob, id) = row?;
            map.insert(super::blob_to_hash(&blob), id);
        }
        map
    };

    let tag_map: HashMap<(String, String), i64> = if tags.is_empty() {
        HashMap::new()
    } else {
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _batch_tag (namespace TEXT, subtag TEXT, PRIMARY KEY(namespace, subtag)) WITHOUT ROWID",
        )?;
        tx.execute_batch("DELETE FROM _batch_tag")?;
        multi_insert_text_pairs(
            &tx,
            "INSERT OR IGNORE INTO _batch_tag (namespace, subtag) VALUES",
            tags,
        )?;
        let mut map = HashMap::with_capacity(tags.len());
        let mut stmt = tx.prepare(
            "SELECT t.tag_id, t.namespace, t.subtag FROM ptr_tag t \
             INNER JOIN _batch_tag bt ON t.namespace = bt.namespace AND t.subtag = bt.subtag",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (id, ns, st) = row?;
            map.insert((ns, st), id);
        }
        map
    };

    tx.commit()?;
    Ok((stub_map, tag_map))
}

/// Resolve PTR def_ids directly to internal integer IDs.
///
/// Uses persistent mapping tables:
/// - ptr_hash_def_resolved(def_id -> file_stub_id)
/// - ptr_tag_def_resolved(def_id -> tag_id)
///
/// Missing rows are backfilled incrementally from ptr_hash_def/ptr_tag_def.
pub fn resolve_or_create_def_mappings(
    conn: &mut Connection,
    hash_def_ids: &[i64],
    tag_def_ids: &[i64],
) -> rusqlite::Result<(HashMap<i64, i64>, HashMap<i64, i64>)> {
    let tx_started = Instant::now();
    let tx = conn.transaction()?;
    let mut hash_existing_lookup_ms = 0.0;
    let mut hash_missing_scan_ms = 0.0;
    let mut hash_backfill_ms = 0.0;
    let mut tag_existing_lookup_ms = 0.0;
    let mut tag_missing_scan_ms = 0.0;
    let mut tag_backfill_ms = 0.0;
    let mut hash_existing_batches = 0usize;
    let mut hash_backfill_batches = 0usize;
    let mut tag_existing_batches = 0usize;
    let mut tag_backfill_batches = 0usize;
    let mut hash_missing_count = 0usize;
    let mut tag_missing_count = 0usize;

    // ---- Hash def_id -> file_stub_id ----
    let mut hash_map: HashMap<i64, i64> = HashMap::with_capacity(hash_def_ids.len());
    if !hash_def_ids.is_empty() {
        let lookup_started = Instant::now();
        hash_existing_batches = 1;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _batch_hash_def_id (
                def_id INTEGER PRIMARY KEY
             ) WITHOUT ROWID",
        )?;
        tx.execute_batch("DELETE FROM _batch_hash_def_id")?;
        multi_insert_i64_values(
            &tx,
            "INSERT OR IGNORE INTO _batch_hash_def_id (def_id) VALUES",
            hash_def_ids,
        )?;
        let mut stmt = tx.prepare(
            "SELECT r.def_id, r.file_stub_id
             FROM ptr_hash_def_resolved r
             INNER JOIN _batch_hash_def_id b ON b.def_id = r.def_id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        for row in rows {
            let (def_id, stub_id) = row?;
            hash_map.insert(def_id, stub_id);
        }
        hash_existing_lookup_ms = lookup_started.elapsed().as_secs_f64() * 1000.0;

        let missing_scan_started = Instant::now();
        let mut missing_hash_ids: Vec<i64> = Vec::new();
        if hash_map.len() != hash_def_ids.len() {
            missing_hash_ids.reserve(hash_def_ids.len().saturating_sub(hash_map.len()));
            for &def_id in hash_def_ids {
                if !hash_map.contains_key(&def_id) {
                    missing_hash_ids.push(def_id);
                }
            }
        }
        hash_missing_scan_ms = missing_scan_started.elapsed().as_secs_f64() * 1000.0;
        hash_missing_count = missing_hash_ids.len();

        if !missing_hash_ids.is_empty() {
            let backfill_started = Instant::now();
            tx.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS _missing_hash_def_map (
                    def_id INTEGER PRIMARY KEY,
                    hash   BLOB NOT NULL
                 ) WITHOUT ROWID",
            )?;

            for def_chunk in missing_hash_ids.chunks(DEF_RESOLVE_BATCH_IDS) {
                hash_backfill_batches += 1;
                tx.execute_batch("DELETE FROM _missing_hash_def_map")?;

                let placeholders = std::iter::repeat_n("?", def_chunk.len())
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = format!(
                    "SELECT def_id, hash_hex FROM ptr_hash_def WHERE def_id IN ({})",
                    placeholders
                );

                let mut stmt = tx.prepare(&sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(def_chunk.iter()), |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?;

                let mut resolved_rows: Vec<(i64, Vec<u8>)> = Vec::new();
                for row in rows {
                    let (def_id, hash_hex) = row?;
                    resolved_rows.push((def_id, super::hash_to_blob(&hash_hex)));
                }
                if resolved_rows.is_empty() {
                    continue;
                }

                let blobs: Vec<Vec<u8>> = resolved_rows.iter().map(|(_, b)| b.clone()).collect();
                multi_insert_blobs(
                    &tx,
                    "INSERT OR IGNORE INTO ptr_file_stub (hash) VALUES",
                    &blobs,
                )?;

                multi_insert_i64_blob_pairs(
                    &tx,
                    "INSERT OR REPLACE INTO _missing_hash_def_map (def_id, hash) VALUES",
                    &resolved_rows,
                )?;

                let mut pairs: Vec<(i64, i64)> = Vec::new();
                let mut map_stmt = tx.prepare(
                    "SELECT m.def_id, fs.file_stub_id
                     FROM _missing_hash_def_map m
                     JOIN ptr_file_stub fs ON fs.hash = m.hash",
                )?;
                let mapped = map_stmt
                    .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
                for row in mapped {
                    let (def_id, stub_id) = row?;
                    hash_map.insert(def_id, stub_id);
                    pairs.push((def_id, stub_id));
                }

                if !pairs.is_empty() {
                    multi_insert_i64_pairs(
                        &tx,
                        "INSERT OR REPLACE INTO ptr_hash_def_resolved (def_id, file_stub_id) VALUES",
                        &pairs,
                    )?;
                }
            }
            hash_backfill_ms = backfill_started.elapsed().as_secs_f64() * 1000.0;
        }
    }

    // ---- Tag def_id -> tag_id ----
    let mut tag_map: HashMap<i64, i64> = HashMap::with_capacity(tag_def_ids.len());
    if !tag_def_ids.is_empty() {
        let lookup_started = Instant::now();
        tag_existing_batches = 1;
        tx.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS _batch_tag_def_id (
                def_id INTEGER PRIMARY KEY
             ) WITHOUT ROWID",
        )?;
        tx.execute_batch("DELETE FROM _batch_tag_def_id")?;
        multi_insert_i64_values(
            &tx,
            "INSERT OR IGNORE INTO _batch_tag_def_id (def_id) VALUES",
            tag_def_ids,
        )?;
        let mut stmt = tx.prepare(
            "SELECT r.def_id, r.tag_id
             FROM ptr_tag_def_resolved r
             INNER JOIN _batch_tag_def_id b ON b.def_id = r.def_id",
        )?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
        for row in rows {
            let (def_id, tag_id) = row?;
            tag_map.insert(def_id, tag_id);
        }
        tag_existing_lookup_ms = lookup_started.elapsed().as_secs_f64() * 1000.0;

        let missing_scan_started = Instant::now();
        let mut missing_tag_ids: Vec<i64> = Vec::new();
        if tag_map.len() != tag_def_ids.len() {
            missing_tag_ids.reserve(tag_def_ids.len().saturating_sub(tag_map.len()));
            for &def_id in tag_def_ids {
                if !tag_map.contains_key(&def_id) {
                    missing_tag_ids.push(def_id);
                }
            }
        }
        tag_missing_scan_ms = missing_scan_started.elapsed().as_secs_f64() * 1000.0;
        tag_missing_count = missing_tag_ids.len();

        if !missing_tag_ids.is_empty() {
            let backfill_started = Instant::now();
            tx.execute_batch(
                "CREATE TEMP TABLE IF NOT EXISTS _missing_tag_def_map (
                    def_id    INTEGER PRIMARY KEY,
                    namespace TEXT NOT NULL,
                    subtag    TEXT NOT NULL
                 ) WITHOUT ROWID",
            )?;

            for def_chunk in missing_tag_ids.chunks(DEF_RESOLVE_BATCH_IDS) {
                tag_backfill_batches += 1;
                tx.execute_batch("DELETE FROM _missing_tag_def_map")?;

                let placeholders = std::iter::repeat_n("?", def_chunk.len())
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql = format!(
                    "SELECT def_id, tag_string FROM ptr_tag_def WHERE def_id IN ({})",
                    placeholders
                );

                let mut stmt = tx.prepare(&sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(def_chunk.iter()), |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?;

                let mut resolved_rows: Vec<(i64, String, String)> = Vec::new();
                let mut tag_pairs: Vec<(String, String)> = Vec::new();
                for row in rows {
                    let (def_id, tag_string) = row?;
                    if let Some((ns, st)) = crate::tags::parse_tag(&tag_string) {
                        tag_pairs.push((ns.clone(), st.clone()));
                        resolved_rows.push((def_id, ns, st));
                    }
                }
                if resolved_rows.is_empty() {
                    continue;
                }

                multi_insert_text_pairs(
                    &tx,
                    "INSERT OR IGNORE INTO ptr_tag (namespace, subtag) VALUES",
                    &tag_pairs,
                )?;

                multi_insert_i64_text_triples(
                    &tx,
                    "INSERT OR REPLACE INTO _missing_tag_def_map (def_id, namespace, subtag) VALUES",
                    &resolved_rows,
                )?;

                let mut pairs: Vec<(i64, i64)> = Vec::new();
                let mut map_stmt = tx.prepare(
                    "SELECT m.def_id, t.tag_id
                     FROM _missing_tag_def_map m
                     JOIN ptr_tag t
                       ON t.namespace = m.namespace
                      AND t.subtag = m.subtag",
                )?;
                let mapped = map_stmt
                    .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?;
                for row in mapped {
                    let (def_id, tag_id) = row?;
                    tag_map.insert(def_id, tag_id);
                    pairs.push((def_id, tag_id));
                }

                if !pairs.is_empty() {
                    multi_insert_i64_pairs(
                        &tx,
                        "INSERT OR REPLACE INTO ptr_tag_def_resolved (def_id, tag_id) VALUES",
                        &pairs,
                    )?;
                }
            }
            tag_backfill_ms = backfill_started.elapsed().as_secs_f64() * 1000.0;
        }
    }

    let commit_started = Instant::now();
    tx.commit()?;
    let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
    let total_ms = tx_started.elapsed().as_secs_f64() * 1000.0;
    if total_ms >= SLOW_PTR_TX_WARN_MS {
        info!(
            total_ms = %format!("{total_ms:.2}"),
            commit_ms = %format!("{commit_ms:.2}"),
            hash_requested = hash_def_ids.len(),
            hash_resolved = hash_map.len(),
            hash_missing = hash_missing_count,
            hash_existing_batches,
            hash_backfill_batches,
            hash_existing_lookup_ms = %format!("{hash_existing_lookup_ms:.2}"),
            hash_missing_scan_ms = %format!("{hash_missing_scan_ms:.2}"),
            hash_backfill_ms = %format!("{hash_backfill_ms:.2}"),
            tag_requested = tag_def_ids.len(),
            tag_resolved = tag_map.len(),
            tag_missing = tag_missing_count,
            tag_existing_batches,
            tag_backfill_batches,
            tag_existing_lookup_ms = %format!("{tag_existing_lookup_ms:.2}"),
            tag_missing_scan_ms = %format!("{tag_missing_scan_ms:.2}"),
            tag_backfill_ms = %format!("{tag_backfill_ms:.2}"),
            "PTR sync SQL txn slow: resolve_or_create_def_mappings"
        );
    } else {
        tracing::debug!(
            total_ms = %format!("{total_ms:.2}"),
            commit_ms = %format!("{commit_ms:.2}"),
            hash_requested = hash_def_ids.len(),
            hash_resolved = hash_map.len(),
            tag_requested = tag_def_ids.len(),
            tag_resolved = tag_map.len(),
            "PTR sync SQL txn: resolve_or_create_def_mappings"
        );
    }
    Ok((hash_map, tag_map))
}

pub struct ChunkContent {
    pub mapping_adds: Vec<(i64, i64)>,
    pub sibling_adds: Vec<(i64, i64)>,
    pub parent_adds: Vec<(i64, i64)>,
    pub mapping_dels: Vec<(i64, i64)>,
    pub sibling_dels: Vec<(i64, i64)>,
    pub parent_dels: Vec<(i64, i64)>,
}

pub struct ChunkContentCounts {
    pub tags_added: usize,
    pub siblings_added: usize,
    pub parents_added: usize,
}

pub fn process_chunk_content(
    conn: &mut Connection,
    content: ChunkContent,
) -> rusqlite::Result<ChunkContentCounts> {
    let tx_started = Instant::now();
    let mapping_add_count = content.mapping_adds.len();
    let sibling_add_count = content.sibling_adds.len();
    let parent_add_count = content.parent_adds.len();
    let mapping_del_count = content.mapping_dels.len();
    let sibling_del_count = content.sibling_dels.len();
    let parent_del_count = content.parent_dels.len();
    let total_rows = mapping_add_count
        + sibling_add_count
        + parent_add_count
        + mapping_del_count
        + sibling_del_count
        + parent_del_count;

    let mut mapping_add_ms = 0.0;
    let mut sibling_add_ms = 0.0;
    let mut parent_add_ms = 0.0;
    let mut mapping_del_ms = 0.0;
    let mut sibling_del_ms = 0.0;
    let mut parent_del_ms = 0.0;
    let mut mapping_del_strategy = "none";
    let tx = conn.transaction()?;

    if !content.mapping_adds.is_empty() {
        let started = Instant::now();
        if content.mapping_adds.len() >= STAGED_MAPPING_THRESHOLD {
            stage_insert_file_tag(&tx, &content.mapping_adds)?;
        } else {
            multi_insert_i64_pairs(
                &tx,
                "INSERT OR IGNORE INTO ptr_file_tag (file_stub_id, tag_id) VALUES",
                &content.mapping_adds,
            )?;
        }
        mapping_add_ms = started.elapsed().as_secs_f64() * 1000.0;
    }
    if !content.sibling_adds.is_empty() {
        let started = Instant::now();
        if content.sibling_adds.len() >= STAGED_RELATION_THRESHOLD {
            stage_upsert_relation_pairs(
                &tx,
                "_stage_ptr_tag_sibling_add",
                "ptr_tag_sibling",
                "from_tag_id",
                "to_tag_id",
                &content.sibling_adds,
            )?;
        } else {
            multi_insert_i64_pairs(
                &tx,
                "INSERT OR REPLACE INTO ptr_tag_sibling (from_tag_id, to_tag_id) VALUES",
                &content.sibling_adds,
            )?;
        }
        sibling_add_ms = started.elapsed().as_secs_f64() * 1000.0;
    }
    if !content.parent_adds.is_empty() {
        let started = Instant::now();
        if content.parent_adds.len() >= STAGED_RELATION_THRESHOLD {
            stage_upsert_relation_pairs(
                &tx,
                "_stage_ptr_tag_parent_add",
                "ptr_tag_parent",
                "child_tag_id",
                "parent_tag_id",
                &content.parent_adds,
            )?;
        } else {
            multi_insert_i64_pairs(
                &tx,
                "INSERT OR REPLACE INTO ptr_tag_parent (child_tag_id, parent_tag_id) VALUES",
                &content.parent_adds,
            )?;
        }
        parent_add_ms = started.elapsed().as_secs_f64() * 1000.0;
    }

    if !content.mapping_dels.is_empty() {
        let started = Instant::now();
        if content.mapping_dels.len() >= STAGED_MAPPING_DELETE_THRESHOLD {
            stage_delete_file_tag(&tx, &content.mapping_dels)?;
            mapping_del_strategy = "staged";
        } else {
            let mut stmt = tx.prepare_cached(
                "DELETE FROM ptr_file_tag WHERE file_stub_id = ?1 AND tag_id = ?2",
            )?;
            for (sid, tid) in &content.mapping_dels {
                stmt.execute(params![sid, tid])?;
            }
            mapping_del_strategy = "prepared_row_delete";
        }
        mapping_del_ms = started.elapsed().as_secs_f64() * 1000.0;
    }
    if !content.sibling_dels.is_empty() {
        let started = Instant::now();
        if content.sibling_dels.len() >= STAGED_RELATION_THRESHOLD {
            stage_delete_relation_pairs(
                &tx,
                "_stage_ptr_tag_sibling_del",
                "ptr_tag_sibling",
                "from_tag_id",
                "to_tag_id",
                &content.sibling_dels,
            )?;
        } else {
            let mut stmt = tx.prepare_cached(
                "DELETE FROM ptr_tag_sibling WHERE from_tag_id = ?1 AND to_tag_id = ?2",
            )?;
            for (a, b) in &content.sibling_dels {
                stmt.execute(params![a, b])?;
            }
        }
        sibling_del_ms = started.elapsed().as_secs_f64() * 1000.0;
    }
    if !content.parent_dels.is_empty() {
        let started = Instant::now();
        if content.parent_dels.len() >= STAGED_RELATION_THRESHOLD {
            stage_delete_relation_pairs(
                &tx,
                "_stage_ptr_tag_parent_del",
                "ptr_tag_parent",
                "child_tag_id",
                "parent_tag_id",
                &content.parent_dels,
            )?;
        } else {
            let mut stmt = tx.prepare_cached(
                "DELETE FROM ptr_tag_parent WHERE child_tag_id = ?1 AND parent_tag_id = ?2",
            )?;
            for (a, b) in &content.parent_dels {
                stmt.execute(params![a, b])?;
            }
        }
        parent_del_ms = started.elapsed().as_secs_f64() * 1000.0;
    }

    let commit_started = Instant::now();
    tx.commit()?;
    let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
    let total_ms = tx_started.elapsed().as_secs_f64() * 1000.0;
    if total_ms >= SLOW_PTR_TX_WARN_MS {
        info!(
            total_ms = %format!("{total_ms:.2}"),
            commit_ms = %format!("{commit_ms:.2}"),
            total_rows,
            mapping_add_count,
            sibling_add_count,
            parent_add_count,
            mapping_del_count,
            sibling_del_count,
            parent_del_count,
            mapping_add_ms = %format!("{mapping_add_ms:.2}"),
            sibling_add_ms = %format!("{sibling_add_ms:.2}"),
            parent_add_ms = %format!("{parent_add_ms:.2}"),
            mapping_del_ms = %format!("{mapping_del_ms:.2}"),
            mapping_del_strategy,
            sibling_del_ms = %format!("{sibling_del_ms:.2}"),
            parent_del_ms = %format!("{parent_del_ms:.2}"),
            "PTR sync SQL txn slow: process_chunk_content"
        );
    } else {
        tracing::debug!(
            total_ms = %format!("{total_ms:.2}"),
            total_rows,
            "PTR sync SQL txn: process_chunk_content"
        );
    }
    Ok(ChunkContentCounts {
        tags_added: mapping_add_count,
        siblings_added: sibling_add_count,
        parents_added: parent_add_count,
    })
}

pub fn enter_bulk_content_mode(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        DROP INDEX IF EXISTS idx_ptr_ft_tag;
        DROP INDEX IF EXISTS idx_ptr_ts_to;
        DROP INDEX IF EXISTS idx_ptr_tp_parent;
        "#,
    )?;
    conn.execute(
        "INSERT INTO ptr_schema_meta (key, value) VALUES ('bulk_index_rebuild_required', '1')
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [],
    )?;
    Ok(())
}

pub fn exit_bulk_content_mode(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE INDEX IF NOT EXISTS idx_ptr_ft_tag ON ptr_file_tag(tag_id);
        CREATE INDEX IF NOT EXISTS idx_ptr_ts_to ON ptr_tag_sibling(to_tag_id);
        CREATE INDEX IF NOT EXISTS idx_ptr_tp_parent ON ptr_tag_parent(parent_tag_id);
        "#,
    )?;
    conn.execute(
        "INSERT INTO ptr_schema_meta (key, value) VALUES ('bulk_index_rebuild_required', '0')
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [],
    )?;
    Ok(())
}

pub fn get_bulk_index_rebuild_required(conn: &Connection) -> rusqlite::Result<bool> {
    let required: Option<i64> = conn
        .query_row(
            "SELECT CAST(value AS INTEGER) FROM ptr_schema_meta WHERE key='bulk_index_rebuild_required'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(required.unwrap_or(0) != 0)
}

pub fn insert_all_defs(
    conn: &mut Connection,
    hash_defs: &[(i64, String)],
    tag_defs: &[(i64, String)],
) -> rusqlite::Result<()> {
    let tx_started = Instant::now();
    if hash_defs.is_empty() && tag_defs.is_empty() {
        return Ok(());
    }
    let tx = conn.transaction()?;
    let hash_insert_ms = {
        let started = Instant::now();
        let mut stmt = tx.prepare_cached(
            "INSERT OR IGNORE INTO ptr_hash_def (def_id, hash_hex) VALUES (?1, ?2)",
        )?;
        for (id, hex) in hash_defs {
            stmt.execute(params![id, hex])?;
        }
        started.elapsed().as_secs_f64() * 1000.0
    };
    let tag_insert_ms = {
        let started = Instant::now();
        let mut stmt = tx.prepare_cached(
            "INSERT OR IGNORE INTO ptr_tag_def (def_id, tag_string) VALUES (?1, ?2)",
        )?;
        for (id, tag) in tag_defs {
            stmt.execute(params![id, tag])?;
        }
        started.elapsed().as_secs_f64() * 1000.0
    };
    let commit_started = Instant::now();
    tx.commit()?;
    let commit_ms = commit_started.elapsed().as_secs_f64() * 1000.0;
    let total_ms = tx_started.elapsed().as_secs_f64() * 1000.0;
    if total_ms >= SLOW_PTR_TX_WARN_MS {
        info!(
            total_ms = %format!("{total_ms:.2}"),
            commit_ms = %format!("{commit_ms:.2}"),
            hash_insert_ms = %format!("{hash_insert_ms:.2}"),
            tag_insert_ms = %format!("{tag_insert_ms:.2}"),
            hash_defs = hash_defs.len(),
            tag_defs = tag_defs.len(),
            "PTR sync SQL txn slow: insert_all_defs"
        );
    } else {
        tracing::debug!(
            total_ms = %format!("{total_ms:.2}"),
            hash_defs = hash_defs.len(),
            tag_defs = tag_defs.len(),
            "PTR sync SQL txn: insert_all_defs"
        );
    }
    Ok(())
}

impl PtrSqliteDatabase {
    pub async fn get_cursor(&self) -> Result<i64, String> {
        self.with_read_conn(get_cursor).await
    }

    pub async fn set_cursor(&self, last_index: i64) -> Result<(), String> {
        self.with_conn(move |conn| set_cursor(conn, last_index))
            .await
    }

    pub async fn insert_all_defs(
        &self,
        hash_defs: Vec<(i64, String)>,
        tag_defs: Vec<(i64, String)>,
    ) -> Result<(), String> {
        self.with_conn_mut(move |conn| insert_all_defs(conn, &hash_defs, &tag_defs))
            .await
    }

    pub async fn ensure_and_resolve(
        &self,
        hashes: Vec<String>,
        tags: Vec<(String, String)>,
    ) -> Result<(HashMap<String, i64>, HashMap<(String, String), i64>), String> {
        self.with_conn_mut(move |conn| ensure_and_resolve(conn, &hashes, &tags))
            .await
    }

    pub async fn resolve_or_create_def_mappings(
        &self,
        hash_def_ids: Vec<i64>,
        tag_def_ids: Vec<i64>,
    ) -> Result<(HashMap<i64, i64>, HashMap<i64, i64>), String> {
        self.with_conn_mut(move |conn| {
            resolve_or_create_def_mappings(conn, &hash_def_ids, &tag_def_ids)
        })
        .await
    }

    pub async fn process_chunk_content(
        &self,
        content: ChunkContent,
    ) -> Result<ChunkContentCounts, String> {
        self.with_conn_mut(move |conn| process_chunk_content(conn, content))
            .await
    }

    pub async fn enter_bulk_content_mode(&self) -> Result<(), String> {
        self.with_conn(enter_bulk_content_mode).await
    }

    pub async fn exit_bulk_content_mode(&self) -> Result<(), String> {
        self.with_conn(exit_bulk_content_mode).await
    }

    pub async fn needs_bulk_index_rebuild(&self) -> Result<bool, String> {
        self.with_read_conn(get_bulk_index_rebuild_required).await
    }

    pub async fn run_bulk_index_rebuild(&self) -> Result<(), String> {
        self.with_conn(exit_bulk_content_mode).await
    }
}
