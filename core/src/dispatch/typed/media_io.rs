//! Handler functions for media I/O operations: path resolution,
//! OS integration (open, reveal, export), thumbnails, and color search.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use ts_rs::TS;

use crate::blob_store::mime_to_extension;
use crate::state::AppState;
use crate::types::SelectionQuerySpec;

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
pub struct ExportFileInput {
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
pub struct ExportMediaInput {
    pub hashes: Option<Vec<String>>,
    pub selection: Option<SelectionQuerySpec>,
    pub output_dir: String,
    pub format: Option<String>,
    pub quality: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default = "default_keep_aspect")]
    pub keep_aspect: bool,
}

#[derive(Debug, serde::Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ExportMediaResult {
    pub total: usize,
    pub exported: usize,
    pub skipped: usize,
    pub errors: usize,
}

// ─── Private result structs ────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
struct EnsureThumbnailResult {
    regenerated_thumbnail: bool,
    has_thumbnail: bool,
}

#[derive(Debug, serde::Serialize)]
struct ReanalyzeFileColorsResult {
    colors_extracted: usize,
    dominant_color_hex: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Original,
    Png,
    Jpeg,
    Webp,
    Avif,
}

fn default_keep_aspect() -> bool {
    true
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

fn sanitize_file_stem(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    let raw = if trimmed.is_empty() { fallback } else { trimmed };
    let cleaned_raw = raw
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>();
    let cleaned = cleaned_raw
        .trim()
        .trim_matches('.');
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned.to_string()
    }
}

fn ensure_unique_dest_path(dir: &Path, stem: &str, ext: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{ext}"));
    if !candidate.exists() {
        return candidate;
    }
    let mut suffix = 2usize;
    loop {
        candidate = dir.join(format!("{stem} ({suffix}).{ext}"));
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn parse_export_format(value: Option<&str>) -> Result<ExportFormat, String> {
    match value.unwrap_or("original").to_ascii_lowercase().as_str() {
        "original" => Ok(ExportFormat::Original),
        "png" => Ok(ExportFormat::Png),
        "jpg" | "jpeg" => Ok(ExportFormat::Jpeg),
        "webp" => Ok(ExportFormat::Webp),
        "avif" => Ok(ExportFormat::Avif),
        other => Err(format!("Unsupported export format: {other}")),
    }
}

fn resize_for_export(
    image: image::DynamicImage,
    width: Option<u32>,
    height: Option<u32>,
    keep_aspect: bool,
) -> image::DynamicImage {
    let target_w = width.unwrap_or(0);
    let target_h = height.unwrap_or(0);
    if target_w == 0 && target_h == 0 {
        return image;
    }
    if keep_aspect {
        if target_w > 0 && target_h > 0 {
            image.resize(target_w, target_h, image::imageops::FilterType::Lanczos3)
        } else if target_w > 0 {
            image.resize(target_w, u32::MAX, image::imageops::FilterType::Lanczos3)
        } else {
            image.resize(u32::MAX, target_h, image::imageops::FilterType::Lanczos3)
        }
    } else {
        image.resize_exact(
            width.unwrap_or_else(|| image.width()),
            height.unwrap_or_else(|| image.height()),
            image::imageops::FilterType::Lanczos3,
        )
    }
}

fn encode_export_image(
    image: &image::DynamicImage,
    format: ExportFormat,
    quality: u8,
) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    match format {
        ExportFormat::Original => unreachable!("original format should bypass re-encode"),
        ExportFormat::Png => {
            use image::ImageEncoder;
            let rgba = image.to_rgba8();
            let encoder = image::codecs::png::PngEncoder::new(&mut out);
            encoder
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| format!("PNG encode failed: {e}"))?;
        }
        ExportFormat::Jpeg => {
            let rgb = image.to_rgb8();
            let mut encoder =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100));
            encoder
                .encode_image(&image::DynamicImage::ImageRgb8(rgb))
                .map_err(|e| format!("JPEG encode failed: {e}"))?;
        }
        ExportFormat::Webp => {
            let rgba = image.to_rgba8();
            let encoder = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
            let encoded = encoder.encode(quality.clamp(1, 100) as f32);
            out.extend_from_slice(encoded.as_ref());
        }
        ExportFormat::Avif => {
            let mut cursor = std::io::Cursor::new(&mut out);
            image
                .write_to(&mut cursor, image::ImageFormat::Avif)
                .map_err(|e| format!("AVIF encode failed: {e}"))?;
        }
    }
    Ok(out)
}

fn extension_for_export_format(format: ExportFormat, original_mime: &str) -> &'static str {
    match format {
        ExportFormat::Original => mime_to_extension(original_mime),
        ExportFormat::Png => "png",
        ExportFormat::Jpeg => "jpg",
        ExportFormat::Webp => "webp",
        ExportFormat::Avif => "avif",
    }
}

async fn resolve_export_hashes(
    state: &AppState,
    hashes: Option<Vec<String>>,
    selection: Option<SelectionQuerySpec>,
) -> Result<Vec<String>, String> {
    if let Some(hashes) = hashes {
        return Ok(hashes);
    }
    if let Some(selection) = selection {
        let bitmap = crate::dispatch::typed::media_lifecycle::resolve_selection_bitmap(state, &selection).await?;
        let file_ids: Vec<i64> = bitmap.iter().map(|id| id as i64).collect();
        let pairs = state.db.resolve_ids_batch(&file_ids).await?;
        return Ok(pairs.into_iter().map(|(_, hash)| hash).collect());
    }
    Err("Either hashes or selection must be provided".into())
}

