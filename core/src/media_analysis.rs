use std::path::PathBuf;
use std::sync::Arc;

use crate::background_work::DeferredWorkType;
use crate::blob_store::{mime_to_extension, BlobStore};
use crate::db::{query::ingest::DerivativeTarget, LibraryDatabase};
use crate::media_capabilities::{capabilities_for_stored_media, ThumbnailBackend};
use crate::media_processing::{
    self, encode_thumbnail, get_thumbnail_resolution, ThumbnailScaleType,
    DEFAULT_THUMBNAIL_DIMENSIONS,
};
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::MediaDerivativeField;

pub const TARGET_COLOR_ANALYSIS_VERSION: i64 = 2;

#[derive(Debug, Serialize)]
pub struct EnsureThumbnailResult {
    pub regenerated_thumbnail: bool,
    pub has_thumbnail: bool,
}

#[derive(Debug, Serialize)]
pub struct ReanalyzeFileColorsResult {
    pub colors_extracted: usize,
    pub dominant_color_hex: Option<String>,
}

#[derive(Debug, Default)]
pub struct DerivativeBatchOutcome {
    pub regenerated_thumbnail: bool,
    pub has_thumbnail: bool,
    pub colors_extracted: usize,
    pub dominant_color_hex: Option<String>,
    pub phash_changed: bool,
}

#[derive(Debug)]
struct LoadedRasterSource {
    decoded: image::DynamicImage,
}

#[derive(Debug)]
struct AnalysisContext {
    target: DerivativeTarget,
    original_path: PathBuf,
    caps: crate::media_capabilities::MediaCapabilities,
    thumbnail_exists: bool,
    raster_source: Option<LoadedRasterSource>,
}

fn load_original_path(
    blob_store: &BlobStore,
    target: &DerivativeTarget,
) -> Result<PathBuf, String> {
    let ext = mime_to_extension(&target.mime_type).to_string();
    blob_store
        .find_original(&target.file_hash, Some(&ext))
        .map_err(|e| format!("Blob error: {e}"))?
        .map(|(path, _)| path)
        .ok_or_else(|| format!("Original file not found for hash {}", target.file_hash))
}

fn load_raster_source(original_path: &std::path::Path) -> Result<LoadedRasterSource, String> {
    let bytes =
        std::fs::read(original_path).map_err(|e| format!("Failed to read original file: {e}"))?;
    let decoded =
        image::load_from_memory(&bytes).map_err(|e| format!("Image decode failed: {e}"))?;
    Ok(LoadedRasterSource { decoded })
}

fn render_thumbnail_from_decoded_image(
    decoded: &image::DynamicImage,
) -> Result<(Vec<u8>, String), String> {
    let (tw, th) = get_thumbnail_resolution(
        (decoded.width(), decoded.height()),
        DEFAULT_THUMBNAIL_DIMENSIONS,
        ThumbnailScaleType::ScaleDownOnly,
        100,
    );
    let resized = decoded.resize_exact(tw, th, image::imageops::FilterType::Lanczos3);
    let (bytes, ext) =
        encode_thumbnail(&resized).map_err(|e| format!("Thumbnail encode failed: {e}"))?;
    Ok((bytes, ext.to_string()))
}

fn emit_derivative_state_change(entity_hash: &str, fields: &[MediaDerivativeField]) {
    if fields.is_empty() {
        return;
    }

    crate::events::emit_state_changed(
        "deferred_work_batch",
        ChangeImpact::new()
            .entity_hashes(vec![entity_hash.to_string()])
            .derivative_fields_changed(fields)
            .smart_folder_scopes_changed_for_derivative_fields(fields),
    );
}

fn build_context(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
    work_types: &[DeferredWorkType],
) -> Result<Option<AnalysisContext>, String> {
    let Some(target) = db.get_derivative_target_by_entity_hash(entity_hash)? else {
        return Ok(None);
    };
    let caps = capabilities_for_stored_media(&target.mime_type, target.frame_count);
    let wants_any = work_types.iter().any(|work_type| match work_type {
        DeferredWorkType::Thumbnail => caps.can_thumbnail(),
        DeferredWorkType::DominantColors => caps.can_dominant_colors,
        DeferredWorkType::PerceptualHash => caps.can_perceptual_hash,
    });
    if !wants_any {
        return Ok(None);
    }

    let original_path = load_original_path(blob_store, &target)?;
    let thumbnail_exists = blob_store
        .find_thumbnail_path(&target.file_hash)
        .map_err(|e| format!("Thumbnail lookup failed: {e}"))?
        .is_some();
    let raster_source = if matches!(caps.thumbnail_backend, Some(ThumbnailBackend::Inline))
        && work_types.iter().any(|work_type| {
            matches!(
                work_type,
                DeferredWorkType::Thumbnail
                    | DeferredWorkType::DominantColors
                    | DeferredWorkType::PerceptualHash
            )
        }) {
        Some(load_raster_source(&original_path)?)
    } else {
        None
    };

    Ok(Some(AnalysisContext {
        target,
        original_path,
        caps,
        thumbnail_exists,
        raster_source,
    }))
}

