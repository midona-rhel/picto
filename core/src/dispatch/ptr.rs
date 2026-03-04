//! PTR (Public Tag Repository) domain handlers.

use crate::state::AppState;

use super::common::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "get_ptr_status" => {
            let result = crate::ptr_controller::PtrController::get_ptr_status(&state.ptr_db).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "is_ptr_syncing" => {
            let result = crate::ptr_controller::PtrController::is_ptr_syncing();
            Some(to_json(&result))
        }
        "get_ptr_sync_progress" => {
            let result = crate::ptr_controller::PtrController::get_sync_progress();
            Some(to_json(&result))
        }
        "ptr_sync" => {
            let result = crate::ptr_controller::PtrController::sync(
                &state.ptr_db,
                &state.settings,
                state.db.compiler_tx.clone(),
            )
            .await;
            Some(result.and_then(|()| {
                to_json(&serde_json::json!({
                    "id": "ptr-sync",
                    "message": "PTR sync started in background",
                }))
            }))
        }
        "cancel_ptr_sync" => {
            let result = crate::ptr_controller::PtrController::cancel_sync(&state.ptr_db);
            Some(result.and_then(|()| ok_null()))
        }
        "ptr_cancel_bootstrap" => {
            let result = crate::ptr_controller::PtrController::cancel_bootstrap(&state.ptr_db);
            Some(result.and_then(|()| ok_null()))
        }
        "ptr_bootstrap_from_hydrus_snapshot" => {
            let req: crate::ptr_controller::PtrBootstrapRequest =
                match serde_json::from_value(args.clone())
                    .map_err(|e| format!("Invalid ptr bootstrap input: {e}"))
                {
                    Ok(r) => r,
                    Err(e) => return Some(Err(e)),
                };
            let result = crate::ptr_controller::PtrController::bootstrap_from_hydrus_snapshot(
                &state.ptr_db,
                req,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "ptr_get_bootstrap_status" => {
            let result = crate::ptr_controller::PtrController::get_bootstrap_status();
            Some(to_json(&result))
        }
        "ptr_get_compact_index_status" => {
            let result =
                crate::ptr_controller::PtrController::get_compact_index_status(&state.ptr_db).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_ptr_sync_perf_breakdown" => {
            let result = crate::ptr_sync::get_ptr_sync_perf_breakdown();
            Some(to_json(&result))
        }
        "ptr_get_namespace_summary" => {
            let result = state.ptr_db.get_namespace_summary().await;
            Some(result.and_then(|r| {
                let json_result: Vec<serde_json::Value> = r
                    .iter()
                    .map(|(ns, count)| serde_json::json!({"namespace": ns, "count": count}))
                    .collect();
                to_json(&json_result)
            }))
        }
        "ptr_get_tags_paginated" => {
            let namespace: Option<String> = de_opt(args, "namespace");
            let search: Option<String> = de_opt(args, "search");
            let cursor: Option<String> = de_opt(args, "cursor");
            let limit: i64 = de_opt(args, "limit").unwrap_or(500);
            let result = state
                .ptr_db
                .get_tags_paginated(namespace, search, cursor, limit)
                .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "ptr_get_tag_siblings" => {
            let tag_id: Result<i64, String> = de(args, "tag_id");
            let tag_id = match tag_id {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.ptr_db.get_tag_siblings(tag_id).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "ptr_get_tag_parents" => {
            let tag_id: Result<i64, String> = de(args, "tag_id");
            let tag_id = match tag_id {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.ptr_db.get_tag_parents(tag_id).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        _ => None,
    }
}
