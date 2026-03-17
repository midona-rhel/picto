//! Handler functions for media lifecycle operations:
//! import, status changes, deletion, and FTS rebuild.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use crate::types::*;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ImportFilesInput {
    pub paths: Vec<String>,
    pub tag_strings: Option<Vec<String>>,
    pub source_urls: Option<Vec<String>>,
    #[serde(default = "default_initial_status")]
    #[ts(type = "number")]
    pub initial_status: i64,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ImportFolderInput {
    pub path: String,
    #[serde(default)]
    pub preserve_structure: bool,
    #[serde(default)]
    pub parent_folder_id: Option<i64>,
    #[serde(default = "default_initial_status")]
    #[ts(type = "number")]
    pub initial_status: i64,
}

fn default_initial_status() -> i64 {
    1
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct UpdateFileStatusInput {
    pub hash: Option<String>,
    pub selection: Option<SelectionQuerySpec>,
    pub status: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteFilesInput {
    pub hashes: Option<Vec<String>>,
    pub selection: Option<SelectionQuerySpec>,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn import_files(
    state: &AppState,
    input: ImportFilesInput,
) -> Result<crate::types::ImportBatchResult, String> {
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
    crate::import::service::ImportService::import_files(
        &state.db,
        &state.blob_store,
        input.paths,
        input.tag_strings,
        input.source_urls,
        auto_merge_enabled,
        auto_merge_distance,
        input.initial_status,
    )
    .await
}

pub async fn import_folder(
    state: &AppState,
    input: ImportFolderInput,
) -> Result<crate::types::ImportBatchResult, String> {
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
    crate::import::service::ImportService::import_folder(
        &state.db,
        &state.blob_store,
        input.path,
        input.preserve_structure,
        input.parent_folder_id,
        auto_merge_enabled,
        auto_merge_distance,
        input.initial_status,
    )
    .await
}

pub async fn update_file_status(
    state: &AppState,
    input: UpdateFileStatusInput,
) -> Result<usize, String> {
    let file_status = crate::types::parse_file_status(&input.status)?;

    if let Some(hash) = input.hash {
        // Single file mode
        state.db.update_file_status(&hash, file_status).await?;
        let folder_ids = collect_folder_ids_for_hashes(state, &[hash.clone()], 1).await;
        if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
            &state.db,
            &folder_ids,
        )
        .await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status update");
        }
        let mut impact =
            crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(
                &state.db,
            )
            .file_hashes(vec![hash]);
        if !folder_ids.is_empty() {
            impact = impact.folder_ids(folder_ids);
        }
        crate::events::emit_mutation("update_file_status", impact);
        Ok(1)
    } else if let Some(selection) = input.selection {
        // Selection mode
        let bitmap = resolve_selection_bitmap(state, &selection).await?;
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
            state
                .db
                .update_file_status_batch(&bitmap, file_status)
                .await?;
            if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
                &state.db,
                &folder_ids,
            )
            .await
            {
                tracing::warn!(error = %err, "failed to refresh folder sidebar projection after status batch update");
            }
            let mut impact =
                crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(
                    &state.db,
                );
            if !folder_ids.is_empty() {
                impact = impact.folder_ids(folder_ids);
            }
            crate::events::emit_mutation("update_file_status", impact);
        }
        Ok(count)
    } else {
        Err("Either hash or selection must be provided".into())
    }
}

pub async fn delete_files(state: &AppState, input: DeleteFilesInput) -> Result<usize, String> {
    let hashes = if let Some(hashes) = input.hashes {
        hashes
    } else if let Some(selection) = input.selection {
        let bitmap = resolve_selection_bitmap(state, &selection).await?;
        let file_ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
        let pairs = state.db.resolve_ids_batch(&file_ids).await?;
        pairs.into_iter().map(|(_, h)| h).collect()
    } else {
        return Err("Either hashes or selection must be provided".into());
    };

    let count = hashes.len();
    let folder_ids = collect_folder_ids_for_hashes(state, &hashes, count).await;
    for hash in &hashes {
        state.db.delete_file_by_hash(hash).await?;
        state.blob_store.delete(hash).map_err(|e| e.to_string())?;
    }

    if count > 0 {
        if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
            &state.db,
            &folder_ids,
        )
        .await
        {
            tracing::warn!(error = %err, "failed to refresh folder sidebar projection after delete_files");
        }
        let mut impact =
            crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(
                &state.db,
            )
            .file_hashes(hashes);
        if !folder_ids.is_empty() {
            impact = impact.folder_ids(folder_ids);
        }
        crate::events::emit_mutation("delete_files", impact);
    }
    Ok(count)
}

pub async fn wipe_image_data(state: &AppState, _input: serde_json::Value) -> Result<(), String> {
    state.db.wipe_all_files().await?;
    state.blob_store.wipe().map_err(|e| e.to_string())?;
    crate::events::emit_mutation(
        "wipe_image_data",
        crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(&state.db),
    );
    Ok(())
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
    let entity_ids: Vec<i64> = resolved
        .into_iter()
        .map(|(_, entity_id)| entity_id)
        .collect();
    if entity_ids.is_empty() {
        return Vec::new();
    }

    let mut folder_ids: Vec<i64> = match state
        .db
        .with_read_conn(move |conn| {
            let mut all = Vec::<i64>::new();
            for chunk in entity_ids.chunks(900) {
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
