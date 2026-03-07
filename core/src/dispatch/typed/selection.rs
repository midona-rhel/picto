//! Typed command implementations for selection operations.

use std::collections::HashMap;
use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use crate::types::SelectionQuerySpec;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct AddTagsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RemoveTagsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetSelectionSummaryInput {
    pub selection: SelectionQuerySpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateRatingSelectionInput {
    pub selection: SelectionQuerySpec,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetNotesSelectionInput {
    pub selection: SelectionQuerySpec,
    pub notes: HashMap<String, String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetSourceUrlsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub urls: Vec<String>,
}

// ─── Command structs ───────────────────────────────────────────────────────

pub struct AddTagsSelection;
pub struct RemoveTagsSelection;
pub struct GetSelectionSummary;
pub struct UpdateRatingSelection;
pub struct SetNotesSelection;
pub struct SetSourceUrlsSelection;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for AddTagsSelection {
    const NAME: &'static str = "add_tags_selection";
    type Input = AddTagsSelectionInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for RemoveTagsSelection {
    const NAME: &'static str = "remove_tags_selection";
    type Input = RemoveTagsSelectionInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for GetSelectionSummary {
    const NAME: &'static str = "get_selection_summary";
    type Input = GetSelectionSummaryInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let started = std::time::Instant::now();
        let result = crate::selection::controller::SelectionController::get_selection_summary(
            &state.db, input.selection,
        ).await?;
        crate::perf::record_selection_summary(started.elapsed().as_secs_f64() * 1000.0);
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for UpdateRatingSelection {
    const NAME: &'static str = "update_rating_selection";
    type Input = UpdateRatingSelectionInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for SetNotesSelection {
    const NAME: &'static str = "set_notes_selection";
    type Input = SetNotesSelectionInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for SetSourceUrlsSelection {
    const NAME: &'static str = "set_source_urls_selection";
    type Input = SetSourceUrlsSelectionInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        AddTagsSelection::NAME => Some(run_typed::<AddTagsSelection>(state, args).await),
        RemoveTagsSelection::NAME => Some(run_typed::<RemoveTagsSelection>(state, args).await),
        GetSelectionSummary::NAME => Some(run_typed::<GetSelectionSummary>(state, args).await),
        UpdateRatingSelection::NAME => Some(run_typed::<UpdateRatingSelection>(state, args).await),
        SetNotesSelection::NAME => Some(run_typed::<SetNotesSelection>(state, args).await),
        SetSourceUrlsSelection::NAME => Some(run_typed::<SetSourceUrlsSelection>(state, args).await),
        _ => None,
    }
}
