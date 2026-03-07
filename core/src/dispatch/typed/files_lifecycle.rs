//! Typed command implementations for file lifecycle operations.
//!
//! Each command has a typed Input struct (with ts-rs export) and a TypedCommand
//! impl that contains the same business logic previously in the legacy
//! `files_lifecycle::handle()` match arms.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
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

// ─── Command structs ───────────────────────────────────────────────────────

pub struct ImportFiles;
pub struct UpdateFileStatus;
pub struct DeleteFile;
pub struct DeleteFiles;
pub struct RebuildFileFts;
pub struct WipeImageData;

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
            crate::settings::similarity_pct_to_distance(
                app_settings.duplicate_auto_merge_similarity_pct,
            )
        } else {
            0
        };
        let result = crate::import_controller::ImportController::import_files(
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
        crate::lifecycle_controller::LifecycleController::update_file_status(
            &state.db,
            input.hash.clone(),
            file_status,
        )
        .await?;

        let folder_ids =
            super::super::files_lifecycle::collect_folder_ids_for_hashes(state, &[input.hash.clone()], 1).await;
        if let Err(err) = crate::folder_controller::FolderController::
            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids)
            .await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status update");
        }
        let scopes = super::super::files_lifecycle::status_mutation_grid_scopes(&folder_ids);
        let mut impact = crate::events::MutationImpact::new()
            .domains(&[
                crate::events::Domain::Files,
                crate::events::Domain::Sidebar,
                crate::events::Domain::Folders,
                crate::events::Domain::SmartFolders,
                crate::events::Domain::Selection,
            ])
            .file_hashes(vec![input.hash.clone()])
            .sidebar_tree()
            .selection_summary()
            .metadata_hashes(vec![input.hash])
            .grid_scopes(scopes)
            .sidebar_counts_from(&state.db);
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
            super::super::files_lifecycle::collect_folder_ids_for_hashes(state, &[input.hash.clone()], 1).await;
        crate::lifecycle_controller::LifecycleController::delete_file(
            &state.db,
            &state.blob_store,
            input.hash.clone(),
        )
        .await?;

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
            .file_hashes(vec![input.hash])
            .sidebar_tree()
            .selection_summary()
            .grid_all()
            .sidebar_counts_from(&state.db);
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
        let folder_ids = super::super::files_lifecycle::collect_folder_ids_for_hashes(
            state,
            &hashes_for_impact,
            hashes_for_impact.len(),
        )
        .await;
        let count = crate::lifecycle_controller::LifecycleController::delete_files(
            &state.db,
            &state.blob_store,
            input.hashes,
        )
        .await?;

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
                .file_hashes(hashes_for_impact)
                .sidebar_tree()
                .selection_summary()
                .grid_all()
                .sidebar_counts_from(&state.db);
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
        crate::lifecycle_controller::LifecycleController::wipe_all_files(
            &state.db,
            &state.blob_store,
        )
        .await?;

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
        Ok(())
    }
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
        _ => None,
    }
}
