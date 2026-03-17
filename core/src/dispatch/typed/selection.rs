//! Handler functions for selection operations.

use serde::Deserialize;
use std::collections::HashMap;
use ts_rs::TS;

use crate::state::AppState;
use crate::types::SelectionQuerySpec;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddTagsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveTagsSelectionInput {
    pub selection: SelectionQuerySpec,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetSelectionSummaryInput {
    pub selection: SelectionQuerySpec,
}

/// Unified selection metadata update. All fields except `selection` are optional.
#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateSelectionMetadataInput {
    pub selection: SelectionQuerySpec,
    #[serde(default, deserialize_with = "deserialize_some")]
    #[ts(type = "number | null")]
    pub rating: Option<Option<i64>>,
    #[serde(default)]
    pub notes: Option<HashMap<String, String>>,
    #[serde(default)]
    pub source_urls: Option<Vec<String>>,
}

use super::super::common::deserialize_some;

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn add_tags_selection(
    state: &AppState,
    input: AddTagsSelectionInput,
) -> Result<usize, String> {
    let count = crate::selection::mutations::add_tags_selection(
        &state.db,
        input.selection,
        input.tag_strings,
    )
    .await?;
    if count > 0 {
        crate::events::emit_mutation(
            "add_tags_selection",
            crate::runtime_contract::mutation_builder::MutationImpact::selection_batch_tags(),
        );
    }
    Ok(count)
}

pub async fn remove_tags_selection(
    state: &AppState,
    input: RemoveTagsSelectionInput,
) -> Result<usize, String> {
    let count = crate::selection::mutations::remove_tags_selection(
        &state.db,
        input.selection,
        input.tag_strings,
    )
    .await?;
    if count > 0 {
        crate::events::emit_mutation(
            "remove_tags_selection",
            crate::runtime_contract::mutation_builder::MutationImpact::selection_batch_tags(),
        );
    }
    Ok(count)
}

pub async fn get_selection_summary(
    state: &AppState,
    input: GetSelectionSummaryInput,
) -> Result<serde_json::Value, String> {
    let started = std::time::Instant::now();
    let result =
        crate::selection::summary::get_selection_summary(&state.db, input.selection).await?;
    crate::perf::record_selection_summary(started.elapsed().as_secs_f64() * 1000.0);
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn update_selection_metadata(
    state: &AppState,
    input: UpdateSelectionMetadataInput,
) -> Result<usize, String> {
    let mut total_count = 0;
    let mut need_grid = false;

    if let Some(rating) = input.rating {
        let count = crate::selection::mutations::update_rating_selection(
            &state.db,
            input.selection.clone(),
            rating,
        )
        .await?;
        total_count += count;
        need_grid = true;
    }

    if let Some(notes) = input.notes {
        let count = crate::selection::mutations::set_notes_selection(
            &state.db,
            input.selection.clone(),
            notes,
        )
        .await?;
        total_count += count;
    }

    if let Some(urls) = input.source_urls {
        let count = crate::selection::mutations::set_source_urls_selection(
            &state.db,
            input.selection,
            urls,
        )
        .await?;
        total_count += count;
    }

    if total_count > 0 {
        let impact = if need_grid {
            crate::runtime_contract::mutation_builder::MutationImpact::selection_metadata_grid()
        } else {
            crate::runtime_contract::mutation_builder::MutationImpact::selection_metadata()
        };
        crate::events::emit_mutation("update_selection_metadata", impact);
    }
    Ok(total_count)
}
