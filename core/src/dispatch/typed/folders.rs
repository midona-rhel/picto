//! Handler functions for folder and collection operations.

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

fn folder_meta_json_canonical(folder: &crate::db::query::folders::FolderRow) -> String {
    serde_json::json!({
        "folder_id": folder.folder_id,
        "notes": folder.notes,
        "auto_tags": folder.auto_tags,
        "watch_path": folder.watch_path,
        "watch_enabled": folder.watch_enabled,
        "watch_subfolders": folder.watch_subfolders,
        "watch_import_status_mode": folder.watch_import_status_mode,
    })
    .to_string()
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
    pub notes: Option<String>,
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
) -> Result<Vec<crate::db::query::folders::FolderRow>, String> {
    state.engine.list_folders()
}

// ── Legacy-only read handlers ────────────────────────────────────────────
// The following handlers are NOT called by the rebuilt frontend (src/).
// They exist only for the legacy frontend (legacy/frontend/) and will be
// removed when the legacy frontend is deleted.

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
        .engine
        .move_folder(input.folder_id, input.new_parent_id)?;
    if !sibling_order.is_empty() {
        state.engine.reorder_folders(&sibling_order)?;
    }
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "move_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(vec![input.folder_id])
            .folder_parent_changes(vec![(input.folder_id, input.new_parent_id)])
            .folder_order_changes(sibling_order),
    );
    Ok(())
}

pub async fn create_folder(
    state: &AppState,
    input: CreateFolderInput,
) -> Result<crate::db::query::folders::FolderRow, String> {
    let folder_id = state.engine.create_folder(
        &input.name,
        input.parent_id,
        input.icon.as_deref(),
        input.color.as_deref(),
    )?;
    let folder = state
        .engine
        .get_folder(folder_id)?
        .ok_or_else(|| format!("Folder {folder_id} not found after create"))?;
    let meta = folder_meta_json_canonical(&folder);
    let upsert = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{folder_id}"),
        removed: None,
        upsert: Some(true),
        kind: Some("folder".into()),
        parent_id: Some(
            folder
                .parent_id
                .map(|pid| format!("folder:{pid}"))
                .or(Some("section:folders".into())),
        ),
        name: Some(folder.name.clone()),
        icon: Some(folder.icon.clone()),
        color: Some(folder.color.clone()),
        sort_order: Some(folder.sort_order),
        count: Some(Some(0)),
        selectable: Some(true),
        freshness: Some("exact".into()),
        meta_json: Some(Some(meta)),
    };
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "create_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(vec![folder_id])
            .sidebar_node_patch(upsert),
    );
    Ok(folder)
}

pub async fn update_folder(state: &AppState, input: UpdateFolderInput) -> Result<(), String> {
    let patch = crate::db::types::FolderPatch {
        name: input.name.clone(),
        icon: input.icon.clone(),
        color: input.color.clone(),
        notes: input.notes,
        auto_tags: input
            .auto_tags
            .map(|tags| serde_json::to_string(&tags).unwrap_or_else(|_| "[]".into())),
        ..Default::default()
    };
    state.engine.update_folder(input.folder_id, &patch)?;
    let folder = state
        .engine
        .get_folder(input.folder_id)?
        .ok_or_else(|| format!("Folder {} not found after update", input.folder_id))?;
    let sidebar_patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: None,
        upsert: None,
        kind: None,
        parent_id: None,
        name: input.name,
        icon: input.icon.map(Some),
        color: input.color.map(Some),
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: Some(Some(folder_meta_json_canonical(&folder))),
    };
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "update_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(vec![input.folder_id])
            .sidebar_node_patch(sidebar_patch),
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

    let watch_patch = crate::db::types::FolderPatch {
        watch_path: Some(canonical_path.clone()),
        watch_enabled: Some(input.watch_enabled),
        watch_subfolders: Some(input.watch_subfolders),
        watch_import_status_mode: Some(input.watch_import_status_mode.clone()),
        ..Default::default()
    };
    state.engine.update_folder(input.folder_id, &watch_patch)?;

    if input.import_existing_now {
        // import_existing_for_folder_watch emits its own final combined delta
        // that includes both the imported files and folder membership changes.
        // No separate watch-config emission needed — one combined action.
        crate::folders::watch::import_existing_for_folder_watch(
            &state.db,
            state.engine.db(),
            &state.blob_store,
            input.folder_id,
            &canonical_path,
            input.watch_subfolders,
            &input.watch_import_status_mode,
        )
        .await?;
    } else {
        let patch = crate::runtime_contract::state_change::SidebarNodePatch {
            node_id: format!("folder:{}", input.folder_id),
            removed: None,
            upsert: None,
            kind: None,
            parent_id: None,
            name: None,
            icon: None,
            color: None,
            sort_order: None,
            count: None,
            selectable: None,
            freshness: None,
            meta_json: Some(
                state
                    .engine
                    .get_folder(input.folder_id)
                    .ok()
                    .flatten()
                    .map(|f| folder_meta_json_canonical(&f)),
            ),
        };
        crate::events::emit_state_changed(
            "set_folder_watch_config",
            crate::runtime_contract::change_builder::ChangeImpact::new()
                .add_domains(&[
                    crate::runtime_contract::state_change::Domain::Folders,
                    crate::runtime_contract::state_change::Domain::Sidebar,
                ])
                .folder_ids(vec![input.folder_id])
                .sidebar_node_patch(patch),
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
    let clear_patch = crate::db::types::FolderPatch {
        watch_path: Some(String::new()),
        watch_enabled: Some(false),
        watch_subfolders: Some(false),
        watch_import_status_mode: Some("inherit".into()),
        ..Default::default()
    };
    state.engine.update_folder(input.folder_id, &clear_patch)?;
    let _ = state
        .folder_watch_commands
        .send(crate::folders::watch::FolderWatchCommand::Reload);
    let folder = state
        .engine
        .get_folder(input.folder_id)?
        .ok_or_else(|| format!("Folder {} not found", input.folder_id))?;
    let patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: None,
        upsert: None,
        kind: None,
        parent_id: None,
        name: None,
        icon: None,
        color: None,
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: Some(Some(folder_meta_json_canonical(&folder))),
    };
    crate::events::emit_state_changed(
        "clear_folder_watch_config",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(vec![input.folder_id])
            .sidebar_node_patch(patch),
    );
    Ok(())
}

