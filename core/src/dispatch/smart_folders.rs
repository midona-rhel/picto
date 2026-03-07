//! Smart folder domain handlers.

use crate::state::AppState;

use super::common::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "reorder_smart_folders" => {
            let moves: Vec<(i64, i64)> = match de(args, "moves") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            if let Err(e) = state.db.reorder_smart_folders(moves).await {
                return Some(Err(e));
            }
            crate::events::emit_mutation(
                "reorder_smart_folders",
                crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders),
            );
            Some(ok_null())
        }
        "create_smart_folder" => {
            let folder: crate::sqlite::smart_folders::SmartFolder = match de(args, "folder") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                match crate::smart_folder_controller::SmartFolderController::create_smart_folder(
                    &state.db, folder,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
            crate::events::emit_mutation(
                "create_smart_folder",
                crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders),
            );
            Some(to_json(&result))
        }
        "update_smart_folder" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let folder: crate::sqlite::smart_folders::SmartFolder = match de(args, "folder") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let (result, predicate_changed) =
                match crate::smart_folder_controller::SmartFolderController::update_smart_folder(
                    &state.db,
                    id.clone(),
                    folder,
                )
                .await
                {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
            let sf_id: i64 = id.parse().unwrap_or(0);
            // Only invalidate the grid when the predicate (content filter)
            // changed — metadata-only edits (name/icon/color) don't affect
            // which files match.
            let mut impact =
                crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders)
                    .smart_folder_ids(vec![sf_id]);
            if predicate_changed {
                impact = impact
                    .grid_scopes(vec![format!("smart:{}", sf_id)])
                    .selection_summary();
            }
            crate::events::emit_mutation("update_smart_folder", impact);
            Some(to_json(&result))
        }
        "delete_smart_folder" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let sf_id: i64 = id.parse().unwrap_or(0);
            if let Err(e) =
                crate::smart_folder_controller::SmartFolderController::delete_smart_folder(
                    &state.db, id,
                )
                .await
            {
                return Some(Err(e));
            }
            crate::events::emit_mutation(
                "delete_smart_folder",
                crate::events::MutationImpact::sidebar(crate::events::Domain::SmartFolders)
                    .smart_folder_ids(vec![sf_id])
                    .selection_summary(),
            );
            Some(ok_null())
        }
        "list_smart_folders" => {
            let result = crate::smart_folder_controller::SmartFolderController::list_smart_folders(
                &state.db,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "query_smart_folder" => {
            let predicate: crate::sqlite::smart_folders::SmartFolderPredicate =
                match de(args, "predicate") {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
            let limit: Option<usize> = de_opt(args, "limit");
            let offset: Option<usize> = de_opt(args, "offset");
            let result = crate::smart_folder_controller::SmartFolderController::query_smart_folder(
                &state.db, predicate, limit, offset,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "count_smart_folder" => {
            let predicate: crate::sqlite::smart_folders::SmartFolderPredicate =
                match de(args, "predicate") {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                };
            let count = crate::smart_folder_controller::SmartFolderController::count_smart_folder(
                &state.db, predicate,
            )
            .await;
            Some(count.and_then(|c| to_json(&c)))
        }
        _ => None,
    }
}
