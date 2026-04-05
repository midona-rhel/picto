//! Media I/O surface: asset paths, shell open/reveal, exports, and derivative actions.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use ts_rs::TS;

use crate::blob_store::{mime_to_extension, BlobStore};
use crate::db::types::{EntityTarget, QueryPage};
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::MediaDerivativeField;

use super::{target, ApplicationEngine};

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ExportMediaResult {
    pub total: usize,
    pub exported: usize,
    pub skipped: usize,
    pub errors: usize,
}

#[derive(Debug, Clone)]
pub struct ExportMediaRequest {
    pub target: EntityTarget,
    pub output_dir: PathBuf,
    pub format: Option<String>,
    pub quality: Option<u8>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub keep_aspect: bool,
}

type EnsureThumbnailResult = crate::background_work::EnsureThumbnailResult;
type ReanalyzeFileColorsResult = crate::background_work::ReanalyzeFileColorsResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Original,
    Png,
    Jpeg,
    Webp,
    Avif,
}

impl ApplicationEngine {
    pub async fn resolve_file_path(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hash: &str,
    ) -> Result<String, String> {
        let _ = crate::background_work::ensure_missing_color_analysis_jobs(
            &self.db,
            &[entity_hash.to_string()],
        );
        let target = self
            .db
            .get_derivative_target_by_entity_hash(entity_hash)?
            .ok_or_else(|| format!("Entity not found: {entity_hash}"))?;
        let ext = mime_to_extension(&target.mime_type).to_string();
        let bs = blob_store.clone();
        let file_hash = target.file_hash;
        tokio::task::spawn_blocking(move || {
            bs.find_original(&file_hash, Some(&ext))
                .map_err(|e| format!("Blob error: {e}"))?
                .map(|(path, _)| path.to_string_lossy().into_owned())
                .ok_or_else(|| format!("File not found in blob store: {file_hash}"))
        })
        .await
        .map_err(|e| format!("Task error: {e}"))?
    }

    pub async fn resolve_file_paths(
        &self,
        blob_store: &Arc<BlobStore>,
        target: EntityTarget,
    ) -> Result<Vec<String>, String> {
        let hashes = self.resolve_target_hashes(target)?;
        let _ = crate::background_work::ensure_missing_color_analysis_jobs(&self.db, &hashes);
        let mut paths = Vec::with_capacity(hashes.len());
        for hash in hashes {
            if let Ok(path) = self.resolve_file_path(blob_store, &hash).await {
                paths.push(path);
            }
        }
        Ok(paths)
    }

    pub async fn open_file_default(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hash: &str,
    ) -> Result<(), String> {
        let path = self.resolve_file_path(blob_store, entity_hash).await?;
        open::that(&path).map_err(|e| format!("Failed to open file: {e}"))?;
        Ok(())
    }

    pub async fn reveal_in_folder(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hash: &str,
    ) -> Result<(), String> {
        let path = self.resolve_file_path(blob_store, entity_hash).await?;
        reveal_in_folder_os(&path)?;
        Ok(())
    }

