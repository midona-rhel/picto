//! Review queue handlers: listing, approve/reject actions, and image data.

use crate::blob_store::mime_to_extension;
use crate::state::AppState;

use super::common::{de, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "get_review_queue" => {
            let result = state
                .db
                .list_files_slim(
                    50,
                    Some(0),
                    "imported_at".to_string(),
                    "desc".to_string(),
                    None,
                    None,
                )
                .await;
            Some(result.and_then(|rows| {
                let items: Vec<serde_json::Value> = rows
                    .into_iter()
                    .map(|f| {
                        serde_json::json!({
                            "hash": f.hash,
                            "filename": f.name,
                            "width": f.width,
                            "height": f.height,
                            "file_size": f.size,
                            "mime": f.mime,
                            "source": "local",
                            "imported_at": f.imported_at,
                            "has_thumbnail": f.mime.starts_with("image/") || f.mime.starts_with("video/"),
                            "blurhash": f.blurhash,
                            "rating": f.rating,
                        })
                    })
                    .collect();
                to_json(&items)
            }))
        }
        "review_image_action" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let action_val = &args["action"];
            let action_str = action_val
                .get("action")
                .or_else(|| action_val.as_str().map(|_| action_val))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let new_status: i64 = match action_str {
                "approve" => 1,
                "reject" => 2,
                _ => return Some(Err(format!("Unknown review action: {}", action_str))),
            };
            let hash_for_impact = hash.clone();
            let result = crate::lifecycle_controller::LifecycleController::update_file_status(
                &state.db, hash, new_status,
            )
            .await;
            match result {
                Ok(()) => {
                    let folder_ids = super::files_lifecycle::collect_folder_ids_for_hashes(
                        state,
                        &[hash_for_impact.clone()],
                        1,
                    )
                    .await;
                    if let Err(err) = crate::folder_controller::FolderController::
                        refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                        .await
                    {
                        tracing::warn!(error = %err, "failed to refresh folder sidebar projection after review_image_action");
                    }
                    let scopes = super::files_lifecycle::status_mutation_grid_scopes(&folder_ids);
                    let mut impact = crate::events::MutationImpact::new()
                        .domains(&[
                            crate::events::Domain::Files,
                            crate::events::Domain::Sidebar,
                            crate::events::Domain::Folders,
                            crate::events::Domain::SmartFolders,
                            crate::events::Domain::Selection,
                        ])
                        .file_hashes(vec![hash_for_impact.clone()])
                        .metadata_hashes(vec![hash_for_impact])
                        .sidebar_tree()
                        .selection_summary()
                        .grid_scopes(scopes)
                        .sidebar_counts_from(&state.db);
                    if !folder_ids.is_empty() {
                        impact = impact.folder_ids(folder_ids);
                    }
                    crate::events::emit_state_changed("review_image_action", impact);
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "get_review_item_image" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let file = match state.db.get_file_by_hash(&hash).await {
                Ok(Some(f)) => f,
                Ok(None) => return Some(Err(format!("File not found: {}", hash))),
                Err(e) => return Some(Err(e)),
            };
            let ext = mime_to_extension(&file.mime).to_string();
            let bs = state.blob_store.clone();
            let h = hash.clone();
            let result = tokio::task::spawn_blocking(move || {
                bs.read_original(&h, Some(&ext)).map_err(|e| e.to_string())
            })
            .await
            .map_err(|e| format!("Task error: {}", e));
            Some(match result {
                Ok(inner) => inner.and_then(|r| to_json(&r)),
                Err(e) => Err(e),
            })
        }

        _ => None,
    }
}
