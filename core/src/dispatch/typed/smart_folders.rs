//! Typed command implementations for smart folder operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ReorderSmartFoldersInput {
    #[ts(type = "[number, number][]")]
    pub moves: Vec<(i64, i64)>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct CreateSmartFolderInput {
    #[ts(type = "Record<string, unknown>")]
    pub folder: crate::smart_folders::db::SmartFolder,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateSmartFolderInput {
    pub id: String,
    #[ts(type = "Record<string, unknown>")]
    pub folder: crate::smart_folders::db::SmartFolder,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteSmartFolderInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct QuerySmartFolderInput {
    pub predicate: crate::smart_folders::db::SmartFolderPredicate,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub limit: Option<usize>,
    #[serde(default)]
    #[ts(type = "number | null")]
    pub offset: Option<usize>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct CountSmartFolderInput {
    pub predicate: crate::smart_folders::db::SmartFolderPredicate,
}

// ─── Command structs ───────────────────────────────────────────────────────

struct ReorderSmartFolders;
struct CreateSmartFolder;
struct UpdateSmartFolder;
struct DeleteSmartFolder;
struct ListSmartFolders;
struct QuerySmartFolder;
struct CountSmartFolder;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for ReorderSmartFolders {
    const NAME: &'static str = "reorder_smart_folders";
    type Input = ReorderSmartFoldersInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        state.db.reorder_smart_folders(input.moves).await?;
        crate::events::emit_mutation(
            "reorder_smart_folders",
            crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders),
        );
        Ok(())
    }
}

impl TypedCommand for CreateSmartFolder {
    const NAME: &'static str = "create_smart_folder";
    type Input = CreateSmartFolderInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::smart_folders::controller::SmartFolderController::create_smart_folder(
                &state.db,
                input.folder,
            )
            .await?;
        crate::events::emit_mutation(
            "create_smart_folder",
            crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders),
        );
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for UpdateSmartFolder {
    const NAME: &'static str = "update_smart_folder";
    type Input = UpdateSmartFolderInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let (result, predicate_changed) =
            crate::smart_folders::controller::SmartFolderController::update_smart_folder(
                &state.db,
                input.id.clone(),
                input.folder,
            )
            .await?;
        let sf_id: i64 = input.id.parse().unwrap_or(0);
        let mut impact =
            crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders)
                .smart_folder_ids(vec![sf_id]);
        if predicate_changed {
            impact = impact
                .grid_scopes(vec![format!("smart:{}", sf_id)])
                .selection_summary();
        }
        crate::events::emit_mutation("update_smart_folder", impact);
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for DeleteSmartFolder {
    const NAME: &'static str = "delete_smart_folder";
    type Input = DeleteSmartFolderInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let sf_id: i64 = input.id.parse().unwrap_or(0);
        crate::smart_folders::controller::SmartFolderController::delete_smart_folder(
            &state.db,
            input.id,
        )
        .await?;
        crate::events::emit_mutation(
            "delete_smart_folder",
            crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders)
                .smart_folder_ids(vec![sf_id])
                .selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for ListSmartFolders {
    const NAME: &'static str = "list_smart_folders";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::smart_folders::controller::SmartFolderController::list_smart_folders(&state.db)
                .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for QuerySmartFolder {
    const NAME: &'static str = "query_smart_folder";
    type Input = QuerySmartFolderInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::smart_folders::controller::SmartFolderController::query_smart_folder(
                &state.db,
                input.predicate,
                input.limit,
                input.offset,
            )
            .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for CountSmartFolder {
    const NAME: &'static str = "count_smart_folder";
    type Input = CountSmartFolderInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let count =
            crate::smart_folders::controller::SmartFolderController::count_smart_folder(
                &state.db,
                input.predicate,
            )
            .await?;
        Ok(serde_json::to_value(&count).map_err(|e| e.to_string())?)
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        ReorderSmartFolders::NAME => Some(run_typed::<ReorderSmartFolders>(state, args).await),
        CreateSmartFolder::NAME => Some(run_typed::<CreateSmartFolder>(state, args).await),
        UpdateSmartFolder::NAME => Some(run_typed::<UpdateSmartFolder>(state, args).await),
        DeleteSmartFolder::NAME => Some(run_typed::<DeleteSmartFolder>(state, args).await),
        ListSmartFolders::NAME => Some(run_typed::<ListSmartFolders>(state, args).await),
        QuerySmartFolder::NAME => Some(run_typed::<QuerySmartFolder>(state, args).await),
        CountSmartFolder::NAME => Some(run_typed::<CountSmartFolder>(state, args).await),
        _ => None,
    }
}