    pub async fn export_file(
        &self,
        blob_store: &Arc<BlobStore>,
        library_root: &Path,
        entity_hash: &str,
        dest: &Path,
    ) -> Result<(), String> {
        reject_library_export_path(library_root, dest)?;
        let target = self
            .db
            .get_derivative_target_by_entity_hash(entity_hash)?
            .ok_or_else(|| format!("Entity not found: {entity_hash}"))?;
        let ext = mime_to_extension(&target.mime_type).to_string();
        let blob_store = blob_store.clone();
        let file_hash = target.file_hash;
        let data = tokio::task::spawn_blocking(move || blob_store.read_original(&file_hash, Some(&ext)))
            .await
            .map_err(|e| format!("Export read task failed: {e}"))?
            .map_err(|e| format!("Export read failed: {e}"))?;
        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create export directory: {e}"))?;
        }
        tokio::fs::write(dest, data)
            .await
            .map_err(|e| format!("Failed to write export: {e}"))?;
        Ok(())
    }

    pub async fn export_media(
        &self,
        blob_store: &Arc<BlobStore>,
        library_root: &Path,
        request: ExportMediaRequest,
    ) -> Result<ExportMediaResult, String> {
        let hashes = self.resolve_target_hashes(request.target)?;
        let total = hashes.len();
        reject_library_export_path(library_root, &request.output_dir)?;

        let format = parse_export_format(request.format.as_deref())?;
        let quality = request.quality.unwrap_or(82).clamp(1, 100);

        tokio::fs::create_dir_all(&request.output_dir)
            .await
            .map_err(|e| format!("Failed to create export directory: {e}"))?;

        let mut exported = 0usize;
        let skipped = 0usize;
        let mut errors = 0usize;

        for (index, hash) in hashes.iter().enumerate() {
            let export_result = async {
                let record = self
                    .db
                    .get_derivative_target_by_entity_hash(hash)?
                    .ok_or_else(|| format!("Entity not found: {hash}"))?;

                let original_ext = mime_to_extension(&record.mime_type).to_string();
                let blob_store = blob_store.clone();
                let file_hash = record.file_hash.clone();
                let original_data = tokio::task::spawn_blocking(move || {
                    blob_store.read_original(&file_hash, Some(&original_ext))
                })
                .await
                .map_err(|e| format!("Export read task failed: {e}"))?
                .map_err(|e| format!("Export read failed: {e}"))?;

                let display_name = self
                    .db
                    .get_entity_details(hash)?
                    .and_then(|details| details.name)
                    .unwrap_or_default();
                let stem = sanitize_file_stem(&display_name, &hash[..12]);
                let dest_ext = extension_for_export_format(format, &record.mime_type);
                let dest_path = ensure_unique_dest_path(&request.output_dir, &stem, dest_ext);

                if format == ExportFormat::Original {
                    tokio::fs::write(&dest_path, &original_data)
                        .await
                        .map_err(|e| format!("Failed to write export: {e}"))?;
                    return Ok(dest_path);
                }

                if !record.mime_type.starts_with("image/") {
                    return Err(format!(
                        "Format conversion is only supported for image files: {}",
                        if display_name.is_empty() { hash } else { &display_name }
                    ));
                }

                let width = request.width;
                let height = request.height;
                let keep_aspect = request.keep_aspect;
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

    pub async fn resolve_thumbnail_path(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hash: &str,
    ) -> Result<String, String> {
        let thumbnail_hash = self
            .db
            .get_entity_details(entity_hash)?
            .map(|details| details.thumbnail_hash)
            .ok_or_else(|| format!("Entity not found: {entity_hash}"))?;
        let blob_store = blob_store.clone();
        tokio::task::spawn_blocking(move || {
            blob_store
                .find_thumbnail_path(&thumbnail_hash)
                .map_err(|e| format!("Blob error: {e}"))?
                .map(|path| path.to_string_lossy().into_owned())
                .ok_or_else(|| format!("Thumbnail not found: {thumbnail_hash}"))
        })
        .await
        .map_err(|e| format!("Task error: {e}"))?
    }

    pub async fn ensure_thumbnail(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hash: &str,
    ) -> Result<EnsureThumbnailResult, String> {
        let result =
            crate::background_work::ensure_thumbnail_now(&self.db, blob_store, entity_hash, false)
                .await?;
        if result.regenerated_thumbnail {
            crate::events::emit_state_changed(
                "ensure_thumbnail",
                ChangeImpact::new()
                    .entity_hashes(vec![entity_hash.to_string()])
                    .derivative_fields_changed(&[MediaDerivativeField::Thumbnail]),
            );
        }
        Ok(result)
    }

    pub async fn regenerate_thumbnail(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hash: &str,
    ) -> Result<EnsureThumbnailResult, String> {
        let result =
            crate::background_work::ensure_thumbnail_now(&self.db, blob_store, entity_hash, true)
                .await?;
        if result.regenerated_thumbnail {
            crate::events::emit_state_changed(
                "regenerate_thumbnail",
                ChangeImpact::new()
                    .entity_hashes(vec![entity_hash.to_string()])
                    .derivative_fields_changed(&[MediaDerivativeField::Thumbnail]),
            );
        }
        Ok(result)
    }

    pub async fn regenerate_thumbnails_batch(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hashes: &[String],
    ) -> Result<ExportMediaResult, String> {
        let mut regenerated = 0usize;
        let mut errors = 0usize;
        let mut changed_hashes = Vec::new();
        for hash in entity_hashes {
            match crate::background_work::ensure_thumbnail_now(&self.db, blob_store, hash, true)
                .await
            {
                Ok(result) => {
                    if result.regenerated_thumbnail {
                        regenerated += 1;
                        changed_hashes.push(hash.clone());
                    }
                }
                Err(_) => {
                    errors += 1;
                }
            }
        }
        if !changed_hashes.is_empty() {
            crate::events::emit_state_changed(
                "regenerate_thumbnails_batch",
                ChangeImpact::new()
                    .entity_hashes(changed_hashes)
                    .derivative_fields_changed(&[MediaDerivativeField::Thumbnail]),
            );
        }
        Ok(ExportMediaResult {
            total: entity_hashes.len(),
            exported: regenerated,
            skipped: 0,
            errors,
        })
    }

    pub async fn reanalyze_file_colors(
        &self,
        blob_store: &Arc<BlobStore>,
        entity_hash: &str,
    ) -> Result<ReanalyzeFileColorsResult, String> {
        let result =
            crate::background_work::reanalyze_file_colors_now(&self.db, blob_store, entity_hash)
                .await?;
        crate::events::emit_state_changed(
            "reanalyze_file_colors",
            ChangeImpact::new()
                .entity_hashes(vec![entity_hash.to_string()])
                .derivative_fields_changed(&[MediaDerivativeField::DominantColorHex])
                .smart_folder_scopes_changed_for_derivative_fields(&[
                    MediaDerivativeField::DominantColorHex,
                ]),
        );
        Ok(result)
    }

    fn resolve_target_hashes(&self, target: EntityTarget) -> Result<Vec<String>, String> {
        let resolved = target::resolve(&self.db, &target)?;
        match resolved {
            target::ResolvedTarget::Ids(ids) => self.db.get_entity_hashes_by_ids(&ids),
            target::ResolvedTarget::Query {
                mut view_query,
                exclusions,
            } => {
                view_query.page = QueryPage {
                    limit: i64::MAX,
                    cursor: None,
                };
                let excluded: HashSet<&str> = exclusions.iter().map(String::as_str).collect();
                Ok(self
                    .db
                    .query_entity_view(&view_query)?
                    .items
                    .into_iter()
                    .map(|item| item.entity_hash)
                    .filter(|hash| !excluded.contains(hash.as_str()))
                    .collect())
            }
        }
    }
}

fn reveal_in_folder_os(path: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn()
            .map_err(|e| format!("Failed to reveal in Finder: {e}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| format!("Failed to reveal in Explorer: {e}"))?;
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| format!("Failed to open folder: {e}"))?;
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
    let cleaned = cleaned_raw.trim().trim_matches('.');
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

fn file_name_for_progress(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn reject_library_export_path(library_root: &Path, dest: &Path) -> Result<(), String> {
    if let Ok(canonical) = dest.canonicalize().or_else(|_| {
        dest.parent()
            .and_then(|p| p.canonicalize().ok())
            .ok_or(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no parent",
            ))
    }) {
        if canonical.starts_with(library_root) {
            return Err(format!(
                "Cannot export to a path inside the library directory: {}",
                canonical.display()
            ));
        }
    }
    Ok(())
}
