//! Handler functions for folder and collection operations.

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
pub struct SetFolderWatchConfigInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub watch_path: String,
    #[serde(default = "default_true")]
    pub watch_enabled: bool,
    #[serde(default)]
    pub watch_subfolders: bool,
    pub watch_import_status_mode: String,
    #[serde(default)]
    pub import_existing_now: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ClearFolderWatchConfigInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
}

fn default_true() -> bool {
    true
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
    #[serde(default)]
    pub hashes: Vec<String>,
    #[serde(default)]
    pub selection: Option<crate::types::SelectionQuerySpec>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveFilesFromFolderInput {
    #[ts(type = "number")]
    pub folder_id: i64,
    #[serde(default)]
    pub hashes: Vec<String>,
    #[serde(default)]
    pub selection: Option<crate::types::SelectionQuerySpec>,
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
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateCollectionInput {
    #[ts(type = "number")]
    pub id: i64,
    pub name: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddCollectionTagsInput {
    #[ts(type = "number")]
    pub id: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RemoveCollectionTagsInput {
    #[ts(type = "number")]
    pub id: i64,
    pub tags: Vec<String>,
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

pub async fn list_folders(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<Vec<crate::folders::db::Folder>, String> {
    state.db.list_folders().await
}

pub async fn get_folder_files(
    state: &AppState,
    input: GetFolderFilesInput,
) -> Result<Vec<String>, String> {
    state.db.get_folder_entity_hashes(input.folder_id).await
}

pub async fn get_folder_cover_hash(
    state: &AppState,
    input: GetFolderCoverHashInput,
) -> Result<Option<String>, String> {
    state.db.get_folder_cover_hash(input.folder_id).await
}

pub async fn get_file_folders(
    state: &AppState,
    input: GetFileFoldersInput,
) -> Result<Vec<crate::folders::db::FolderMembership>, String> {
    state.db.get_entity_folder_memberships(&input.hash).await
}

pub async fn get_entity_folders(
    state: &AppState,
    input: GetEntityFoldersInput,
) -> Result<Vec<crate::folders::db::FolderMembership>, String> {
    state
        .db
        .get_entity_folder_memberships_by_entity_id(input.entity_id)
        .await
}

pub async fn move_folder(state: &AppState, input: MoveFolderInput) -> Result<(), String> {
    let sibling_order = input.sibling_order;
    state
        .db
        .move_folder(input.folder_id, input.new_parent_id, sibling_order.clone())
        .await?;
    crate::events::emit_state_changed(
        "move_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[crate::runtime_contract::state_change::Domain::Folders, crate::runtime_contract::state_change::Domain::Sidebar])
            .folder_ids(vec![input.folder_id])
            .folder_parent_changes(vec![(input.folder_id, input.new_parent_id)])
            .folder_order_changes(sibling_order),
    );
    Ok(())
}

pub async fn create_folder(
    state: &AppState,
    input: CreateFolderInput,
) -> Result<crate::folders::db::Folder, String> {
    let folder = crate::folders::service::create_folder(
        &state.db,
        input.name,
        input.parent_id,
        input.icon,
        input.color,
    )
    .await?;
    crate::events::emit_state_changed(
        "create_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[crate::runtime_contract::state_change::Domain::Folders, crate::runtime_contract::state_change::Domain::Sidebar])
        .folder_ids(vec![folder.folder_id]),
    );
    Ok(folder)
}

pub async fn update_folder(state: &AppState, input: UpdateFolderInput) -> Result<(), String> {
    crate::folders::service::update_folder(
        &state.db,
        input.folder_id,
        input.name,
        input.icon,
        input.color,
        input.auto_tags,
    )
    .await?;
    crate::events::emit_state_changed(
        "update_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[crate::runtime_contract::state_change::Domain::Folders, crate::runtime_contract::state_change::Domain::Sidebar])
        .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn set_folder_watch_config(
    state: &AppState,
    input: SetFolderWatchConfigInput,
) -> Result<(), String> {
    if !matches!(
        input.watch_import_status_mode.as_str(),
        "inherit" | "inbox" | "active"
    ) {
        return Err("watch_import_status_mode must be inherit, inbox, or active".into());
    }
    let canonical_path = std::fs::canonicalize(&input.watch_path)
        .map_err(|err| format!("Failed to resolve watch path: {err}"))?;
    if !canonical_path.is_dir() {
        return Err(format!(
            "Watch path is not a directory: {}",
            canonical_path.display()
        ));
    }
    // Prevent watching paths inside the library directory (would cause circular imports)
    if canonical_path.starts_with(&state.library_root) {
        return Err("Cannot watch a folder inside the current library directory.".into());
    }
    let canonical_path = canonical_path.to_string_lossy().to_string();

    crate::folders::service::set_folder_watch_config(
        &state.db,
        input.folder_id,
        canonical_path.clone(),
        input.watch_enabled,
        input.watch_subfolders,
        input.watch_import_status_mode.clone(),
    )
    .await?;

    if input.import_existing_now {
        // import_existing_for_folder_watch emits its own final combined delta
        // that includes both the imported files and folder membership changes.
        // No separate watch-config emission needed — one combined action.
        crate::folders::watch::import_existing_for_folder_watch(
            &state.db,
            &state.blob_store,
            input.folder_id,
            &canonical_path,
            input.watch_subfolders,
            &input.watch_import_status_mode,
        )
        .await?;
    } else {
        crate::events::emit_state_changed(
            "set_folder_watch_config",
            crate::runtime_contract::change_builder::ChangeImpact::new()
                .add_domain(crate::runtime_contract::state_change::Domain::Folders)
                .folder_ids(vec![input.folder_id]),
        );
    }

    let _ = state
        .folder_watch_commands
        .send(crate::folders::watch::FolderWatchCommand::Reload);

    Ok(())
}

pub async fn clear_folder_watch_config(
    state: &AppState,
    input: ClearFolderWatchConfigInput,
) -> Result<(), String> {
    crate::folders::service::clear_folder_watch_config(&state.db, input.folder_id).await?;
    let _ = state
        .folder_watch_commands
        .send(crate::folders::watch::FolderWatchCommand::Reload);
    crate::events::emit_state_changed(
        "clear_folder_watch_config",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Folders)
            .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn delete_folder(state: &AppState, input: DeleteFolderInput) -> Result<(), String> {
    crate::folders::service::delete_folder(&state.db, input.folder_id).await?;
    crate::events::emit_state_changed(
        "delete_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
                crate::runtime_contract::state_change::Domain::Selection,
            ])
            .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn update_folder_parent(
    state: &AppState,
    input: UpdateFolderParentInput,
) -> Result<(), String> {
    crate::folders::service::update_folder_parent(&state.db, input.folder_id, input.new_parent_id)
        .await?;
    crate::events::emit_state_changed(
        "update_folder_parent",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[crate::runtime_contract::state_change::Domain::Folders, crate::runtime_contract::state_change::Domain::Sidebar])
        .folder_ids(vec![input.folder_id]),
    );
    Ok(())
}

pub async fn add_files_to_folder(
    state: &AppState,
    input: AddFilesToFolderInput,
) -> Result<usize, String> {
    let hashes = resolve_folder_op_hashes(state, input.hashes, input.selection).await?;
    if hashes.is_empty() {
        return Ok(0);
    }
    let resolved = state
        .db
        .resolve_entity_hashes_with_expansion(&hashes, EntityExpansionMode::EntityAndDescendants)
        .await?;
    let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    let count = state
        .db
        .add_entities_to_folder_batch(input.folder_id, &entity_ids)
        .await?;
    if count > 0 {
        crate::events::emit_state_changed(
            "add_files_to_folder",
            crate::runtime_contract::change_builder::ChangeImpact::folder_file_change(
                input.folder_id,
            )
            .entity_hashes(hashes.clone())
            .member_hashes(descendant_hashes(&hashes, &resolved)),
        );
    }
    Ok(count)
}

pub async fn remove_files_from_folder(
    state: &AppState,
    input: RemoveFilesFromFolderInput,
) -> Result<usize, String> {
    let hashes = resolve_folder_op_hashes(state, input.hashes, input.selection).await?;
    if hashes.is_empty() {
        return Ok(0);
    }
    let resolved = state
        .db
        .resolve_entity_hashes_with_expansion(&hashes, EntityExpansionMode::EntityAndDescendants)
        .await?;
    let entity_ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    let count = state
        .db
        .remove_entities_from_folder_batch(input.folder_id, &entity_ids)
        .await?;
    if count > 0 {
        crate::events::emit_state_changed(
            "remove_files_from_folder",
            crate::runtime_contract::change_builder::ChangeImpact::folder_file_change(
                input.folder_id,
            )
            .entity_hashes(hashes.clone())
            .member_hashes(descendant_hashes(&hashes, &resolved)),
        );
    }
    Ok(count)
}

pub async fn reorder_folders(state: &AppState, input: ReorderFoldersInput) -> Result<(), String> {
    let fids: Vec<i64> = input.moves.iter().map(|(id, _)| *id).collect();
    let order_changes = input.moves.clone();
    state.db.reorder_folders(input.moves).await?;
    crate::events::emit_state_changed(
        "reorder_folders",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[crate::runtime_contract::state_change::Domain::Folders, crate::runtime_contract::state_change::Domain::Sidebar])
            .folder_ids(fids)
            .folder_order_changes(order_changes),
    );
    Ok(())
}

pub async fn reorder_folder_items(
    state: &AppState,
    input: ReorderFolderItemsInput,
) -> Result<(), String> {
    if let Some(moves) = input.moves {
        crate::folders::service::reorder_folder_items(&state.db, input.folder_id, moves).await?;
    } else if let Some(sort_by) = input.sort_by {
        let direction = input.direction.unwrap_or_else(|| "asc".to_string());
        state
            .db
            .sort_folder_items(input.folder_id, sort_by, direction, input.hashes)
            .await?;
    } else if input.reverse == Some(true) {
        state
            .db
            .reverse_folder_items(input.folder_id, input.hashes)
            .await?;
    } else {
        return Err("No reorder operation specified".to_string());
    }
    crate::events::emit_state_changed(
        "reorder_folder_items",
        crate::runtime_contract::change_builder::ChangeImpact::folder_item_reorder(input.folder_id),
    );
    Ok(())
}

pub async fn get_collections(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<Vec<crate::folders::collections_db::CollectionRecord>, String> {
    state.db.list_collections().await
}

pub async fn get_collection_summary(
    state: &AppState,
    input: GetCollectionSummaryInput,
) -> Result<crate::folders::collections_db::CollectionSummary, String> {
    state.db.get_collection_summary(input.id).await
}

pub async fn create_collection(
    state: &AppState,
    input: CreateCollectionInput,
) -> Result<i64, String> {
    let collection_id = state.db.create_collection(&input.name).await?;
    crate::events::emit_state_changed(
        "create_collection",
        crate::runtime_contract::change_builder::ChangeImpact::collection_membership_change(
            collection_id,
        )
        .status_changed()
        .sidebar_counts_from(&state.db)
        .extra_grid_scopes(vec!["system:active".into()]),
    );
    Ok(collection_id)
}

pub async fn update_collection(
    state: &AppState,
    input: UpdateCollectionInput,
) -> Result<(), String> {
    let member_hashes = if input.tags.is_some() {
        state.db.list_collection_member_hashes(input.id).await?
    } else {
        Vec::new()
    };
    state
        .db
        .update_collection(input.id, input.name.as_deref(), input.tags.as_deref())
        .await?;
    let mut impact =
        crate::runtime_contract::change_builder::ChangeImpact::collection_update(input.id);
    if input.tags.is_some() {
        impact = impact.tags_changed();
        if !member_hashes.is_empty() {
            impact = impact.entity_hashes(member_hashes);
        }
    }
    crate::events::emit_state_changed("update_collection", impact);
    Ok(())
}

// add_collection_tags / remove_collection_tags removed —
// collection tags use the generic add_tags/remove_tags path via entity_tag_raw.

pub async fn reorder_collection_members(
    state: &AppState,
    input: ReorderCollectionMembersInput,
) -> Result<(), String> {
    state
        .db
        .reorder_collection_members_by_hashes(input.id, &input.hashes)
        .await?;
    crate::events::emit_state_changed(
        "reorder_collection_members",
        crate::runtime_contract::change_builder::ChangeImpact::collection_update(
            input.id,
        ),
    );
    Ok(())
}

pub async fn add_collection_members(
    state: &AppState,
    input: AddCollectionMembersInput,
) -> Result<usize, String> {
    let added = state
        .db
        .add_collection_members_by_hashes(input.id, &input.hashes)
        .await?;
    if added > 0 {
        // Members are now hidden inside a collection — remove them from status bitmaps
        let resolved = state
            .db
            .resolve_entity_hashes_batch(&input.hashes)
            .await
            .unwrap_or_default();
        {
            use crate::sqlite::bitmaps::BitmapKey;
            for (_, fid) in &resolved {
                for s in 0..=2i64 {
                    state.db.bitmaps.remove(&BitmapKey::Status(s), *fid as u32);
                }
            }
        }
        state
            .db
            .emit_read_model_event(crate::sqlite::ReadModelEvent::StatusBatchChanged);
        crate::events::emit_state_changed(
            "add_collection_members",
            crate::runtime_contract::change_builder::ChangeImpact::collection_membership_change(
                input.id,
            )
            .status_changed()
            .sidebar_counts_from(&state.db)
            .extra_grid_scopes(vec!["system:active".into()]),
        );
    }
    Ok(added)
}

pub async fn remove_collection_members(
    state: &AppState,
    input: RemoveCollectionMembersInput,
) -> Result<usize, String> {
    // Resolve member file_ids before removal (they'll be orphaned after)
    let resolved = state
        .db
        .resolve_entity_hashes_batch(&input.hashes)
        .await
        .unwrap_or_default();
    let removed = state
        .db
        .remove_collection_members_by_hashes(input.id, &input.hashes)
        .await?;
    if removed > 0 {
        // Members are now standalone — add them back to status bitmaps with their actual status
        {
            use crate::sqlite::bitmaps::BitmapKey;
            for (_, fid) in &resolved {
                let status = state
                    .db
                    .with_read_conn({
                        let f = *fid;
                        move |conn| {
                            conn.query_row(
                                "SELECT status FROM file WHERE file_id = ?1",
                                [f],
                                |row| row.get::<_, i64>(0),
                            )
                        }
                    })
                    .await
                    .unwrap_or(1);
                state
                    .db
                    .bitmaps
                    .insert(&BitmapKey::Status(status), *fid as u32);
            }
        }
        state
            .db
            .emit_read_model_event(crate::sqlite::ReadModelEvent::StatusBatchChanged);
        crate::events::emit_state_changed(
            "remove_collection_members",
            crate::runtime_contract::change_builder::ChangeImpact::collection_membership_change(
                input.id,
            )
            .status_changed()
            .sidebar_counts_from(&state.db)
            .extra_grid_scopes(vec!["system:active".into()]),
        );
    }
    Ok(removed)
}

pub async fn delete_collection(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<(), String> {
    let member_hashes = state.db.list_collection_member_hashes(input.id).await?;
    let affected_folder_ids = state
        .db
        .get_entity_folder_memberships_by_entity_id(input.id)
        .await?
        .into_iter()
        .map(|folder| folder.folder_id)
        .collect::<Vec<_>>();
    // Get member file_ids BEFORE deletion (for bitmap updates)
    let member_file_ids = state
        .db
        .with_read_conn({
            let id = input.id;
            move |conn| crate::folders::collections_db::get_collection_member_file_ids(conn, id)
        })
        .await?;

    state.db.delete_collection(input.id).await?;

    // Members are now standalone — add them to the correct Status bitmap based on their actual status.
    // The collection entity was deleted — remove it from Status bitmaps.
    {
        use crate::sqlite::bitmaps::BitmapKey;
        let cid = input.id as u32;
        for s in 0..=2i64 {
            state.db.bitmaps.remove(&BitmapKey::Status(s), cid);
        }
        // Look up each member's actual status and add to the right bitmap
        let member_statuses = state
            .db
            .with_read_conn({
                let fids = member_file_ids.clone();
                move |conn| {
                    let mut result = Vec::with_capacity(fids.len());
                    for &fid in &fids {
                        let status: i64 = conn
                            .query_row("SELECT status FROM file WHERE file_id = ?1", [fid], |row| {
                                row.get(0)
                            })
                            .unwrap_or(1);
                        result.push((fid, status));
                    }
                    Ok(result)
                }
            })
            .await?;
        for (fid, status) in &member_statuses {
            state
                .db
                .bitmaps
                .insert(&BitmapKey::Status(*status), *fid as u32);
        }
    }

    // Trigger compiler to rebuild sidebar projection with updated bitmaps
    state
        .db
        .emit_read_model_event(crate::sqlite::ReadModelEvent::StatusBatchChanged);

    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::collection_delete(
        input.id,
        affected_folder_ids,
    )
    .status_changed()
    .sidebar_counts_from(&state.db);
    // Clone before moving into impact — needed for color backfill below.
    let backfill_hashes = member_hashes.clone();
    if !member_hashes.is_empty() {
        impact = impact.entity_hashes(member_hashes);
    }
    crate::events::emit_state_changed("delete_collection", impact);

    // Backfill missing colors for members that were hidden inside the collection.
    if !backfill_hashes.is_empty() {
        let db = state.db.clone();
        let blob_store = state.blob_store.clone();
        tokio::spawn(async move {
            super::media_io::backfill_missing_deferred(&db, &blob_store, &backfill_hashes).await;
        });
    }

    Ok(())
}

pub async fn list_collection_member_hashes(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<Vec<String>, String> {
    state.db.list_collection_member_hashes(input.id).await
}

// ─── Helpers ──────────────────────────────────────────────────────────────

/// Resolve hashes for folder operations — from explicit hashes or a selection spec.
async fn resolve_folder_op_hashes(
    state: &AppState,
    hashes: Vec<String>,
    selection: Option<crate::types::SelectionQuerySpec>,
) -> Result<Vec<String>, String> {
    if let Some(sel) = selection {
        let bitmap = super::media_lifecycle::resolve_selection_bitmap(state, &sel).await?;
        let ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
        let pairs = state.db.resolve_ids_batch(&ids).await?;
        Ok(pairs.into_iter().map(|(_, h)| h).collect())
    } else {
        Ok(hashes)
    }
}
