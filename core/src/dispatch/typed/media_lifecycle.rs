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
    // Reject paths inside the library directory to prevent circular imports
    let library_root = &state.library_root;
    for p in &input.paths {
        if let Ok(canonical) = std::fs::canonicalize(p) {
            if canonical.starts_with(library_root) {
                return Err(format!(
                    "Cannot import files from inside the library directory: {}",
                    canonical.display()
                ));
            }
        }
    }

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
    let auto_merge_require_matching_dimensions =
        app_settings.duplicate_auto_merge_require_matching_dimensions;
    let result = crate::import::service::ImportService::import_files(
        &state.db,
        &state.blob_store,
        input.paths,
        input.tag_strings,
        input.source_urls,
        auto_merge_enabled,
        auto_merge_distance,
        auto_merge_require_matching_dimensions,
        input.initial_status,
        Some(&state.library_root),
    )
    .await?;

    // Auto-tag imported files if enabled
    let imported_hashes: Vec<String> = result.imported.iter().map(|r| r.hash.clone()).collect();
    crate::dispatch::typed::ai_tagger::auto_tag_imported(state, &imported_hashes).await;

    Ok(result)
}

pub async fn import_folder(
    state: &AppState,
    input: ImportFolderInput,
) -> Result<crate::types::ImportBatchResult, String> {
    // Reject paths inside the library directory to prevent circular imports
    if let Ok(canonical) = std::fs::canonicalize(&input.path) {
        if canonical.starts_with(&state.library_root) {
            return Err(format!(
                "Cannot import a folder inside the library directory: {}",
                canonical.display()
            ));
        }
    }

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
    let auto_merge_require_matching_dimensions =
        app_settings.duplicate_auto_merge_require_matching_dimensions;
    let result = crate::import::service::ImportService::import_folder(
        &state.db,
        &state.blob_store,
        input.path,
        input.preserve_structure,
        input.parent_folder_id,
        auto_merge_enabled,
        auto_merge_distance,
        auto_merge_require_matching_dimensions,
        input.initial_status,
    )
    .await?;

    // Auto-tag imported files if enabled
    let imported_hashes: Vec<String> = result.imported.iter().map(|r| r.hash.clone()).collect();
    crate::dispatch::typed::ai_tagger::auto_tag_imported(state, &imported_hashes).await;

    Ok(result)
}

// ─── Entity-level lifecycle commands ──────────────────────────────────────────
// These work uniformly for single files AND collections.

/// Set status on a media entity (single or collection) by hash.
/// Set status on entities. For collections, cascades status to members.
/// Sidebar counts exclude members via CollectionMember bitmap.
pub async fn set_entity_status(
    state: &AppState,
    input: UpdateFileStatusInput,
) -> Result<usize, String> {
    let status = crate::types::parse_file_status(&input.status)?;

    // Resolve to hashes — single hash or selection
    let hashes: Vec<String> = if let Some(hash) = input.hash {
        vec![hash]
    } else if let Some(selection) = input.selection {
        let bitmap = resolve_selection_bitmap(state, &selection).await?;
        let ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
        state.db.resolve_ids_batch(&ids).await?
            .into_iter().map(|(_, h)| h).collect()
    } else {
        return Err("Either hash or selection must be provided".into());
    };

    if hashes.is_empty() {
        return Ok(0);
    }

    // Use the same update_file_status path for every hash — it handles
    // collection member cascade and CollectionMember bitmap correctly.
    for hash in &hashes {
        state.db.update_file_status(hash, status).await?;
    }

    let folder_ids = collect_folder_ids_for_hashes(state, &hashes, 200).await;
    if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
        &state.db,
        &folder_ids,
    )
    .await
    {
        tracing::warn!(error = %err, "failed to refresh sidebar after set_entity_status");
    }

    // Don't include sidebar_counts — the bitmaps haven't been rebuilt by the
    // compiler yet and would include stale member counts. The frontend's eager
    // count is correct; the compiler will emit authoritative counts on rebuild.
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::new()
        .status_changed()
        .status_sensitive_grid_scopes_changed()
        .file_hashes(hashes.clone());
    if !folder_ids.is_empty() {
        impact = impact.folder_ids(folder_ids);
    }
    crate::events::emit_state_changed("set_entity_status", impact);
    Ok(hashes.len())
}

