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

    let result = state
        .engine
        .import_files(
            &state.db,
            &state.blob_store,
            input.paths,
            input.tag_strings,
            input.source_urls,
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

    let result = state
        .engine
        .import_folder(
            &state.db,
            &state.blob_store,
            input.path,
            input.preserve_structure,
            input.parent_folder_id,
            input.initial_status,
        )
        .await?;

    // Auto-tag imported files if enabled
    let imported_hashes: Vec<String> = result.imported.iter().map(|r| r.hash.clone()).collect();
    crate::dispatch::typed::ai_tagger::auto_tag_imported(state, &imported_hashes).await;

    Ok(result)
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
            let pairs = state.db.resolve_entity_hashes_batch(&hashes).await?;
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