fn file_name_for_progress(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

async fn export_media_inner(state: &AppState, input: ExportMediaInput) -> Result<ExportMediaResult, String> {
    let hashes = resolve_export_hashes(state, input.hashes, input.selection).await?;
    let total = hashes.len();
    let output_dir = PathBuf::from(&input.output_dir);
    let format = parse_export_format(input.format.as_deref())?;
    let quality = input.quality.unwrap_or(82).clamp(1, 100);

    tokio::fs::create_dir_all(&output_dir)
        .await
        .map_err(|e| format!("Failed to create export directory: {e}"))?;

    let mut exported = 0usize;
    let skipped = 0usize;
    let mut errors = 0usize;

    for (index, hash) in hashes.iter().enumerate() {
        let export_result = async {
            let record = state
                .db
                .get_file_by_hash(hash)
                .await?
                .ok_or_else(|| format!("File not found in database: {hash}"))?;

            let original_ext = mime_to_extension(&record.mime).to_string();
            let blob_store = state.blob_store.clone();
            let hash_owned = hash.clone();
            let original_data = tokio::task::spawn_blocking(move || {
                blob_store.read_original(&hash_owned, Some(&original_ext))
            })
            .await
            .map_err(|e| format!("Export read task failed: {e}"))?
            .map_err(|e| format!("Export read failed: {e}"))?;

            let display_name = record.name.as_deref().unwrap_or("");
            let stem = sanitize_file_stem(display_name, &hash[..12]);
            let dest_ext = extension_for_export_format(format, &record.mime);
            let dest_path = ensure_unique_dest_path(&output_dir, &stem, dest_ext);

            if format == ExportFormat::Original {
                tokio::fs::write(&dest_path, &original_data)
                    .await
                    .map_err(|e| format!("Failed to write export: {e}"))?;
                return Ok(dest_path);
            }

            if !record.mime.starts_with("image/") {
                return Err(format!(
                    "Format conversion is only supported for image files: {}",
                    record.name.as_deref().unwrap_or(hash)
                ));
            }

            let width = input.width;
            let height = input.height;
            let keep_aspect = input.keep_aspect;
            let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
                let image = image::load_from_memory(&original_data)
                    .map_err(|e| format!("Image decode failed: {e}"))?;
                let resized = resize_for_export(image, width, height, keep_aspect);
                encode_export_image(&resized, format, quality)
            })
            .await
            .map_err(|e| format!("Export transform task failed: {e}"))??;

            tokio::fs::write(&dest_path, bytes)
                .await
                .map_err(|e| format!("Failed to write export: {e}"))?;
            Ok(dest_path)
        }
        .await;

        match export_result {
            Ok(dest_path) => {
                exported += 1;
                crate::events::emit(
                    crate::events::event_names::MEDIA_EXPORT_PROGRESS,
                    &crate::events::MediaExportProgressEvent {
                        done: index + 1,
                        total,
                        current_file: file_name_for_progress(&dest_path),
                        exported,
                        skipped,
                        errors,
                    },
                );
            }
            Err(err) => {
                errors += 1;
                tracing::warn!(hash = %hash, error = %err, "export item failed");
                crate::events::emit(
                    crate::events::event_names::MEDIA_EXPORT_PROGRESS,
                    &crate::events::MediaExportProgressEvent {
                        done: index + 1,
                        total,
                        current_file: hash.clone(),
                        exported,
                        skipped,
                        errors,
                    },
                );
            }
        }
    }

    Ok(ExportMediaResult {
        total,
        exported,
        skipped,
        errors,
    })
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

    let ext = mime_to_extension(&file.mime).to_string();
    let h = hash.to_string();
    let bs = blob_store.clone();

    let (regenerated_thumbnail, has_thumbnail) =
        tokio::task::spawn_blocking(move || -> Result<(bool, bool), String> {
            if force {
                bs.delete_thumbnail(&h)
                    .map_err(|e| format!("Delete thumbnail failed: {}", e))?;
            }

            let original = bs
                .find_original(&h, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", h))?;

            if !force {
                let thumb_exists = bs
                    .find_thumbnail_path(&h)
                    .map_err(|e| format!("Thumbnail lookup failed: {}", e))?
                    .is_some();

                if thumb_exists {
                    return Ok((false, true));
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

            Ok((true, true))
        })
        .await
        .map_err(|e| format!("Thumbnail task failed: {}", e))??;

    Ok(EnsureThumbnailResult {
        regenerated_thumbnail,
        has_thumbnail,
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

pub async fn export_file(state: &AppState, input: ExportFileInput) -> Result<(), String> {
    crate::import::pipeline::ImportPipeline::new(&state.db, &state.blob_store)
        .export_file(&input.hash, Path::new(&input.dest_path))
        .await
        .map_err(|e| format!("Export failed: {e}"))?;
    Ok(())
}

pub async fn export_media(state: &AppState, input: ExportMediaInput) -> Result<serde_json::Value, String> {
    let result = export_media_inner(state, input).await?;
    serde_json::to_value(result).map_err(|e| e.to_string())
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
