//! Handler functions for tag operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

fn resolve_existing_tag_id(state: &AppState, tag: &str) -> Result<i64, String> {
    state
        .engine
        .find_tag_id(tag)?
        .ok_or_else(|| format!("Tag does not exist: {tag}"))
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ManageTagAliasInput {
    pub from: String,
    /// When present, sets the alias. When absent/null, removes it.
    pub to: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ManageTagImplicationInput {
    pub child: String,
    pub parent: String,
    /// "add" or "remove"
    pub action: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetTagRelationsInput {
    #[ts(type = "number")]
    pub tag_id: i64,
    /// "aliases" or "implications"
    pub relation_type: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct MergeTagsInput {
    pub from_tag: String,
    pub to_tag: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetTagsPaginatedInput {
    pub namespace: Option<String>,
    pub search: Option<String>,
    pub cursor: Option<String>,
    #[serde(default = "default_tags_limit")]
    #[ts(type = "number")]
    pub limit: i64,
}

fn default_tags_limit() -> i64 {
    200
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RenameTagInput {
    #[ts(type = "number")]
    pub tag_id: i64,
    pub new_name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteTagInput {
    #[ts(type = "number")]
    pub tag_id: i64,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn manage_tag_alias(state: &AppState, input: ManageTagAliasInput) -> Result<(), String> {
    let from_tag_id = resolve_existing_tag_id(state, &input.from)?;
    let to_tag_id = match input.to.as_deref() {
        Some("") | None => None,
        Some(to) => Some(resolve_existing_tag_id(state, to)?),
    };
    state.engine.manage_tag_alias(from_tag_id, to_tag_id)?;
    Ok(())
}

pub async fn get_tag_relations(
    state: &AppState,
    input: GetTagRelationsInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .engine
        .get_tag_relations(input.tag_id, &input.relation_type)?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn manage_tag_implication(
    state: &AppState,
    input: ManageTagImplicationInput,
) -> Result<(), String> {
    let child_tag_id = resolve_existing_tag_id(state, &input.child)?;
    let parent_tag_id = resolve_existing_tag_id(state, &input.parent)?;
    match input.action.as_str() {
        "add" => state
            .engine
            .manage_tag_implication(child_tag_id, parent_tag_id, true)?,
        "remove" => state
            .engine
            .manage_tag_implication(child_tag_id, parent_tag_id, false)?,
        _ => return Err(format!("Invalid action: {}", input.action)),
    }
    Ok(())
}

pub async fn merge_tags(state: &AppState, input: MergeTagsInput) -> Result<(), String> {
    let from_tag_id = resolve_existing_tag_id(state, &input.from_tag)?;
    let to_tag_id = resolve_existing_tag_id(state, &input.to_tag)?;
    state.engine.merge_tags(from_tag_id, to_tag_id)?;
    Ok(())
}

pub async fn get_tags_paginated(
    state: &AppState,
    input: GetTagsPaginatedInput,
) -> Result<serde_json::Value, String> {
    let result = state.engine.get_tags_paginated(
        input.namespace,
        input.search,
        input.cursor,
        input.limit,
    )?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_namespace_summary(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let data = state.engine.get_namespace_summary()?;
    serde_json::to_value(&data).map_err(|e| e.to_string())
}

pub async fn rename_tag(
    state: &AppState,
    input: RenameTagInput,
) -> Result<serde_json::Value, String> {
    let result = state.engine.rename_tag(input.tag_id, &input.new_name)?;
    Ok(serde_json::json!({
        "affected_files": result.entity_ids.len(),
        "merged_into": result.merged_into_tag_id,
    }))
}

pub async fn delete_tag(
    state: &AppState,
    input: DeleteTagInput,
) -> Result<serde_json::Value, String> {
    let affected_entity_ids = state.engine.delete_tag(input.tag_id)?;
    Ok(serde_json::json!({
        "affected_files": affected_entity_ids.len(),
    }))
}
