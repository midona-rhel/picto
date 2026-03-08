//! Handler functions for media metadata operations.

use std::collections::HashMap;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileAllMetadataInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileTagsDisplayInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileParentsInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateRatingInput {
    pub hash: String,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SetFileNameInput {
    pub hash: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileNotesInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SetFileNotesInput {
    pub hash: String,
    pub notes: HashMap<String, String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct IncrementViewCountInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SetSourceUrlsInput {
    pub hash: String,
    pub urls: Vec<String>,
}

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

pub async fn update_rating(state: &AppState, input: UpdateRatingInput) -> Result<(), String> {
    let hash_clone = input.hash.clone();
    crate::metadata::controller::MetadataController::update_rating(
        &state.db, input.hash, input.rating,
    ).await?;
    crate::events::emit_mutation(
        "update_rating",
        crate::events::MutationImpact::file_metadata(hash_clone),
    );
    Ok(())
}

pub async fn set_file_name(state: &AppState, input: SetFileNameInput) -> Result<(), String> {
    let hash_clone = input.hash.clone();
    crate::metadata::controller::MetadataController::set_file_name(
        &state.db, input.hash, input.name,
    ).await?;
    crate::events::emit_mutation(
        "set_file_name",
        crate::events::MutationImpact::file_metadata(hash_clone),
    );
    Ok(())
}

pub async fn get_file_notes(state: &AppState, input: GetFileNotesInput) -> Result<serde_json::Value, String> {
    let result =
        crate::metadata::controller::MetadataController::get_file_notes(&state.db, input.hash)
            .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn set_file_notes(state: &AppState, input: SetFileNotesInput) -> Result<(), String> {
    let hash_clone = input.hash.clone();
    crate::metadata::controller::MetadataController::set_file_notes(
        &state.db, input.hash, input.notes,
    ).await?;
    crate::events::emit_mutation(
        "set_file_notes",
        crate::events::MutationImpact::file_metadata(hash_clone),
    );
    Ok(())
}

pub async fn increment_view_count(state: &AppState, input: IncrementViewCountInput) -> Result<(), String> {
    let hash_clone = input.hash.clone();
    crate::metadata::controller::MetadataController::increment_view_count(
        &state.db, input.hash,
    ).await?;
    crate::events::emit_mutation(
        "increment_view_count",
        crate::events::MutationImpact::file_metadata(hash_clone)
            .domains(&[crate::events::Domain::Files, crate::events::Domain::Sidebar])
            .extra_grid_scopes(vec!["system:recently_viewed".to_string()]),
    );
    Ok(())
}

pub async fn set_source_urls(state: &AppState, input: SetSourceUrlsInput) -> Result<(), String> {
    let urls_json = if input.urls.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&input.urls).map_err(|e| e.to_string())?)
    };
    state.db.set_source_urls(&input.hash, urls_json.as_deref()).await?;
    crate::events::emit_mutation(
        "set_source_urls",
        crate::events::MutationImpact::file_metadata(input.hash),
    );
    Ok(())
}

pub async fn get_storage_stats(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let file_count = state.db.count_files(None).await?;
    serde_json::to_value(&crate::types::StorageStats { file_count })
        .map_err(|e| e.to_string())
}

pub async fn get_image_storage_stats(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let stats = state.db.aggregate_file_stats().await?;
    serde_json::to_value(&stats).map_err(|e| e.to_string())
}
