//! Typed command implementations for tag operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SearchTagsInput {
    pub query: String,
    #[ts(type = "number | null")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SearchTagsPagedInput {
    #[serde(default)]
    pub query: Option<String>,
    #[ts(type = "number | null")]
    pub limit: Option<usize>,
    #[ts(type = "number | null")]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileTagsInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct AddTagsInput {
    pub hash: String,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveTagsInput {
    pub hash: String,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct AddTagsBatchInput {
    pub hashes: Vec<String>,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveTagsBatchInput {
    pub hashes: Vec<String>,
    pub tag_strings: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct FindFilesByTagsInput {
    pub tag_strings: Vec<String>,
    #[ts(type = "number | null")]
    pub limit: Option<usize>,
    #[ts(type = "number | null")]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct SetTagAliasInput {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveTagAliasInput {
    pub from: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetTagSiblingsInput {
    #[ts(type = "number")]
    pub tag_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct GetTagParentsInput {
    #[ts(type = "number")]
    pub tag_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct AddTagParentInput {
    pub child: String,
    pub parent: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveTagParentInput {
    pub child: String,
    pub parent: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct MergeTagsInput {
    pub from_tag: String,
    pub to_tag: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
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
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct RenameTagInput {
    #[ts(type = "number")]
    pub tag_id: i64,
    pub new_name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteTagInput {
    #[ts(type = "number")]
    pub tag_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct CompanionGetNamespaceValuesInput {
    pub namespace: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct CompanionGetFilesByTagInput {
    pub tag: String,
}

// ─── Command structs ───────────────────────────────────────────────────────

pub struct SearchTags;
pub struct SearchTagsPaged;
pub struct GetAllTagsWithCounts;
pub struct GetFileTags;
pub struct AddTags;
pub struct RemoveTags;
pub struct AddTagsBatch;
pub struct RemoveTagsBatch;
pub struct FindFilesByTags;
pub struct SetTagAlias;
pub struct RemoveTagAlias;
pub struct GetTagAliases;
pub struct GetTagSiblingsForTag;
pub struct GetTagParentsForTag;
pub struct AddTagParent;
pub struct RemoveTagParent;
pub struct MergeTags;
pub struct LookupTagTypes;
pub struct GetTagsPaginated;
pub struct GetNamespaceSummary;
pub struct RenameTag;
pub struct DeleteTag;
pub struct NormalizeIngestedNamespaces;
pub struct CompanionGetNamespaceValues;
pub struct CompanionGetFilesByTag;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for SearchTags {
    const NAME: &'static str = "search_tags";
    type Input = SearchTagsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::tags::controller::TagController::search_tags(
            &state.db, input.query, input.limit,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for SearchTagsPaged {
    const NAME: &'static str = "search_tags_paged";
    type Input = SearchTagsPagedInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let query = input.query.unwrap_or_default();
        let result = crate::tags::controller::TagController::search_tags_paged(
            &state.db, query, input.limit, input.offset,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetAllTagsWithCounts {
    const NAME: &'static str = "get_all_tags_with_counts";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::tags::controller::TagController::get_all_tags_with_counts(&state.db).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetFileTags {
    const NAME: &'static str = "get_file_tags";
    type Input = GetFileTagsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::tags::controller::TagController::get_entity_tags(
            &state.db, input.hash,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for AddTags {
    const NAME: &'static str = "add_tags";
    type Input = AddTagsInput;
    type Output = Vec<String>;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hash_clone = input.hash.clone();
        let val = crate::tags::controller::TagController::add_tags(
            &state.db, input.hash, input.tag_strings,
        ).await?;
        if !val.is_empty() {
            crate::events::emit_mutation(
                "add_tags",
                crate::events::MutationImpact::file_tags(hash_clone)
                    .selection_summary(),
            );
        }
        Ok(val)
    }
}

impl TypedCommand for RemoveTags {
    const NAME: &'static str = "remove_tags";
    type Input = RemoveTagsInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hash_clone = input.hash.clone();
        crate::tags::controller::TagController::remove_tags(
            &state.db, input.hash, input.tag_strings,
        ).await?;
        crate::events::emit_mutation(
            "remove_tags",
            crate::events::MutationImpact::file_tags(hash_clone).selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for AddTagsBatch {
    const NAME: &'static str = "add_tags_batch";
    type Input = AddTagsBatchInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hashes_clone = input.hashes.clone();
        crate::tags::controller::TagController::add_tags_batch(
            &state.db, input.hashes, input.tag_strings,
        ).await?;
        if !hashes_clone.is_empty() {
            crate::events::emit_mutation(
                "add_tags_batch",
                crate::events::MutationImpact::batch_tags().file_hashes(hashes_clone),
            );
        }
        Ok(())
    }
}

impl TypedCommand for RemoveTagsBatch {
    const NAME: &'static str = "remove_tags_batch";
    type Input = RemoveTagsBatchInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hashes_clone = input.hashes.clone();
        crate::tags::controller::TagController::remove_tags_batch(
            &state.db, input.hashes, input.tag_strings,
        ).await?;
        if !hashes_clone.is_empty() {
            crate::events::emit_mutation(
                "remove_tags_batch",
                crate::events::MutationImpact::batch_tags().file_hashes(hashes_clone),
            );
        }
        Ok(())
    }
}

impl TypedCommand for FindFilesByTags {
    const NAME: &'static str = "find_files_by_tags";
    type Input = FindFilesByTagsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::tags::controller::TagController::find_files_by_tags(
            &state.db, input.tag_strings, input.limit, input.offset,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for SetTagAlias {
    const NAME: &'static str = "set_tag_alias";
    type Input = SetTagAliasInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let (from_ns, from_st) = crate::tags::normalize::parse_tag(&input.from)
            .ok_or_else(|| format!("Invalid tag: {}", input.from))?;
        let (to_ns, to_st) = crate::tags::normalize::parse_tag(&input.to)
            .ok_or_else(|| format!("Invalid tag: {}", input.to))?;
        state.db.add_sibling(&from_ns, &from_st, &to_ns, &to_st, "local").await?;
        crate::events::emit_mutation(
            "set_tag_alias",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                .grid_all()
                .selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for RemoveTagAlias {
    const NAME: &'static str = "remove_tag_alias";
    type Input = RemoveTagAliasInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let (from_ns, from_st) = crate::tags::normalize::parse_tag(&input.from)
            .ok_or_else(|| format!("Invalid tag: {}", input.from))?;
        state.db.remove_sibling(&from_ns, &from_st, "local").await?;
        crate::events::emit_mutation(
            "remove_tag_alias",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                .grid_all()
                .selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for GetTagAliases {
    const NAME: &'static str = "get_tag_aliases";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let aliases = state.db.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT t1.namespace || ':' || t1.subtag, t2.namespace || ':' || t2.subtag FROM tag_sibling ts JOIN tag t1 ON ts.from_tag_id = t1.tag_id JOIN tag t2 ON ts.to_tag_id = t2.tag_id")?;
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
}

impl TypedCommand for GetTagSiblingsForTag {
    const NAME: &'static str = "get_tag_siblings_for_tag";
    type Input = GetTagSiblingsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = state.db.get_siblings_for_tag(input.tag_id).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetTagParentsForTag {
    const NAME: &'static str = "get_tag_parents_for_tag";
    type Input = GetTagParentsInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = state.db.get_parents_for_tag(input.tag_id).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for AddTagParent {
    const NAME: &'static str = "add_tag_parent";
    type Input = AddTagParentInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let (cns, cst) = crate::tags::normalize::parse_tag(&input.child)
            .ok_or_else(|| format!("Invalid tag: {}", input.child))?;
        let (pns, pst) = crate::tags::normalize::parse_tag(&input.parent)
            .ok_or_else(|| format!("Invalid tag: {}", input.parent))?;
        state.db.add_parent(&cns, &cst, &pns, &pst, "local").await?;
        crate::events::emit_mutation(
            "add_tag_parent",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                .grid_all()
                .selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for RemoveTagParent {
    const NAME: &'static str = "remove_tag_parent";
    type Input = RemoveTagParentInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let (cns, cst) = crate::tags::normalize::parse_tag(&input.child)
            .ok_or_else(|| format!("Invalid tag: {}", input.child))?;
        let (pns, pst) = crate::tags::normalize::parse_tag(&input.parent)
            .ok_or_else(|| format!("Invalid tag: {}", input.parent))?;
        state.db.remove_parent(&cns, &cst, &pns, &pst, "local").await?;
        crate::events::emit_mutation(
            "remove_tag_parent",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                .grid_all()
                .selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for MergeTags {
    const NAME: &'static str = "merge_tags";
    type Input = MergeTagsInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for LookupTagTypes {
    const NAME: &'static str = "lookup_tag_types";
    type Input = serde_json::Value;
    type Output = Vec<String>;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        state.db.with_read_conn(|conn| {
            let mut stmt = conn.prepare("SELECT DISTINCT namespace FROM tag WHERE file_count > 0")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()
        }).await
    }
}

impl TypedCommand for GetTagsPaginated {
    const NAME: &'static str = "get_tags_paginated";
    type Input = GetTagsPaginatedInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = state.db.get_tags_paginated(
            input.namespace, input.search, input.cursor, input.limit,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for GetNamespaceSummary {
    const NAME: &'static str = "get_namespace_summary";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let data = state.db.get_namespace_summary().await?;
        let json_result: Vec<serde_json::Value> = data
            .iter()
            .map(|(ns, count)| serde_json::json!({"namespace": ns, "count": count}))
            .collect();
        serde_json::to_value(&json_result).map_err(|e| e.to_string())
    }
}

impl TypedCommand for RenameTag {
    const NAME: &'static str = "rename_tag";
    type Input = RenameTagInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let (affected_file_ids, merged_into) = state.db.rename_tag_by_id(input.tag_id, &input.new_name).await?;
        crate::events::emit_mutation(
            "rename_tag",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Tags)
                .grid_all()
                .selection_summary(),
        );
        Ok(serde_json::json!({
            "affected_files": affected_file_ids.len(),
            "merged_into": merged_into,
        }))
    }
}

impl TypedCommand for DeleteTag {
    const NAME: &'static str = "delete_tag";
    type Input = DeleteTagInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let affected_file_ids = state.db.delete_tag_by_id(input.tag_id).await?;
        crate::events::emit_mutation(
            "delete_tag",
            crate::events::MutationImpact::tag_structure_change(),
        );
        Ok(serde_json::json!({
            "affected_files": affected_file_ids.len(),
        }))
    }
}

impl TypedCommand for NormalizeIngestedNamespaces {
    const NAME: &'static str = "normalize_ingested_namespaces";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for CompanionGetNamespaceValues {
    const NAME: &'static str = "companion_get_namespace_values";
    type Input = CompanionGetNamespaceValuesInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
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
}

impl TypedCommand for CompanionGetFilesByTag {
    const NAME: &'static str = "companion_get_files_by_tag";
    type Input = CompanionGetFilesByTagInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::tags::controller::TagController::find_files_by_tags(
            &state.db, vec![input.tag], None, None,
        ).await?;
        serde_json::to_value(&result).map_err(|e| e.to_string())
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        SearchTags::NAME => Some(run_typed::<SearchTags>(state, args).await),
        SearchTagsPaged::NAME => Some(run_typed::<SearchTagsPaged>(state, args).await),
        GetAllTagsWithCounts::NAME => Some(run_typed::<GetAllTagsWithCounts>(state, args).await),
        GetFileTags::NAME => Some(run_typed::<GetFileTags>(state, args).await),
        AddTags::NAME => Some(run_typed::<AddTags>(state, args).await),
        RemoveTags::NAME => Some(run_typed::<RemoveTags>(state, args).await),
        AddTagsBatch::NAME => Some(run_typed::<AddTagsBatch>(state, args).await),
        RemoveTagsBatch::NAME => Some(run_typed::<RemoveTagsBatch>(state, args).await),
        FindFilesByTags::NAME => Some(run_typed::<FindFilesByTags>(state, args).await),
        SetTagAlias::NAME => Some(run_typed::<SetTagAlias>(state, args).await),
        RemoveTagAlias::NAME => Some(run_typed::<RemoveTagAlias>(state, args).await),
        GetTagAliases::NAME => Some(run_typed::<GetTagAliases>(state, args).await),
        GetTagSiblingsForTag::NAME => Some(run_typed::<GetTagSiblingsForTag>(state, args).await),
        GetTagParentsForTag::NAME => Some(run_typed::<GetTagParentsForTag>(state, args).await),
        AddTagParent::NAME => Some(run_typed::<AddTagParent>(state, args).await),
        RemoveTagParent::NAME => Some(run_typed::<RemoveTagParent>(state, args).await),
        MergeTags::NAME => Some(run_typed::<MergeTags>(state, args).await),
        LookupTagTypes::NAME => Some(run_typed::<LookupTagTypes>(state, args).await),
        GetTagsPaginated::NAME => Some(run_typed::<GetTagsPaginated>(state, args).await),
        GetNamespaceSummary::NAME => Some(run_typed::<GetNamespaceSummary>(state, args).await),
        RenameTag::NAME => Some(run_typed::<RenameTag>(state, args).await),
        DeleteTag::NAME => Some(run_typed::<DeleteTag>(state, args).await),
        NormalizeIngestedNamespaces::NAME => Some(run_typed::<NormalizeIngestedNamespaces>(state, args).await),
        CompanionGetNamespaceValues::NAME => Some(run_typed::<CompanionGetNamespaceValues>(state, args).await),
        CompanionGetFilesByTag::NAME => Some(run_typed::<CompanionGetFilesByTag>(state, args).await),
        _ => None,
    }
}
