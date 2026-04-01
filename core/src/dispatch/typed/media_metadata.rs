//! Handler functions for media metadata operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetMediaEntityMetadataInput {
    pub hash: String,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_media_entity_metadata(
    state: &AppState,
    input: GetMediaEntityMetadataInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .engine
        .get_entity_all_metadata(&input.hash)?
        .ok_or_else(|| format!("Entity not found: {}", input.hash))?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_storage_stats(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (stats, breakdown) = state.engine.get_storage_stats()?;
    let blob_store = state.blob_store.clone();
    let (originals_disk, thumbnails_disk) =
        tokio::task::spawn_blocking(move || blob_store.disk_usage())
            .await
            .map_err(|e| e.to_string())?;

    let mut result = serde_json::to_value(&stats).map_err(|e| e.to_string())?;
    let obj = result.as_object_mut().ok_or("expected object")?;
    obj.insert(
        "breakdown".to_string(),
        serde_json::to_value(&breakdown).map_err(|e| e.to_string())?,
    );
    obj.insert(
        "originals_disk".to_string(),
        serde_json::Value::Number(originals_disk.into()),
    );
    obj.insert(
        "thumbnails_disk".to_string(),
        serde_json::Value::Number(thumbnails_disk.into()),
    );
    Ok(result)
}
