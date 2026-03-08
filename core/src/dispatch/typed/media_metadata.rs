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

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileTagsDisplayInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileParentsInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileNotesInput {
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
    pub increment_view_count: Option<bool>,
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

pub async fn get_file_tags_display(state: &AppState, input: GetFileTagsDisplayInput) -> Result<serde_json::Value, String> {
    let result = crate::metadata::controller::MetadataController::get_file_tags_display(
        &state.db, input.hash,
    ).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_file_parents(state: &AppState, input: GetFileParentsInput) -> Result<serde_json::Value, String> {
    let result =
        crate::metadata::controller::MetadataController::get_file_parents(&state.db, input.hash)
            .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_file_notes(state: &AppState, input: GetFileNotesInput) -> Result<serde_json::Value, String> {
    let file = state.db.get_file_by_hash(&input.hash).await?;
    let notes: Option<HashMap<String, String>> = file
        .and_then(|f| f.notes.as_deref().and_then(|s| serde_json::from_str(s).ok()));
    serde_json::to_value(&notes).map_err(|e| e.to_string())
}

pub async fn update_file_metadata(state: &AppState, input: UpdateFileMetadataInput) -> Result<(), String> {
    let hash = input.hash;

    if let Some(rating) = input.rating {
        state.db.update_rating(&hash, rating).await?;
    }

    if let Some(name) = input.name {
        state.db.set_file_name(&hash, name.as_deref()).await?;
    }

    if let Some(notes) = input.notes {
        let json = serde_json::to_string(&notes).map_err(|e| e.to_string())?;
        state.db.set_notes(&hash, Some(&json)).await?;
    }

    if input.increment_view_count == Some(true) {
        state.db.increment_view_count(&hash).await?;
    }

    if let Some(ref urls) = input.source_urls {
        let urls_json = if urls.is_empty() {
            None
        } else {
            Some(serde_json::to_string(urls).map_err(|e| e.to_string())?)
        };
        state.db.set_source_urls(&hash, urls_json.as_deref()).await?;
    }

    let mut impact = crate::events::MutationImpact::file_metadata(hash);
    if input.increment_view_count == Some(true) {
        impact = impact
            .domains(&[crate::events::Domain::Files, crate::events::Domain::Sidebar])
            .extra_grid_scopes(vec!["system:recently_viewed".to_string()]);
    }
    crate::events::emit_mutation("update_file_metadata", impact);
    Ok(())
}

pub async fn get_storage_stats(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let stats = state.db.aggregate_file_stats().await?;
    serde_json::to_value(&stats).map_err(|e| e.to_string())
}
