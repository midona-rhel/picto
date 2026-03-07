//! Typed command implementations for media lifecycle operations:
//! import, status changes, deletion, and FTS rebuild.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use crate::types::*;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ImportFilesInput {
    pub paths: Vec<String>,
    pub tag_strings: Option<Vec<String>>,
    pub source_urls: Option<Vec<String>>,
    #[serde(default = "default_initial_status")]
    #[ts(type = "number")]
    pub initial_status: i64,
}

fn default_initial_status() -> i64 {
    1
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateFileStatusInput {
    pub hash: String,
    pub status: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteFileInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteFilesInput {
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteFilesSelectionInput {
    pub selection: SelectionQuerySpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct UpdateFileStatusSelectionInput {
    pub selection: SelectionQuerySpec,
    pub status: String,
}

// ─── Command structs ───────────────────────────────────────────────────────

pub struct ImportFiles;
pub struct UpdateFileStatus;
pub struct DeleteFile;
pub struct DeleteFiles;
pub struct RebuildFileFts;
pub struct WipeImageData;
pub struct DeleteFilesSelection;
pub struct UpdateFileStatusSelection;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for ImportFiles {
    const NAME: &'static str = "import_files";
    type Input = ImportFilesInput;
    type Output = crate::types::ImportBatchResult;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let app_settings = state.settings.get();
        let auto_merge_enabled = app_settings.duplicate_auto_merge_enabled
            && !app_settings.duplicate_auto_merge_subscriptions_only;
        let auto_merge_distance = if auto_merge_enabled {
            crate::settings::store::similarity_pct_to_distance(
                app_settings.duplicate_auto_merge_similarity_pct,
            )
        } else {
            0
        };
        let result = crate::import::controller::ImportController::import_files(
            &state.db,
            &state.blob_store,
            input.paths,
            input.tag_strings,
            input.source_urls,
            auto_merge_enabled,
            auto_merge_distance,
            input.initial_status,
        )
        .await?;

        if !result.imported.is_empty() {
            crate::events::emit_mutation(
                "import_files",
                crate::events::MutationImpact::file_lifecycle(&state.db)
                    .grid_scopes(vec!["system:all".into(), "system:inbox".into()]),
            );
        }
        Ok(result)
    }
}

impl TypedCommand for UpdateFileStatus {
    const NAME: &'static str = "update_file_status";
    type Input = UpdateFileStatusInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let file_status = crate::types::parse_file_status(&input.status)?;
        crate::lifecycle::controller::LifecycleController::update_file_status(
            &state.db,
            input.hash.clone(),
            file_status,
        )
        .await?;

        let folder_ids =
            collect_folder_ids_for_hashes(state, &[input.hash.clone()], 1).await;
        if let Err(err) = crate::folders::controller::FolderController::
            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
            .await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status update");
        }
        let scopes = status_mutation_grid_scopes(&folder_ids);
        let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
            .file_hashes(vec![input.hash.clone()])
            .metadata_hashes(vec![input.hash])
            .grid_scopes(scopes);
        if !folder_ids.is_empty() {
            impact = impact.folder_ids(folder_ids);
        }
        crate::events::emit_mutation("update_file_status", impact);
        Ok(())
    }
}

impl TypedCommand for DeleteFile {
    const NAME: &'static str = "delete_file";
    type Input = DeleteFileInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let folder_ids =
            collect_folder_ids_for_hashes(state, &[input.hash.clone()], 1).await;
        crate::lifecycle::controller::LifecycleController::delete_file(
            &state.db,
            &state.blob_store,
            input.hash.clone(),
        )
        .await?;

        if let Err(err) = crate::folders::controller::FolderController::
            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
            .await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_file");
        }
        let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
            .file_hashes(vec![input.hash])
            .grid_all();
        if !folder_ids.is_empty() {
            impact = impact.folder_ids(folder_ids);
        }
        crate::events::emit_mutation("delete_file", impact);
        Ok(())
    }
}

impl TypedCommand for DeleteFiles {
    const NAME: &'static str = "delete_files";
    type Input = DeleteFilesInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let hashes_for_impact = input.hashes.clone();
        let folder_ids = collect_folder_ids_for_hashes(
            state,
            &hashes_for_impact,
            hashes_for_impact.len(),
        )
        .await;
        let count = crate::lifecycle::controller::LifecycleController::delete_files(
            &state.db,
            &state.blob_store,
            input.hashes,
        )
        .await?;

        if count > 0 {
            if let Err(err) = crate::folders::controller::FolderController::
                refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                .await
            {
                tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_files");
            }
            let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
                .file_hashes(hashes_for_impact)
                .grid_all();
            if !folder_ids.is_empty() {
                impact = impact.folder_ids(folder_ids);
            }
            crate::events::emit_mutation("delete_files", impact);
        }
        Ok(count)
    }
}

