//! Handler functions for tag operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::db::types::{
    BaseScope, EntityTarget, EntityTargetKind, EntityViewQuery, QueryFilters, QueryPage,
    QuerySort, ScopeKind, TagFilter, TagMatchMode,
};
use crate::engine::tags::TagOperation;
use crate::state::AppState;

fn tag_display_key(namespace: &str, subtag: &str) -> String {
    if namespace.is_empty() {
        subtag.to_string()
    } else {
        format!("{namespace}:{subtag}")
    }
}

fn resolve_tag_id_or_create(state: &AppState, tag: &str) -> Result<i64, String> {
    state.engine.ensure_tag(tag)
}

fn entity_hash_target(hashes: Vec<String>) -> EntityTarget {
    EntityTarget {
        kind: EntityTargetKind::EntityHashes,
        entity_hashes: Some(hashes),
        query: None,
        excluded_entity_hashes: None,
    }
}

fn find_files_by_tags_canonical(
    state: &AppState,
    tag_strings: Vec<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<Vec<crate::types::FileGridInfo>, String> {
    if tag_strings.is_empty() {
        return Ok(Vec::new());
    }

    let limit = limit.unwrap_or(500);
    let offset = offset.unwrap_or(0);
    let query = EntityViewQuery {
        base_scope: BaseScope {
            kind: ScopeKind::System,
            key: Some("all".to_string()),
            id: None,
        },
        filters: QueryFilters {
            tags: Some(
                tag_strings
                    .into_iter()
                    .map(|tag| TagFilter {
                        tag,
                        match_mode: TagMatchMode::Include,
                    })
                    .collect(),
            ),
            ..Default::default()
        },
        sort: QuerySort::default(),
        page: QueryPage {
            limit: (limit + offset) as i64,
            cursor: None,
        },
    };

    let mut items: Vec<crate::types::FileGridInfo> = state
        .engine
        .query_entity_view(query)?
        .items
        .into_iter()
        .map(crate::types::FileGridInfo::from)
        .collect();
    if offset > 0 {
        items = items.into_iter().skip(offset).collect();
    }
    if items.len() > limit {
        items.truncate(limit);
    }
    Ok(items)
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SearchTagsInput {
    #[serde(default)]
    pub query: Option<String>,
    #[ts(type = "number | null")]
    pub limit: Option<usize>,
    #[ts(type = "number | null")]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileTagsInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddTagsInput {
    pub hashes: Vec<String>,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveTagsInput {
    pub hashes: Vec<String>,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct FindFilesByTagsInput {
    pub tag_strings: Vec<String>,
    #[ts(type = "number | null")]
    pub limit: Option<usize>,
    #[ts(type = "number | null")]
    pub offset: Option<usize>,
}

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

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CompanionGetNamespaceValuesInput {
    pub namespace: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CompanionGetFilesByTagInput {
    pub tag: String,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn search_tags(
    state: &AppState,
    input: SearchTagsInput,
) -> Result<serde_json::Value, String> {
    let query = input.query.unwrap_or_default();
    let limit = input.limit.unwrap_or(20) as i64;
    let offset = input.offset.unwrap_or(0) as i64;
    let result = state.engine.search_tags(&query, limit, offset)?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_all_tags_with_counts(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = state.engine.get_all_tags_with_counts()?;
    let legacy_rows: Vec<(String, String, i64)> = result
        .iter()
        .map(|row| {
            (
                tag_display_key(&row.namespace, &row.subtag),
                row.namespace.clone(),
                row.file_count,
            )
        })
        .collect();
    serde_json::to_value(&legacy_rows).map_err(|e| e.to_string())
}

pub async fn get_file_tags(
    state: &AppState,
    input: GetFileTagsInput,
) -> Result<serde_json::Value, String> {
    let tags = state.engine.get_entity_tags(&input.hash)?;
    let result: Vec<serde_json::Value> = tags
        .iter()
        .map(|t| {
            serde_json::json!({
                "tag_id": t.tag_id,
                "display": tag_display_key(&t.namespace, &t.subtag),
                "namespace": t.namespace,
                "subtag": t.subtag,
                "file_count": 0,
                "read_only": t.source != "local",
                "site_mask": t.site_mask,
                "provenance_mask": t.provenance_mask,
                "source": t.source,
            })
        })
        .collect();
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

// Legacy-only: rebuilt frontend uses engine-routed `apply_entity_tags` command.
pub async fn add_tags(state: &AppState, input: AddTagsInput) -> Result<(), String> {
    if input.tag_strings.is_empty() || input.hashes.is_empty() {
        return Ok(());
    }
    state.engine.apply_entity_tags(
        entity_hash_target(input.hashes),
        TagOperation::Add,
        &input.tag_strings,
        None,
    )?;
    Ok(())
}

// Legacy-only: rebuilt frontend uses engine-routed `apply_entity_tags` command.
pub async fn remove_tags(state: &AppState, input: RemoveTagsInput) -> Result<(), String> {
    if input.tag_strings.is_empty() || input.hashes.is_empty() {
        return Ok(());
    }
    state.engine.apply_entity_tags(
        entity_hash_target(input.hashes),
        TagOperation::Remove,
        &input.tag_strings,
        None,
    )?;
    Ok(())
}

pub async fn find_files_by_tags(
    state: &AppState,
    input: FindFilesByTagsInput,
) -> Result<serde_json::Value, String> {
    let result = find_files_by_tags_canonical(state, input.tag_strings, input.limit, input.offset)?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

// Legacy-only: tag management UI not yet in rebuilt frontend.
pub async fn manage_tag_alias(state: &AppState, input: ManageTagAliasInput) -> Result<(), String> {
    let from_tag_id = resolve_tag_id_or_create(state, &input.from)?;
    let to_tag_id = match input.to.as_deref() {
        Some("") | None => None,
        Some(to) => Some(resolve_tag_id_or_create(state, to)?),
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

// Legacy-only: tag management UI not yet in rebuilt frontend.
pub async fn manage_tag_implication(
    state: &AppState,
    input: ManageTagImplicationInput,
) -> Result<(), String> {
    let child_tag_id = resolve_tag_id_or_create(state, &input.child)?;
    let parent_tag_id = resolve_tag_id_or_create(state, &input.parent)?;
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

// Legacy-only: tag management UI not yet in rebuilt frontend.
pub async fn merge_tags(state: &AppState, input: MergeTagsInput) -> Result<(), String> {
    let from_tag_id = resolve_tag_id_or_create(state, &input.from_tag)?;
    let to_tag_id = resolve_tag_id_or_create(state, &input.to_tag)?;
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

// Legacy-only: tag management UI not yet in rebuilt frontend.
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

// Legacy-only: tag management UI not yet in rebuilt frontend.
pub async fn delete_tag(
    state: &AppState,
    input: DeleteTagInput,
) -> Result<serde_json::Value, String> {
    let affected_entity_ids = state.engine.delete_tag(input.tag_id)?;
    Ok(serde_json::json!({
        "affected_files": affected_entity_ids.len(),
    }))
}

pub async fn companion_get_namespace_values(
    state: &AppState,
    input: CompanionGetNamespaceValuesInput,
) -> Result<serde_json::Value, String> {
    let values: Vec<serde_json::Value> = state
        .engine
        .get_all_tags_with_counts()?
        .into_iter()
        .filter(|row| row.namespace == input.namespace && row.file_count > 0)
        .map(|row| {
            serde_json::json!({
                "value": row.subtag,
                "count": row.file_count,
                "thumbnail_hash": null,
            })
        })
        .collect();
    serde_json::to_value(&values).map_err(|e| e.to_string())
}

pub async fn companion_get_files_by_tag(
    state: &AppState,
    input: CompanionGetFilesByTagInput,
) -> Result<serde_json::Value, String> {
    let result = find_files_by_tags_canonical(state, vec![input.tag], None, None)?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}
