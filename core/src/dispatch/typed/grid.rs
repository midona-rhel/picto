//! Handler functions for grid and file query operations.

use std::time::Instant;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetGridPageSlimInput {
    pub query: crate::types::GridPageSlimQuery,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFilesMetadataBatchInput {
    pub hashes: Vec<String>,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_grid_page_slim(state: &AppState, input: GetGridPageSlimInput) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    let result =
        crate::grid::controller::GridController::get_grid_page_slim(&state.db, input.query)
            .await?;
    crate::perf::record_grid_page_slim(started.elapsed().as_secs_f64() * 1000.0);
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_file(state: &AppState, input: GetFileInput) -> Result<serde_json::Value, String> {
    let result =
        crate::metadata::controller::MetadataController::get_file(&state.db, input.hash)
            .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_files_metadata_batch(state: &AppState, input: GetFilesMetadataBatchInput) -> Result<serde_json::Value, String> {
    let result = crate::grid::controller::GridController::get_files_metadata_batch(
        &state.db,
        input.hashes,
    )
    .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_file_count(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let count = state.db.count_files(None).await?;
    serde_json::to_value(&count).map_err(|e| e.to_string())
}
