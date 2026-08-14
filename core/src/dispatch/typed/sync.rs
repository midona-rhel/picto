//! Handler functions for cloud sync: binding a library to a remote on a
//! file share, listing/creating/connecting remote libraries, and running
//! sync cycles. The remote is never deleted or overwritten from here.

use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::db::LibraryDatabase;
use crate::oplog::backend_fs::FsBackend;
use crate::oplog::remote_library::{
    create_remote_library, detect_share_roots, list_remote_libraries, read_remote_manifest,
    remote_library_root, RemoteLibraryInfo, RemoteLibraryManifest, ShareRootCandidate,
};
use crate::oplog::sync::{sync_cycle, SyncReport};
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::Domain;
use crate::state::AppState;

const KV_SHARE_ROOT: &str = "sync_share_root";
const KV_LIBRARY_NAME: &str = "sync_library_name";

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
}

#[derive(Debug, Serialize)]
pub struct SyncCycleResult {
    pub report: SyncReport,
}

// ─── Helpers ───────────────────────────────────────────────────────────────

fn binding(db: &LibraryDatabase) -> Result<Option<(PathBuf, String)>, String> {
    let root = db.kv_get(KV_SHARE_ROOT)?;
    let name = db.kv_get(KV_LIBRARY_NAME)?;
    match (root, name) {
        (Some(root), Some(name)) if !root.is_empty() && !name.is_empty() => {
            Ok(Some((PathBuf::from(root), name)))
        }
        _ => Ok(None),
    }
}

/// Run one sync cycle against the bound remote and, if remote ops changed
/// local truth, tell the frontend to refresh everything derived.
fn run_bound_sync(
    db: &Arc<LibraryDatabase>,
    blob_store: &crate::blob_store::BlobStore,
) -> Result<SyncReport, String> {
    let Some((share_root, name)) = binding(db)? else {
        return Err("This library is not connected to a cloud sync remote".to_string());
    };
    let backend = FsBackend::open(&remote_library_root(&share_root, &name))
        .map_err(|e| format!("Cannot open sync remote: {e}"))?;
    let report = sync_cycle(db, blob_store, &backend)?;
    if report.ops_applied > 0 {
        crate::events::emit_state_changed(
            "cloud_sync",
            ChangeImpact {
                domains: vec![
                    Domain::Files,
                    Domain::Folders,
                    Domain::SmartFolders,
                    Domain::Tags,
                    Domain::Sidebar,
                ],
                status_changed: Some(true),
                tags_changed: Some(true),
                tag_structure_changed: Some(true),
                media_metadata_changed: Some(true),
                compiler_batch_done: Some(true),
                ..Default::default()
            },
        );
    }
    Ok(report)
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn sync_get_status(
    state: &AppState,
    _input: SyncEmptyInput,
) -> Result<SyncStatus, String> {
    let db = state.engine.db();
    let bound = binding(db)?;
    Ok(SyncStatus {
        bound: bound.is_some(),
        share_root: bound.as_ref().map(|(root, _)| root.display().to_string()),
        library_name: bound.map(|(_, name)| name),
        library_uuid: db.kv_get("library_uuid")?,
        device_id: db.device_id().to_string(),
        pending_ops: db.pending_op_count()?,
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
    let share_root = PathBuf::from(input.share_root.clone());
    let name = input.name.trim().to_string();
    let report = tokio::task::spawn_blocking(move || -> Result<SyncReport, String> {
        let manifest = RemoteLibraryManifest {
            format_version: 1,
            library_uuid: db.library_uuid()?,
            name: name.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
            created_by_device: db.device_id().to_string(),
        };
        create_remote_library(&share_root, &manifest)?;
        db.kv_set(KV_SHARE_ROOT, &share_root.display().to_string())?;
        db.kv_set(KV_LIBRARY_NAME, &name)?;
        run_bound_sync(&db, &blob_store)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(SyncCycleResult { report })
}

pub async fn sync_connect_remote_library(
    state: &AppState,
    input: SyncConnectRemoteLibraryInput,
) -> Result<SyncCycleResult, String> {
    let db = state.engine.db_arc();
    let blob_store = state.blob_store.clone();
    let share_root = PathBuf::from(input.share_root.clone());
    let name = input.name.trim().to_string();
    let report = tokio::task::spawn_blocking(move || -> Result<SyncReport, String> {
        let manifest = read_remote_manifest(&share_root, &name)?;
        db.adopt_library_uuid(&manifest.library_uuid)?;
        db.kv_set(KV_SHARE_ROOT, &share_root.display().to_string())?;
        db.kv_set(KV_LIBRARY_NAME, &name)?;
        run_bound_sync(&db, &blob_store)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(SyncCycleResult { report })
}

pub async fn sync_disconnect(state: &AppState, _input: SyncEmptyInput) -> Result<(), String> {
    // Unbind only — the remote library stays exactly as it is.
    let db = state.engine.db();
    db.kv_delete(KV_SHARE_ROOT)?;
    db.kv_delete(KV_LIBRARY_NAME)?;
    Ok(())
}

pub async fn sync_now(state: &AppState, _input: SyncEmptyInput) -> Result<SyncCycleResult, String> {
    let db = state.engine.db_arc();
    let blob_store = state.blob_store.clone();
    let report = tokio::task::spawn_blocking(move || run_bound_sync(&db, &blob_store))
        .await
        .map_err(|e| e.to_string())??;
    Ok(SyncCycleResult { report })
}
