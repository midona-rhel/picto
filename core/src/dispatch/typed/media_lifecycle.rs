//! Handler functions for media lifecycle operations:
//! import, status changes, deletion, and FTS rebuild.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

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
    let root_hashes = state.engine.db().with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT entity_hash
             FROM media_entity
             WHERE parent_collection_entity_id IS NULL
             ORDER BY entity_id",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
    })?;
    if !root_hashes.is_empty() {
        state.engine.delete_entities(crate::db::types::EntityTarget {
            kind: crate::db::types::EntityTargetKind::EntityHashes,
            entity_hashes: Some(root_hashes),
            query: None,
            excluded_entity_hashes: None,
        })?;
    }
    state.blob_store.wipe().map_err(|e| e.to_string())?;
    Ok(())
}
