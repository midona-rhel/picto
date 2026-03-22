//! Handler functions for smart folder operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderSmartFoldersInput {
    #[ts(type = "number | null")]
    pub parent_id: Option<i64>,
    #[ts(type = "[number, number][]")]
    pub moves: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct MoveSmartFolderInput {
    pub smart_folder_id: i64,
    #[ts(type = "number | null")]
    pub new_parent_id: Option<i64>,
    #[ts(type = "[number, number][]")]
    pub sibling_order: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateSmartFolderInput {
    #[ts(type = "Record<string, unknown>")]
    pub folder: crate::smart_folders::db::SmartFolder,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateSmartFolderInput {
    pub id: String,
    #[ts(type = "Record<string, unknown>")]
    pub folder: crate::smart_folders::db::SmartFolder,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteSmartFolderInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CountSmartFolderInput {
    pub predicate: crate::smart_folders::db::SmartFolderPredicate,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn list_smart_folders(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = state.db.list_smart_folders().await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn create_smart_folder(
    state: &AppState,
    input: CreateSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let parent_id = input.folder.parent_id;
    if let Some(target_parent_id) = parent_id {
        let exists = state
            .db
            .with_read_conn(move |conn| {
                Ok(crate::smart_folders::db::get_smart_folder(conn, target_parent_id)?.is_some())
            })
            .await?;
        if !exists {
            return Err(format!(
                "Invalid smart folder parent id: {target_parent_id}"
            ));
        }
    }
    let result = crate::smart_folders::service::SmartFolderService::create_smart_folder(
        &state.db,
        input.folder,
    )
    .await?;
    crate::events::emit_state_changed(
        "create_smart_folder",
        crate::runtime_contract::change_builder::ChangeImpact::sidebar(
            crate::runtime_contract::state_change::Domain::SmartFolders,
        ),
    );
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn update_smart_folder(
    state: &AppState,
    input: UpdateSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let sf_id: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid smart folder id: {}", input.id))?;
    if input.folder.parent_id == Some(sf_id) {
        return Err("A smart folder cannot be its own parent".to_string());
    }
    if let Some(parent_id) = input.folder.parent_id {
        let blocked = state
            .db
            .with_read_conn(move |conn| {
                let descendants =
                    crate::smart_folders::db::collect_descendant_smart_folder_ids(conn, sf_id)?;
                Ok(descendants.into_iter().any(|id| id == parent_id))
            })
            .await?;
        if blocked {
            return Err("A smart folder cannot be moved under one of its descendants".to_string());
        }
    }
    let (result, predicate_changed) =
        crate::smart_folders::service::SmartFolderService::update_smart_folder(
            &state.db,
            input.id.clone(),
            input.folder,
        )
        .await?;
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::sidebar(
        crate::runtime_contract::state_change::Domain::SmartFolders,
    );
    if predicate_changed {
        impact = impact.smart_folder_ids(vec![sf_id]);
    }
    crate::events::emit_state_changed("update_smart_folder", impact);
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn move_smart_folder(
    state: &AppState,
    input: MoveSmartFolderInput,
) -> Result<(), String> {
    if input.new_parent_id == Some(input.smart_folder_id) {
        return Err("A smart folder cannot be its own parent".to_string());
    }
    if let Some(new_parent_id) = input.new_parent_id {
        let blocked = state
            .db
            .with_read_conn(move |conn| {
                let descendants = crate::smart_folders::db::collect_descendant_smart_folder_ids(
                    conn,
                    input.smart_folder_id,
                )?;
                Ok(descendants.into_iter().any(|id| id == new_parent_id))
            })
            .await?;
        if blocked {
            return Err("A smart folder cannot be moved under one of its descendants".to_string());
        }
    }
    crate::smart_folders::service::SmartFolderService::move_smart_folder(
        &state.db,
        input.smart_folder_id,
        input.new_parent_id,
        input.sibling_order,
    )
    .await?;
    crate::events::emit_state_changed(
        "move_smart_folder",
        crate::runtime_contract::change_builder::ChangeImpact::sidebar(
            crate::runtime_contract::state_change::Domain::SmartFolders,
        )
        .smart_folder_ids(vec![input.smart_folder_id]),
    );
    Ok(())
}

pub async fn delete_smart_folder(
    state: &AppState,
    input: DeleteSmartFolderInput,
) -> Result<(), String> {
    let sf_id: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid smart folder id: {}", input.id))?;
    crate::smart_folders::service::SmartFolderService::delete_smart_folder(&state.db, input.id)
        .await?;
    crate::events::emit_state_changed(
        "delete_smart_folder",
        crate::runtime_contract::change_builder::ChangeImpact::sidebar(
            crate::runtime_contract::state_change::Domain::SmartFolders,
        )
        .smart_folder_ids(vec![sf_id]),
    );
    Ok(())
}

pub async fn count_smart_folder(
    state: &AppState,
    input: CountSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let count = crate::smart_folders::service::SmartFolderService::count_smart_folder(
        &state.db,
        input.predicate,
    )
    .await?;
    Ok(serde_json::to_value(&count).map_err(|e| e.to_string())?)
}

pub async fn reorder_smart_folders(
    state: &AppState,
    input: ReorderSmartFoldersInput,
) -> Result<(), String> {
    crate::smart_folders::service::SmartFolderService::reorder_smart_folders(
        &state.db,
        input.parent_id,
        input.moves,
    )
    .await?;
    crate::events::emit_state_changed(
        "reorder_smart_folders",
        crate::runtime_contract::change_builder::ChangeImpact::sidebar(
            crate::runtime_contract::state_change::Domain::SmartFolders,
        ),
    );
    Ok(())
}
