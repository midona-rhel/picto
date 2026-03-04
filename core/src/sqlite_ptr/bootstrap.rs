//! Hydrus PTR snapshot bootstrap importer.
//!
//! Imports service-scoped PTR state from Hydrus QuickSync databases:
//! - client.db
//! - client.master.db
//! - client.mappings.db
//!
//! The importer is hard-cut only: no legacy compatibility paths.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use super::PtrSqliteDatabase;

#[derive(Debug, Clone, Serialize)]
pub struct PtrBootstrapProgress {
    pub phase: String,
    pub rows_done: i64,
    pub rows_total: i64,
    pub ts: String,
}

type ProgressCallback = Arc<dyn Fn(PtrBootstrapProgress) + Send + Sync>;

/// Synchronous cancellation check for bootstrap phases (PBI-016).
/// Wraps an `Arc<AtomicBool>` that is set to `true` when cancellation is requested.
pub type CancelCheck = Arc<std::sync::atomic::AtomicBool>;

fn is_cancelled(cancel: &Option<CancelCheck>) -> bool {
    cancel
        .as_ref()
        .map(|c| c.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(false)
}

// Hydrus service types (from upstream HydrusConstants.py)
const HYDRUS_SERVICE_TYPE_TAG_REPOSITORY: i64 = 0;
const HYDRUS_SERVICE_TYPE_LOCAL_TAG: i64 = 5;
const HYDRUS_SERVICE_TYPE_COMBINED_TAG: i64 = 10;
const COMPACT_TAG_BATCH_ROWS: i64 = 200_000;
const COMPACT_HASH_BATCH_ROWS: i64 = 200_000;
const COMPACT_POSTING_HASH_RANGE: i64 = 200_000;

#[derive(Debug, Clone, Default, Serialize)]
pub struct PtrCompactCheckpoint {
    pub phase: String,
    pub last_hash_id: i64,
    pub last_tag_id: i64,
    pub last_service_hash_id: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct PtrCompactIndexStatus {
    pub running: bool,
    pub stage: String,
    pub rows_done_stage: i64,
    pub rows_total_stage: i64,
    pub rows_per_sec: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_max_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub checkpoint: PtrCompactCheckpoint,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtrBootstrapCounts {
    pub hash_defs: i64,
    pub tag_defs: i64,
    pub mappings: i64,
    pub siblings: i64,
    pub parents: i64,
    pub max_update_index: i64,
    /// True when counts are sqlite_stat1 estimates rather than exact (PBI-026).
    #[serde(default)]
    pub estimated: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtrBootstrapResult {
    pub service_id: i64,
    pub counts: PtrBootstrapCounts,
    pub projected_import_seconds: i64,
    pub snapshot_dir: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PtrBootstrapImportResult {
    pub service_id: i64,
    pub counts: PtrBootstrapCounts,
    pub cursor_index: i64,
    pub snapshot_dir: String,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PtrBootstrapMode {
    DryRun,
    Import,
}

impl PtrBootstrapMode {
    pub fn parse(mode: &str) -> Result<Self, String> {
        match mode.trim().to_ascii_lowercase().as_str() {
            "dry_run" | "dry-run" | "dryrun" => Ok(Self::DryRun),
            "import" => Ok(Self::Import),
            other => Err(format!(
                "Invalid ptr bootstrap mode '{}'. Expected 'dry_run' or 'import'.",
                other
            )),
        }
    }
}

#[derive(Debug, Clone)]
enum TagProjection {
    NamespaceSubtagIds,
    NamespaceSubtagDirect,
    CombinedTag,
}

#[derive(Debug, Clone)]
struct BootstrapState {
    snapshot_dir: String,
    service_id: i64,
    phase: String,
    rows_total: i64,
    rows_done: i64,
    stubs_last_service_hash_id: i64,
    tags_last_service_tag_id: i64,
    mappings_last_tag_id: i64,
    mappings_last_hash_id: i64,
}

#[derive(Debug, Clone)]
pub struct HydrusSnapshotProbe {
    pub snapshot_dir: PathBuf,
    pub client_db: PathBuf,
    pub master_db: PathBuf,
    pub mappings_db: PathBuf,
    pub service_id: i64,
    updates_table: Option<String>,
    mappings_table: String,
    hash_map_table: String,
    tag_map_table: String,
    mappings_hash_col: String,
    mappings_tag_col: String,
    hash_map_service_col: String,
    hash_map_master_col: String,
    tag_map_service_col: String,
    tag_map_master_col: String,
    tag_projection: TagProjection,
    siblings_table: Option<String>,
    parents_table: Option<String>,
    sibling_from_col: Option<String>,
    sibling_to_col: Option<String>,
    parent_child_col: Option<String>,
    parent_parent_col: Option<String>,
}

pub fn probe_snapshot(
    snapshot_dir: &Path,
    service_id_hint: Option<i64>,
) -> Result<HydrusSnapshotProbe, String> {
    let client_db = snapshot_dir.join("client.db");
    let master_db = snapshot_dir.join("client.master.db");
    let mappings_db = snapshot_dir.join("client.mappings.db");
    for p in [&client_db, &master_db, &mappings_db] {
        if !p.exists() {
            return Err(format!("Missing snapshot DB file: {}", p.display()));
        }
    }

    let client_conn = open_readonly(&client_db)?;
    let master_conn = open_readonly(&master_db)?;
    let mappings_conn = open_readonly(&mappings_db)?;

    let service_id = service_id_hint.unwrap_or(detect_service_id(&client_conn, &mappings_conn)?);
    if service_id <= 0 {
        return Err("Invalid PTR service_id detected".into());
    }

    let updates_table_candidate = format!("repository_updates_{}", service_id);
    let mappings_table = format!("current_mappings_{}", service_id);
    let hash_map_table_candidate = format!("repository_hash_id_map_{}", service_id);
    let tag_map_table_candidate = format!("repository_tag_id_map_{}", service_id);
    let siblings_table_candidate = format!("current_tag_siblings_{}", service_id);
    let parents_table_candidate = format!("current_tag_parents_{}", service_id);

    ensure_table(&mappings_conn, &mappings_table)?;
    ensure_table(&master_conn, "hashes")?;
    ensure_table(&master_conn, "tags")?;

    let updates_table = if table_exists(&client_conn, &updates_table_candidate)? {
        Some(updates_table_candidate)
    } else {
        None
    };
    let (hash_map_table, hash_map_cols) = if table_exists(&master_conn, &hash_map_table_candidate)?
    {
        let cols = table_columns(&master_conn, &hash_map_table_candidate)?;
        (hash_map_table_candidate, cols)
    } else {
        let fallback = "hashes".to_string();
        let cols = table_columns(&master_conn, &fallback)?;
        (fallback, cols)
    };
    let (tag_map_table, tag_map_cols) = if table_exists(&master_conn, &tag_map_table_candidate)? {
        let cols = table_columns(&master_conn, &tag_map_table_candidate)?;
        (tag_map_table_candidate, cols)
    } else {
        let fallback = "tags".to_string();
        let cols = table_columns(&master_conn, &fallback)?;
        (fallback, cols)
    };
    let mappings_cols = table_columns(&mappings_conn, &mappings_table)?;
    let tags_cols = table_columns(&master_conn, "tags")?;

    let mappings_hash_col = pick_col(
        &mappings_cols,
        &["service_hash_id", "hash_id", "file_hash_id"],
    )?;
    let mappings_tag_col = pick_col(&mappings_cols, &["service_tag_id", "tag_id"])?;

    let hash_map_service_col = pick_col(&hash_map_cols, &["service_hash_id", "hash_id"])?;
    let hash_map_master_col = pick_col(&hash_map_cols, &["master_hash_id", "hash_id"])?;

    let tag_map_service_col = pick_col(&tag_map_cols, &["service_tag_id", "tag_id"])?;
    let tag_map_master_col = pick_col(&tag_map_cols, &["master_tag_id", "tag_id"])?;

    let tag_projection = if has_col(&tags_cols, "namespace_id") && has_col(&tags_cols, "subtag_id")
    {
        ensure_table(&master_conn, "namespaces")?;
        ensure_table(&master_conn, "subtags")?;
        TagProjection::NamespaceSubtagIds
    } else if has_col(&tags_cols, "namespace") && has_col(&tags_cols, "subtag") {
        TagProjection::NamespaceSubtagDirect
    } else if has_col(&tags_cols, "tag") {
        TagProjection::CombinedTag
    } else {
        return Err(
            "Unsupported Hydrus tags schema (expected namespace/subtag or tag columns)".into(),
        );
    };

    let (siblings_table, sibling_from_col, sibling_to_col) =
        if table_exists(&client_conn, &siblings_table_candidate)? {
            let cols = table_columns(&client_conn, &siblings_table_candidate)?;
            let from = pick_col(
                &cols,
                &[
                    "bad_tag_id",
                    "old_tag_id",
                    "from_tag_id",
                    "child_tag_id",
                    "tag_id",
                ],
            )?;
            let to = pick_col(
                &cols,
                &[
                    "good_tag_id",
                    "new_tag_id",
                    "to_tag_id",
                    "parent_tag_id",
                    "ideal_tag_id",
                ],
            )?;
            (Some(siblings_table_candidate), Some(from), Some(to))
        } else {
            (None, None, None)
        };

    let (parents_table, parent_child_col, parent_parent_col) =
        if table_exists(&client_conn, &parents_table_candidate)? {
            let cols = table_columns(&client_conn, &parents_table_candidate)?;
            let child = pick_col(&cols, &["child_tag_id", "from_tag_id", "tag_id"])?;
            let parent = pick_col(&cols, &["parent_tag_id", "to_tag_id", "ideal_tag_id"])?;
            (Some(parents_table_candidate), Some(child), Some(parent))
        } else {
            (None, None, None)
        };

    Ok(HydrusSnapshotProbe {
        snapshot_dir: snapshot_dir.to_path_buf(),
        client_db,
        master_db,
        mappings_db,
        service_id,
        updates_table,
        mappings_table,
        hash_map_table,
        tag_map_table,
        mappings_hash_col,
        mappings_tag_col,
        hash_map_service_col,
        hash_map_master_col,
        tag_map_service_col,
        tag_map_master_col,
        tag_projection,
        siblings_table,
        parents_table,
        sibling_from_col,
        sibling_to_col,
        parent_child_col,
        parent_parent_col,
    })
}

pub fn dry_run_snapshot(probe: &HydrusSnapshotProbe) -> Result<PtrBootstrapResult, String> {
    let client_conn = open_readonly(&probe.client_db)?;
    let master_conn = open_readonly(&probe.master_db)?;
    let mappings_conn = open_readonly(&probe.mappings_db)?;

    let max_update_index = if let Some(table) = &probe.updates_table {
        scalar_i64(
            &client_conn,
            &format!("SELECT COALESCE(MAX(update_index), 0) FROM {}", table),
        )?
    } else {
        0
    };
    // PBI-026: Use cheap sqlite_stat1 estimates for large tables to avoid
    // multi-minute blocking COUNT(*) scans. Falls back to exact count if
    // stat1 is unavailable (small databases).
    let hash_defs = fast_row_estimate(&master_conn, &probe.hash_map_table).unwrap_or_else(|| {
        scalar_i64(
            &master_conn,
            &format!("SELECT COUNT(*) FROM {}", probe.hash_map_table),
        )
        .unwrap_or(0)
    });
    let tag_defs = fast_row_estimate(&master_conn, &probe.tag_map_table).unwrap_or_else(|| {
        scalar_i64(
            &master_conn,
            &format!("SELECT COUNT(*) FROM {}", probe.tag_map_table),
        )
        .unwrap_or(0)
    });
    let mappings = fast_row_estimate(&mappings_conn, &probe.mappings_table).unwrap_or_else(|| {
        scalar_i64(
            &mappings_conn,
            &format!("SELECT COUNT(*) FROM {}", probe.mappings_table),
        )
        .unwrap_or(0)
    });
    // Sibling/parent tables are small; exact counts are fine.
    let siblings = if let Some(table) = &probe.siblings_table {
        scalar_i64(&client_conn, &format!("SELECT COUNT(*) FROM {}", table))?
    } else {
        0
    };
    let parents = if let Some(table) = &probe.parents_table {
        scalar_i64(&client_conn, &format!("SELECT COUNT(*) FROM {}", table))?
    } else {
        0
    };

    // stat1 estimates are available when ANALYZE has been run on the source DBs
    let used_estimates = fast_row_estimate(&master_conn, &probe.hash_map_table).is_some()
        || fast_row_estimate(&master_conn, &probe.tag_map_table).is_some()
        || fast_row_estimate(&mappings_conn, &probe.mappings_table).is_some();

    Ok(PtrBootstrapResult {
        service_id: probe.service_id,
        counts: PtrBootstrapCounts {
            hash_defs,
            tag_defs,
            mappings,
            siblings,
            parents,
            max_update_index,
            estimated: used_estimates,
        },
        // Heuristic projection for UX only; real duration depends heavily on disk/CPU.
        projected_import_seconds: ((mappings / 2_500_000).max(1) * 3)
            + ((hash_defs + tag_defs) / 2_000_000).max(1),
        snapshot_dir: probe.snapshot_dir.to_string_lossy().to_string(),
    })
}

pub async fn import_snapshot(
    ptr_db: &Arc<PtrSqliteDatabase>,
    probe: HydrusSnapshotProbe,
    progress_cb: Option<ProgressCallback>,
    library_db_path: Option<PathBuf>,
    cancel: Option<CancelCheck>,
) -> Result<PtrBootstrapImportResult, String> {
    let dry = dry_run_snapshot(&probe)?;
    let rows_total = dry.counts.hash_defs
        + dry.counts.tag_defs
        + dry.counts.mappings
        + dry.counts.siblings
        + dry.counts.parents;

    emit_progress(&progress_cb, "reset", 0, rows_total);

    ptr_db.set_synchronous_off().await?;
    ptr_db.enter_bulk_content_mode().await?;

    let probe_for_tx = probe.clone();
    let dry_for_tx = dry.clone();
    let progress_cb_for_tx = progress_cb.clone();
    let lib_db_path = library_db_path.clone();
    let cancel_for_tx = cancel.clone();
    let tx_result = ptr_db
        .with_conn_mut(move |conn| {
            import_snapshot_local_warmup(
                conn,
                &probe_for_tx,
                &dry_for_tx,
                progress_cb_for_tx.clone(),
                lib_db_path,
                cancel_for_tx,
            )
        })
        .await;

    // Best-effort cleanup: detach ALL aliases (including `lib`) in case the
    // import failed mid-way. PBI-014: the original code missed `lib` here,
    // leaving the writer connection in a poisoned attached state on failure.
    let _ = ptr_db
        .with_conn(|conn| {
            let _ = conn.execute_batch("PRAGMA foreign_keys = ON;");
            let _ = conn.execute_batch("DETACH DATABASE lib;");
            let _ = conn.execute_batch("DETACH DATABASE hyd_maps;");
            let _ = conn.execute_batch("DETACH DATABASE hyd_master;");
            let _ = conn.execute_batch("DETACH DATABASE hyd_client;");
            Ok(())
        })
        .await;

    let restore_indexes_result = ptr_db.exit_bulk_content_mode().await;
    let restore_sync_result = ptr_db.set_synchronous_normal().await;
    let checkpoint_result = ptr_db.checkpoint_passive().await;

    if let Err(e) = restore_indexes_result {
        tracing::warn!(error = %e, "PTR bootstrap: failed to rebuild indexes");
    }
    if let Err(e) = restore_sync_result {
        tracing::warn!(error = %e, "PTR bootstrap: failed to restore synchronous mode");
    }
    if let Err(e) = checkpoint_result {
        tracing::warn!(error = %e, "PTR bootstrap: checkpoint failed");
    }

    let imported_cursor = tx_result??;

    emit_progress(&progress_cb, "completed", rows_total, rows_total);

    Ok(PtrBootstrapImportResult {
        service_id: probe.service_id,
        counts: dry.counts.clone(),
        cursor_index: imported_cursor,
        snapshot_dir: probe.snapshot_dir.to_string_lossy().to_string(),
    })
}

fn import_snapshot_local_warmup(
    conn: &mut Connection,
    probe: &HydrusSnapshotProbe,
    dry: &PtrBootstrapResult,
    progress_cb: Option<ProgressCallback>,
    library_db_path: Option<PathBuf>,
    cancel: Option<CancelCheck>,
) -> rusqlite::Result<Result<i64, String>> {
    for ident in [
        &probe.mappings_table,
        &probe.hash_map_table,
        &probe.tag_map_table,
        &probe.mappings_hash_col,
        &probe.mappings_tag_col,
        &probe.hash_map_service_col,
        &probe.hash_map_master_col,
        &probe.tag_map_service_col,
        &probe.tag_map_master_col,
    ] {
        if !is_identifier(ident) {
            return Ok(Err(format!("Invalid SQL identifier: {}", ident)));
        }
    }
    if let Some(table) = &probe.siblings_table {
        if !is_identifier(table) {
            return Ok(Err(format!("Invalid SQL identifier: {}", table)));
        }
    }
    if let Some(table) = &probe.parents_table {
        if !is_identifier(table) {
            return Ok(Err(format!("Invalid SQL identifier: {}", table)));
        }
    }

    let attach_client = format!(
        "ATTACH DATABASE '{}' AS hyd_client",
        sql_quote_path(&probe.client_db)
    );
    let attach_master = format!(
        "ATTACH DATABASE '{}' AS hyd_master",
        sql_quote_path(&probe.master_db)
    );
    let attach_mappings = format!(
        "ATTACH DATABASE '{}' AS hyd_maps",
        sql_quote_path(&probe.mappings_db)
    );
    conn.execute_batch(&attach_client)?;
    conn.execute_batch(&attach_master)?;
    conn.execute_batch(&attach_mappings)?;

    let library_db_path = if let Some(ref explicit) = library_db_path {
        if !explicit.exists() {
            return Ok(Err(format!(
                "Provided library DB path does not exist: {}",
                explicit.display()
            )));
        }
        explicit.clone()
    } else {
        tracing::warn!(
            "No explicit library DB path provided to bootstrap import; \
             falling back to sibling-path heuristic (may fail if PTR is on a different volume)"
        );
        match discover_library_db_path(conn) {
            Some(path) if path.exists() => path,
            _ => {
                return Ok(Err(
                    "Could not find library.sqlite for local warmup import. \
                     Ensure the library is open or pass an explicit library path."
                        .to_string(),
                ))
            }
        }
    };
    let attach_library = format!(
        "ATTACH DATABASE '{}' AS lib",
        sql_quote_path(&library_db_path)
    );
    conn.execute_batch(&attach_library)?;
    conn.execute_batch("PRAGMA foreign_keys = OFF;")?;
    ensure_bootstrap_state_table(conn)?;

    // Hard reset PTR graph-like tables for this import mode.
    conn.execute_batch(
        r#"
        DELETE FROM ptr_overlay;
        DELETE FROM ptr_negative_cache;
        DELETE FROM ptr_file_tag;
        DELETE FROM ptr_tag_sibling;
        DELETE FROM ptr_tag_parent;
        DELETE FROM ptr_tag_count;
        DELETE FROM ptr_tag_display;
        DELETE FROM ptr_hash_def_resolved;
        DELETE FROM ptr_tag_def_resolved;
        DELETE FROM ptr_hash_def;
        DELETE FROM ptr_tag_def;
        DELETE FROM ptr_file_stub;
        DELETE FROM ptr_tag;
        UPDATE ptr_cursor SET last_index = 0 WHERE id = 1;
    "#,
    )?;

    let rows_total = dry.counts.hash_defs
        + dry.counts.tag_defs
        + dry.counts.mappings
        + dry.counts.siblings
        + dry.counts.parents;
    let snapshot_dir = probe.snapshot_dir.to_string_lossy().to_string();
    let mut state = BootstrapState {
        snapshot_dir,
        service_id: probe.service_id,
        phase: "warmup_local".to_string(),
        rows_total,
        rows_done: 0,
        stubs_last_service_hash_id: 0,
        tags_last_service_tag_id: 0,
        mappings_last_tag_id: 0,
        mappings_last_hash_id: 0,
    };
    save_bootstrap_state(conn, &state)?;

    // Stage local active/inbox hashes and resolve Hydrus service hash IDs.
    // PBI-019: Emit fine-grained sub-phase progress between warmup stages.
    emit_progress(&progress_cb, "warmup_stage_init", 0, rows_total);
    conn.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS _warmup_local_hash (
            hash BLOB PRIMARY KEY
        ) WITHOUT ROWID;
         DELETE FROM _warmup_local_hash;
         CREATE TEMP TABLE IF NOT EXISTS _warmup_local_service_hash (
            service_hash_id INTEGER PRIMARY KEY,
            hash BLOB NOT NULL
         ) WITHOUT ROWID;
         DELETE FROM _warmup_local_service_hash;
         CREATE TEMP TABLE IF NOT EXISTS _warmup_used_tag (
            tag_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM _warmup_used_tag;",
    )?;
    conn.execute_batch(
        "INSERT OR IGNORE INTO _warmup_local_hash (hash)
         SELECT CAST(X'' || lower(hash) AS BLOB)
         FROM lib.file
         WHERE status IN (0, 1)",
    )?;

    emit_progress(&progress_cb, "warmup_resolve_hashes", 0, rows_total);
    let warmup_stubs_sql = format!(
        "INSERT OR IGNORE INTO _warmup_local_service_hash (service_hash_id, hash)
         SELECT hm.{svc_col}, h.hash
         FROM hyd_master.{map_table} hm
         JOIN hyd_master.hashes h ON h.hash_id = hm.{master_col}
         JOIN _warmup_local_hash lh ON lh.hash = h.hash",
        svc_col = probe.hash_map_service_col,
        map_table = probe.hash_map_table,
        master_col = probe.hash_map_master_col
    );
    conn.execute_batch(&warmup_stubs_sql)?;

    let local_hash_total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM _warmup_local_service_hash",
        [],
        |row| row.get(0),
    )?;

    conn.execute_batch(
        "INSERT OR REPLACE INTO ptr_file_stub (file_stub_id, hash)
         SELECT service_hash_id, hash
         FROM _warmup_local_service_hash",
    )?;
    state.rows_done = local_hash_total.min(rows_total);
    save_bootstrap_state(conn, &state)?;
    emit_progress(&progress_cb, "warmup_local", state.rows_done, rows_total);

    if is_cancelled(&cancel) {
        return Ok(Err("Cancelled".to_string()));
    }

    // Import mappings only for local hashes.
    emit_progress(
        &progress_cb,
        "warmup_import_mappings",
        state.rows_done,
        rows_total,
    );
    let warmup_mapping_sql = format!(
        "INSERT OR IGNORE INTO ptr_file_tag (file_stub_id, tag_id)
         SELECT m.{hash_col}, m.{tag_col}
         FROM hyd_maps.{table_name} m
         JOIN _warmup_local_service_hash lh ON lh.service_hash_id = m.{hash_col}",
        hash_col = probe.mappings_hash_col,
        tag_col = probe.mappings_tag_col,
        table_name = probe.mappings_table
    );
    conn.execute_batch(&warmup_mapping_sql)?;
    let mapping_rows: i64 =
        conn.query_row("SELECT COUNT(*) FROM ptr_file_tag", [], |row| row.get(0))?;
    state.rows_done = (local_hash_total + mapping_rows).min(rows_total);
    save_bootstrap_state(conn, &state)?;
    emit_progress(&progress_cb, "warmup_local", state.rows_done, rows_total);

    if is_cancelled(&cancel) {
        return Ok(Err("Cancelled".to_string()));
    }

    // Materialize tags for the local set.
    emit_progress(
        &progress_cb,
        "warmup_materialize_tags",
        state.rows_done,
        rows_total,
    );
    conn.execute_batch(
        "INSERT OR IGNORE INTO _warmup_used_tag (tag_id)
         SELECT DISTINCT tag_id FROM ptr_file_tag",
    )?;
    let import_tag_sql = match probe.tag_projection {
        TagProjection::NamespaceSubtagIds => format!(
            "INSERT OR REPLACE INTO ptr_tag (tag_id, namespace, subtag)
             SELECT ut.tag_id, COALESCE(ns.namespace, ''), st.subtag
             FROM _warmup_used_tag ut
             JOIN hyd_master.{tag_map} tm ON tm.{svc_col} = ut.tag_id
             JOIN hyd_master.tags t ON t.tag_id = tm.{master_col}
             LEFT JOIN hyd_master.namespaces ns ON ns.namespace_id = t.namespace_id
             JOIN hyd_master.subtags st ON st.subtag_id = t.subtag_id",
            tag_map = probe.tag_map_table,
            svc_col = probe.tag_map_service_col,
            master_col = probe.tag_map_master_col
        ),
        TagProjection::NamespaceSubtagDirect => format!(
            "INSERT OR REPLACE INTO ptr_tag (tag_id, namespace, subtag)
             SELECT ut.tag_id, COALESCE(t.namespace, ''), t.subtag
             FROM _warmup_used_tag ut
             JOIN hyd_master.{tag_map} tm ON tm.{svc_col} = ut.tag_id
             JOIN hyd_master.tags t ON t.tag_id = tm.{master_col}",
            tag_map = probe.tag_map_table,
            svc_col = probe.tag_map_service_col,
            master_col = probe.tag_map_master_col
        ),
        TagProjection::CombinedTag => format!(
            "INSERT OR REPLACE INTO ptr_tag (tag_id, namespace, subtag)
             SELECT ut.tag_id,
                    CASE WHEN instr(t.tag, ':') > 0 THEN substr(t.tag, 1, instr(t.tag, ':') - 1) ELSE '' END,
                    CASE WHEN instr(t.tag, ':') > 0 THEN substr(t.tag, instr(t.tag, ':') + 1) ELSE t.tag END
             FROM _warmup_used_tag ut
             JOIN hyd_master.{tag_map} tm ON tm.{svc_col} = ut.tag_id
             JOIN hyd_master.tags t ON t.tag_id = tm.{master_col}",
            tag_map = probe.tag_map_table,
            svc_col = probe.tag_map_service_col,
            master_col = probe.tag_map_master_col
        ),
    };
    conn.execute_batch(&import_tag_sql)?;

    if is_cancelled(&cancel) {
        return Ok(Err("Cancelled".to_string()));
    }

    // Local-only siblings/parents for used tags.
    emit_progress(
        &progress_cb,
        "warmup_siblings_parents",
        state.rows_done,
        rows_total,
    );
    if let (Some(table), Some(from_col), Some(to_col)) = (
        probe.siblings_table.as_ref(),
        probe.sibling_from_col.as_ref(),
        probe.sibling_to_col.as_ref(),
    ) {
        let sql = format!(
            "INSERT OR REPLACE INTO ptr_tag_sibling (from_tag_id, to_tag_id)
             SELECT {from_col}, {to_col}
             FROM hyd_client.{table_name}
             WHERE {from_col} IN (SELECT tag_id FROM _warmup_used_tag)
               AND {to_col} IN (SELECT tag_id FROM _warmup_used_tag)",
            from_col = from_col,
            to_col = to_col,
            table_name = table
        );
        conn.execute_batch(&sql)?;
    }
    if let (Some(table), Some(child_col), Some(parent_col)) = (
        probe.parents_table.as_ref(),
        probe.parent_child_col.as_ref(),
        probe.parent_parent_col.as_ref(),
    ) {
        let sql = format!(
            "INSERT OR REPLACE INTO ptr_tag_parent (child_tag_id, parent_tag_id)
             SELECT {child_col}, {parent_col}
             FROM hyd_client.{table_name}
             WHERE {child_col} IN (SELECT tag_id FROM _warmup_used_tag)
               AND {parent_col} IN (SELECT tag_id FROM _warmup_used_tag)",
            child_col = child_col,
            parent_col = parent_col,
            table_name = table
        );
        conn.execute_batch(&sql)?;
    }

    // Build def and mapping caches for local subset.
    conn.execute_batch(
        "INSERT OR REPLACE INTO ptr_hash_def (def_id, hash_hex)
         SELECT file_stub_id, lower(hex(hash))
         FROM ptr_file_stub",
    )?;
    conn.execute_batch(
        "INSERT OR REPLACE INTO ptr_tag_def (def_id, tag_string)
         SELECT tag_id,
                CASE WHEN namespace = '' THEN subtag ELSE namespace || ':' || subtag END
         FROM ptr_tag",
    )?;
    conn.execute_batch(
        "INSERT OR REPLACE INTO ptr_hash_def_resolved (def_id, file_stub_id)
         SELECT file_stub_id, file_stub_id
         FROM ptr_file_stub",
    )?;
    conn.execute_batch(
        "INSERT OR REPLACE INTO ptr_tag_def_resolved (def_id, tag_id)
         SELECT tag_id, tag_id
         FROM ptr_tag",
    )?;

    // Rebuild display + overlay for local hashes only.
    emit_progress(
        &progress_cb,
        "warmup_build_overlay",
        state.rows_done,
        rows_total,
    );
    let local_hashes: Vec<String> = {
        let mut stmt = conn.prepare("SELECT lower(hex(hash)) FROM ptr_file_stub")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        out
    };
    let epoch = chrono::Utc::now().timestamp();
    super::overlay::rebuild_overlay_for_hashes(conn, &local_hashes, epoch)?;
    conn.execute_batch("ANALYZE;")?;
    state.phase = "warmup_done".to_string();
    state.rows_done = rows_total;
    save_bootstrap_state(conn, &state)?;
    emit_progress(&progress_cb, "warmup_done", state.rows_done, rows_total);

    let max_update_index = match probe_max_update_index(probe) {
        Ok(v) => v,
        Err(e) => return Ok(Err(e)),
    };
    conn.execute(
        "UPDATE ptr_cursor SET last_index = ?1 WHERE id = 1",
        [max_update_index],
    )?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")?;
    conn.execute_batch("DETACH DATABASE lib;")?;
    conn.execute_batch("DETACH DATABASE hyd_maps;")?;
    conn.execute_batch("DETACH DATABASE hyd_master;")?;
    conn.execute_batch("DETACH DATABASE hyd_client;")?;
    Ok(Ok(max_update_index))
}

fn ensure_bootstrap_state_table(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ptr_bootstrap_state (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            snapshot_dir TEXT NOT NULL,
            service_id INTEGER NOT NULL,
            phase TEXT NOT NULL,
            rows_total INTEGER NOT NULL,
            rows_done INTEGER NOT NULL,
            stubs_last_service_hash_id INTEGER NOT NULL DEFAULT 0,
            tags_last_service_tag_id INTEGER NOT NULL DEFAULT 0,
            mappings_last_tag_id INTEGER NOT NULL DEFAULT 0,
            mappings_last_hash_id INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );",
    )?;

    let mut stmt = conn.prepare("PRAGMA table_info(ptr_bootstrap_state)")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    let mut cols = std::collections::HashSet::new();
    for row in rows {
        cols.insert(row?);
    }
    if !cols.contains("stubs_last_service_hash_id") {
        conn.execute(
            "ALTER TABLE ptr_bootstrap_state ADD COLUMN stubs_last_service_hash_id INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !cols.contains("tags_last_service_tag_id") {
        conn.execute(
            "ALTER TABLE ptr_bootstrap_state ADD COLUMN tags_last_service_tag_id INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    Ok(())
}

fn save_bootstrap_state(conn: &Connection, state: &BootstrapState) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ptr_bootstrap_state (
            id, snapshot_dir, service_id, phase, rows_total, rows_done,
            stubs_last_service_hash_id, tags_last_service_tag_id,
            mappings_last_tag_id, mappings_last_hash_id, updated_at
         ) VALUES (
            1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, strftime('%s','now')
         )
         ON CONFLICT(id) DO UPDATE SET
            snapshot_dir = excluded.snapshot_dir,
            service_id = excluded.service_id,
            phase = excluded.phase,
            rows_total = excluded.rows_total,
            rows_done = excluded.rows_done,
            stubs_last_service_hash_id = excluded.stubs_last_service_hash_id,
            tags_last_service_tag_id = excluded.tags_last_service_tag_id,
            mappings_last_tag_id = excluded.mappings_last_tag_id,
            mappings_last_hash_id = excluded.mappings_last_hash_id,
            updated_at = excluded.updated_at",
        rusqlite::params![
            state.snapshot_dir,
            state.service_id,
            state.phase,
            state.rows_total,
            state.rows_done,
            state.stubs_last_service_hash_id,
            state.tags_last_service_tag_id,
            state.mappings_last_tag_id,
            state.mappings_last_hash_id
        ],
    )?;
    Ok(())
}

fn emit_progress(
    callback: &Option<ProgressCallback>,
    phase: &str,
    rows_done: i64,
    rows_total: i64,
) {
    if let Some(cb) = callback.as_ref() {
        cb(PtrBootstrapProgress {
            phase: phase.to_string(),
            rows_done,
            rows_total,
            ts: chrono::Utc::now().to_rfc3339(),
        });
    }
}

pub async fn build_compact_index(
    ptr_db: &Arc<PtrSqliteDatabase>,
    probe: HydrusSnapshotProbe,
    progress_cb: Option<ProgressCallback>,
    cancel: Option<CancelCheck>,
) -> Result<(), String> {
    let probe_for_tx = probe.clone();
    let cb_for_tx = progress_cb.clone();
    let cancel_for_tx = cancel.clone();
    let result = ptr_db
        .with_conn_mut(move |conn| {
            build_compact_index_resumable(conn, &probe_for_tx, cb_for_tx, cancel_for_tx)
        })
        .await;

    // PBI-014: Best-effort cleanup of attached databases on any failure path.
    // The success path detaches inside build_compact_index_resumable, but errors
    // may leave hyd_master/hyd_maps attached and poison the writer connection.
    if result.is_err() {
        let _ = ptr_db
            .with_conn(|conn| {
                let _ = conn.execute_batch("DETACH DATABASE hyd_maps;");
                let _ = conn.execute_batch("DETACH DATABASE hyd_master;");
                Ok(())
            })
            .await;
    }

    result??;
    Ok(())
}

pub async fn get_compact_index_status(
    ptr_db: &Arc<PtrSqliteDatabase>,
) -> Result<PtrCompactIndexStatus, String> {
    ptr_db.with_read_conn(load_compact_index_status).await
}

fn load_compact_index_status(conn: &Connection) -> rusqlite::Result<PtrCompactIndexStatus> {
    let mut status = conn
        .query_row(
            "SELECT running, stage, rows_done_stage, rows_total_stage, rows_per_sec,
                    snapshot_dir, service_id, snapshot_max_index, updated_at,
                    checkpoint_phase, checkpoint_last_hash_id, checkpoint_last_tag_id, checkpoint_last_service_hash_id
             FROM ptr_compact_state
             WHERE id = 1",
            [],
            |row| {
                Ok(PtrCompactIndexStatus {
                    running: row.get::<_, i64>(0)? != 0,
                    stage: row.get(1)?,
                    rows_done_stage: row.get(2)?,
                    rows_total_stage: row.get(3)?,
                    rows_per_sec: row.get(4)?,
                    snapshot_dir: row.get(5)?,
                    service_id: row.get(6)?,
                    snapshot_max_index: row.get(7)?,
                    updated_at: row.get(8)?,
                    checkpoint: PtrCompactCheckpoint {
                        phase: row.get(9)?,
                        last_hash_id: row.get(10)?,
                        last_tag_id: row.get(11)?,
                        last_service_hash_id: row.get(12)?,
                    },
                })
            },
        )
        .optional()?
        .unwrap_or_default();

    if status.stage.is_empty() {
        status.stage = "idle".to_string();
    }
    Ok(status)
}

fn build_compact_index_resumable(
    conn: &mut Connection,
    probe: &HydrusSnapshotProbe,
    progress_cb: Option<ProgressCallback>,
    cancel: Option<CancelCheck>,
) -> rusqlite::Result<Result<(), String>> {
    for ident in [
        &probe.mappings_table,
        &probe.hash_map_table,
        &probe.tag_map_table,
        &probe.mappings_hash_col,
        &probe.mappings_tag_col,
        &probe.hash_map_service_col,
        &probe.hash_map_master_col,
        &probe.tag_map_service_col,
        &probe.tag_map_master_col,
    ] {
        if !is_identifier(ident) {
            return Ok(Err(format!("Invalid SQL identifier: {}", ident)));
        }
    }

    let attach_master = format!(
        "ATTACH DATABASE '{}' AS hyd_master",
        sql_quote_path(&probe.master_db)
    );
    let attach_mappings = format!(
        "ATTACH DATABASE '{}' AS hyd_maps",
        sql_quote_path(&probe.mappings_db)
    );
    conn.execute_batch(&attach_master)?;
    conn.execute_batch(&attach_mappings)?;

    let snapshot_dir = probe.snapshot_dir.to_string_lossy().to_string();
    let max_update_index = probe_max_update_index(probe).unwrap_or(0);
    let tags_total = scalar_i64(
        conn,
        &format!("SELECT COUNT(*) FROM hyd_master.{}", probe.tag_map_table),
    )
    .unwrap_or(0);
    let hashes_total = scalar_i64(
        conn,
        &format!("SELECT COUNT(*) FROM hyd_master.{}", probe.hash_map_table),
    )
    .unwrap_or(0);
    let postings_total = scalar_i64(
        conn,
        &format!("SELECT COUNT(*) FROM hyd_maps.{}", probe.mappings_table),
    )
    .unwrap_or(0);

    let previous = load_compact_index_status(conn)?;
    let reset_required = previous.service_id != Some(probe.service_id)
        || previous.snapshot_dir.as_deref() != Some(snapshot_dir.as_str())
        || previous.stage == "completed";
    if reset_required {
        conn.execute_batch(
            "DELETE FROM ptr_compact_hash;
             DELETE FROM ptr_compact_tag;
             DELETE FROM ptr_compact_posting;",
        )?;
        store_compact_status(
            conn,
            &PtrCompactIndexStatus {
                running: true,
                stage: "compact_tags".to_string(),
                rows_done_stage: 0,
                rows_total_stage: tags_total,
                rows_per_sec: 0.0,
                snapshot_dir: Some(snapshot_dir.clone()),
                service_id: Some(probe.service_id),
                snapshot_max_index: Some(max_update_index),
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
                checkpoint: PtrCompactCheckpoint {
                    phase: "compact_tags".to_string(),
                    ..Default::default()
                },
            },
        )?;
    } else if !previous.running {
        let mut resumed = previous.clone();
        resumed.running = true;
        if resumed.stage.is_empty() || resumed.stage == "idle" {
            resumed.stage = "compact_tags".to_string();
            resumed.rows_total_stage = tags_total;
            resumed.checkpoint.phase = "compact_tags".to_string();
        }
        store_compact_status(conn, &resumed)?;
    }

    let started = Instant::now();
    let mut status = load_compact_index_status(conn)?;
    status.running = true;

    // Stage 1: compact tags
    if status.checkpoint.phase == "compact_tags" || status.stage == "compact_tags" {
        status.stage = "compact_tags".to_string();
        status.rows_total_stage = tags_total;
        let mut last_tag = status.checkpoint.last_tag_id.max(0);
        loop {
            let tx = conn.transaction()?;
            let sql = match probe.tag_projection {
                TagProjection::NamespaceSubtagIds => format!(
                    "INSERT OR REPLACE INTO ptr_compact_tag (service_tag_id, namespace, subtag)
                     SELECT tm.{svc_col}, COALESCE(ns.namespace, ''), st.subtag
                     FROM hyd_master.{map_table} tm
                     JOIN hyd_master.tags t ON t.tag_id = tm.{master_col}
                     LEFT JOIN hyd_master.namespaces ns ON ns.namespace_id = t.namespace_id
                     JOIN hyd_master.subtags st ON st.subtag_id = t.subtag_id
                     WHERE tm.{svc_col} > ?1
                     ORDER BY tm.{svc_col}
                     LIMIT ?2",
                    svc_col = probe.tag_map_service_col,
                    map_table = probe.tag_map_table,
                    master_col = probe.tag_map_master_col
                ),
                TagProjection::NamespaceSubtagDirect => format!(
                    "INSERT OR REPLACE INTO ptr_compact_tag (service_tag_id, namespace, subtag)
                     SELECT tm.{svc_col}, COALESCE(t.namespace, ''), t.subtag
                     FROM hyd_master.{map_table} tm
                     JOIN hyd_master.tags t ON t.tag_id = tm.{master_col}
                     WHERE tm.{svc_col} > ?1
                     ORDER BY tm.{svc_col}
                     LIMIT ?2",
                    svc_col = probe.tag_map_service_col,
                    map_table = probe.tag_map_table,
                    master_col = probe.tag_map_master_col
                ),
                TagProjection::CombinedTag => format!(
                    "INSERT OR REPLACE INTO ptr_compact_tag (service_tag_id, namespace, subtag)
                     SELECT tm.{svc_col},
                            CASE WHEN instr(t.tag, ':') > 0 THEN substr(t.tag, 1, instr(t.tag, ':') - 1) ELSE '' END,
                            CASE WHEN instr(t.tag, ':') > 0 THEN substr(t.tag, instr(t.tag, ':') + 1) ELSE t.tag END
                     FROM hyd_master.{map_table} tm
                     JOIN hyd_master.tags t ON t.tag_id = tm.{master_col}
                     WHERE tm.{svc_col} > ?1
                     ORDER BY tm.{svc_col}
                     LIMIT ?2",
                    svc_col = probe.tag_map_service_col,
                    map_table = probe.tag_map_table,
                    master_col = probe.tag_map_master_col
                ),
            };
            tx.execute(&sql, rusqlite::params![last_tag, COMPACT_TAG_BATCH_ROWS])?;
            let inserted = tx.changes() as i64;
            if inserted == 0 {
                status.checkpoint.phase = "compact_hashes".to_string();
                status.stage = "compact_hashes".to_string();
                status.rows_done_stage = 0;
                status.rows_total_stage = hashes_total;
                status.checkpoint.last_tag_id = last_tag;
                store_compact_status(&tx, &status)?;
                tx.commit()?;
                break;
            }
            last_tag = tx.query_row(
                "SELECT COALESCE(MAX(service_tag_id), 0) FROM ptr_compact_tag",
                [],
                |row| row.get(0),
            )?;
            status.rows_done_stage = last_tag.min(tags_total);
            status.checkpoint.last_tag_id = last_tag;
            status.rows_per_sec = if started.elapsed().as_secs_f64() > 0.0 {
                status.rows_done_stage as f64 / started.elapsed().as_secs_f64()
            } else {
                0.0
            };
            store_compact_status(&tx, &status)?;
            tx.commit()?;
            emit_progress(
                &progress_cb,
                "compact_tags",
                status.rows_done_stage,
                status.rows_total_stage,
            );
            if is_cancelled(&cancel) {
                return Ok(Err("Cancelled".to_string()));
            }
        }
    }

    // Stage 2: compact hashes
    if status.checkpoint.phase == "compact_hashes" || status.stage == "compact_hashes" {
        let mut last_hash = status.checkpoint.last_hash_id.max(0);
        loop {
            let tx = conn.transaction()?;
            let sql = format!(
                "INSERT OR REPLACE INTO ptr_compact_hash (hash, service_hash_id)
                 SELECT h.hash, hm.{svc_col}
                 FROM hyd_master.{map_table} hm
                 JOIN hyd_master.hashes h ON h.hash_id = hm.{master_col}
                 WHERE hm.{svc_col} > ?1
                 ORDER BY hm.{svc_col}
                 LIMIT ?2",
                svc_col = probe.hash_map_service_col,
                map_table = probe.hash_map_table,
                master_col = probe.hash_map_master_col
            );
            tx.execute(&sql, rusqlite::params![last_hash, COMPACT_HASH_BATCH_ROWS])?;
            let inserted = tx.changes() as i64;
            if inserted == 0 {
                status.checkpoint.phase = "compact_postings".to_string();
                status.stage = "compact_postings".to_string();
                status.rows_done_stage = 0;
                status.rows_total_stage = postings_total;
                status.checkpoint.last_hash_id = last_hash;
                store_compact_status(&tx, &status)?;
                tx.commit()?;
                break;
            }
            last_hash = tx.query_row(
                "SELECT COALESCE(MAX(service_hash_id), 0) FROM ptr_compact_hash",
                [],
                |row| row.get(0),
            )?;
            status.rows_done_stage = last_hash.min(hashes_total);
            status.checkpoint.last_hash_id = last_hash;
            status.rows_per_sec = if started.elapsed().as_secs_f64() > 0.0 {
                status.rows_done_stage as f64 / started.elapsed().as_secs_f64()
            } else {
                0.0
            };
            store_compact_status(&tx, &status)?;
            tx.commit()?;
            emit_progress(
                &progress_cb,
                "compact_hashes",
                status.rows_done_stage,
                status.rows_total_stage,
            );
            if is_cancelled(&cancel) {
                return Ok(Err("Cancelled".to_string()));
            }
        }
    }

    // Stage 3: compact postings — streaming ordered scan (PBI-018).
    // Instead of iterating by numeric hash-ID ranges (which stalls on sparse IDs),
    // we stream all mappings ordered by (hash_col, tag_col) and group in one pass.
    // Checkpoint uses last_service_hash_id for resumability.
    if status.checkpoint.phase == "compact_postings" || status.stage == "compact_postings" {
        let resume_hash_id = status.checkpoint.last_service_hash_id.max(0);
        let sql = format!(
            "SELECT {hash_col}, {tag_col}
             FROM hyd_maps.{table_name}
             WHERE {hash_col} > ?1
             ORDER BY {hash_col}, {tag_col}",
            hash_col = probe.mappings_hash_col,
            tag_col = probe.mappings_tag_col,
            table_name = probe.mappings_table
        );
        let mut rows_processed: i64 = status.rows_done_stage;
        let mut current_hash: Option<i64> = None;
        let mut current_tags: Vec<i64> = Vec::new();
        let mut chunk_rows: i64 = 0;

        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(&sql)?;
            let mut rows = stmt.query(rusqlite::params![resume_hash_id])?;
            let mut insert_stmt = tx.prepare(
                "INSERT OR REPLACE INTO ptr_compact_posting (service_hash_id, tag_ids_blob, tag_count)
                 VALUES (?1, ?2, ?3)",
            )?;

            while let Some(row) = rows.next()? {
                let hash_id: i64 = row.get(0)?;
                let tag_id: i64 = row.get(1)?;
                rows_processed += 1;
                chunk_rows += 1;

                if current_hash != Some(hash_id) {
                    if let Some(prev_hash) = current_hash.take() {
                        current_tags.sort_unstable();
                        current_tags.dedup();
                        let blob = encode_delta_varint(&current_tags);
                        insert_stmt.execute(rusqlite::params![
                            prev_hash,
                            blob,
                            current_tags.len() as i64
                        ])?;
                        current_tags.clear();

                        // Checkpoint at hash boundaries every COMPACT_POSTING_HASH_RANGE rows.
                        if chunk_rows >= COMPACT_POSTING_HASH_RANGE {
                            status.rows_done_stage = rows_processed;
                            status.rows_total_stage = postings_total;
                            status.checkpoint.last_service_hash_id = prev_hash;
                            status.rows_per_sec = if started.elapsed().as_secs_f64() > 0.0 {
                                rows_processed as f64 / started.elapsed().as_secs_f64()
                            } else {
                                0.0
                            };
                            emit_progress(
                                &progress_cb,
                                "compact_postings",
                                rows_processed,
                                postings_total,
                            );
                            chunk_rows = 0;

                            if is_cancelled(&cancel) {
                                // Flush the current tag if any before returning.
                                drop(rows);
                                drop(insert_stmt);
                                drop(stmt);
                                status.checkpoint.phase = "compact_postings".to_string();
                                status.stage = "compact_postings".to_string();
                                store_compact_status(&tx, &status)?;
                                tx.commit()?;
                                return Ok(Err("Cancelled".to_string()));
                            }
                        }
                    }
                    current_hash = Some(hash_id);
                }
                current_tags.push(tag_id);
            }
            // Flush last hash group.
            if let Some(prev_hash) = current_hash {
                current_tags.sort_unstable();
                current_tags.dedup();
                let blob = encode_delta_varint(&current_tags);
                insert_stmt.execute(rusqlite::params![
                    prev_hash,
                    blob,
                    current_tags.len() as i64
                ])?;
                status.checkpoint.last_service_hash_id = prev_hash;
            }
        }

        status.rows_done_stage = rows_processed;
        status.rows_total_stage = postings_total;
        status.stage = "compact_postings".to_string();
        status.checkpoint.phase = "compact_postings".to_string();
        status.rows_per_sec = if started.elapsed().as_secs_f64() > 0.0 {
            rows_processed as f64 / started.elapsed().as_secs_f64()
        } else {
            0.0
        };
        store_compact_status(&tx, &status)?;
        tx.commit()?;

        emit_progress(
            &progress_cb,
            "compact_postings",
            rows_processed,
            postings_total,
        );
    }

    status.running = false;
    status.stage = "completed".to_string();
    status.rows_done_stage = status.rows_total_stage;
    status.checkpoint.phase = "completed".to_string();
    status.updated_at = Some(chrono::Utc::now().to_rfc3339());
    store_compact_status(conn, &status)?;
    emit_progress(
        &progress_cb,
        "compact_completed",
        status.rows_done_stage,
        status.rows_total_stage,
    );

    conn.execute_batch("DETACH DATABASE hyd_maps;")?;
    conn.execute_batch("DETACH DATABASE hyd_master;")?;
    Ok(Ok(()))
}

fn store_compact_status(conn: &Connection, status: &PtrCompactIndexStatus) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ptr_compact_state (
            id, running, stage, rows_done_stage, rows_total_stage, rows_per_sec,
            snapshot_dir, service_id, snapshot_max_index, updated_at,
            checkpoint_phase, checkpoint_last_hash_id, checkpoint_last_tag_id, checkpoint_last_service_hash_id
         ) VALUES (
            1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, strftime('%Y-%m-%dT%H:%M:%fZ','now'),
            ?9, ?10, ?11, ?12
         )
         ON CONFLICT(id) DO UPDATE SET
            running = excluded.running,
            stage = excluded.stage,
            rows_done_stage = excluded.rows_done_stage,
            rows_total_stage = excluded.rows_total_stage,
            rows_per_sec = excluded.rows_per_sec,
            snapshot_dir = excluded.snapshot_dir,
            service_id = excluded.service_id,
            snapshot_max_index = excluded.snapshot_max_index,
            updated_at = excluded.updated_at,
            checkpoint_phase = excluded.checkpoint_phase,
            checkpoint_last_hash_id = excluded.checkpoint_last_hash_id,
            checkpoint_last_tag_id = excluded.checkpoint_last_tag_id,
            checkpoint_last_service_hash_id = excluded.checkpoint_last_service_hash_id",
        rusqlite::params![
            if status.running { 1 } else { 0 },
            status.stage.as_str(),
            status.rows_done_stage,
            status.rows_total_stage,
            status.rows_per_sec,
            status.snapshot_dir.as_deref(),
            status.service_id,
            status.snapshot_max_index,
            status.checkpoint.phase.as_str(),
            status.checkpoint.last_hash_id,
            status.checkpoint.last_tag_id,
            status.checkpoint.last_service_hash_id
        ],
    )?;
    Ok(())
}

/// Fast row count estimate from sqlite_stat1 (PBI-026).
/// Returns None if stat1 is unavailable for this table.
fn fast_row_estimate(conn: &Connection, table: &str) -> Option<i64> {
    conn.query_row(
        "SELECT stat FROM sqlite_stat1 WHERE tbl = ?1 AND idx IS NOT NULL LIMIT 1",
        rusqlite::params![table],
        |row| {
            let stat: String = row.get(0)?;
            // stat column format: "nrow ncol1 ncol2 ..."
            // First token is the estimated row count.
            let nrow = stat
                .split_whitespace()
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            Ok(nrow)
        },
    )
    .ok()
    .filter(|&n| n > 0)
}

fn encode_delta_varint(sorted_ids: &[i64]) -> Vec<u8> {
    let mut out = Vec::with_capacity(sorted_ids.len() * 2);
    let mut prev: u64 = 0;
    for &id in sorted_ids {
        let id_u = if id < 0 { 0 } else { id as u64 };
        let delta = id_u.saturating_sub(prev);
        prev = id_u;
        push_uvarint(&mut out, delta);
    }
    out
}

fn push_uvarint(out: &mut Vec<u8>, mut n: u64) {
    while n >= 0x80 {
        out.push((n as u8) | 0x80);
        n >>= 7;
    }
    out.push(n as u8);
}

fn discover_library_db_path(conn: &Connection) -> Option<PathBuf> {
    let mut stmt = conn.prepare("PRAGMA database_list").ok()?;
    let mut rows = stmt.query([]).ok()?;
    while let Ok(Some(row)) = rows.next() {
        let name: String = row.get(1).ok()?;
        if name == "main" {
            let file: String = row.get(2).ok()?;
            let p = PathBuf::from(file);
            let db_dir = p.parent()?;
            let lib_db = db_dir.join("library.sqlite");
            return Some(lib_db);
        }
    }
    None
}

fn probe_max_update_index(probe: &HydrusSnapshotProbe) -> Result<i64, String> {
    let client_conn = open_readonly(&probe.client_db)?;
    if let Some(table) = &probe.updates_table {
        let sql = format!("SELECT COALESCE(MAX(update_index), 0) FROM {}", table);
        return scalar_i64(&client_conn, &sql);
    }

    let caches_db = probe.snapshot_dir.join("client.caches.db");
    if caches_db.exists() {
        let caches_conn = open_readonly(&caches_db)?;
        let update_table = format!("repository_updates_{}", probe.service_id);
        if table_exists(&caches_conn, &update_table)? {
            let sql = format!(
                "SELECT COALESCE(MAX(update_index), 0) FROM {}",
                update_table
            );
            return scalar_i64(&caches_conn, &sql);
        }
    }
    Ok(0)
}

fn detect_service_id(client_conn: &Connection, mappings_conn: &Connection) -> Result<i64, String> {
    let mapping_candidates = collect_service_ids(mappings_conn, "current_mappings_")?;
    let service_meta = load_service_meta(client_conn).unwrap_or_default();

    if !mapping_candidates.is_empty() {
        let mut best_repo: Option<(i64, i64)> = None; // (rows, service_id)
        let mut best_any: Option<(i64, i64)> = None; // (rows, service_id)
        let mut inspected: Vec<(i64, i64)> = Vec::new();
        for service_id in mapping_candidates {
            let table = format!("current_mappings_{}", service_id);
            let count = scalar_i64(mappings_conn, &format!("SELECT COUNT(*) FROM {}", table))?;
            inspected.push((service_id, count));
            best_any = match best_any {
                None => Some((count, service_id)),
                Some((prev_count, prev_id)) => {
                    if count > prev_count || (count == prev_count && service_id > prev_id) {
                        Some((count, service_id))
                    } else {
                        Some((prev_count, prev_id))
                    }
                }
            };
            if let Some((service_type, _name)) = service_meta.get(&service_id) {
                if *service_type == HYDRUS_SERVICE_TYPE_TAG_REPOSITORY {
                    best_repo = match best_repo {
                        None => Some((count, service_id)),
                        Some((prev_count, prev_id)) => {
                            if count > prev_count || (count == prev_count && service_id > prev_id) {
                                Some((count, service_id))
                            } else {
                                Some((prev_count, prev_id))
                            }
                        }
                    };
                }
            }
        }
        if let Some((_, chosen)) = best_repo {
            tracing::info!(
                chosen_service_id = chosen,
                candidates = ?inspected,
                "PTR bootstrap: auto-detected TAG_REPOSITORY service_id from current_mappings_*"
            );
            return Ok(chosen);
        }

        // If we have service metadata and all candidates are local/combined tags,
        // fail fast: this snapshot does not include a remote tag repository mapping
        // table suitable for PTR bootstrap.
        if !service_meta.is_empty() {
            let only_localish = inspected.iter().all(|(id, _)| {
                matches!(
                    service_meta.get(id).map(|(ty, _)| *ty),
                    Some(HYDRUS_SERVICE_TYPE_LOCAL_TAG | HYDRUS_SERVICE_TYPE_COMBINED_TAG)
                )
            });
            if only_localish {
                let labels: Vec<String> = inspected
                    .iter()
                    .map(|(id, count)| {
                        if let Some((ty, name)) = service_meta.get(id) {
                            format!("id={id} type={ty} name={name} rows={count}")
                        } else {
                            format!("id={id} rows={count}")
                        }
                    })
                    .collect();
                return Err(format!(
                    "Hydrus snapshot has only local/combined tag mapping services in current_mappings_* ({}) and no tag repository service. This cannot be used as PTR snapshot source.",
                    labels.join(", ")
                ));
            }
        }

        // Last-resort fallback when service metadata is unavailable: choose largest table.
        if let Some((_, chosen)) = best_any {
            tracing::warn!(
                chosen_service_id = chosen,
                candidates = ?inspected,
                "PTR bootstrap: service metadata unavailable; using largest current_mappings_* table"
            );
            return Ok(chosen);
        }
    }

    let update_candidates = collect_service_ids(client_conn, "repository_updates_")?;
    if let Some(chosen) = update_candidates.into_iter().max() {
        tracing::info!(
            chosen_service_id = chosen,
            "PTR bootstrap: auto-detected service_id from repository_updates_*"
        );
        return Ok(chosen);
    }

    Err(
        "Could not auto-detect PTR service id. Provide ptr_service_id explicitly or use a snapshot that contains current_mappings_* or repository_updates_* tables.".into()
    )
}

fn load_service_meta(
    client_conn: &Connection,
) -> Result<std::collections::HashMap<i64, (i64, String)>, String> {
    if !table_exists(client_conn, "services")? {
        return Ok(std::collections::HashMap::new());
    }
    let mut stmt = client_conn
        .prepare("SELECT service_id, service_type, name FROM services")
        .map_err(|e| format!("Failed to read services table: {e}"))?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| format!("Failed to query services table: {e}"))?;
    let mut out = std::collections::HashMap::new();
    for row in rows {
        let (id, ty, name) = row.map_err(|e| format!("Failed to parse services row: {e}"))?;
        out.insert(id, (ty, name));
    }
    Ok(out)
}

fn collect_service_ids(conn: &Connection, prefix: &str) -> Result<Vec<i64>, String> {
    let like_pattern = format!("{prefix}%");
    let mut stmt = conn
        .prepare(
            "SELECT name
             FROM sqlite_master
             WHERE type='table' AND name LIKE ?1",
        )
        .map_err(|e| format!("Failed to inspect schema for '{}': {e}", prefix))?;
    let rows = stmt
        .query_map([like_pattern], |row| row.get::<_, String>(0))
        .map_err(|e| format!("Failed to read schema for '{}': {e}", prefix))?;

    let mut out = Vec::new();
    for row in rows {
        let table = row.map_err(|e| format!("Failed to parse table name: {e}"))?;
        if let Some((_, suffix)) = table.rsplit_once('_') {
            if let Ok(id) = suffix.parse::<i64>() {
                out.push(id);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn open_readonly(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|e| format!("Failed to open {}: {e}", path.display()))
}

fn scalar_i64(conn: &Connection, sql: &str) -> Result<i64, String> {
    conn.query_row(sql, [], |row| row.get::<_, i64>(0))
        .map_err(|e| format!("Query failed '{}': {e}", sql))
}

fn ensure_table(conn: &Connection, table: &str) -> Result<(), String> {
    let exists = table_exists(conn, table)?;
    if exists {
        Ok(())
    } else {
        Err(format!("Required table missing: {}", table))
    }
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type='table' AND name = ?1 LIMIT 1",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .optional()
    .map(|v| v.is_some())
    .map_err(|e| format!("Failed checking table existence for '{}': {e}", table))
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
    if !is_identifier(table) {
        return Err(format!("Invalid table identifier '{}'", table));
    }
    let sql = format!("PRAGMA table_info({})", table);
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed preparing '{}': {e}", sql))?;
    let rows = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("Failed reading pragma for '{}': {e}", table))?;
    let mut cols = Vec::new();
    for row in rows {
        cols.push(row.map_err(|e| format!("Failed reading column metadata: {e}"))?);
    }
    Ok(cols)
}

fn has_col(cols: &[String], name: &str) -> bool {
    cols.iter().any(|c| c == name)
}

fn pick_col(cols: &[String], candidates: &[&str]) -> Result<String, String> {
    for candidate in candidates {
        if cols.iter().any(|c| c == candidate) {
            return Ok((*candidate).to_string());
        }
    }
    Err(format!(
        "None of candidate columns {:?} found in {:?}",
        candidates, cols
    ))
}

fn sql_quote_path(path: &Path) -> String {
    path.to_string_lossy().replace('\'', "''")
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}
