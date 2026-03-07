//! Typed command implementations for grid and file query operations.

use std::time::Instant;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetGridPageSlimInput {
    pub query: crate::types::GridPageSlimQuery,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFileInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFilesMetadataBatchInput {
    pub hashes: Vec<String>,
}

// ─── Command structs ───────────────────────────────────────────────────────

pub struct GetGridPageSlim;
pub struct GetFile;
pub struct GetFilesMetadataBatch;
pub struct GetFileCount;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for GetGridPageSlim {
    const NAME: &'static str = "get_grid_page_slim";
    type Input = GetGridPageSlimInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let started = Instant::now();
        let result =
            crate::grid::controller::GridController::get_grid_page_slim(&state.db, input.query)
                .await?;
        crate::perf::record_grid_page_slim(started.elapsed().as_secs_f64() * 1000.0);
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetFile {
    const NAME: &'static str = "get_file";
    type Input = GetFileInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::metadata::controller::MetadataController::get_file(&state.db, input.hash)
                .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetFilesMetadataBatch {
    const NAME: &'static str = "get_files_metadata_batch";
    type Input = GetFilesMetadataBatchInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::grid::controller::GridController::get_files_metadata_batch(
            &state.db,
            &state.ptr_db,
            input.hashes,
        )
        .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetFileCount {
    const NAME: &'static str = "get_file_count";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let count = state.db.count_files(None).await?;
        serde_json::to_value(&count).map_err(|e| e.to_string())
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        GetGridPageSlim::NAME => Some(run_typed::<GetGridPageSlim>(state, args).await),
        GetFile::NAME => Some(run_typed::<GetFile>(state, args).await),
        GetFilesMetadataBatch::NAME => {
            Some(run_typed::<GetFilesMetadataBatch>(state, args).await)
        }
        GetFileCount::NAME => Some(run_typed::<GetFileCount>(state, args).await),
        _ => None,
    }
}