async fn render_thumbnail(context: &AnalysisContext) -> Result<(Vec<u8>, String), String> {
    if let Some(source) = &context.raster_source {
        return render_thumbnail_from_decoded_image(&source.decoded);
    }
    if matches!(
        context.caps.thumbnail_backend,
        Some(ThumbnailBackend::Ffmpeg)
    ) && context.target.mime_type.starts_with("video/")
    {
        let bytes = media_processing::ffmpeg::render_video_thumbnail(
            &context.original_path,
            DEFAULT_THUMBNAIL_DIMENSIONS,
            35,
            context.target.duration_ms.map(|ms| ms as u64),
        )
        .await
        .map_err(|e| format!("Video thumbnail generation failed: {e}"))?;
        return Ok((bytes, "jpg".to_string()));
    }

    let info = media_processing::get_file_info(&context.original_path, None)
        .await
        .map_err(|e| format!("File info failed: {e}"))?;
    media_processing::generate_thumbnail_bytes(
        &context.original_path,
        DEFAULT_THUMBNAIL_DIMENSIONS,
        info.mime,
        info.duration_ms,
        info.num_frames,
        35,
    )
    .await
    .map_err(|e| format!("Thumbnail generation failed: {e}"))
}

async fn analyze_batch(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
    work_types: &[DeferredWorkType],
    force_thumbnail: bool,
    force_colors: bool,
    force_phash: bool,
) -> Result<(DerivativeBatchOutcome, Vec<MediaDerivativeField>), String> {
    let Some(context) = build_context(db, blob_store, entity_hash, work_types)? else {
        return Ok((DerivativeBatchOutcome::default(), Vec::new()));
    };

    let want_thumbnail =
        work_types.contains(&DeferredWorkType::Thumbnail) && context.caps.can_thumbnail();
    let want_colors =
        work_types.contains(&DeferredWorkType::DominantColors) && context.caps.can_dominant_colors;
    let want_phash =
        work_types.contains(&DeferredWorkType::PerceptualHash) && context.caps.can_perceptual_hash;

    let mut outcome = DerivativeBatchOutcome {
        has_thumbnail: context.thumbnail_exists,
        ..Default::default()
    };
    let mut changed_fields = Vec::new();

    if want_thumbnail && (force_thumbnail || !context.thumbnail_exists) {
        let (thumb_bytes, thumb_ext) = render_thumbnail(&context).await?;
        if force_thumbnail {
            let _ = blob_store.delete_thumbnail(&context.target.file_hash);
        }
        blob_store
            .write_thumbnail(&context.target.file_hash, &thumb_bytes, &thumb_ext)
            .map_err(|e| format!("Thumbnail write failed: {e}"))?;
        outcome.regenerated_thumbnail = true;
        outcome.has_thumbnail = true;
        changed_fields.push(MediaDerivativeField::Thumbnail);
    }

    if want_colors
        && (force_colors
            || context.target.color_analysis_version < TARGET_COLOR_ANALYSIS_VERSION
            || !context.target.has_dominant_palette_blob)
    {
        let Some(source) = &context.raster_source else {
            return Err("Dominant color analysis requires decoded image".to_string());
        };
        let palette = media_processing::colors::extract_dominant_colors(&source.decoded, 10);
        let colors: Vec<(String, f32, f32, f32)> = palette
            .iter()
            .map(|c| (c.hex.clone(), c.l as f32, c.a as f32, c.b as f32))
            .collect();
        let dominant_palette_blob =
            media_processing::colors::serialize_dominant_palette_blob(&palette)
                .map_err(|e| format!("Dominant palette serialization failed: {e}"))?;
        let dominant_color_hex = colors.first().map(|(hex, _, _, _)| hex.clone());
        db.replace_file_colors(
            context.target.file_id,
            &colors,
            dominant_color_hex.as_deref(),
            Some(dominant_palette_blob.as_slice()),
            TARGET_COLOR_ANALYSIS_VERSION,
        )?;
        outcome.colors_extracted = colors.len();
        outcome.dominant_color_hex = dominant_color_hex;
        changed_fields.push(MediaDerivativeField::DominantColorHex);
    }

    if want_phash && (force_phash || context.target.perceptual_hash.is_none()) {
        let Some(source) = &context.raster_source else {
            return Err("Perceptual hash analysis requires decoded image".to_string());
        };
        let phash_b64 = crate::duplicates::phash::compute_phash_base64_from_image(&source.decoded)
            .map_err(|e| format!("{e}"))?;
        db.replace_file_phash(context.target.file_id, Some(&phash_b64))?;
        outcome.phash_changed = true;
        changed_fields.push(MediaDerivativeField::Phash);
    }

    Ok((outcome, changed_fields))
}

