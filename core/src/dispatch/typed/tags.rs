//! Handler functions for tag operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::sqlite::EntityExpansionMode;
use crate::state::AppState;

fn descendant_hashes(
    top_level_hashes: &[String],
    effective_hashes: &[(String, i64)],
) -> Vec<String> {
    let top_level: std::collections::HashSet<&str> =
        top_level_hashes.iter().map(String::as_str).collect();
    effective_hashes
        .iter()
        .map(|(hash, _)| hash)
        .filter(|hash| !top_level.contains(hash.as_str()))
        .cloned()
        .collect()
}

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
    let resolved = state
        .legacy_db
        .resolve_entity_hashes_with_expansion(
            &input.hashes,
            EntityExpansionMode::EntityAndDescendants,
        )
        .await?;
    let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    state
        .legacy_db
        .add_tags_batch_by_entity_ids(entity_ids, input.tag_strings.clone(), "local".to_string())
        .await?;
    crate::events::emit_state_changed(
        "add_tags",
        crate::runtime_contract::change_builder::ChangeImpact::batch_tags()
            .entity_hashes(input.hashes.clone())
            .member_hashes(descendant_hashes(&input.hashes, &resolved))
            .tags_added(input.tag_strings),
    );
    Ok(())
}

// Legacy-only: rebuilt frontend uses engine-routed `apply_entity_tags` command.
pub async fn remove_tags(state: &AppState, input: RemoveTagsInput) -> Result<(), String> {
    if input.tag_strings.is_empty() || input.hashes.is_empty() {
        return Ok(());
    }
    let resolved = state
        .legacy_db
        .resolve_entity_hashes_with_expansion(
            &input.hashes,
            EntityExpansionMode::EntityAndDescendants,
        )
        .await?;
    let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    state
        .legacy_db
        .remove_tags_batch_by_entity_ids(entity_ids, input.tag_strings.clone())
        .await?;
    crate::events::emit_state_changed(
        "remove_tags",
        crate::runtime_contract::change_builder::ChangeImpact::batch_tags()
            .entity_hashes(input.hashes.clone())
            .member_hashes(descendant_hashes(&input.hashes, &resolved))
            .tags_removed(input.tag_strings),
    );
    Ok(())
}

pub async fn find_files_by_tags(
    state: &AppState,
    input: FindFilesByTagsInput,
) -> Result<serde_json::Value, String> {
    let result = crate::tags::service::find_files_by_tags(
        &state.legacy_db,
        input.tag_strings,
        input.limit,
        input.offset,
    )
    .await?;
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
    let values = state
        .legacy_db
        .with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT subtag, file_count FROM tag
             WHERE namespace = ?1 AND file_count > 0
             ORDER BY file_count DESC",
            )?;
            let rows = stmt.query_map([&input.namespace], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            let mut result = Vec::new();
            for row in rows {
                let (subtag, count) = row?;
                result.push(serde_json::json!({
                    "value": subtag,
                    "count": count,
                    "thumbnail_hash": null,
                }));
            }
            Ok(result)
        })
        .await?;
    serde_json::to_value(&values).map_err(|e| e.to_string())
}

pub async fn companion_get_files_by_tag(
    state: &AppState,
    input: CompanionGetFilesByTagInput,
) -> Result<serde_json::Value, String> {
    let result =
        crate::tags::service::find_files_by_tags(&state.legacy_db, vec![input.tag], None, None)
            .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}
