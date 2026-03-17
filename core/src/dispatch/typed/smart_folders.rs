//! Handler functions for smart folder operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReorderSmartFoldersInput {
    #[ts(type = "[number, number][]")]
    pub moves: Vec<(i64, i64)>,
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
    let result = crate::smart_folders::service::SmartFolderService::create_smart_folder(
        &state.db,
        input.folder,
    )
    .await?;
    crate::events::emit_mutation(
        "create_smart_folder",
        crate::runtime_contract::mutation_builder::MutationImpact::sidebar(
            crate::runtime_contract::mutation::Domain::SmartFolders,
        ),
    );
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn update_smart_folder(
    state: &AppState,
    input: UpdateSmartFolderInput,
) -> Result<serde_json::Value, String> {
    let (result, predicate_changed) =
        crate::smart_folders::service::SmartFolderService::update_smart_folder(
            &state.db,
            input.id.clone(),
            input.folder,
        )
        .await?;
    let sf_id: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid smart folder id: {}", input.id))?;
    let mut impact = crate::runtime_contract::mutation_builder::MutationImpact::sidebar(
        crate::runtime_contract::mutation::Domain::SmartFolders,
    );
    if predicate_changed {
        impact = impact.smart_folder_ids(vec![sf_id]);
    }
    crate::events::emit_mutation("update_smart_folder", impact);
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
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
    crate::events::emit_mutation(
        "delete_smart_folder",
        crate::runtime_contract::mutation_builder::MutationImpact::sidebar(
            crate::runtime_contract::mutation::Domain::SmartFolders,
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
    state.db.reorder_smart_folders(input.moves).await?;
    crate::events::emit_mutation(
        "reorder_smart_folders",
        crate::runtime_contract::mutation_builder::MutationImpact::sidebar(
            crate::runtime_contract::mutation::Domain::SmartFolders,
        ),
    );
    Ok(())
}