pub async fn delete_folder(state: &AppState, input: DeleteFolderInput) -> Result<(), String> {
    state.engine.delete_folder(input.folder_id)?;
    let patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: Some(true),
        upsert: None,
        kind: None,
        parent_id: None,
        name: None,
        icon: None,
        color: None,
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: None,
    };
    crate::events::emit_state_changed(
        "delete_folder",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
                crate::runtime_contract::state_change::Domain::Selection,
            ])
            .folder_ids(vec![input.folder_id])
            .sidebar_node_patch(patch),
    );
    Ok(())
}

pub async fn update_folder_parent(
    state: &AppState,
    input: UpdateFolderParentInput,
) -> Result<(), String> {
    state
        .engine
        .move_folder(input.folder_id, input.new_parent_id)?;
    let patch = crate::runtime_contract::state_change::SidebarNodePatch {
        node_id: format!("folder:{}", input.folder_id),
        removed: None,
        upsert: None,
        kind: None,
        parent_id: Some(
            input
                .new_parent_id
                .map(|pid| format!("folder:{pid}"))
                .or(Some("section:folders".into())),
        ),
        name: None,
        icon: None,
        color: None,
        sort_order: None,
        count: None,
        selectable: None,
        freshness: None,
        meta_json: None,
    };
    crate::events::emit_state_changed(
        "update_folder_parent",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(vec![input.folder_id])
            .folder_parent_changes(vec![(input.folder_id, input.new_parent_id)])
            .sidebar_node_patch(patch),
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
    state.engine.reorder_folders(&input.moves)?;
    state
        .engine
        .run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    crate::events::emit_state_changed(
        "reorder_folders",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Folders,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .folder_ids(fids)
            .folder_order_changes(order_changes),
    );
    Ok(())
}

// Legacy-only: reorder_folder_items uses old DB for hash-to-position resolution.
// Not called by the rebuilt frontend.
pub async fn reorder_folder_items(
    state: &AppState,
    input: ReorderFolderItemsInput,
) -> Result<(), String> {
    if let Some(moves) = input.moves {
        // Fenced legacy: relative reorder (before/after hash) uses old DB which resolves hashes to positions
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
) -> Result<Vec<crate::db::types::CollectionRecord>, String> {
    state.engine.get_collections()
}

pub async fn get_collection_summary(
    state: &AppState,
    input: GetCollectionSummaryInput,
) -> Result<crate::db::types::CollectionSummary, String> {
    state.engine.get_collection_summary(input.id)
}

pub async fn create_collection(
    state: &AppState,
    input: CreateCollectionInput,
) -> Result<i64, String> {
    state.engine.create_collection(&input.name)
}

pub async fn update_collection(
    state: &AppState,
    input: UpdateCollectionInput,
) -> Result<(), String> {
    if input.tags.is_some() {
        return Err(
            "Collection tag editing no longer lives in update_collection; use the canonical tag commands instead"
                .to_string(),
        );
    }
    let Some(name) = input.name.as_deref() else {
        return Ok(());
    };
    state.engine.update_collection(input.id, name)
}

// add_collection_tags / remove_collection_tags removed —
// collection tags use the generic add_tags/remove_tags path via entity_tag_raw.

pub async fn reorder_collection_members(
    state: &AppState,
    input: ReorderCollectionMembersInput,
) -> Result<(), String> {
    state
        .engine
        .reorder_collection_members_by_hashes(input.id, &input.hashes)
}

pub async fn add_collection_members(
    state: &AppState,
    input: AddCollectionMembersInput,
) -> Result<usize, String> {
    Ok(state
        .engine
        .add_collection_members_by_hashes(input.id, &input.hashes)?
        .added
        .len())
}

pub async fn remove_collection_members(
    state: &AppState,
    input: RemoveCollectionMembersInput,
) -> Result<usize, String> {
    Ok(state
        .engine
        .remove_collection_members_by_hashes(input.id, &input.hashes)?
        .removed
        .len())
}

pub async fn delete_collection(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<(), String> {
    state.engine.delete_collection(input.id)
}

pub async fn list_collection_member_hashes(
    state: &AppState,
    input: DeleteCollectionInput,
) -> Result<Vec<String>, String> {
    state.engine.list_collection_member_hashes(input.id)
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
