//! Handler functions for folder and collection operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Serde helper: null → Some(""), absent → None ─────────────────────────

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let val: serde_json::Value = serde::Deserialize::deserialize(deserializer)?;
    if val.is_null() {
        Ok(Some(String::new()))
    } else if let Some(s) = val.as_str() {
        Ok(Some(s.to_string()))
    } else {
        Err(serde::de::Error::custom("expected string or null"))
    }
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFolderFilesInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFolderCoverHashInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetFileFoldersInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetEntityFoldersInput {
    #[ts(type = "number")]
    pub entity_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct MoveFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
    #[ts(type = "[number, number][]")]
    pub sibling_order: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateFolderInput {
    pub name: String,
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub auto_tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateFolderParentInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddFilesToFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveFilesFromFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderFoldersInput {
    #[ts(type = "[number, number][]")]
    pub moves: Vec<(i64, i64)>,
}

/// Unified folder item reorder. Exactly one mode: `moves` (drag-drop),
/// `sort_by`+`direction` (sort), or `reverse: true` (reverse).
#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderFolderItemsInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[serde(default)]
    pub moves: Option<Vec<crate::types::FolderReorderMove>>,
    #[serde(default)]
    pub sort_by: Option<String>,
    #[serde(default)]
    pub direction: Option<String>,
    #[serde(default)]
    pub hashes: Option<Vec<String>>,
    #[serde(default)]
    pub reverse: Option<bool>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetCollectionSummaryInput {
    #[ts(type = "number")]
    pub id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateCollectionInput {
    pub name: String,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    #[ts(type = "string | null")]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    #[ts(type = "string | null")]
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
    #[serde(alias = "sourceUrls")]
    pub source_urls: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetCollectionRatingInput {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetCollectionSourceUrlsInput {
    #[ts(type = "number")]
    pub id: i64,
    #[serde(alias = "sourceUrls")]
    pub source_urls: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveCollectionMembersInput {
    #[ts(type = "number")]
    pub id: i64,
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn list_folders(state: &AppState, _input: serde_json::Value) -> Result<Vec<crate::folders::db::Folder>, String> {
    state.db.list_folders().await
}

pub async fn get_folder_files(state: &AppState, input: GetFolderFilesInput) -> Result<Vec<String>, String> {
    state.db.get_folder_entity_hashes(input.folder_id).await
}

pub async fn get_folder_cover_hash(state: &AppState, input: GetFolderCoverHashInput) -> Result<Option<String>, String> {
    state.db.get_folder_cover_hash(input.folder_id).await
}

pub async fn get_file_folders(state: &AppState, input: GetFileFoldersInput) -> Result<Vec<crate::folders::db::FolderMembership>, String> {
    state.db.get_entity_folder_memberships(&input.hash).await
}

pub async fn get_entity_folders(state: &AppState, input: GetEntityFoldersInput) -> Result<Vec<crate::folders::db::FolderMembership>, String> {
    state.db.get_entity_folder_memberships_by_entity_id(input.entity_id).await
}

pub async fn move_folder(state: &AppState, input: MoveFolderInput) -> Result<(), String> {
    state.db.move_folder(input.folder_id, input.new_parent_id, input.sibling_order).await?;
    crate::events::emit_mutation(
        "move_folder",
        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
            .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn create_folder(state: &AppState, input: CreateFolderInput) -> Result<crate::folders::db::Folder, String> {
    let folder = crate::folders::controller::FolderController::create_folder(
        &state.db, input.name, input.parent_id, input.icon, input.color,
    ).await?;
    crate::events::emit_mutation(
        "create_folder",
        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders),
    );
    Ok(folder)
}

pub async fn update_folder(state: &AppState, input: UpdateFolderInput) -> Result<(), String> {
    crate::folders::controller::FolderController::update_folder(
        &state.db, input.folder_id, input.name, input.icon, input.color, input.auto_tags,
    ).await?;
    crate::events::emit_mutation(
        "update_folder",
        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
            .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn delete_folder(state: &AppState, input: DeleteFolderInput) -> Result<(), String> {
    crate::folders::controller::FolderController::delete_folder(&state.db, input.folder_id).await?;
    crate::events::emit_mutation(
        "delete_folder",
        crate::events::MutationImpact::new()
            .domains(&[crate::events::Domain::Folders, crate::events::Domain::Sidebar, crate::events::Domain::Selection])
            .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn update_folder_parent(state: &AppState, input: UpdateFolderParentInput) -> Result<(), String> {
    crate::folders::controller::FolderController::update_folder_parent(
        &state.db, input.folder_id, input.new_parent_id,
    ).await?;
    crate::events::emit_mutation(
        "update_folder_parent",
        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
            .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn add_files_to_folder(state: &AppState, input: AddFilesToFolderInput) -> Result<usize, String> {
    // Expand to include collection member hashes
    let expanded = state.db.expand_hashes_for_collections(&input.hashes).await?;
    let count = state.db.add_entities_to_folder_batch(input.folder_id, &expanded).await?;
    if count > 0 {
        crate::events::emit_mutation(
            "add_files_to_folder",
            crate::events::MutationImpact::folder_file_change(input.folder_id),
        );
    }
    Ok(count)
}

pub async fn remove_files_from_folder(state: &AppState, input: RemoveFilesFromFolderInput) -> Result<usize, String> {
    // Expand to include collection member hashes
    let expanded = state.db.expand_hashes_for_collections(&input.hashes).await?;
    let count = state.db.remove_entities_from_folder_batch(input.folder_id, &expanded).await?;
    if count > 0 {
        crate::events::emit_mutation(
            "remove_files_from_folder",
            crate::events::MutationImpact::folder_file_change(input.folder_id),
        );
    }
    Ok(count)
}

pub async fn reorder_folders(state: &AppState, input: ReorderFoldersInput) -> Result<(), String> {
    state.db.reorder_folders(input.moves).await?;
    crate::events::emit_mutation(
        "reorder_folders",
        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders),
    );
    Ok(())
}

pub async fn reorder_folder_items(state: &AppState, input: ReorderFolderItemsInput) -> Result<(), String> {
    if let Some(moves) = input.moves {
        crate::folders::controller::FolderController::reorder_folder_items(
            &state.db, input.folder_id, moves,
        ).await?;
    } else if let Some(sort_by) = input.sort_by {
        let direction = input.direction.unwrap_or_else(|| "asc".to_string());
        state.db.sort_folder_items(input.folder_id, sort_by, direction, input.hashes).await?;
    } else if input.reverse == Some(true) {
        state.db.reverse_folder_items(input.folder_id, input.hashes).await?;
    } else {
        return Err("No reorder operation specified".to_string());
    }
    crate::events::emit_mutation(
        "reorder_folder_items",
        crate::events::MutationImpact::folder_item_reorder(input.folder_id),
    );
    Ok(())
}

pub async fn get_collections(state: &AppState, _input: serde_json::Value) -> Result<Vec<crate::folders::collections_db::CollectionRecord>, String> {
    state.db.list_collections().await
}

pub async fn get_collection_summary(state: &AppState, input: GetCollectionSummaryInput) -> Result<crate::folders::collections_db::CollectionSummary, String> {
    state.db.get_collection_summary(input.id).await
}

pub async fn create_collection(state: &AppState, input: CreateCollectionInput) -> Result<i64, String> {
    let tags = input.tags.unwrap_or_default();
    let collection_id = state.db.create_collection(
        &input.name, input.description.as_deref(), &tags,
    ).await?;
    crate::events::emit_mutation(
        "create_collection",
        crate::events::MutationImpact::new()
            .domains(&[crate::events::Domain::Folders, crate::events::Domain::Sidebar, crate::events::Domain::Selection])
            .extra_grid_scopes(vec!["system:all".into()]),
    );
    Ok(collection_id)
}

pub async fn update_collection(state: &AppState, input: UpdateCollectionInput) -> Result<(), String> {
    state.db.update_collection(
        input.id,
        input.name.as_deref(),
        input.description.as_deref(),
        input.tags.as_deref(),
        input.source_urls.as_deref(),
    ).await?;
    crate::events::emit_mutation(
        "update_collection",
        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
            .folder_ids(vec![input.id]),
    );
    crate::events::emit_mutation(
        "update_collection_grid",
        crate::events::MutationImpact::selection_metadata_grid(),
    );
    Ok(())
}

pub async fn set_collection_rating(state: &AppState, input: SetCollectionRatingInput) -> Result<(), String> {
    state.db.set_collection_rating(input.id, input.rating).await?;
    crate::events::emit_mutation(
        "set_collection_rating",
        crate::events::MutationImpact::collection_update(input.id),
    );
    Ok(())
}

pub async fn set_collection_source_urls(state: &AppState, input: SetCollectionSourceUrlsInput) -> Result<(), String> {
    state.db.set_collection_source_urls(input.id, &input.source_urls).await?;
    crate::events::emit_mutation(
        "set_collection_source_urls",
        crate::events::MutationImpact::collection_update(input.id),
    );
    Ok(())
}

pub async fn reorder_collection_members(state: &AppState, input: ReorderCollectionMembersInput) -> Result<(), String> {
    state.db.reorder_collection_members_by_hashes(input.id, &input.hashes).await?;
    state.db.scope_cache_invalidate_scope("collection");
    crate::events::emit_mutation(
        "reorder_collection_members",
        crate::events::MutationImpact::collection_members_reordered(input.id),
    );
    Ok(())
}

/// Look up the cover-file hash for a collection entity (best-effort, returns None on error).
async fn collection_cover_hash(
    db: &crate::sqlite::SqliteDatabase,
    entity_id: i64,
) -> Option<String> {
    db.with_read_conn(move |conn| {
        use rusqlite::OptionalExtension;
        conn.query_row(
            "SELECT f.hash FROM media_entity me \
             JOIN file f ON f.file_id = me.cover_file_id \
             WHERE me.entity_id = ?1",
            [entity_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
    })
    .await
    .ok()
    .flatten()
}

pub async fn add_collection_members(state: &AppState, input: AddCollectionMembersInput) -> Result<usize, String> {
    let added = state.db.add_collection_members_by_hashes(input.id, &input.hashes).await?;
    state.db.scope_cache_invalidate_scope("collection");
    let cover_hash = collection_cover_hash(&state.db, input.id).await;
    let mut impact = crate::events::MutationImpact::collection_membership_change(input.id);
    if let Some(h) = cover_hash {
        impact = impact.file_hashes(vec![h]);
    }
    crate::events::emit_mutation("add_collection_members", impact);
    Ok(added)
}

pub async fn remove_collection_members(state: &AppState, input: RemoveCollectionMembersInput) -> Result<usize, String> {
    let removed = state.db.remove_collection_members_by_hashes(input.id, &input.hashes).await?;
    state.db.scope_cache_invalidate_scope("collection");
    let cover_hash = collection_cover_hash(&state.db, input.id).await;
    let mut impact = crate::events::MutationImpact::collection_membership_change(input.id);
    if let Some(h) = cover_hash {
        impact = impact.file_hashes(vec![h]);
    }
    crate::events::emit_mutation("remove_collection_members", impact);
    Ok(removed)
}

pub async fn delete_collection(state: &AppState, input: DeleteCollectionInput) -> Result<(), String> {
    state.db.delete_collection(input.id).await?;
    state.db.scope_cache_invalidate_scope("collection");
    crate::events::emit_mutation(
        "delete_collection",
        crate::events::MutationImpact::new()
            .domains(&[crate::events::Domain::Folders, crate::events::Domain::Sidebar, crate::events::Domain::Selection])
            .folder_ids(vec![input.id])
            .extra_grid_scopes(vec!["system:all".into()]),
    );
    Ok(())
}
