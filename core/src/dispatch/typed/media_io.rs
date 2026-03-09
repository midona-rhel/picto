//! Handler functions for media I/O operations: path resolution,
//! OS integration (open, reveal, export), thumbnails, blurhash backfill,
//! and color search.

use serde::Deserialize;
use ts_rs::TS;

use crate::blob_store::mime_to_extension;
use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ResolveFilePathInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct OpenFileDefaultInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RevealInFolderInput {
    pub hash: String,
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
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct EnsureThumbnailInput {
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RegenerateThumbnailInput {
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
    pub hash: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct BackfillMissingBlurhashesInput {
    pub limit: Option<usize>,
}

// ─── Private result structs ────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct EnsureThumbnailResult {
    regenerated_thumbnail: bool,
    generated_blurhash: bool,
    has_thumbnail: bool,
    blurhash: Option<String>,
}

#[derive(Debug, serde::Serialize)]
struct BackfillMissingBlurhashesResult {
    processed: usize,
    regenerated_thumbnails: usize,
    generated_blurhashes: usize,
    remaining: usize,
}

#[derive(Debug, serde::Serialize)]
struct ReanalyzeFileColorsResult {
    colors_extracted: usize,
    dominant_color_hex: Option<String>,
}

// ─── Helper functions ──────────────────────────────────────────────────────

async fn resolve_file_path_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
) -> Result<String, String> {
    let file = db
        .get_file_by_hash(hash)
        .await?
        .ok_or_else(|| format!("File not found in database: {}", hash))?;
    let ext = mime_to_extension(&file.mime).to_string();
    let bs = blob_store.clone();
    let h = hash.to_string();
    tokio::task::spawn_blocking(move || {
        bs.find_original(&h, Some(&ext))
            .map_err(|e| format!("Blob error: {}", e))?
            .map(|(path, _)| path.to_string_lossy().into_owned())
            .ok_or_else(|| format!("File not found in blob store: {}", h))
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?
}

fn reveal_in_folder_os(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to reveal in Finder: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| format!("Failed to reveal in Explorer: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| format!("Failed to open folder: {}", e))?;
        }
    }

    Ok(())
}

async fn ensure_thumbnail_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
) -> Result<EnsureThumbnailResult, String> {
    generate_thumbnail_inner(db, blob_store, hash, false).await
}

/// Core thumbnail generation. When `force` is true, deletes existing thumbnail
/// first (used by regenerate). When false, skips if thumbnail already exists.
async fn generate_thumbnail_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
    force: bool,
) -> Result<EnsureThumbnailResult, String> {
    let file = db
        .get_file_by_hash(hash)
        .await?
        .ok_or_else(|| format!("File not found in database: {}", hash))?;

    let current_blurhash = file.blurhash.clone();
    let ext = mime_to_extension(&file.mime).to_string();
    let h = hash.to_string();
    let bs = blob_store.clone();
    let need_blurhash = current_blurhash.is_none() || force;

    let (regenerated_thumbnail, has_thumbnail, thumb_for_blurhash) =
        tokio::task::spawn_blocking(move || -> Result<(bool, bool, Option<Vec<u8>>), String> {
            if force {
                bs.delete_thumbnail(&h)
                    .map_err(|e| format!("Delete thumbnail failed: {}", e))?;
            }

            let original = bs
                .find_original(&h, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", h))?;

            let mut thumb_bytes_for_blurhash: Option<Vec<u8>> = None;

            if !force {
                let thumb_exists = bs
                    .find_thumbnail_path(&h)
                    .map_err(|e| format!("Thumbnail lookup failed: {}", e))?
                    .is_some();

                if thumb_exists {
                    if need_blurhash {
                        thumb_bytes_for_blurhash = bs
                            .read_thumbnail(&h)
                            .map_err(|e| format!("Thumbnail read failed: {}", e))?;
                    }
                    return Ok((false, true, thumb_bytes_for_blurhash));
                }
            }

            let info = crate::media_processing::get_file_info(&original.0, None)
                .map_err(|e| format!("File info failed: {}", e))?;
            let (thumb_bytes, thumb_ext) = crate::media_processing::generate_thumbnail_bytes(
                &original.0,
                crate::media_processing::DEFAULT_THUMBNAIL_DIMENSIONS,
                info.mime,
                info.duration_ms,
                info.num_frames,
                35,
            )
            .map_err(|e| format!("Thumbnail generation failed: {}", e))?;

            bs.write_thumbnail(&h, &thumb_bytes, &thumb_ext)
                .map_err(|e| format!("Thumbnail write failed: {}", e))?;

            if need_blurhash {
                thumb_bytes_for_blurhash = Some(thumb_bytes);
            }

            Ok((true, true, thumb_bytes_for_blurhash))
        })
        .await
        .map_err(|e| format!("Thumbnail task failed: {}", e))??;

    let mut generated_blurhash = false;
    let mut blurhash = current_blurhash;
    if (blurhash.is_none() || force) && regenerated_thumbnail {
        if let Some(thumb_bytes) = thumb_for_blurhash {
            if let Ok(bh) = crate::media_processing::blurhash::get_blurhash_from_thumbnail_bytes(&thumb_bytes)
            {
                db.set_blurhash(hash, Some(&bh)).await?;
                blurhash = Some(bh);
                generated_blurhash = true;
            }
        }
    }

    Ok(EnsureThumbnailResult {
        regenerated_thumbnail,
        generated_blurhash,
        has_thumbnail,
        blurhash,
    })
}

async fn backfill_missing_blurhashes_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    limit: Option<usize>,
) -> Result<BackfillMissingBlurhashesResult, String> {
    let batch_limit = limit.unwrap_or(128).clamp(1, 1000);
    let hashes: Vec<String> = db
        .with_read_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT hash FROM file
                 WHERE blurhash IS NULL
                 ORDER BY file_id ASC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map([batch_limit as i64], |row| row.get::<_, String>(0))?;
            rows.collect()
        })
        .await?;

    let mut regenerated_thumbnails = 0usize;
    let mut generated_blurhashes = 0usize;
    for hash in &hashes {
        if let Ok(result) = ensure_thumbnail_inner(db, blob_store, hash).await {
            if result.regenerated_thumbnail {
                regenerated_thumbnails += 1;
            }
            if result.generated_blurhash {
                generated_blurhashes += 1;
            }
        }
    }

    let remaining: i64 = db
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM file WHERE blurhash IS NULL",
                [],
                |row| row.get(0),
            )
        })
        .await?;

    Ok(BackfillMissingBlurhashesResult {
        processed: hashes.len(),
        regenerated_thumbnails,
        generated_blurhashes,
        remaining: remaining.max(0) as usize,
    })
}

