//! Typed command implementations for media metadata operations.

use std::collections::HashMap;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFileAllMetadataInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFileTagsDisplayInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFileParentsInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateRatingInput {
    pub hash: String,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetFileNameInput {
    pub hash: String,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetFileNotesInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetFileNotesInput {
    pub hash: String,
    pub notes: HashMap<String, String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct IncrementViewCountInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetSourceUrlsInput {
    pub hash: String,
    pub urls: Vec<String>,
}

// ─── Command structs ───────────────────────────────────────────────────────

pub struct GetFileAllMetadata;
pub struct GetFileTagsDisplay;
pub struct GetFileParents;
pub struct UpdateRating;
pub struct SetFileName;
pub struct GetFileNotes;
pub struct SetFileNotes;
pub struct IncrementViewCount;
pub struct SetSourceUrls;
pub struct GetStorageStats;
pub struct GetImageStorageStats;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for GetFileAllMetadata {
    const NAME: &'static str = "get_file_all_metadata";
    type Input = GetFileAllMetadataInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::metadata::controller::MetadataController::get_file_all_metadata(
            &state.db,
            &state.ptr_db,
            input.hash,
        )
        .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetFileTagsDisplay {
    const NAME: &'static str = "get_file_tags_display";
    type Input = GetFileTagsDisplayInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::metadata::controller::MetadataController::get_file_tags_display(
            &state.db,
            &state.ptr_db,
            input.hash,
        )
        .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetFileParents {
    const NAME: &'static str = "get_file_parents";
    type Input = GetFileParentsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::metadata::controller::MetadataController::get_file_parents(&state.db, input.hash)
                .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for UpdateRating {
    const NAME: &'static str = "update_rating";
    type Input = UpdateRatingInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hash_clone = input.hash.clone();
        crate::metadata::controller::MetadataController::update_rating(
            &state.db,
            input.hash,
            input.rating,
        )
        .await?;
        crate::events::emit_mutation(
            "update_rating",
            crate::events::MutationImpact::file_metadata(hash_clone),
        );
        Ok(())
    }
}

impl TypedCommand for SetFileName {
    const NAME: &'static str = "set_file_name";
    type Input = SetFileNameInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hash_clone = input.hash.clone();
        crate::metadata::controller::MetadataController::set_file_name(
            &state.db,
            input.hash,
            input.name,
        )
        .await?;
        crate::events::emit_mutation(
            "set_file_name",
            crate::events::MutationImpact::file_metadata(hash_clone),
        );
        Ok(())
    }
}

impl TypedCommand for GetFileNotes {
    const NAME: &'static str = "get_file_notes";
    type Input = GetFileNotesInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::metadata::controller::MetadataController::get_file_notes(&state.db, input.hash)
                .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for SetFileNotes {
    const NAME: &'static str = "set_file_notes";
    type Input = SetFileNotesInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hash_clone = input.hash.clone();
        crate::metadata::controller::MetadataController::set_file_notes(
            &state.db,
            input.hash,
            input.notes,
        )
        .await?;
        crate::events::emit_mutation(
            "set_file_notes",
            crate::events::MutationImpact::file_metadata(hash_clone),
        );
        Ok(())
    }
}

impl TypedCommand for IncrementViewCount {
    const NAME: &'static str = "increment_view_count";
    type Input = IncrementViewCountInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hash_clone = input.hash.clone();
        crate::metadata::controller::MetadataController::increment_view_count(
            &state.db,
            input.hash,
        )
        .await?;
        crate::events::emit_mutation(
            "increment_view_count",
            crate::events::MutationImpact::file_metadata(hash_clone)
                .domains(&[crate::events::Domain::Files, crate::events::Domain::Sidebar])
                .sidebar_tree()
                .grid_scopes(vec!["system:recently_viewed".to_string()]),
        );
        Ok(())
    }
}

impl TypedCommand for SetSourceUrls {
    const NAME: &'static str = "set_source_urls";
    type Input = SetSourceUrlsInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let urls_json = if input.urls.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&input.urls).map_err(|e| e.to_string())?)
        };
        state
            .db
            .set_source_urls(&input.hash, urls_json.as_deref())
            .await?;
        crate::events::emit_mutation(
            "set_source_urls",
            crate::events::MutationImpact::file_metadata(input.hash),
        );
        Ok(())
    }
}

impl TypedCommand for GetStorageStats {
    const NAME: &'static str = "get_storage_stats";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let file_count = state.db.count_files(None).await?;
        serde_json::to_value(&crate::types::StorageStats { file_count })
            .map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetImageStorageStats {
    const NAME: &'static str = "get_image_storage_stats";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let stats = state.db.aggregate_file_stats().await?;
        serde_json::to_value(&stats).map_err(|e| e.to_string())
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        GetFileAllMetadata::NAME => Some(run_typed::<GetFileAllMetadata>(state, args).await),
        GetFileTagsDisplay::NAME => Some(run_typed::<GetFileTagsDisplay>(state, args).await),
        GetFileParents::NAME => Some(run_typed::<GetFileParents>(state, args).await),
        UpdateRating::NAME => Some(run_typed::<UpdateRating>(state, args).await),
        SetFileName::NAME => Some(run_typed::<SetFileName>(state, args).await),
        GetFileNotes::NAME => Some(run_typed::<GetFileNotes>(state, args).await),
        SetFileNotes::NAME => Some(run_typed::<SetFileNotes>(state, args).await),
        IncrementViewCount::NAME => Some(run_typed::<IncrementViewCount>(state, args).await),
        SetSourceUrls::NAME => Some(run_typed::<SetSourceUrls>(state, args).await),
        GetStorageStats::NAME => Some(run_typed::<GetStorageStats>(state, args).await),
        GetImageStorageStats::NAME => Some(run_typed::<GetImageStorageStats>(state, args).await),
        _ => None,
    }
}
