//! Handler functions for grid and file query operations.

use std::time::Instant;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetGridPageSlimInput {
    pub query: crate::types::GridPageSlimQuery,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetEntityInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetEntitiesMetadataBatchInput {
    pub hashes: Vec<String>,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_grid_page_slim(state: &AppState, input: GetGridPageSlimInput) -> Result<serde_json::Value, String> {
    let started = Instant::now();
    let result =
        crate::grid::query::get_grid_page_slim(&state.db, input.query)
            .await?;
    crate::perf::record_grid_page_slim(started.elapsed().as_secs_f64() * 1000.0);

    // Backfill missing dominant colors in the background.
    let missing_color_hashes: Vec<String> = result
        .items
        .iter()
        .filter(|item| item.dominant_color_hex.is_none() && item.mime.starts_with("image/"))
        .map(|item| item.hash.clone())
        .collect();
    if !missing_color_hashes.is_empty() {
        let db = state.db.clone();
        let blob_store = state.blob_store.clone();
        tokio::spawn(async move {
            super::media_io::backfill_missing_colors(&db, &blob_store, &missing_color_hashes).await;
        });
    }

    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_grid_outline(state: &AppState, input: GetGridPageSlimInput) -> Result<serde_json::Value, String> {
    let result = crate::grid::query::get_grid_outline(&state.db, input.query).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_entity(
    state: &AppState,
    input: GetEntityInput,
) -> Result<serde_json::Value, String> {
    let entity = state.db.get_entity_details_by_hash(&input.hash).await?;
    let result = entity.map(crate::types::EntityInfo::from);
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_entities_metadata_batch(
    state: &AppState,
    input: GetEntitiesMetadataBatchInput,
) -> Result<serde_json::Value, String> {
    let result = crate::grid::metadata::get_entities_metadata_batch(
        &state.db,
        input.hashes,
    )
    .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}
