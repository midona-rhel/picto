//! Handler functions for tag operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

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

pub async fn search_tags(state: &AppState, input: SearchTagsInput) -> Result<serde_json::Value, String> {
    let query = input.query.unwrap_or_default();
    if input.offset.is_some() {
        let result = crate::tags::controller::TagController::search_tags_paged(
            &state.db, query, input.limit, input.offset,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    } else {
        let result = crate::tags::controller::TagController::search_tags(
            &state.db, query, input.limit,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

pub async fn get_all_tags_with_counts(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = crate::tags::controller::TagController::get_all_tags_with_counts(&state.db).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_file_tags(state: &AppState, input: GetFileTagsInput) -> Result<serde_json::Value, String> {
    let result = crate::tags::controller::TagController::get_entity_tags(
        &state.db, input.hash,
    ).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn add_tags(state: &AppState, input: AddTagsInput) -> Result<(), String> {
    if input.tag_strings.is_empty() || input.hashes.is_empty() {
        return Ok(());
    }
    let hashes_clone = input.hashes.clone();
    state.db.add_tags_batch(&input.hashes, &input.tag_strings).await?;
    crate::events::emit_mutation(
        "add_tags",
        crate::events::MutationImpact::batch_tags().file_hashes(hashes_clone),
    );
    Ok(())
}

pub async fn remove_tags(state: &AppState, input: RemoveTagsInput) -> Result<(), String> {
    if input.tag_strings.is_empty() || input.hashes.is_empty() {
        return Ok(());
    }
    let hashes_clone = input.hashes.clone();
    state.db.remove_tags_batch(&input.hashes, &input.tag_strings).await?;
    crate::events::emit_mutation(
        "remove_tags",
        crate::events::MutationImpact::batch_tags().file_hashes(hashes_clone),
    );
    Ok(())
}

pub async fn find_files_by_tags(state: &AppState, input: FindFilesByTagsInput) -> Result<serde_json::Value, String> {
    let result = crate::tags::controller::TagController::find_files_by_tags(
        &state.db, input.tag_strings, input.limit, input.offset,
    ).await?;
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
            state.db.add_alias(&from_ns, &from_st, &to_ns, &to_st, "local").await?;
        }
    } else {
        state.db.remove_alias(&from_ns, &from_st, "local").await?;
    }

    crate::events::emit_mutation(
        "manage_tag_alias",
        crate::events::MutationImpact::tag_structure_change(),
    );
    Ok(())
}

pub async fn get_tag_aliases(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let aliases = state.db.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT t1.namespace || ':' || t1.subtag, t2.namespace || ':' || t2.subtag FROM tag_alias ts JOIN tag t1 ON ts.from_tag_id = t1.tag_id JOIN tag t2 ON ts.to_tag_id = t2.tag_id")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut result: Vec<(String, String)> = Vec::new();
        for row in rows {
            result.push(row?);
        }
        Ok(result)
    }).await?;
    let json_aliases: Vec<serde_json::Value> = aliases
        .iter()
        .map(|(from, to)| serde_json::json!({"from": from, "to": to}))
        .collect();
    serde_json::to_value(&json_aliases).map_err(|e| e.to_string())
}

pub async fn get_tag_relations(state: &AppState, input: GetTagRelationsInput) -> Result<serde_json::Value, String> {
    let result = match input.relation_type.as_str() {
        "aliases" => state.db.get_aliases_for_tag(input.tag_id).await?,
        "implications" => state.db.get_implications_for_tag(input.tag_id).await?,
        _ => return Err(format!("Invalid relation_type: {}", input.relation_type)),
    };
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn manage_tag_implication(state: &AppState, input: ManageTagImplicationInput) -> Result<(), String> {
    let (cns, cst) = crate::tags::normalize::parse_tag(&input.child)
        .ok_or_else(|| format!("Invalid tag: {}", input.child))?;
    let (pns, pst) = crate::tags::normalize::parse_tag(&input.parent)
        .ok_or_else(|| format!("Invalid tag: {}", input.parent))?;

    match input.action.as_str() {
        "add" => state.db.add_implication(&cns, &cst, &pns, &pst, "local").await?,
        "remove" => state.db.remove_implication(&cns, &cst, &pns, &pst, "local").await?,
        _ => return Err(format!("Invalid action: {}", input.action)),
    }

    crate::events::emit_mutation(
        "manage_tag_implication",
        crate::events::MutationImpact::tag_structure_change(),
    );
    Ok(())
}

pub async fn merge_tags(state: &AppState, input: MergeTagsInput) -> Result<(), String> {
    let (from_ns, from_st) = crate::tags::normalize::parse_tag(&input.from_tag)
        .ok_or_else(|| format!("Invalid tag: {}", input.from_tag))?;
    let (to_ns, to_st) = crate::tags::normalize::parse_tag(&input.to_tag)
        .ok_or_else(|| format!("Invalid tag: {}", input.to_tag))?;
    let (from_id, to_id, affected_file_ids) = state.db.with_conn(move |conn| {
        let from_id = crate::tags::db::get_or_create_tag(conn, &from_ns, &from_st)?;
        let to_id = crate::tags::db::get_or_create_tag(conn, &to_ns, &to_st)?;
        let mut stmt = conn.prepare("SELECT entity_id FROM entity_tag_raw WHERE tag_id = ?1")?;
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
    }).await?;

    use crate::sqlite::compilers::CompilerEvent;
    state.db.emit_compiler_event(CompilerEvent::TagChanged { tag_id: from_id });
    state.db.emit_compiler_event(CompilerEvent::TagChanged { tag_id: to_id });
    for file_id in affected_file_ids {
        state.db.emit_compiler_event(CompilerEvent::FileTagsChanged { file_id });
    }
    crate::events::emit_mutation(
        "merge_tags",
        crate::events::MutationImpact::tag_structure_change(),
    );
    Ok(())
}

pub async fn lookup_tag_types(state: &AppState, _input: serde_json::Value) -> Result<Vec<String>, String> {
    state.db.with_read_conn(|conn| {
        let mut stmt = conn.prepare("SELECT DISTINCT namespace FROM tag WHERE file_count > 0")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()
    }).await
}

pub async fn get_tags_paginated(state: &AppState, input: GetTagsPaginatedInput) -> Result<serde_json::Value, String> {
    let result = state.db.get_tags_paginated(
        input.namespace, input.search, input.cursor, input.limit,
    ).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn get_namespace_summary(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let data = state.db.get_namespace_summary().await?;
    let json_result: Vec<serde_json::Value> = data
        .iter()
        .map(|(ns, count)| serde_json::json!({"namespace": ns, "count": count}))
        .collect();
    serde_json::to_value(&json_result).map_err(|e| e.to_string())
}

pub async fn rename_tag(state: &AppState, input: RenameTagInput) -> Result<serde_json::Value, String> {
    let (affected_file_ids, merged_into) = state.db.rename_tag_by_id(input.tag_id, &input.new_name).await?;
    crate::events::emit_mutation(
        "rename_tag",
        crate::events::MutationImpact::tag_structure_change(),
    );
    Ok(serde_json::json!({
        "affected_files": affected_file_ids.len(),
        "merged_into": merged_into,
    }))
}

pub async fn delete_tag(state: &AppState, input: DeleteTagInput) -> Result<serde_json::Value, String> {
    let affected_file_ids = state.db.delete_tag_by_id(input.tag_id).await?;
    crate::events::emit_mutation(
        "delete_tag",
        crate::events::MutationImpact::tag_structure_change(),
    );
    Ok(serde_json::json!({
        "affected_files": affected_file_ids.len(),
    }))
}

pub async fn normalize_ingested_namespaces(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let stats = state.db.normalize_disallowed_namespaces().await?;
    if stats.tags_rewritten > 0 {
        crate::events::emit_mutation(
            "normalize_ingested_namespaces",
            crate::events::MutationImpact::tag_structure_change(),
        );
    }
    Ok(serde_json::json!({
        "tags_rewritten": stats.tags_rewritten,
        "tags_merged": stats.tags_merged,
        "affected_files": stats.affected_files,
    }))
}

pub async fn companion_get_namespace_values(state: &AppState, input: CompanionGetNamespaceValuesInput) -> Result<serde_json::Value, String> {
    let values = state.db.with_read_conn(move |conn| {
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
    }).await?;
    serde_json::to_value(&values).map_err(|e| e.to_string())
}

pub async fn companion_get_files_by_tag(state: &AppState, input: CompanionGetFilesByTagInput) -> Result<serde_json::Value, String> {
    let result = crate::tags::controller::TagController::find_files_by_tags(
        &state.db, vec![input.tag], None, None,
    ).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}