async fn reanalyze_file_colors_inner(
    db: &crate::sqlite::SqliteDatabase,
    blob_store: &std::sync::Arc<crate::blob_store::BlobStore>,
    hash: &str,
) -> Result<ReanalyzeFileColorsResult, String> {
    let file = db
        .get_file_by_hash(hash)
        .await?
        .ok_or_else(|| format!("File not found in database: {}", hash))?;

    if !file.mime.starts_with("image/") {
        db.set_file_colors(hash, Vec::new(), None).await?;
        db.emit_read_model_event(crate::sqlite::ReadModelEvent::RebuildAll);
        return Ok(ReanalyzeFileColorsResult {
            colors_extracted: 0,
            dominant_color_hex: None,
        });
    }

    let ext = mime_to_extension(&file.mime).to_string();
    let h = hash.to_string();
    let bs = blob_store.clone();
    let colors =
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, f32, f32, f32)>, String> {
            let original = bs
                .find_original(&h, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", h))?;

            let bytes = std::fs::read(&original.0)
                .map_err(|e| format!("Failed to read original file: {}", e))?;
            let img =
                image::load_from_memory(&bytes).map_err(|e| format!("Image decode failed: {}", e))?;
            let extracted = crate::media_processing::colors::extract_dominant_colors(&img, 8);
            Ok(extracted
                .iter()
                .map(|c| (c.hex.clone(), c.l as f32, c.a as f32, c.b as f32))
                .collect())
        })
        .await
        .map_err(|e| format!("Color extraction task failed: {}", e))??;

    let dominant_color_hex = colors.first().map(|(hex, _, _, _)| hex.clone());
    let colors_extracted = colors.len();

    db.set_file_colors(hash, colors, dominant_color_hex.clone())
        .await?;
    db.emit_read_model_event(crate::sqlite::ReadModelEvent::RebuildAll);

    Ok(ReanalyzeFileColorsResult {
        colors_extracted,
        dominant_color_hex,
    })
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn resolve_file_path(state: &AppState, input: ResolveFilePathInput) -> Result<String, String> {
    resolve_file_path_inner(&state.db, &state.blob_store, &input.hash).await
}