pub fn derivative_work_types_for_target(
    mime: &str,
    frame_count: Option<i64>,
    needs_thumbnail: bool,
) -> Vec<DeferredWorkType> {
    let caps = capabilities_for_stored_media(mime, frame_count);
    let mut work_types = Vec::new();
    if needs_thumbnail && caps.can_thumbnail() {
        work_types.push(DeferredWorkType::Thumbnail);
    }
    if caps.can_dominant_colors {
        work_types.push(DeferredWorkType::DominantColors);
    }
    if caps.can_perceptual_hash {
        work_types.push(DeferredWorkType::PerceptualHash);
    }
    work_types
}

pub fn enqueue_derivative_jobs(
    db: &LibraryDatabase,
    entity_hash: &str,
    mime: &str,
    frame_count: Option<i64>,
    needs_thumbnail: bool,
) -> Result<(), String> {
    let work_types = derivative_work_types_for_target(mime, frame_count, needs_thumbnail);
    db.enqueue_deferred_jobs(entity_hash, &work_types)
}

pub async fn enqueue_missing_derivative_jobs(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hashes: &[String],
) {
    for entity_hash in entity_hashes {
        let target = match db.get_derivative_target_by_entity_hash(entity_hash) {
            Ok(Some(target)) => target,
            _ => continue,
        };
        let caps = capabilities_for_stored_media(&target.mime_type, target.frame_count);
        let thumb_missing = blob_store
            .find_thumbnail_path(&target.file_hash)
            .ok()
            .flatten()
            .is_none();

        let mut work_types = Vec::new();
        if caps.can_thumbnail() && thumb_missing {
            work_types.push(DeferredWorkType::Thumbnail);
        }
        if caps.can_dominant_colors
            && (target.color_analysis_version < TARGET_COLOR_ANALYSIS_VERSION
                || !target.has_dominant_palette_blob)
        {
            work_types.push(DeferredWorkType::DominantColors);
        }
        if caps.can_perceptual_hash && target.perceptual_hash.is_none() {
            work_types.push(DeferredWorkType::PerceptualHash);
        }

        if !work_types.is_empty() {
            let _ = db.enqueue_deferred_jobs(entity_hash, &work_types);
        }
    }
}

pub async fn ensure_thumbnail_now(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
    force: bool,
) -> Result<EnsureThumbnailResult, String> {
    let (outcome, _) = analyze_batch(
        db,
        blob_store,
        entity_hash,
        &[DeferredWorkType::Thumbnail],
        force,
        false,
        false,
    )
    .await?;
    Ok(EnsureThumbnailResult {
        regenerated_thumbnail: outcome.regenerated_thumbnail,
        has_thumbnail: outcome.has_thumbnail,
    })
}

pub async fn reanalyze_file_colors_now(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
) -> Result<ReanalyzeFileColorsResult, String> {
    let (outcome, _) = analyze_batch(
        db,
        blob_store,
        entity_hash,
        &[DeferredWorkType::DominantColors],
        false,
        true,
        false,
    )
    .await?;
    Ok(ReanalyzeFileColorsResult {
        colors_extracted: outcome.colors_extracted,
        dominant_color_hex: outcome.dominant_color_hex,
    })
}

pub fn enqueue_stale_color_backfill(db: &LibraryDatabase) -> Result<usize, String> {
    db.enqueue_stale_color_analysis_jobs(TARGET_COLOR_ANALYSIS_VERSION)
}

pub async fn process_deferred_batch(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    jobs: &[crate::db::types::ClaimedDeferredWorkItem],
) -> Result<(), String> {
    if jobs.is_empty() {
        return Ok(());
    }

    let entity_hash = jobs[0].entity_hash.clone();
    let job_types: Vec<DeferredWorkType> = jobs
        .iter()
        .filter_map(|job| DeferredWorkType::from_db_str(&job.work_type))
        .collect();

    match analyze_batch(
        db,
        blob_store,
        &entity_hash,
        &job_types,
        false,
        false,
        false,
    )
    .await
    {
        Ok((_, changed_fields)) => {
            for job in jobs {
                db.complete_deferred_work_item(job.work_id)?;
            }
            emit_derivative_state_change(&entity_hash, &changed_fields);
            Ok(())
        }
        Err(error) => {
            for job in jobs {
                db.retry_deferred_work_item(job.work_id, job.attempt_count + 1, &error)?;
            }
            Err(error)
        }
    }
}

use serde::Serialize;

#[cfg(test)]
mod tests {
    use super::derivative_work_types_for_target;
    use crate::background_work::DeferredWorkType;

    #[test]
    fn static_raster_image_gets_full_analysis_bundle() {
        let work = derivative_work_types_for_target("image/png", Some(1), true);
        assert_eq!(
            work,
            vec![
                DeferredWorkType::Thumbnail,
                DeferredWorkType::DominantColors,
                DeferredWorkType::PerceptualHash
            ]
        );
    }

    #[test]
    fn video_only_gets_thumbnail_work() {
        let work = derivative_work_types_for_target("video/mp4", None, true);
        assert_eq!(work, vec![DeferredWorkType::Thumbnail]);
    }

    #[test]
    fn animated_image_only_gets_thumbnail_work() {
        let work = derivative_work_types_for_target("image/gif", Some(12), true);
        assert_eq!(work, vec![DeferredWorkType::Thumbnail]);
    }
}
