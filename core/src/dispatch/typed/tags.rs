//! Handler functions for tag operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::sqlite::EntityExpansionMode;
use crate::state::AppState;

fn descendant_hashes(top_level_hashes: &[String], effective_hashes: &[(String, i64)]) -> Vec<String> {
    let top_level: std::collections::HashSet<&str> =
        top_level_hashes.iter().map(String::as_str).collect();
    effective_hashes
        .iter()
        .map(|(hash, _)| hash)
        .filter(|hash| !top_level.contains(hash.as_str()))
        .cloned()
        .collect()
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
    if input.offset.is_some() {
        let result =
            crate::tags::service::search_tags_paged(&state.db, query, input.limit, input.offset)
                .await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    } else {
        let result = crate::tags::service::search_tags(&state.db, query, input.limit).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

pub async fn get_all_tags_with_counts(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = crate::tags::service::get_all_tags_with_counts(&state.db).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_file_tags(
    state: &AppState,
    input: GetFileTagsInput,
) -> Result<serde_json::Value, String> {
    let tags = state.db.get_entity_tags(&input.hash).await?;
    let result: Vec<crate::types::TagInfo> = tags
        .iter()
        .map(|t| crate::types::TagInfo {
            tag_id: t.tag_id,
            display: crate::types::tag_display_key(&t.namespace, &t.subtag),
            namespace: t.namespace.clone(),
            subtag: t.subtag.clone(),
            file_count: 0,
            read_only: t.source != "local",
        })
        .collect();
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn add_tags(state: &AppState, input: AddTagsInput) -> Result<(), String> {
    if input.tag_strings.is_empty() || input.hashes.is_empty() {
        return Ok(());
    }
    let resolved = state
        .db
        .resolve_entity_hashes_with_expansion(
            &input.hashes,
            EntityExpansionMode::EntityAndDescendants,
        )
        .await?;
    let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    state
        .db
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

pub async fn remove_tags(state: &AppState, input: RemoveTagsInput) -> Result<(), String> {
    if input.tag_strings.is_empty() || input.hashes.is_empty() {
        return Ok(());
    }
    let resolved = state
        .db
        .resolve_entity_hashes_with_expansion(
            &input.hashes,
            EntityExpansionMode::EntityAndDescendants,
        )
        .await?;
    let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    state
        .db
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
        &state.db,
        input.tag_strings,
        input.limit,
        input.offset,
    )
    .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn manage_tag_alias(state: &AppState, input: ManageTagAliasInput) -> Result<(), String> {
    let (from_ns, from_st) = crate::tags::normalize::parse_tag(&input.from)
        .ok_or_else(|| format!("Invalid tag: {}", input.from))?;

    if let Some(to) = &input.to {
        if to.is_empty() {
            state.db.remove_alias(&from_ns, &from_st, "local").await?;
        } else {
            let (to_ns, to_st) = crate::tags::normalize::parse_tag(to)
                .ok_or_else(|| format!("Invalid tag: {}", to))?;
            state
                .db
                .add_alias(&from_ns, &from_st, &to_ns, &to_st, "local")
                .await?;
        }
    } else {
        state.db.remove_alias(&from_ns, &from_st, "local").await?;
    }

    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::tag_structure_change()
        .tags_removed(vec![input.from.clone()]);
    if let Some(to) = &input.to {
        if !to.is_empty() {
            impact = impact.tags_added(vec![to.clone()]);
        }
    }
    crate::events::emit_state_changed("manage_tag_alias", impact);
    Ok(())
}

pub async fn get_tag_relations(
    state: &AppState,
    input: GetTagRelationsInput,
) -> Result<serde_json::Value, String> {
    let result = match input.relation_type.as_str() {
        "aliases" => state.db.get_aliases_for_tag(input.tag_id).await?,
        "implications" => state.db.get_implications_for_tag(input.tag_id).await?,
        _ => return Err(format!("Invalid relation_type: {}", input.relation_type)),
    };
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn manage_tag_implication(
    state: &AppState,
    input: ManageTagImplicationInput,
) -> Result<(), String> {
    let (cns, cst) = crate::tags::normalize::parse_tag(&input.child)
        .ok_or_else(|| format!("Invalid tag: {}", input.child))?;
    let (pns, pst) = crate::tags::normalize::parse_tag(&input.parent)
        .ok_or_else(|| format!("Invalid tag: {}", input.parent))?;

    match input.action.as_str() {
        "add" => {
            state
                .db
                .add_implication(&cns, &cst, &pns, &pst, "local")
                .await?
        }
        "remove" => {
            state
                .db
                .remove_implication(&cns, &cst, &pns, &pst, "local")
                .await?
        }
        _ => return Err(format!("Invalid action: {}", input.action)),
    }

    crate::events::emit_state_changed(
        "manage_tag_implication",
        crate::runtime_contract::change_builder::ChangeImpact::tag_structure_change()
            .tags_added(vec![input.parent.clone()])
            .tags_removed(vec![input.child.clone()]),
    );
    Ok(())
}

pub async fn merge_tags(state: &AppState, input: MergeTagsInput) -> Result<(), String> {
    let (from_ns, from_st) = crate::tags::normalize::parse_tag(&input.from_tag)
        .ok_or_else(|| format!("Invalid tag: {}", input.from_tag))?;
    let (to_ns, to_st) = crate::tags::normalize::parse_tag(&input.to_tag)
        .ok_or_else(|| format!("Invalid tag: {}", input.to_tag))?;
    let (from_id, to_id, affected_file_ids) = state
        .db
        .with_conn(move |conn| {
            let from_id = crate::tags::db::get_or_create_tag(conn, &from_ns, &from_st)?;
            let to_id = crate::tags::db::get_or_create_tag(conn, &to_ns, &to_st)?;
            let mut stmt =
                conn.prepare("SELECT entity_id FROM entity_tag_raw WHERE tag_id = ?1")?;
            let file_ids: Vec<i64> = stmt
                .query_map(rusqlite::params![from_id], |row| row.get(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            conn.execute(
                "UPDATE OR IGNORE entity_tag_raw SET tag_id = ?1 WHERE tag_id = ?2",
                rusqlite::params![to_id, from_id],
            )?;
            conn.execute(
                "DELETE FROM entity_tag_raw WHERE tag_id = ?1",
                rusqlite::params![from_id],
            )?;
            Ok((from_id, to_id, file_ids))
        })
        .await?;

    use crate::sqlite::ReadModelEvent;
    state
        .db
        .emit_read_model_event(ReadModelEvent::TagChanged { tag_id: from_id });
    state
        .db
        .emit_read_model_event(ReadModelEvent::TagChanged { tag_id: to_id });
    for file_id in &affected_file_ids {
        state
            .db
            .emit_read_model_event(ReadModelEvent::FileTagsChanged { file_id: *file_id });
    }
    // Resolve affected file_ids → hashes for exact state change emission
    let affected_hashes: Vec<String> = if !affected_file_ids.is_empty() {
        state.db.resolve_ids_batch(&affected_file_ids).await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, hash)| hash)
            .collect()
    } else {
        Vec::new()
    };
    crate::events::emit_state_changed(
        "merge_tags",
        crate::runtime_contract::change_builder::ChangeImpact::tag_structure_change()
            .entity_hashes(affected_hashes)
            .tags_removed(vec![input.from_tag.clone()])
            .tags_added(vec![input.to_tag.clone()]),
    );
    Ok(())
}

pub async fn get_tags_paginated(
    state: &AppState,
    input: GetTagsPaginatedInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .db
        .get_tags_paginated(input.namespace, input.search, input.cursor, input.limit)
        .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_namespace_summary(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let data = state.db.get_namespace_summary().await?;
    let json_result: Vec<serde_json::Value> = data
        .iter()
        .map(|(ns, count)| serde_json::json!({"namespace": ns, "count": count}))
        .collect();
    serde_json::to_value(&json_result).map_err(|e| e.to_string())
}

pub async fn rename_tag(
    state: &AppState,
    input: RenameTagInput,
) -> Result<serde_json::Value, String> {
    // Look up the old tag string before the rename mutates it
    let old_tag_string: Option<String> = state.db.with_read_conn({
        let tag_id = input.tag_id;
        move |conn| {
            use rusqlite::OptionalExtension;
            conn.query_row(
                "SELECT namespace, subtag FROM tag WHERE tag_id = ?1",
                rusqlite::params![tag_id],
                |row| {
                    let ns: String = row.get(0)?;
                    let st: String = row.get(1)?;
                    Ok(crate::tags::normalize::combine_tag(&ns, &st))
                },
            ).optional()
        }
    }).await.unwrap_or(None);

    let (affected_file_ids, merged_into) = state
        .db
        .rename_tag_by_id(input.tag_id, &input.new_name)
        .await?;
    let affected_hashes: Vec<String> = if !affected_file_ids.is_empty() {
        state.db.resolve_ids_batch(&affected_file_ids).await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, hash)| hash)
            .collect()
    } else {
        Vec::new()
    };
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::tag_structure_change()
        .entity_hashes(affected_hashes);
    if let Some(old_tag) = old_tag_string {
        impact = impact
            .tags_removed(vec![old_tag])
            .tags_added(vec![input.new_name.clone()]);
    }
    crate::events::emit_state_changed("rename_tag", impact);
    Ok(serde_json::json!({
        "affected_files": affected_file_ids.len(),
        "merged_into": merged_into,
    }))
}

pub async fn delete_tag(
    state: &AppState,
    input: DeleteTagInput,
) -> Result<serde_json::Value, String> {
    // Look up the tag string before deletion removes it
    let tag_string: Option<String> = state.db.with_read_conn({
        let tag_id = input.tag_id;
        move |conn| {
            use rusqlite::OptionalExtension;
            conn.query_row(
                "SELECT namespace, subtag FROM tag WHERE tag_id = ?1",
                rusqlite::params![tag_id],
                |row| {
                    let ns: String = row.get(0)?;
                    let st: String = row.get(1)?;
                    Ok(crate::tags::normalize::combine_tag(&ns, &st))
                },
            ).optional()
        }
    }).await.unwrap_or(None);

    let affected_file_ids = state.db.delete_tag_by_id(input.tag_id).await?;
    let affected_hashes: Vec<String> = if !affected_file_ids.is_empty() {
        state.db.resolve_ids_batch(&affected_file_ids).await
            .unwrap_or_default()
            .into_iter()
            .map(|(_, hash)| hash)
            .collect()
    } else {
        Vec::new()
    };
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::tag_structure_change()
        .entity_hashes(affected_hashes);
    if let Some(tag) = tag_string {
        impact = impact.tags_removed(vec![tag]);
    }
    crate::events::emit_state_changed("delete_tag", impact);
    Ok(serde_json::json!({
        "affected_files": affected_file_ids.len(),
    }))
}

pub async fn companion_get_namespace_values(
    state: &AppState,
    input: CompanionGetNamespaceValuesInput,
) -> Result<serde_json::Value, String> {
    let values = state
        .db
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
        crate::tags::service::find_files_by_tags(&state.db, vec![input.tag], None, None).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}