pub async fn open_file_default(state: &AppState, input: OpenFileDefaultInput) -> Result<(), String> {
    let path = resolve_file_path_inner(&state.db, &state.blob_store, &input.hash).await?;
    open::that(&path).map_err(|e| format!("Failed to open file: {}", e))?;
    Ok(())
}

pub async fn reveal_in_folder(state: &AppState, input: RevealInFolderInput) -> Result<(), String> {
    let path = resolve_file_path_inner(&state.db, &state.blob_store, &input.hash).await?;
    reveal_in_folder_os(&path)?;
    Ok(())
}

pub async fn open_in_new_window(_state: &AppState, input: OpenInNewWindowInput) -> Result<(), String> {
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

pub async fn resolve_thumbnail_path(state: &AppState, input: ResolveThumbnailPathInput) -> Result<String, String> {
    let bs = state.blob_store.clone();
    let hash = input.hash;
    let result = tokio::task::spawn_blocking(move || {
        bs.find_thumbnail_path(&hash)
            .map_err(|e| format!("Blob error: {}", e))?
            .map(|p| p.to_string_lossy().into_owned())
            .ok_or_else(|| format!("Thumbnail not found: {}", hash))
    })
    .await
    .map_err(|e| format!("Task error: {}", e))?;
    result
}

pub async fn ensure_thumbnail(state: &AppState, input: EnsureThumbnailInput) -> Result<serde_json::Value, String> {
    let result = ensure_thumbnail_inner(&state.db, &state.blob_store, &input.hash).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn regenerate_thumbnail(state: &AppState, input: RegenerateThumbnailInput) -> Result<serde_json::Value, String> {
    let result =
        generate_thumbnail_inner(&state.db, &state.blob_store, &input.hash, true).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn regenerate_thumbnails_batch(state: &AppState, input: RegenerateThumbnailsBatchInput) -> Result<serde_json::Value, String> {
    let mut regenerated = 0usize;
    let mut errors = 0usize;
    for hash in &input.hashes {
        match generate_thumbnail_inner(&state.db, &state.blob_store, hash, true).await {
            Ok(r) => {
                if r.regenerated_thumbnail {
                    regenerated += 1;
                }
            }
            Err(_) => {
                errors += 1;
            }
        }
    }
    Ok(serde_json::json!({
        "total": input.hashes.len(),
        "regenerated": regenerated,
        "errors": errors,
    }))
}

pub async fn reanalyze_file_colors(state: &AppState, input: ReanalyzeFileColorsInput) -> Result<serde_json::Value, String> {
    let result =
        reanalyze_file_colors_inner(&state.db, &state.blob_store, &input.hash).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

pub async fn backfill_missing_blurhashes(state: &AppState, input: BackfillMissingBlurhashesInput) -> Result<serde_json::Value, String> {
    let result =
        backfill_missing_blurhashes_inner(&state.db, &state.blob_store, input.limit).await?;
    serde_json::to_value(&result).map_err(|e| e.to_string())
}

