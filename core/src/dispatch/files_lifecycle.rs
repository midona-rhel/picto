//! File lifecycle handlers: import, status changes, deletion, wipe.

use crate::state::AppState;
use crate::types::*;

use super::common::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "import_files" => {
            let paths: Vec<String> = match de(args, "paths") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let tag_strings: Option<Vec<String>> = de_opt(args, "tag_strings");
            let source_urls: Option<Vec<String>> = de_opt(args, "source_urls");
            let initial_status: i64 = de_opt(args, "initial_status").unwrap_or(1);
            let app_settings = state.settings.get();
            let auto_merge_enabled = app_settings.duplicate_auto_merge_enabled
                && !app_settings.duplicate_auto_merge_subscriptions_only;
            let auto_merge_distance = if auto_merge_enabled {
                crate::settings::similarity_pct_to_distance(
                    app_settings.duplicate_auto_merge_similarity_pct,
                )
            } else {
                0
            };
            let result = crate::import_controller::ImportController::import_files(
                &state.db,
                &state.blob_store,
                paths,
                tag_strings,
                source_urls,
                auto_merge_enabled,
                auto_merge_distance,
                initial_status,
            )
            .await;
            match result {
                Ok(ref r) => {
                    if !r.imported.is_empty() {
                        crate::events::emit_mutation(
                            "import_files",
                            crate::events::MutationImpact::file_lifecycle(&state.db)
                                .grid_scopes(vec!["system:all".into(), "system:inbox".into()]),
                        );
                    }
                    Some(to_json(r))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "rebuild_file_fts" => {
            let result = state
                .db
                .with_conn(|conn| crate::sqlite::files::rebuild_file_fts(conn))
                .await;
            Some(result.and_then(|_| ok_null()))
        }

        "update_file_status" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let status: String = match de(args, "status") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let file_status = match parse_file_status(&status) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::lifecycle_controller::LifecycleController::update_file_status(
                &state.db,
                hash.clone(),
                file_status,
            )
            .await;
            match result {
                Ok(()) => {
                    let folder_ids = collect_folder_ids_for_hashes(state, &[hash.clone()], 1).await;
                    if let Err(err) = crate::folder_controller::FolderController::
                        refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                        .await
                    {
                        tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status update");
                    }
                    let scopes = status_mutation_grid_scopes(&folder_ids);
                    let mut impact = crate::events::MutationImpact::new()
                        .domains(&[
                            crate::events::Domain::Files,
                            crate::events::Domain::Sidebar,
                            crate::events::Domain::Folders,
                            crate::events::Domain::SmartFolders,
                            crate::events::Domain::Selection,
                        ])
                        .file_hashes(vec![hash.clone()])
                        .sidebar_tree()
                        .selection_summary()
                        .metadata_hashes(vec![hash])
                        .grid_scopes(scopes)
                        .sidebar_counts_from(&state.db);
                    if !folder_ids.is_empty() {
                        impact = impact.folder_ids(folder_ids);
                    }
                    crate::events::emit_mutation("update_file_status", impact);
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_file" => {
            let hash: String = match de(args, "hash") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let folder_ids = collect_folder_ids_for_hashes(state, &[hash.clone()], 1).await;
            let result = crate::lifecycle_controller::LifecycleController::delete_file(
                &state.db,
                &state.blob_store,
                hash.clone(),
            )
            .await;
            match result {
                Ok(()) => {
                    if let Err(err) = crate::folder_controller::FolderController::
                        refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                        .await
                    {
                        tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_file");
                    }
                    let mut impact = crate::events::MutationImpact::new()
                        .domains(&[
                            crate::events::Domain::Files,
                            crate::events::Domain::Sidebar,
                            crate::events::Domain::Folders,
                            crate::events::Domain::SmartFolders,
                            crate::events::Domain::Selection,
                        ])
                        .file_hashes(vec![hash])
                        .sidebar_tree()
                        .selection_summary()
                        .grid_all()
                        .sidebar_counts_from(&state.db);
                    if !folder_ids.is_empty() {
                        impact = impact.folder_ids(folder_ids);
                    }
                    crate::events::emit_mutation("delete_file", impact);
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_files" => {
            let hashes: Vec<String> = match de(args, "hashes") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let hashes_clone = hashes.clone();
            let folder_ids =
                collect_folder_ids_for_hashes(state, &hashes_clone, hashes_clone.len()).await;
            let result = crate::lifecycle_controller::LifecycleController::delete_files(
                &state.db,
                &state.blob_store,
                hashes,
            )
            .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        if let Err(err) = crate::folder_controller::FolderController::
                            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                            .await
                        {
                            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_files");
                        }
                        let mut impact = crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Files,
                                crate::events::Domain::Sidebar,
                                crate::events::Domain::Folders,
                                crate::events::Domain::SmartFolders,
                                crate::events::Domain::Selection,
                            ])
                            .file_hashes(hashes_clone)
                            .sidebar_tree()
                            .selection_summary()
                            .grid_all()
                            .sidebar_counts_from(&state.db);
                        if !folder_ids.is_empty() {
                            impact = impact.folder_ids(folder_ids);
                        }
                        crate::events::emit_mutation("delete_files", impact);
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_files_selection" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };

            let bitmap = match resolve_selection_bitmap(state, &selection).await {
                Ok(bm) => bm,
                Err(e) => return Some(Err(e)),
            };

            let file_ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
            let pairs = match state.db.resolve_ids_batch(&file_ids).await {
                Ok(p) => p,
                Err(e) => return Some(Err(e)),
            };
            let hashes: Vec<String> = pairs.into_iter().map(|(_, h)| h).collect();
            let hashes_clone = hashes.clone();
            let folder_ids =
                collect_folder_ids_for_hashes(state, &hashes_clone, hashes_clone.len()).await;

            let result = crate::lifecycle_controller::LifecycleController::delete_files(
                &state.db,
                &state.blob_store,
                hashes,
            )
            .await;
            match result {
                Ok(count) => {
                    if count > 0 {
                        if let Err(err) = crate::folder_controller::FolderController::
                            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                            .await
                        {
                            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_files_selection");
                        }
                        let mut impact = crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Files,
                                crate::events::Domain::Sidebar,
                                crate::events::Domain::Folders,
                                crate::events::Domain::SmartFolders,
                                crate::events::Domain::Selection,
                            ])
                            .file_hashes(hashes_clone)
                            .sidebar_tree()
                            .selection_summary()
                            .grid_all()
                            .sidebar_counts_from(&state.db);
                        if !folder_ids.is_empty() {
                            impact = impact.folder_ids(folder_ids);
                        }
                        crate::events::emit_mutation("delete_files_selection", impact);
                    }
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "update_file_status_selection" => {
            let selection: SelectionQuerySpec = match de(args, "selection") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let status_str: String = match de(args, "status") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let status_code = match crate::types::parse_file_status(&status_str) {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };

            let bitmap = match resolve_selection_bitmap(state, &selection).await {
                Ok(bm) => bm,
                Err(e) => return Some(Err(e)),
            };
            let count = bitmap.len() as usize;

            if count > 0 {
                let mut folder_ids = selection.folder_ids.clone().unwrap_or_default();
                if matches!(selection.mode, SelectionMode::ExplicitHashes) {
                    let explicit_hashes = selection.hashes.clone().unwrap_or_default();
                    let mut from_hashes =
                        collect_folder_ids_for_hashes(state, &explicit_hashes, 200).await;
                    folder_ids.append(&mut from_hashes);
                    folder_ids.sort_unstable();
                    folder_ids.dedup();
                }
                let scopes = status_mutation_grid_scopes(&folder_ids);
                if let Err(e) = state
                    .db
                    .update_file_status_batch(&bitmap, status_code)
                    .await
                {
                    return Some(Err(e));
                }
                if let Err(err) = crate::folder_controller::FolderController::
                    refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                    .await
                {
                    tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status batch update");
                }
                let mut impact = crate::events::MutationImpact::new()
                    .domains(&[
                        crate::events::Domain::Files,
                        crate::events::Domain::Sidebar,
                        crate::events::Domain::Folders,
                        crate::events::Domain::SmartFolders,
                        crate::events::Domain::Selection,
                    ])
                    .sidebar_tree()
                    .selection_summary()
                    .grid_scopes(scopes)
                    .sidebar_counts_from(&state.db);
                if !folder_ids.is_empty() {
                    impact = impact.folder_ids(folder_ids);
                }
                crate::events::emit_mutation("update_file_status_selection", impact);
            }
            Some(to_json(&count))
        }
        "wipe_image_data" => {
            let result = crate::lifecycle_controller::LifecycleController::wipe_all_files(
                &state.db,
                &state.blob_store,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "wipe_image_data",
                        crate::events::MutationImpact::new()
                            .domains(&[
                                crate::events::Domain::Files,
                                crate::events::Domain::Sidebar,
                                crate::events::Domain::Folders,
                                crate::events::Domain::SmartFolders,
                                crate::events::Domain::Selection,
                            ])
                            .sidebar_tree()
                            .grid_all()
                            .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        _ => None,
    }
}

pub(super) async fn resolve_selection_bitmap(
    state: &AppState,
    selection: &SelectionQuerySpec,
) -> Result<roaring::RoaringBitmap, String> {
    match &selection.mode {
        SelectionMode::ExplicitHashes => {
            let hashes = selection.hashes.clone().unwrap_or_default();
            let pairs = state.db.resolve_hashes_batch(&hashes).await?;
            let mut bm = roaring::RoaringBitmap::new();
            for (_, fid) in pairs {
                bm.insert(fid as u32);
            }
            Ok(bm)
        }
        SelectionMode::AllResults => {
            let (_, filtered) =
                crate::selection_helpers::selection_bitmap_for_all_results(&state.db, selection)
                    .await?;
            Ok(filtered)
        }
    }
}

pub(super) async fn collect_folder_ids_for_hashes(
    state: &AppState,
    hashes: &[String],
    max_hashes: usize,
) -> Vec<i64> {
    let limited_hashes: Vec<String> = hashes.iter().take(max_hashes).cloned().collect();
    if limited_hashes.is_empty() {
        return Vec::new();
    }
    let resolved = match state.db.resolve_hashes_batch(&limited_hashes).await {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let entity_ids: Vec<i64> = resolved.into_iter().map(|(_, entity_id)| entity_id).collect();
    if entity_ids.is_empty() {
        return Vec::new();
    }

    let query_ids = entity_ids.clone();
    let mut folder_ids: Vec<i64> = match state
        .db
        .with_read_conn(move |conn| {
            let mut all = Vec::<i64>::new();
            for chunk in query_ids.chunks(900) {
                let placeholders = (0..chunk.len())
                    .map(|i| format!("?{}", i + 1))
                    .collect::<Vec<_>>()
                    .join(",");
                let sql = format!(
                    "SELECT DISTINCT folder_id FROM folder_entity WHERE entity_id IN ({placeholders})"
                );
                let mut stmt = conn.prepare_cached(&sql)?;
                let rows = stmt.query_map(rusqlite::params_from_iter(chunk.iter()), |row| {
                    row.get::<_, i64>(0)
                })?;
                for folder_id in rows.flatten() {
                    all.push(folder_id);
                }
            }
            Ok(all)
        })
        .await
    {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    folder_ids.sort_unstable();
    folder_ids.dedup();
    folder_ids
}

pub(super) fn status_mutation_grid_scopes(folder_ids: &[i64]) -> Vec<String> {
    let mut scopes = vec![
        "system:all".to_string(),
        "system:inbox".to_string(),
        "system:trash".to_string(),
        "system:recently_viewed".to_string(),
        // Conservative smart-folder fallback when precise smart IDs are unavailable.
        "smart:all".to_string(),
    ];
    for folder_id in folder_ids {
        scopes.push(format!("folder:{folder_id}"));
    }
    scopes.sort();
    scopes.dedup();
    scopes
}

#[cfg(test)]
mod tests {
    use super::status_mutation_grid_scopes;

    #[test]
    fn status_scopes_include_system_and_smart_fallback() {
        let scopes = status_mutation_grid_scopes(&[]);
        assert!(scopes.contains(&"system:all".to_string()));
        assert!(scopes.contains(&"system:inbox".to_string()));
        assert!(scopes.contains(&"system:trash".to_string()));
        assert!(scopes.contains(&"smart:all".to_string()));
    }

    #[test]
    fn status_scopes_include_folder_targets_without_duplicates() {
        let scopes = status_mutation_grid_scopes(&[4, 4, 9]);
        let folder_scopes = scopes
            .iter()
            .filter(|s| s.starts_with("folder:"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            folder_scopes,
            vec!["folder:4".to_string(), "folder:9".to_string()]
        );
    }
}
