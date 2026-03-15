//! Handler functions for media metadata operations.

use std::collections::HashMap;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileAllMetadataInput {
    pub hash: String,
}

/// Unified metadata update. All fields except `hash` are optional —
/// only present fields are applied. Use `null` to clear rating/name.
#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateFileMetadataInput {
    pub hash: String,
    #[serde(default, deserialize_with = "deserialize_some")]
    #[ts(type = "number | null")]
    pub rating: Option<Option<i64>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    #[ts(type = "string | null")]
    pub name: Option<Option<String>>,
    #[serde(default)]
    pub notes: Option<HashMap<String, String>>,
    #[serde(default)]
    pub source_urls: Option<Vec<String>>,
}

use super::super::common::deserialize_some;

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_file_all_metadata(state: &AppState, input: GetFileAllMetadataInput) -> Result<serde_json::Value, String> {
    let result = crate::metadata::controller::MetadataController::get_file_all_metadata(
        &state.db, input.hash,
    ).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn update_file_metadata(state: &AppState, input: UpdateFileMetadataInput) -> Result<(), String> {
    let hash = input.hash;

    if let Some(rating) = input.rating {
        // Cascade rating to collection members
        let hashes = state.db.expand_hashes_for_collections(&[hash.clone()]).await?;
        for h in &hashes {
            state.db.update_rating(h, rating).await?;
        }
    }

    if let Some(name) = input.name {
        state.db.set_file_name(&hash, name.as_deref()).await?;
    }

    if let Some(notes) = input.notes {
        let json = serde_json::to_string(&notes).map_err(|e| e.to_string())?;
        state.db.set_notes(&hash, Some(&json)).await?;
    }

    if let Some(ref urls) = input.source_urls {
        let urls_json = if urls.is_empty() {
            None
        } else {
            Some(serde_json::to_string(urls).map_err(|e| e.to_string())?)
        };
        state.db.set_source_urls(&hash, urls_json.as_deref()).await?;
    }

    let impact = crate::events::MutationImpact::file_metadata(hash);
    crate::events::emit_mutation("update_file_metadata", impact);
    Ok(())
}

pub async fn get_storage_stats(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let stats = state.db.aggregate_file_stats().await?;
    serde_json::to_value(&stats).map_err(|e| e.to_string())
}
