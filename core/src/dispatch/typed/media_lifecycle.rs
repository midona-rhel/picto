//! Handler functions for media lifecycle operations:
//! import, status changes, deletion, and FTS rebuild.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddMediaInput {
    pub paths: Vec<String>,
    #[serde(default)]
    pub tag_strings: Option<Vec<String>>,
    #[serde(default)]
    pub source_urls: Option<Vec<String>>,
    #[serde(default = "default_initial_status")]
    #[ts(type = "number")]
    pub initial_status: i64,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub parent_folder_id: Option<i64>,
    #[serde(default)]
    pub preserve_structure: bool,
    #[serde(default)]
    pub collection_name: Option<String>,
}

fn default_initial_status() -> i64 {
    1
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn add_media(state: &AppState, input: AddMediaInput) -> Result<(), String> {
    if input.paths.is_empty() {
        return Err("At least one media path is required".to_string());
    }
    if input.collection_name.is_some() && input.preserve_structure {
        return Err("Collection imports cannot preserve folder structure".to_string());
    }
    if input.preserve_structure && input.paths.len() != 1 {
        return Err("Preserving folder structure requires exactly one folder path".to_string());
    }

    // Reject paths inside the library directory to prevent circular imports
    let library_root = &state.library_root;
    for p in &input.paths {
        if let Ok(canonical) = std::fs::canonicalize(p) {
            if canonical.starts_with(library_root) {
                return Err(format!(
                    "Cannot import files from inside the library directory: {}",
                    canonical.display()
                ));
            }
        }
    }

    state
        .engine
        .add_media(
            input.paths,
            input.tag_strings,
            input.source_urls,
            input.initial_status,
            input.parent_folder_id,
            input.preserve_structure,
            input.collection_name,
            Some(&state.library_root),
        )
        .await
}

#[derive(Debug, Serialize)]
pub struct SweepOrphanedBlobsResult {
    pub deleted_count: u64,
    pub freed_bytes: u64,
}

/// Remove blob files no media_file row references anymore. Blobs younger
/// than ten minutes are left alone so in-flight ingest is never raced.
pub async fn sweep_orphaned_blobs(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let referenced: std::collections::HashSet<String> = state.engine.db().with_read(|conn| {
        let mut stmt = conn.prepare("SELECT file_hash FROM media_file")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<std::collections::HashSet<_>>>()
    })?;
    let (deleted_count, freed_bytes) = state
        .blob_store
        .sweep_orphans(&referenced, std::time::Duration::from_secs(600));
    tracing::info!(deleted_count, freed_bytes, "orphaned blob sweep complete");
    Ok(serde_json::to_value(SweepOrphanedBlobsResult {
        deleted_count,
        freed_bytes,
    })
    .map_err(|e| e.to_string())?)
}
