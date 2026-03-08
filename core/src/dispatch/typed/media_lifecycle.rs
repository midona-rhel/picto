//! Handler functions for media lifecycle operations:
//! import, status changes, deletion, and FTS rebuild.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use crate::types::*;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
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
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateFileStatusInput {
    pub hash: String,
    pub status: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteFilesInput {
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteFilesSelectionInput {
    pub selection: SelectionQuerySpec,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateFileStatusSelectionInput {
    pub selection: SelectionQuerySpec,
    pub status: String,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn import_files(state: &AppState, input: ImportFilesInput) -> Result<crate::types::ImportBatchResult, String> {
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
            crate::events::MutationImpact::file_lifecycle(&state.db),
        );
    }
    Ok(result)
}

pub async fn update_file_status(state: &AppState, input: UpdateFileStatusInput) -> Result<(), String> {
    let file_status = crate::types::parse_file_status(&input.status)?;
    crate::lifecycle::controller::LifecycleController::update_file_status(
        &state.db, input.hash.clone(), file_status,
    ).await?;

    let folder_ids =
        collect_folder_ids_for_hashes(state, &[input.hash.clone()], 1).await;
    if let Err(err) = crate::folders::controller::FolderController::
        refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids).await
    {
        tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status update");
    }
    let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
        .file_hashes(vec![input.hash]);
    if !folder_ids.is_empty() {
        impact = impact.folder_ids(folder_ids);
    }
    crate::events::emit_mutation("update_file_status", impact);
    Ok(())
}

pub async fn delete_files(state: &AppState, input: DeleteFilesInput) -> Result<usize, String> {
    let hashes_for_impact = input.hashes.clone();
    let folder_ids = collect_folder_ids_for_hashes(
        state, &hashes_for_impact, hashes_for_impact.len(),
    ).await;
    let count = crate::lifecycle::controller::LifecycleController::delete_files(
        &state.db, &state.blob_store, input.hashes,
    ).await?;

    if count > 0 {
        if let Err(err) = crate::folders::controller::FolderController::
            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids).await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_files");
        }
        let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
            .file_hashes(hashes_for_impact);
        if !folder_ids.is_empty() {
            impact = impact.folder_ids(folder_ids);
        }
        crate::events::emit_mutation("delete_files", impact);
    }
    Ok(count)
}

pub async fn rebuild_file_fts(state: &AppState, _input: serde_json::Value) -> Result<(), String> {
    state.db
        .with_conn(|conn| crate::sqlite::files::rebuild_file_fts(conn))
        .await?;
    Ok(())
}

pub async fn wipe_image_data(state: &AppState, _input: serde_json::Value) -> Result<(), String> {
    crate::lifecycle::controller::LifecycleController::wipe_all_files(
        &state.db, &state.blob_store,
    ).await?;
    crate::events::emit_mutation(
        "wipe_image_data",
        crate::events::MutationImpact::file_status_change(&state.db),
    );
    Ok(())
}

pub async fn delete_files_selection(state: &AppState, input: DeleteFilesSelectionInput) -> Result<usize, String> {
    let bitmap = resolve_selection_bitmap(state, &input.selection).await?;

    let file_ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
    let pairs = state.db.resolve_ids_batch(&file_ids).await?;
    let hashes: Vec<String> = pairs.into_iter().map(|(_, h)| h).collect();
    let hashes_clone = hashes.clone();
    let folder_ids =
        collect_folder_ids_for_hashes(state, &hashes_clone, hashes_clone.len()).await;

    let count = crate::lifecycle::controller::LifecycleController::delete_files(
        &state.db, &state.blob_store, hashes,
    ).await?;

    if count > 0 {
        if let Err(err) = crate::folders::controller::FolderController::
            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids).await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_files_selection");
        }
        let mut impact = crate::events::MutationImpact::file_status_change(&state.db)
            .file_hashes(hashes_clone);
        if !folder_ids.is_empty() {
            impact = impact.folder_ids(folder_ids);
        }
        crate::events::emit_mutation("delete_files_selection", impact);
    }
    Ok(count)
}

pub async fn update_file_status_selection(state: &AppState, input: UpdateFileStatusSelectionInput) -> Result<usize, String> {
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
        state.db.update_file_status_batch(&bitmap, status_code).await?;
        if let Err(err) = crate::folders::controller::FolderController::
            refresh_sidebar_projection_for_folder_ids(&state.db, &folder_ids).await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status batch update");
        }
        let mut impact = crate::events::MutationImpact::file_status_change(&state.db);
        if !folder_ids.is_empty() {
            impact = impact.folder_ids(folder_ids);
        }
        crate::events::emit_mutation("update_file_status_selection", impact);
    }
    Ok(count)
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
