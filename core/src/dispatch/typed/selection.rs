//! Handler functions for selection operations.

use serde::Deserialize;
use std::collections::HashMap;
use ts_rs::TS;

use crate::runtime_contract::state_change::MediaMetadataField;
use crate::sqlite::EntityExpansionMode;
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

fn descendant_hashes(top_level_hashes: &[String], effective_hashes: Vec<String>) -> Vec<String> {
    let top_level: std::collections::HashSet<&str> =
        top_level_hashes.iter().map(String::as_str).collect();
    effective_hashes
        .into_iter()
        .filter(|hash| !top_level.contains(hash.as_str()))
        .collect()
}

// ─── Handlers ──────────────────────────────────────────────────────────────

// Legacy-only: bulk selection mutations not yet in rebuilt frontend.
pub async fn add_tags_selection(
    state: &AppState,
    input: AddTagsSelectionInput,
) -> Result<usize, String> {
    let selection = input.selection.clone();
    let tag_strings = input.tag_strings.clone();
    let count = crate::selection::batch_updates::add_tags_selection(
        &state.db,
        input.selection,
        input.tag_strings,
    )
    .await?;
    if count > 0 {
        let entity_hashes =
            resolve_selection_entity_hashes(
                state,
                GetSelectionSummaryInput {
                    selection: selection.clone(),
                },
            )
            .await?;
        let member_hashes = descendant_hashes(
            &entity_hashes,
            resolve_selection_hashes_for_expansion(
                state,
                &selection,
                EntityExpansionMode::EntityAndDescendants,
            )
            .await?,
        );
        crate::events::emit_state_changed(
            "add_tags_selection",
            crate::runtime_contract::change_builder::ChangeImpact::batch_tags()
                .entity_hashes(entity_hashes)
                .member_hashes(member_hashes)
                .tags_added(tag_strings),
        );
    }
    Ok(count)
}

// Legacy-only: bulk selection mutations not yet in rebuilt frontend.
pub async fn remove_tags_selection(
    state: &AppState,
    input: RemoveTagsSelectionInput,
) -> Result<usize, String> {
    let selection = input.selection.clone();
    let tag_strings = input.tag_strings.clone();
    let count = crate::selection::batch_updates::remove_tags_selection(
        &state.db,
        input.selection,
        input.tag_strings,
    )
    .await?;
    if count > 0 {
        let entity_hashes =
            resolve_selection_entity_hashes(
                state,
                GetSelectionSummaryInput {
                    selection: selection.clone(),
                },
            )
            .await?;
        let member_hashes = descendant_hashes(
            &entity_hashes,
            resolve_selection_hashes_for_expansion(
                state,
                &selection,
                EntityExpansionMode::EntityAndDescendants,
            )
            .await?,
        );
        crate::events::emit_state_changed(
            "remove_tags_selection",
            crate::runtime_contract::change_builder::ChangeImpact::batch_tags()
                .entity_hashes(entity_hashes)
                .member_hashes(member_hashes)
                .tags_removed(tag_strings),
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

// Legacy-only: bulk selection mutations not yet in rebuilt frontend.
pub async fn update_selection_metadata(
    state: &AppState,
    input: UpdateSelectionMetadataInput,
) -> Result<usize, String> {
    let mut total_count = 0;
    let mut changed_fields = Vec::new();
    let selection = input.selection.clone();

    if let Some(rating) = input.rating {
        let count = crate::selection::batch_updates::update_rating_selection(
            &state.db,
            input.selection.clone(),
            rating,
        )
        .await?;
        total_count += count;
        changed_fields.push(MediaMetadataField::Rating);
    }

    if let Some(notes) = input.notes {
        let count = crate::selection::batch_updates::set_notes_selection(
            &state.db,
            input.selection.clone(),
            notes,
        )
        .await?;
        total_count += count;
        changed_fields.push(MediaMetadataField::Notes);
    }

    if let Some(urls) = input.source_urls {
        let count = crate::selection::batch_updates::set_source_urls_selection(
            &state.db,
            input.selection,
            urls,
        )
        .await?;
        total_count += count;
        changed_fields.push(MediaMetadataField::SourceUrls);
    }

    if total_count > 0 {
        let entity_hashes =
            resolve_selection_entity_hashes(
                state,
                GetSelectionSummaryInput {
                    selection: selection.clone(),
                },
            )
            .await?;
        let member_hashes = descendant_hashes(
            &entity_hashes,
            resolve_selection_hashes_for_expansion(
                state,
                &selection,
                EntityExpansionMode::EntityAndDescendants,
            )
            .await?,
        );
        let impact = crate::runtime_contract::change_builder::ChangeImpact::selection_metadata()
            .entity_hashes(entity_hashes)
            .member_hashes(member_hashes)
            .media_fields_changed(&changed_fields)
            .smart_folder_scopes_changed_for_media_fields(&changed_fields);
        crate::events::emit_state_changed("update_selection_metadata", impact);
    }
    Ok(total_count)
}

pub async fn resolve_selection_entity_hashes(
    state: &AppState,
    input: GetSelectionSummaryInput,
) -> Result<Vec<String>, String> {
    let ids = resolve_selection_entity_ids(state, &input.selection).await?;
    let pairs = state.db.resolve_entity_hashes_for_ids(&ids).await?;
    Ok(pairs.into_iter().map(|(h, _)| h).collect())
}

pub async fn resolve_selection_hashes_for_expansion(
    state: &AppState,
    selection: &SelectionQuerySpec,
    expansion: EntityExpansionMode,
) -> Result<Vec<String>, String> {
    let ids = resolve_selection_entity_ids(state, selection).await?;
    let expanded = state.db.expand_entity_ids(ids, expansion).await?;
    let pairs = state.db.resolve_entity_hashes_for_ids(&expanded).await?;
    Ok(pairs.into_iter().map(|(h, _)| h).collect())
}

pub async fn resolve_selection_entity_ids(
    state: &AppState,
    selection: &SelectionQuerySpec,
) -> Result<Vec<i64>, String> {
    let bitmap = super::media_lifecycle::resolve_selection_bitmap(state, selection).await?;
    Ok(bitmap.iter().map(|id| id as i64).collect())
}
