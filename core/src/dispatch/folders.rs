//! Folder and collection domain handlers.

use crate::state::AppState;
use crate::types::*;

use super::common::{de, de_opt, ok_null, to_json};

/// Look up the cover-file hash for a collection entity (best-effort, returns None on any error).
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

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "list_folders" => {
            let result = state.db.list_folders().await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_folder_files" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.get_folder_entity_hashes(folder_id).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_folder_cover_hash" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.get_folder_cover_hash(folder_id).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_file_folders" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.get_entity_folder_memberships(&hash).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_entity_folders" => {
            let entity_id: i64 = match de(args, "entity_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state
                .db
                .get_entity_folder_memberships_by_entity_id(entity_id)
                .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        // PBI-057: Atomic move_folder — reparent + reorder in one transaction.
        "move_folder" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let new_parent_id: Option<i64> = match de(args, "new_parent_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let sibling_order: Vec<(i64, i64)> = match de(args, "sibling_order") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state
                .db
                .move_folder(folder_id, new_parent_id, sibling_order)
                .await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "move_folder",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![folder_id]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        "create_folder" => {
            let name: String = match de(args, "name") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let parent_id: Option<i64> = de_opt(args, "parent_id");
            let icon: Option<String> = de_opt(args, "icon");
            let color: Option<String> = de_opt(args, "color");
            let result = crate::folder_controller::FolderController::create_folder(
                &state.db, name, parent_id, icon, color,
            )
            .await;
            match result {
                Ok(folder) => {
                    crate::events::emit_state_changed(
                        "create_folder",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders),
                    );
                    Some(to_json(&folder))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "update_folder" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let name: Option<String> = de_opt(args, "name");
            let icon: Option<String> = de_opt(args, "icon");
            let color: Option<String> = de_opt(args, "color");
            let auto_tags: Option<Vec<String>> = de_opt(args, "auto_tags");
            let result = crate::folder_controller::FolderController::update_folder(
                &state.db, folder_id, name, icon, color, auto_tags,
            )
            .await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "update_folder",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![folder_id]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_folder" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::folder_controller::FolderController::delete_folder(&state.db, folder_id)
                    .await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "delete_folder",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![folder_id])
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "update_folder_parent" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let new_parent_id: Option<i64> = de_opt(args, "new_parent_id");
            let result = crate::folder_controller::FolderController::update_folder_parent(
                &state.db,
                folder_id,
                new_parent_id,
            )
            .await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "update_folder_parent",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![folder_id]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        "add_file_to_folder" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.add_entity_to_folder(folder_id, &hash).await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "add_file_to_folder",
                        crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Folders,
                                crate::events::Domain::Files,
                                crate::events::Domain::Selection,
                                crate::events::Domain::Sidebar,
                            ])
                            .folder_ids(vec![folder_id])
                            .sidebar_tree()
                            .grid_scopes(vec![format!("folder:{}", folder_id)])
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        // PBI-054: Batch add files to folder (with event emission).
        "add_files_to_folder_batch" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state
                .db
                .add_entities_to_folder_batch(folder_id, &hashes)
                .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        crate::events::emit_state_changed(
                            "add_files_to_folder_batch",
                            crate::events::MutationImpact::new()
                                .domains(&[
                                    crate::events::Domain::Folders,
                                    crate::events::Domain::Files,
                                    crate::events::Domain::Selection,
                                    crate::events::Domain::Sidebar,
                                ])
                                .folder_ids(vec![folder_id])
                                .sidebar_tree()
                                .grid_scopes(vec![format!("folder:{}", folder_id)])
                                .selection_summary(),
                        );
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_file_from_folder" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.remove_entity_from_folder(folder_id, &hash).await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "remove_file_from_folder",
                        crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Folders,
                                crate::events::Domain::Files,
                                crate::events::Domain::Selection,
                                crate::events::Domain::Sidebar,
                            ])
                            .folder_ids(vec![folder_id])
                            .sidebar_tree()
                            .grid_scopes(vec![format!("folder:{}", folder_id)])
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_files_from_folder_batch" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state
                .db
                .remove_entities_from_folder_batch(folder_id, &hashes)
                .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        crate::events::emit_state_changed(
                            "remove_files_from_folder_batch",
                            crate::events::MutationImpact::new()
                                .domains(&[
                                    crate::events::Domain::Folders,
                                    crate::events::Domain::Files,
                                    crate::events::Domain::Selection,
                                    crate::events::Domain::Sidebar,
                                ])
                                .folder_ids(vec![folder_id])
                                .sidebar_tree()
                                .grid_scopes(vec![format!("folder:{}", folder_id)])
                                .selection_summary(),
                        );
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }

        "reorder_folders" => {
            let moves: Vec<(i64, i64)> = match de(args, "moves") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.reorder_folders(moves).await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "reorder_folders",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "reorder_folder_items" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let moves: Vec<FolderReorderMove> = match de(args, "moves") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::folder_controller::FolderController::reorder_folder_items(
                &state.db, folder_id, moves,
            )
            .await;
            match result {
                Ok(_) => {
                    // PBI-055: Emit grid_scopes so other views observing this folder stay consistent.
                    crate::events::emit_state_changed(
                        "reorder_folder_items",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Folders])
                            .folder_ids(vec![folder_id])
                            .grid_scopes(vec![format!("folder:{}", folder_id)]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "sort_folder_items" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let sort_by: String = match de(args, "sort_by") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let direction: String = match de(args, "direction") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Option<Vec<String>> = de_opt(args, "hashes");
            let result = state
                .db
                .sort_folder_items(folder_id, sort_by, direction, hashes)
                .await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "sort_folder_items",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Folders])
                            .folder_ids(vec![folder_id])
                            .grid_scopes(vec![format!("folder:{}", folder_id)]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "reverse_folder_items" => {
            let folder_id: i64 = match de(args, "folder_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Option<Vec<String>> = de_opt(args, "hashes");
            let result = state.db.reverse_folder_items(folder_id, hashes).await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "reverse_folder_items",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Folders])
                            .folder_ids(vec![folder_id])
                            .grid_scopes(vec![format!("folder:{}", folder_id)]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        "get_collections" => {
            let result = state.db.list_collections().await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_collection_summary" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.get_collection_summary(id).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "create_collection" => {
            let name: String = match de(args, "name") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let description: Option<String> = args.get("description").and_then(|v| {
                if v.is_null() {
                    Some(String::new())
                } else {
                    serde_json::from_value::<String>(v.clone()).ok()
                }
            });
            let tags: Vec<String> = de_opt(args, "tags").unwrap_or_default();
            let result = state
                .db
                .create_collection(&name, description.as_deref(), &tags)
                .await;
            match result {
                Ok(collection_id) => {
                    crate::events::emit_state_changed(
                        "create_collection",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(to_json(&collection_id))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "update_collection" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let name: Option<String> = de_opt(args, "name");
            let description: Option<String> = args.get("description").and_then(|v| {
                if v.is_null() {
                    Some(String::new())
                } else {
                    serde_json::from_value::<String>(v.clone()).ok()
                }
            });
            let tags: Option<Vec<String>> = de_opt(args, "tags");
            let source_urls: Option<Vec<String>> = de_opt(args, "source_urls").or_else(|| {
                args.get("sourceUrls")
                    .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            });
            let result = state
                .db
                .update_collection(
                    id,
                    name.as_deref(),
                    description.as_deref(),
                    tags.as_deref(),
                    source_urls.as_deref(),
                )
                .await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "update_collection",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![id]),
                    );
                    crate::events::emit_state_changed(
                        "update_collection_grid",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Files])
                            .grid_scopes(vec![format!("collection:{}", id)])
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "set_collection_rating" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let rating: Option<i64> = de_opt(args, "rating");
            let result = state.db.set_collection_rating(id, rating).await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "set_collection_rating",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![id])
                            .grid_scopes(vec![format!("collection:{}", id)])
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "set_collection_source_urls" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let source_urls: Vec<String> = match de(args, "source_urls") {
                Ok(v) => v,
                Err(_) => match de(args, "sourceUrls") {
                    Ok(v) => v,
                    Err(e) => return Some(Err(e)),
                },
            };
            let result = state.db.set_collection_source_urls(id, &source_urls).await;
            match result {
                Ok(_) => {
                    crate::events::emit_state_changed(
                        "set_collection_source_urls",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![id])
                            .grid_scopes(vec![format!("collection:{}", id)])
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "reorder_collection_members" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state
                .db
                .reorder_collection_members_by_hashes(id, &hashes)
                .await;
            match result {
                Ok(_) => {
                    state.db.scope_cache_invalidate_scope("collection");
                    crate::events::emit_state_changed(
                        "reorder_collection_members",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Files])
                            .grid_scopes(vec![format!("collection:{}", id)])
                            .grid_all(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "add_collection_members" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.add_collection_members_by_hashes(id, &hashes).await;
            match result {
                Ok(added) => {
                    state.db.scope_cache_invalidate_scope("collection");
                    let cover_hash = collection_cover_hash(&state.db, id).await;
                    let mut impact = crate::events::MutationImpact::new()
                        .domains(&[
                            crate::events::Domain::Files,
                            crate::events::Domain::Folders,
                            crate::events::Domain::Tags,
                            crate::events::Domain::Sidebar,
                            crate::events::Domain::SmartFolders,
                        ])
                        .sidebar_tree()
                        .grid_scopes(vec![format!("collection:{}", id), "folder:all".into()])
                        .selection_summary()
                        .sidebar_counts_from(&state.db);
                    if let Some(h) = cover_hash {
                        impact = impact.metadata_hashes(vec![h]);
                    }
                    crate::events::emit_state_changed("add_collection_members", impact);
                    Some(to_json(&added))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "remove_collection_members" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state
                .db
                .remove_collection_members_by_hashes(id, &hashes)
                .await;
            match result {
                Ok(removed) => {
                    state.db.scope_cache_invalidate_scope("collection");
                    let cover_hash = collection_cover_hash(&state.db, id).await;
                    let mut impact = crate::events::MutationImpact::new()
                        .domains(&[
                            crate::events::Domain::Files,
                            crate::events::Domain::Folders,
                            crate::events::Domain::Tags,
                            crate::events::Domain::Sidebar,
                            crate::events::Domain::SmartFolders,
                        ])
                        .sidebar_tree()
                        .grid_scopes(vec![format!("collection:{}", id), "folder:all".into()])
                        .selection_summary()
                        .sidebar_counts_from(&state.db);
                    if let Some(h) = cover_hash {
                        impact = impact.metadata_hashes(vec![h]);
                    }
                    crate::events::emit_state_changed("remove_collection_members", impact);
                    Some(to_json(&removed))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_collection" => {
            let id: i64 = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = state.db.delete_collection(id).await;
            match result {
                Ok(_) => {
                    state.db.scope_cache_invalidate_scope("collection");
                    crate::events::emit_state_changed(
                        "delete_collection",
                        crate::events::MutationImpact::sidebar(crate::events::Domain::Folders)
                            .folder_ids(vec![id])
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "scan_for_collections" => {
            tracing::debug!("Unimplemented command: scan_for_collections");
            Some(Err("Unimplemented: 'scan_for_collections' — collection auto-detection is not yet implemented".into()))
        }

        _ => None,
    }
}
