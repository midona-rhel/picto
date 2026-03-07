//! Typed command implementations for PTR (Public Tag Repository) operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct PtrGetTagsPaginatedInput {
    pub namespace: Option<String>,
    pub search: Option<String>,
    pub cursor: Option<String>,
    #[serde(default = "default_ptr_tags_limit")]
    #[ts(type = "number | null")]
    pub limit: i64,
}

fn default_ptr_tags_limit() -> i64 {
    500
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct PtrGetTagRelationInput {
    #[ts(type = "number")]
    pub tag_id: i64,
}

// ─── Command structs ───────────────────────────────────────────────────────

struct GetPtrStatus;
struct IsPtrSyncing;
struct GetPtrSyncProgress;
struct PtrSync;
struct CancelPtrSync;
struct PtrCancelBootstrap;
struct PtrBootstrapFromHydrusSnapshot;
struct PtrGetBootstrapStatus;
struct PtrGetCompactIndexStatus;
struct GetPtrSyncPerfBreakdown;
struct PtrGetNamespaceSummary;
struct PtrGetTagsPaginated;
struct PtrGetTagSiblings;
struct PtrGetTagParents;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for GetPtrStatus {
    const NAME: &'static str = "get_ptr_status";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::ptr::controller::PtrController::get_ptr_status(&state.ptr_db).await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for IsPtrSyncing {
    const NAME: &'static str = "is_ptr_syncing";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::ptr::controller::PtrController::is_ptr_syncing();
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for GetPtrSyncProgress {
    const NAME: &'static str = "get_ptr_sync_progress";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::ptr::controller::PtrController::get_sync_progress();
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PtrSync {
    const NAME: &'static str = "ptr_sync";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        crate::ptr::controller::PtrController::sync(
            &state.ptr_db,
            &state.settings,
            state.db.compiler_tx.clone(),
        )
        .await?;
        Ok(serde_json::json!({
            "id": "ptr-sync",
            "message": "PTR sync started in background",
        }))
    }
}

impl TypedCommand for CancelPtrSync {
    const NAME: &'static str = "cancel_ptr_sync";
    type Input = serde_json::Value;
    type Output = ();

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        crate::ptr::controller::PtrController::cancel_sync(&state.ptr_db)?;
        Ok(())
    }
}

impl TypedCommand for PtrCancelBootstrap {
    const NAME: &'static str = "ptr_cancel_bootstrap";
    type Input = serde_json::Value;
    type Output = ();

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        crate::ptr::controller::PtrController::cancel_bootstrap(&state.ptr_db)?;
        Ok(())
    }
}

impl TypedCommand for PtrBootstrapFromHydrusSnapshot {
    const NAME: &'static str = "ptr_bootstrap_from_hydrus_snapshot";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let req: crate::ptr::controller::PtrBootstrapRequest =
            serde_json::from_value(input).map_err(|e| format!("Invalid ptr bootstrap input: {e}"))?;
        let result = crate::ptr::controller::PtrController::bootstrap_from_hydrus_snapshot(
            &state.ptr_db,
            req,
        )
        .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PtrGetBootstrapStatus {
    const NAME: &'static str = "ptr_get_bootstrap_status";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::ptr::controller::PtrController::get_bootstrap_status();
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PtrGetCompactIndexStatus {
    const NAME: &'static str = "ptr_get_compact_index_status";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::ptr::controller::PtrController::get_compact_index_status(&state.ptr_db).await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for GetPtrSyncPerfBreakdown {
    const NAME: &'static str = "get_ptr_sync_perf_breakdown";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::ptr::sync_engine::get_ptr_sync_perf_breakdown();
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PtrGetNamespaceSummary {
    const NAME: &'static str = "ptr_get_namespace_summary";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = state.ptr_db.get_namespace_summary().await?;
        let json_result: Vec<serde_json::Value> = result
            .iter()
            .map(|(ns, count)| serde_json::json!({"namespace": ns, "count": count}))
            .collect();
        Ok(serde_json::to_value(&json_result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PtrGetTagsPaginated {
    const NAME: &'static str = "ptr_get_tags_paginated";
    type Input = PtrGetTagsPaginatedInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = state
            .ptr_db
            .get_tags_paginated(input.namespace, input.search, input.cursor, input.limit)
            .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PtrGetTagSiblings {
    const NAME: &'static str = "ptr_get_tag_siblings";
    type Input = PtrGetTagRelationInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = state.ptr_db.get_tag_siblings(input.tag_id).await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PtrGetTagParents {
    const NAME: &'static str = "ptr_get_tag_parents";
    type Input = PtrGetTagRelationInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let result = state.ptr_db.get_tag_parents(input.tag_id).await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        GetPtrStatus::NAME => Some(run_typed::<GetPtrStatus>(state, args).await),
        IsPtrSyncing::NAME => Some(run_typed::<IsPtrSyncing>(state, args).await),
        GetPtrSyncProgress::NAME => Some(run_typed::<GetPtrSyncProgress>(state, args).await),
        PtrSync::NAME => Some(run_typed::<PtrSync>(state, args).await),
        CancelPtrSync::NAME => Some(run_typed::<CancelPtrSync>(state, args).await),
        PtrCancelBootstrap::NAME => Some(run_typed::<PtrCancelBootstrap>(state, args).await),
        PtrBootstrapFromHydrusSnapshot::NAME => {
            Some(run_typed::<PtrBootstrapFromHydrusSnapshot>(state, args).await)
        }
        PtrGetBootstrapStatus::NAME => {
            Some(run_typed::<PtrGetBootstrapStatus>(state, args).await)
        }
        PtrGetCompactIndexStatus::NAME => {
            Some(run_typed::<PtrGetCompactIndexStatus>(state, args).await)
        }
        GetPtrSyncPerfBreakdown::NAME => {
            Some(run_typed::<GetPtrSyncPerfBreakdown>(state, args).await)
        }
        PtrGetNamespaceSummary::NAME => {
            Some(run_typed::<PtrGetNamespaceSummary>(state, args).await)
        }
        PtrGetTagsPaginated::NAME => Some(run_typed::<PtrGetTagsPaginated>(state, args).await),
        PtrGetTagSiblings::NAME => Some(run_typed::<PtrGetTagSiblings>(state, args).await),
        PtrGetTagParents::NAME => Some(run_typed::<PtrGetTagParents>(state, args).await),
        _ => None,
    }
}
