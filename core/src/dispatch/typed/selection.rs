//! Handler functions for selection operations.

use std::collections::HashMap;
use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use crate::types::SelectionQuerySpec;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct AddTagsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveTagsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetSelectionSummaryInput {
    pub selection: SelectionQuerySpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateRatingSelectionInput {
    pub selection: SelectionQuerySpec,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SetNotesSelectionInput {
    pub selection: SelectionQuerySpec,
    pub notes: HashMap<String, String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SetSourceUrlsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub urls: Vec<String>,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn add_tags_selection(state: &AppState, input: AddTagsSelectionInput) -> Result<usize, String> {
    let count = crate::selection::controller::SelectionController::add_tags_selection(
        &state.db, input.selection, input.tag_strings,
    ).await?;
    if count > 0 {
        crate::events::emit_mutation(
            "add_tags_selection",
            crate::events::MutationImpact::selection_batch_tags(),
        );
    }
    Ok(count)
}

pub async fn remove_tags_selection(state: &AppState, input: RemoveTagsSelectionInput) -> Result<usize, String> {
    let count = crate::selection::controller::SelectionController::remove_tags_selection(
        &state.db, input.selection, input.tag_strings,
    ).await?;
    if count > 0 {
        crate::events::emit_mutation(
            "remove_tags_selection",
            crate::events::MutationImpact::selection_batch_tags(),
        );
    }
    Ok(count)
}

pub async fn get_selection_summary(state: &AppState, input: GetSelectionSummaryInput) -> Result<serde_json::Value, String> {
    let started = std::time::Instant::now();
    let result = crate::selection::controller::SelectionController::get_selection_summary(
        &state.db, input.selection,
    ).await?;
    crate::perf::record_selection_summary(started.elapsed().as_secs_f64() * 1000.0);
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn update_rating_selection(state: &AppState, input: UpdateRatingSelectionInput) -> Result<usize, String> {
    let count = crate::selection::controller::SelectionController::update_rating_selection(
        &state.db, input.selection, input.rating,
    ).await?;
    if count > 0 {
        crate::events::emit_mutation(
            "update_rating_selection",
            crate::events::MutationImpact::selection_metadata_grid(),
        );
    }
    Ok(count)
}

pub async fn set_notes_selection(state: &AppState, input: SetNotesSelectionInput) -> Result<usize, String> {
    let count = crate::selection::controller::SelectionController::set_notes_selection(
        &state.db, input.selection, input.notes,
    ).await?;
    if count > 0 {
        crate::events::emit_mutation(
            "set_notes_selection",
            crate::events::MutationImpact::selection_metadata(),
        );
    }
    Ok(count)
}

pub async fn set_source_urls_selection(state: &AppState, input: SetSourceUrlsSelectionInput) -> Result<usize, String> {
    let count = crate::selection::controller::SelectionController::set_source_urls_selection(
        &state.db, input.selection, input.urls,
    ).await?;
    if count > 0 {
        crate::events::emit_mutation(
            "set_source_urls_selection",
            crate::events::MutationImpact::selection_metadata(),
        );
    }
    Ok(count)
}