impl TypedCommand for RebuildFileFts {
    const NAME: &'static str = "rebuild_file_fts";
    type Input = serde_json::Value; // accepts any args (none expected)
    type Output = ();

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        state
            .db
            .with_conn(|conn| crate::sqlite::files::rebuild_file_fts(conn))
            .await?;
        Ok(())
    }
}

impl TypedCommand for WipeImageData {
    const NAME: &'static str = "wipe_image_data";
    type Input = serde_json::Value; // accepts any args (none expected)
    type Output = ();

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        crate::lifecycle::controller::LifecycleController::wipe_all_files(
            &state.db,
            &state.blob_store,
        )
        .await?;

        crate::events::emit_mutation(
            "wipe_image_data",
            crate::events::MutationImpact::file_status_change(&state.db)
                .grid_all(),
        );
        Ok(())
    }
}

impl TypedCommand for DeleteFilesSelection {
    const NAME: &'static str = "delete_files_selection";
    type Input = DeleteFilesSelectionInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let bitmap = resolve_selection_bitmap(state, &input.selection).await?;

        let file_ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
        let pairs = state.db.resolve_ids_batch(&file_ids).await?;
        let hashes: Vec<String> = pairs.into_iter().map(|(_, h)| h).collect();
        let hashes_clone = hashes.clone();
        let folder_ids =
            collect_folder_ids_for_hashes(state, &hashes_clone, hashes_clone.len()).await;

        let count = crate::lifecycle::controller::LifecycleController::delete_files(
            &state.db,
            &state.blob_store,
            hashes,
        )
        .await?;

        if count > 0 {
            if let Err(err) = crate::folders::controller::FolderController::
                refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                .await
            {
                tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_files_selection");
            }
            let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
                .file_hashes(hashes_clone)
                .grid_all();
            if !folder_ids.is_empty() {
                impact = impact.folder_ids(folder_ids);
            }
            crate::events::emit_mutation("delete_files_selection", impact);
        }
        Ok(count)
    }
}

impl TypedCommand for UpdateFileStatusSelection {
    const NAME: &'static str = "update_file_status_selection";
    type Input = UpdateFileStatusSelectionInput;
    type Output = usize;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let status_code = crate::types::parse_file_status(&input.status)?;

        let bitmap = resolve_selection_bitmap(state, &input.selection).await?;
        let count = bitmap.len() as usize;

        if count > 0 {
            let mut folder_ids = input.selection.folder_ids.clone().unwrap_or_default();
            if matches!(input.selection.mode, SelectionMode::ExplicitHashes) {
                let explicit_hashes = input.selection.hashes.clone().unwrap_or_default();
                let mut from_hashes =
                    collect_folder_ids_for_hashes(state, &explicit_hashes, 200).await;
                folder_ids.append(&mut from_hashes);
                folder_ids.sort_unstable();
                folder_ids.dedup();
            }
            let scopes = status_mutation_grid_scopes(&folder_ids);
            state
                .db
                .update_file_status_batch(&bitmap, status_code)
                .await?;
            if let Err(err) = crate::folders::controller::FolderController::
                refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
                .await
            {
                tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status batch update");
            }
            let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
                .grid_scopes(scopes);
            if !folder_ids.is_empty() {
                impact = impact.folder_ids(folder_ids);
            }
            crate::events::emit_mutation("update_file_status_selection", impact);
        }
        Ok(count)
    }
}

// ─── Selection helpers ─────────────────────────────────────────────────────

pub(crate) async fn resolve_selection_bitmap(
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
                crate::selection::helpers::selection_bitmap_for_all_results(&state.db, selection)
                    .await?;
            Ok(filtered)
        }
    }
}

pub(crate) async fn collect_folder_ids_for_hashes(
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

pub(crate) fn status_mutation_grid_scopes(folder_ids: &[i64]) -> Vec<String> {
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

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        ImportFiles::NAME => Some(run_typed::<ImportFiles>(state, args).await),
        UpdateFileStatus::NAME => Some(run_typed::<UpdateFileStatus>(state, args).await),
        DeleteFile::NAME => Some(run_typed::<DeleteFile>(state, args).await),
        DeleteFiles::NAME => Some(run_typed::<DeleteFiles>(state, args).await),
        RebuildFileFts::NAME => Some(run_typed::<RebuildFileFts>(state, args).await),
        WipeImageData::NAME => Some(run_typed::<WipeImageData>(state, args).await),
        DeleteFilesSelection::NAME => Some(run_typed::<DeleteFilesSelection>(state, args).await),
        UpdateFileStatusSelection::NAME => {
            Some(run_typed::<UpdateFileStatusSelection>(state, args).await)
        }
        _ => None,
    }
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
