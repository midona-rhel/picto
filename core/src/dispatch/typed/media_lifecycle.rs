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
/// For collections: trashes/restores all members, then sync derives collection status.
pub async fn set_entity_status(
    state: &AppState,
    input: UpdateFileStatusInput,
) -> Result<usize, String> {
    let status = crate::types::parse_file_status(&input.status)?;

    if let Some(hash) = input.hash {
        let file_id = state.db.resolve_hash(&hash).await?;

        // Check if this file is a collection cover
        let collection_id = state.db.with_read_conn(move |conn| {
            crate::folders::collections_db::find_collection_for_cover_file(conn, file_id)
        }).await?;

        if let Some(cid) = collection_id {
            // Collection: set status on all member files, sync derives collection status
            let member_fids = state.db.with_read_conn(move |conn| {
                crate::folders::collections_db::get_collection_member_file_ids(conn, cid)
            }).await?;

            let mut bitmap = roaring::RoaringBitmap::new();
            for &fid in &member_fids { bitmap.insert(fid as u32); }
            bitmap.insert(file_id as u32); // include cover file itself

            let count = bitmap.len() as usize;
            state.db.update_file_status_batch(&bitmap, status).await?;

            // Remove member file_ids from status bitmaps — the compiler excludes
            // collection members (parent_collection_id IS NOT NULL), so leaving
            // them inflates sidebar counts until the next compiler rebuild.
            for &fid in &member_fids {
                for s in 0..=2i64 {
                    state.db.bitmaps.remove(&crate::sqlite::bitmaps::BitmapKey::Status(s), fid as u32);
                }
            }

            let folder_ids = collect_folder_ids_for_hashes(state, &[hash.clone()], 1).await;
            if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
                &state.db, &folder_ids,
            ).await {
                tracing::warn!(error = %err, "failed to refresh sidebar after set_entity_status");
            }

            let mut impact = crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(&state.db)
                .file_hashes(vec![hash]);
            if !folder_ids.is_empty() { impact = impact.folder_ids(folder_ids); }
            crate::events::emit_mutation("set_entity_status", impact);
            Ok(count)
        } else {
            // Regular single file — use existing path
            state.db.update_file_status(&hash, status).await?;
            let folder_ids = collect_folder_ids_for_hashes(state, &[hash.clone()], 1).await;
            if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
                &state.db, &folder_ids,
            ).await {
                tracing::warn!(error = %err, "failed to refresh sidebar after set_entity_status");
            }
            let mut impact = crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(&state.db)
                .file_hashes(vec![hash]);
            if !folder_ids.is_empty() { impact = impact.folder_ids(folder_ids); }
            crate::events::emit_mutation("set_entity_status", impact);
            Ok(1)
        }
    } else if let Some(selection) = input.selection {
        // Selection mode — expand any collection covers to include members
        let original_ids: Vec<i64> = resolve_selection_bitmap(state, &selection).await?
            .iter().map(|id| id as i64).collect();
        let expanded = state.db.expand_collection_members(original_ids.clone()).await?;

        let mut expanded_bitmap = roaring::RoaringBitmap::new();
        for &fid in &expanded { expanded_bitmap.insert(fid as u32); }

        let count = expanded_bitmap.len() as usize;
        if count > 0 {
            let mut folder_ids = selection.filters.folder_ids.clone().unwrap_or_default();
            if matches!(selection.mode, SelectionMode::ExplicitHashes) {
                let explicit_hashes = selection.hashes.clone().unwrap_or_default();
                let mut from_hashes = collect_folder_ids_for_hashes(state, &explicit_hashes, 200).await;
                folder_ids.append(&mut from_hashes);
                folder_ids.sort_unstable();
                folder_ids.dedup();
            }
            state.db.update_file_status_batch(&expanded_bitmap, status).await?;

            // Remove expanded member file_ids from status bitmaps — the compiler
            // excludes collection members (parent_collection_id IS NOT NULL), so
            // leaving them inflates sidebar counts until the next compiler rebuild.
            for &fid in &expanded {
                if !original_ids.contains(&fid) {
                    for s in 0..=2i64 {
                        state.db.bitmaps.remove(&crate::sqlite::bitmaps::BitmapKey::Status(s), fid as u32);
                    }
                }
            }

            // Sync collection entities whose covers were in the selection
            // (update_file_status_batch only syncs PARENT collections, not the collections themselves)
            let oids = original_ids.clone();
            state.db.with_conn(move |conn| {
                for fid in &oids {
                    if let Some(cid) = crate::folders::collections_db::find_collection_for_cover_file(conn, *fid)? {
                        crate::folders::collections_db::sync_collection_aggregate_metadata(conn, cid)?;
                    }
                }
                Ok(())
            }).await?;

            if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
                &state.db, &folder_ids,
            ).await {
                tracing::warn!(error = %err, "failed to refresh sidebar after set_entity_status batch");
            }
            let mut impact = crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(&state.db);
            if !folder_ids.is_empty() { impact = impact.folder_ids(folder_ids); }
            crate::events::emit_mutation("set_entity_status", impact);
        }
        Ok(count)
    } else {
        Err("Either hash or selection must be provided".into())
    }
}

