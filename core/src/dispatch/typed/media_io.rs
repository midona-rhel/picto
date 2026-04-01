//! Handler functions for media I/O operations: path resolution,
//! OS integration (open, reveal, export), thumbnails, and color search.

use std::path::Path;

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ResolveFilePathInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ResolveFilePathsBatchInput {
    #[ts(type = "import('../../src/shared/types/canonical').EntityTarget")]
    pub target: crate::db::types::EntityTarget,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct OpenFileDefaultInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RevealInFolderInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ExportFileInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
    pub dest_path: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct OpenInNewWindowInput {
    pub hash: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ResolveThumbnailPathInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct EnsureThumbnailInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RegenerateThumbnailInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RegenerateThumbnailsBatchInput {
    pub hashes: Vec<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ReanalyzeFileColorsInput {
    #[serde(alias = "entity_hash")]
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ExportMediaInput {
    #[ts(type = "import('../../src/shared/types/canonical').EntityTarget")]
    pub target: crate::db::types::EntityTarget,
    pub output_dir: String,
    pub format: Option<String>,
    pub quality: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default = "default_keep_aspect")]
    pub keep_aspect: bool,
}

// ─── Private result structs ────────────────────────────────────────────────

fn default_keep_aspect() -> bool {
    true
}

/// Background-backfill missing thumbnails and dominant colors.
/// Fire-and-forget enqueue — the deferred-work worker owns execution.
pub async fn backfill_missing_deferred(
    db: &crate::db::LibraryDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hashes: &[String],
) {
    crate::background_work::enqueue_missing_derivative_jobs(db, blob_store, hashes).await;
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn resolve_file_path(
    state: &AppState,
    input: ResolveFilePathInput,
) -> Result<String, String> {
    state
        .engine
        .resolve_file_path(&state.blob_store, &input.hash)
        .await
}

pub async fn resolve_file_paths_batch(
    state: &AppState,
    input: ResolveFilePathsBatchInput,
) -> Result<serde_json::Value, String> {
    let paths = state
        .engine
        .resolve_file_paths(&state.blob_store, input.target)
        .await?;
    serde_json::to_value(&paths).map_err(|e| e.to_string())
}

pub async fn open_file_default(
    state: &AppState,
    input: OpenFileDefaultInput,
) -> Result<(), String> {
    state
        .engine
        .open_file_default(&state.blob_store, &input.hash)
        .await
}

pub async fn reveal_in_folder(state: &AppState, input: RevealInFolderInput) -> Result<(), String> {
    state
        .engine
        .reveal_in_folder(&state.blob_store, &input.hash)
        .await
}

pub async fn export_file(state: &AppState, input: ExportFileInput) -> Result<(), String> {
    state
        .engine
        .export_file(
            &state.blob_store,
            &state.library_root,
            &input.hash,
            Path::new(&input.dest_path),
        )
        .await
}

pub async fn export_media(
    state: &AppState,
    input: ExportMediaInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .engine
        .export_media(
            &state.blob_store,
            &state.library_root,
            crate::engine::media_io::ExportMediaRequest {
                target: input.target,
                output_dir: input.output_dir.into(),
                format: input.format,
                quality: input.quality,
                width: input.width,
                height: input.height,
                keep_aspect: input.keep_aspect,
            },
        )
        .await?;
    serde_json::to_value(result).map_err(|e| e.to_string())
}

pub async fn open_in_new_window(
    _state: &AppState,
    input: OpenInNewWindowInput,
) -> Result<(), String> {
    crate::events::emit(
        crate::events::event_names::OPEN_DETAIL_WINDOW,
        &crate::events::OpenDetailWindowEvent {
            hash: input.hash,
            width: input.width,
            height: input.height,
        },
    );
    Ok(())
}

pub async fn resolve_thumbnail_path(
    state: &AppState,
    input: ResolveThumbnailPathInput,
) -> Result<String, String> {
    state
        .engine
        .resolve_thumbnail_path(&state.blob_store, &input.hash)
        .await
}

pub async fn ensure_thumbnail(
    state: &AppState,
    input: EnsureThumbnailInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .engine
        .ensure_thumbnail(&state.blob_store, &input.hash)
        .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn regenerate_thumbnail(
    state: &AppState,
    input: RegenerateThumbnailInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .engine
        .regenerate_thumbnail(&state.blob_store, &input.hash)
        .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn regenerate_thumbnails_batch(
    state: &AppState,
    input: RegenerateThumbnailsBatchInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .engine
        .regenerate_thumbnails_batch(&state.blob_store, &input.hashes)
        .await?;
    Ok(serde_json::json!({
        "total": result.total,
        "regenerated": result.exported,
        "errors": result.errors,
    }))
}

pub async fn reanalyze_file_colors(
    state: &AppState,
    input: ReanalyzeFileColorsInput,
) -> Result<serde_json::Value, String> {
    let result = state
        .engine
        .reanalyze_file_colors(&state.blob_store, &input.hash)
        .await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}
