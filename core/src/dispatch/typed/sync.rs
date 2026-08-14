//! Handler functions for cloud sync: binding a library to a remote on a
//! file share, listing/creating/connecting remote libraries, and running
//! sync cycles. Immutable remote library objects are never overwritten here.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::oplog::remote_library::{
    create_remote_library, detect_share_roots, list_remote_libraries, read_remote_manifest,
    RemoteLibraryInfo, RemoteLibraryManifest, ShareRootCandidate,
};
use crate::oplog::sync::{
    binding, clear_binding, run_bound_sync, run_serialized_sync, set_binding, SyncReport,
    KV_LAST_ERROR, KV_LAST_REPORT, KV_LAST_SUCCESS_AT,
};
use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SyncListRemoteLibrariesInput {
    pub share_root: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SyncCreateRemoteLibraryInput {
    pub share_root: String,
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SyncConnectRemoteLibraryInput {
    pub share_root: String,
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SyncEmptyInput {}

// ─── Outputs ───────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SyncStatus {
    pub bound: bool,
    pub share_root: Option<String>,
    pub library_name: Option<String>,
    pub library_uuid: Option<String>,
    pub device_id: String,
    pub pending_ops: i64,
    pub pending_remote_ops: usize,
    pub more_remote_work: bool,
    pub waiting_for_prerequisites: bool,
    pub missing_blobs: i64,
    pub failed_blobs: i64,
    pub syncing: bool,
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    pub last_report: Option<SyncReport>,
}

#[derive(Debug, Serialize)]
pub struct SyncCycleResult {
    pub report: SyncReport,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn sync_get_status(
    state: &AppState,
    _input: SyncEmptyInput,
) -> Result<SyncStatus, String> {
    let db = state.engine.db();
    let bound = binding(db)?;
    let last_report: Option<SyncReport> = db
        .kv_get(KV_LAST_REPORT)?
        .map(|value| serde_json::from_str(&value).map_err(|error| error.to_string()))
        .transpose()?;
    let (missing_blobs, failed_blobs) = db.sync_missing_blob_counts()?;
    let pending_remote_ops = last_report
        .as_ref()
        .map_or(0, |report| report.pending_remote_ops);
    let more_remote_work = last_report
        .as_ref()
        .is_some_and(|report| report.more_remote_work);
    let waiting_for_prerequisites = missing_blobs > 0
        || last_report
            .as_ref()
            .is_some_and(|report| report.waiting_for_prerequisites);
    Ok(SyncStatus {
        bound: bound.is_some(),
        share_root: bound.as_ref().map(|(root, _)| root.display().to_string()),
        library_name: bound.map(|(_, name)| name),
        library_uuid: db.kv_get("library_uuid")?,
        device_id: db.device_id().to_string(),
        pending_ops: db.pending_op_count()?,
        pending_remote_ops,
        more_remote_work,
        waiting_for_prerequisites,
        missing_blobs,
        failed_blobs,
        syncing: state.sync_cycle_lock.try_lock().is_err(),
        last_success_at: db.kv_get(KV_LAST_SUCCESS_AT)?,
        last_error: db.kv_get(KV_LAST_ERROR)?,
        last_report,
    })
}

pub async fn sync_detect_share_roots(
    _state: &AppState,
    _input: SyncEmptyInput,
) -> Result<Vec<ShareRootCandidate>, String> {
    tokio::task::spawn_blocking(|| Ok(detect_share_roots()))
        .await
        .map_err(|e| e.to_string())?
}

pub async fn sync_list_remote_libraries(
    _state: &AppState,
    input: SyncListRemoteLibrariesInput,
) -> Result<Vec<RemoteLibraryInfo>, String> {
    let share_root = PathBuf::from(input.share_root);
    tokio::task::spawn_blocking(move || list_remote_libraries(&share_root))
        .await
        .map_err(|e| e.to_string())?
}

pub async fn sync_create_remote_library(
    state: &AppState,
    input: SyncCreateRemoteLibraryInput,
) -> Result<SyncCycleResult, String> {
    let db = state.engine.db_arc();
    let blob_store = state.blob_store.clone();
    let cycle_lock = state.sync_cycle_lock.clone();
    let share_root = PathBuf::from(input.share_root.clone());
    let name = input.name.trim().to_string();
    let _guard = cycle_lock.lock().await;
    let setup_db = db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let manifest = RemoteLibraryManifest {
            format_version: 1,
            library_uuid: setup_db.library_uuid()?,
            name: name.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by_device: setup_db.device_id().to_string(),
        };
        create_remote_library(&share_root, &manifest)?;
        set_binding(&setup_db, &share_root, &name)
    })
    .await
    .map_err(|e| e.to_string())??;
    let report = tokio::task::spawn_blocking(move || run_bound_sync(&db, &blob_store))
        .await
        .map_err(|error| format!("sync worker failed: {error}"))??;
    Ok(SyncCycleResult { report })
}

pub async fn sync_connect_remote_library(
    state: &AppState,
    input: SyncConnectRemoteLibraryInput,
) -> Result<SyncCycleResult, String> {
    let db = state.engine.db_arc();
    let blob_store = state.blob_store.clone();
    let cycle_lock = state.sync_cycle_lock.clone();
    let share_root = PathBuf::from(input.share_root.clone());
    let name = input.name.trim().to_string();
    let _guard = cycle_lock.lock().await;
    let setup_db = db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        let manifest = read_remote_manifest(&share_root, &name)?;
        setup_db.adopt_library_uuid(&manifest.library_uuid)?;
        set_binding(&setup_db, &share_root, &name)
    })
    .await
    .map_err(|e| e.to_string())??;
    let report = tokio::task::spawn_blocking(move || run_bound_sync(&db, &blob_store))
        .await
        .map_err(|error| format!("sync worker failed: {error}"))??;
    Ok(SyncCycleResult { report })
}

pub async fn sync_disconnect(state: &AppState, _input: SyncEmptyInput) -> Result<(), String> {
    // Unbind only — the remote library stays exactly as it is.
    let _guard = state.sync_cycle_lock.lock().await;
    let db = state.engine.db();
    clear_binding(db)
}

pub async fn sync_now(state: &AppState, _input: SyncEmptyInput) -> Result<SyncCycleResult, String> {
    let db = state.engine.db_arc();
    let blob_store = state.blob_store.clone();
    let report = run_serialized_sync(db, blob_store, state.sync_cycle_lock.clone()).await?;
    Ok(SyncCycleResult { report })
}