/// Permanently delete entities by hash. For collections: deletes collection + all members.
pub async fn delete_entities(
    state: &AppState,
    input: DeleteFilesInput,
) -> Result<usize, String> {
    // Collect entity IDs from the bitmap (includes both file_ids and collection entity_ids)
    let all_ids: Vec<i64> = if let Some(ref hashes) = input.hashes {
        let pairs = state.db.resolve_hashes_batch(hashes).await?;
        pairs.into_iter().map(|(_, fid)| fid).collect()
    } else if let Some(ref selection) = input.selection {
        let bitmap = resolve_selection_bitmap(state, &selection).await?;
        bitmap.iter().map(|id| id as i64).collect()
    } else {
        return Err("Either hashes or selection must be provided".into())
    };

    // Expand to include collection members
    let expanded = state.db.expand_collection_members(all_ids.clone()).await?;

    // Resolve all expanded IDs to hashes for blob deletion
    let all_hash_pairs = state.db.resolve_ids_batch(&expanded).await?;
    let all_hashes: Vec<String> = all_hash_pairs.iter().map(|(_, h)| h.clone()).collect();

    let count = all_ids.len();
    let folder_ids = collect_folder_ids_for_hashes(state, &all_hashes, count).await;

    // Find and delete collection entities first (by checking which IDs are collections)
    let collection_ids = state.db.with_read_conn({
        let ids = all_ids.clone();
        move |conn| {
            let mut cids = Vec::new();
            for &id in &ids {
                // Check if this ID is a collection entity directly
                let is_coll: bool = conn.query_row(
                    "SELECT COUNT(*) > 0 FROM media_entity WHERE entity_id = ?1 AND kind = 'collection'",
                    [id],
                    |row| row.get(0),
                ).unwrap_or(false);
                if is_coll { cids.push(id); }
                // Also check if it's a cover file for a collection
                if let Ok(Some(cid)) = crate::folders::collections_db::find_collection_for_cover_file(conn, id) {
                    if !cids.contains(&cid) { cids.push(cid); }
                }
            }
            Ok(cids)
        }
    }).await?;

    // Delete collections (orphans members, then we delete the orphaned files below)
    for cid in &collection_ids {
        if let Err(e) = state.db.delete_collection(*cid).await {
            tracing::warn!(collection_id = cid, error = %e, "delete_entities: failed to delete collection");
        }
    }

    // Delete all individual files (including orphaned collection members)
    for (_, hash) in &all_hash_pairs {
        if let Err(e) = state.db.delete_file_by_hash(hash).await {
            tracing::warn!(hash, error = %e, "delete_entities: failed to delete file");
        }
        let _ = state.blob_store.delete(hash);
    }

    if count > 0 {
        if let Err(err) = crate::folders::service::refresh_sidebar_projection_for_folder_ids(
            &state.db, &folder_ids,
        ).await {
            tracing::warn!(error = %err, "failed to refresh sidebar after delete_entities");
        }
        let mut impact = crate::runtime_contract::mutation_builder::MutationImpact::file_status_change(&state.db)
            .file_hashes(all_hashes);
        if !folder_ids.is_empty() { impact = impact.folder_ids(folder_ids); }
        crate::events::emit_mutation("delete_entities", impact);
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