/// Permanently delete entities by hash. For collections: flattens and deletes all members.
///
/// Fast path: bulk SQL delete in one transaction. Blob cleanup is deferred to background.
pub async fn delete_entities(state: &AppState, input: DeleteFilesInput) -> Result<usize, String> {
    // 1. Resolve all entity IDs (resolve_hashes_batch expands collections to members)
    let all_ids: Vec<i64> = if let Some(ref hashes) = input.hashes {
        let pairs = state.db.resolve_hashes_batch(hashes).await?;
        pairs.into_iter().map(|(_, fid)| fid).collect()
    } else if let Some(ref selection) = input.selection {
        let bitmap = resolve_selection_bitmap(state, &selection).await?;
        let mut ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
        // Expand collections in the selection
        ids = state.db.expand_collection_members(ids).await?;
        ids
    } else {
        return Err("Either hashes or selection must be provided".into());
    };

    if all_ids.is_empty() {
        return Ok(0);
    }

    // 2. Collect hashes for blob cleanup (before we delete the DB records)
    let all_hash_pairs = state.db.resolve_ids_batch(&all_ids).await?;
    let blob_hashes: Vec<String> = all_hash_pairs.iter().map(|(_, h)| h.clone()).collect();
    let count = all_ids.len();
    let folder_ids = collect_folder_ids_for_hashes(state, &blob_hashes, count).await;

    // 3. Find collection entity_ids (for state-change event scopes)
    let collection_ids: Vec<i64> = state.db.with_read_conn({
        let ids = all_ids.clone();
        move |conn| {
            let mut cids = Vec::new();
            for chunk in ids.chunks(999) {
                let placeholders = std::iter::repeat_n("?", chunk.len()).collect::<Vec<_>>().join(",");
                let sql = format!(
                    "SELECT entity_id FROM media_entity WHERE entity_id IN ({placeholders}) AND kind = 'collection'"
                );
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(
                    rusqlite::params_from_iter(chunk.iter()),
                    |row| row.get::<_, i64>(0),
                )?;
                for row in rows {
                    cids.push(row?);
                }
            }
            Ok(cids)
        }
    }).await?;

    // 4. Bulk delete all entities + cascading metadata in one transaction
    let _deleted = state.db.with_conn_mut({
        let ids = all_ids.clone();
        move |conn| {
            let tx = conn.transaction()?;
            let mut total = 0usize;
            for chunk in ids.chunks(999) {
                let placeholders = std::iter::repeat_n("?", chunk.len()).collect::<Vec<_>>().join(",");

                // Remove from folders
                let folder_sql = format!(
                    "DELETE FROM folder_entity WHERE entity_id IN ({placeholders})"
                );
                tx.execute(&folder_sql, rusqlite::params_from_iter(chunk.iter())).ok();

                // Remove tags
                let tag_sql = format!(
                    "DELETE FROM entity_tag_raw WHERE entity_id IN ({placeholders})"
                );
                tx.execute(&tag_sql, rusqlite::params_from_iter(chunk.iter()))?;

                // Orphan collection members (set parent_collection_id = NULL)
                let orphan_sql = format!(
                    "UPDATE media_entity SET parent_collection_id = NULL, collection_ordinal = NULL
                     WHERE parent_collection_id IN ({placeholders})"
                );
                tx.execute(&orphan_sql, rusqlite::params_from_iter(chunk.iter())).ok();

                // Delete entity_file mappings
                let ef_sql = format!(
                    "DELETE FROM entity_file WHERE entity_id IN ({placeholders})"
                );
                tx.execute(&ef_sql, rusqlite::params_from_iter(chunk.iter()))?;

                // Delete the file rows
                let file_sql = format!(
                    "DELETE FROM file WHERE file_id IN ({placeholders})"
                );
                tx.execute(&file_sql, rusqlite::params_from_iter(chunk.iter())).ok();

                // Delete media entities
                let me_sql = format!(
                    "DELETE FROM media_entity WHERE entity_id IN ({placeholders})"
                );
                let changed = tx.execute(&me_sql, rusqlite::params_from_iter(chunk.iter()))?;
                total += changed;
            }
            tx.commit()?;
            Ok(total)
        }
    }).await?;

    // 5. Clean up bitmaps (remove from all status bitmaps)
    for &id in &all_ids {
        let eid = id as u32;
        state.db.bitmaps.remove(&crate::sqlite::bitmaps::BitmapKey::Status(0), eid);
        state.db.bitmaps.remove(&crate::sqlite::bitmaps::BitmapKey::Status(1), eid);
        state.db.bitmaps.remove(&crate::sqlite::bitmaps::BitmapKey::Status(2), eid);
    }

    // 6. Defer blob cleanup to background (don't block the UI)
    let all_hashes = blob_hashes.clone();
    {
        let blob_store = state.blob_store.clone();
        let hashes = blob_hashes;
        tokio::spawn(async move {
            for hash in &hashes {
                let _ = blob_store.delete(hash);
                let _ = blob_store.delete_thumbnail(hash);
            }
            tracing::info!(count = hashes.len(), "deferred blob cleanup complete");
        });
    }

    if count > 0 {
        if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
            &state.db,
            &folder_ids,
        )
        .await
        {
            tracing::warn!(error = %err, "failed to refresh sidebar after delete_entities");
        }
        let mut extra_grid_scopes = collection_ids
            .iter()
            .map(|id| format!("collection:{id}"))
            .collect::<Vec<_>>();
        let mut impact =
            crate::runtime_contract::change_builder::ChangeImpact::file_lifecycle(&state.db)
                .file_hashes(all_hashes);
        if !folder_ids.is_empty() {
            impact = impact
                .folder_ids(folder_ids.clone())
                .folder_membership_changed(folder_ids);
        }
        if !extra_grid_scopes.is_empty() {
            impact = impact.extra_grid_scopes(std::mem::take(&mut extra_grid_scopes));
        }
        crate::events::emit_state_changed("delete_entities", impact);
    }
    Ok(count)
}

/// Wipe all image data — catastrophic full reset.
/// Uses file_lifecycle without specific hashes because ALL files are removed.
pub async fn wipe_image_data(state: &AppState, _input: serde_json::Value) -> Result<(), String> {
    state.db.wipe_all_files().await?;
    state.blob_store.wipe().map_err(|e| e.to_string())?;
    crate::events::emit_state_changed(
        "wipe_image_data",
        crate::runtime_contract::change_builder::ChangeImpact::file_lifecycle(&state.db),
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
