use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::blob_store::{mime_to_extension, BlobStore};
use crate::db::LibraryDatabase;
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::MediaDerivativeField;
use crate::sqlite::{ReadModelEvent, SqliteDatabase};

// Legacy note:
// The rebuilt live app path now uses `crate::background_work` as the canonical
// deferred/background work boundary. This module remains for legacy queue
// compatibility and older maintenance helpers that still operate on
// `SqliteDatabase`.

const DEFERRED_WORK_TICK: std::time::Duration = std::time::Duration::from_secs(5);
const MAX_BACKOFF_SECS: i64 = 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeferredWorkType {
    Thumbnail,
    DominantColors,
    Phash,
}

impl DeferredWorkType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumbnail",
            Self::DominantColors => "dominant_colors",
            Self::Phash => "phash",
        }
    }

    fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "thumbnail" => Some(Self::Thumbnail),
            "dominant_colors" => Some(Self::DominantColors),
            "phash" => Some(Self::Phash),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct DeferredWorkItem {
    work_id: i64,
    hash: String,
    work_type: DeferredWorkType,
    attempt_count: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct EnsureThumbnailResult {
    pub regenerated_thumbnail: bool,
    pub has_thumbnail: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct ReanalyzeFileColorsResult {
    pub colors_extracted: usize,
    pub dominant_color_hex: Option<String>,
}

fn compute_retry_available_at(next_attempt: i64) -> String {
    let exp = (next_attempt.saturating_sub(1)).clamp(0, 10) as u32;
    let delay_secs = (30_i64.saturating_mul(1_i64 << exp)).min(MAX_BACKOFF_SECS);
    (Utc::now() + ChronoDuration::seconds(delay_secs)).to_rfc3339()
}

fn enqueue_deferred_jobs_sync(
    conn: &Connection,
    hash: &str,
    work_types: &[DeferredWorkType],
) -> rusqlite::Result<()> {
    if work_types.is_empty() {
        return Ok(());
    }

    let now = Utc::now().to_rfc3339();
    let mut stmt = conn.prepare(
        "INSERT INTO deferred_work
             (hash, work_type, status, attempt_count, available_at, last_error, created_at, updated_at)
         VALUES
             (?1, ?2, 'pending', 0, ?3, NULL, ?3, ?3)
         ON CONFLICT(hash, work_type) DO UPDATE SET
             status = 'pending',
             attempt_count = 0,
             available_at = excluded.available_at,
             last_error = NULL,
             updated_at = excluded.updated_at",
    )?;

    for work_type in work_types {
        stmt.execute(params![hash, work_type.as_str(), now])?;
    }

    Ok(())
}

fn reset_running_deferred_work_sync(conn: &Connection) -> rusqlite::Result<usize> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE deferred_work
         SET status = 'pending',
             updated_at = ?1
         WHERE status = 'running'",
        [now],
    )
}

/// Claim all pending jobs for the next available hash.
/// Returns all work items for that single hash so they can be processed together.
fn claim_next_hash_jobs_sync(conn: &mut Connection) -> rusqlite::Result<Vec<DeferredWorkItem>> {
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();

    // Find the next hash that has pending work
    let next_hash: Option<String> = {
        let mut stmt = tx.prepare(
            "SELECT hash FROM deferred_work
             WHERE status = 'pending' AND available_at <= ?1
             ORDER BY work_id ASC LIMIT 1",
        )?;
        stmt.query_row([&now], |row| row.get(0)).optional()?
    };

    let Some(hash) = next_hash else {
        tx.commit()?;
        return Ok(Vec::new());
    };

    // Claim ALL jobs for this hash
    let mut stmt = tx.prepare(
        "SELECT work_id, hash, work_type, attempt_count
         FROM deferred_work
         WHERE hash = ?1 AND status = 'pending' AND available_at <= ?2
         ORDER BY work_id ASC",
    )?;
    let raw_items = stmt
        .query_map(params![hash, now], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);

    let mut items = Vec::with_capacity(raw_items.len());
    for (work_id, hash, work_type, attempt_count) in raw_items {
        let Some(work_type) = DeferredWorkType::from_str(&work_type) else {
            tx.execute("DELETE FROM deferred_work WHERE work_id = ?1", [work_id])?;
            continue;
        };
        tx.execute(
            "UPDATE deferred_work
             SET status = 'running', updated_at = ?2
             WHERE work_id = ?1",
            params![work_id, now],
        )?;
        items.push(DeferredWorkItem {
            work_id,
            hash,
            work_type,
            attempt_count,
        });
    }

    tx.commit()?;
    Ok(items)
}

fn complete_deferred_work_sync(conn: &Connection, work_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM deferred_work WHERE work_id = ?1", [work_id])?;
    Ok(())
}

fn retry_deferred_work_sync(
    conn: &Connection,
    work_id: i64,
    next_attempt: i64,
    error: &str,
) -> rusqlite::Result<()> {
    let now = Utc::now().to_rfc3339();
    let available_at = compute_retry_available_at(next_attempt);
    conn.execute(
        "UPDATE deferred_work
         SET status = 'pending',
             attempt_count = ?2,
             available_at = ?3,
             last_error = ?4,
             updated_at = ?5
         WHERE work_id = ?1",
        params![work_id, next_attempt, available_at, error, now],
    )?;
    Ok(())
}

pub async fn enqueue_import_derivatives(
    db: &SqliteDatabase,
    hash: &str,
    mime: &str,
    needs_thumbnail: bool,
) -> Result<(), String> {
    // Skip collection entities — they don't have their own file data.
    // Thumbnails come from the cover file which is enqueued separately.
    if mime == "application/x-collection" {
        return Ok(());
    }
    let mut work_types = Vec::new();
    if needs_thumbnail && (mime.starts_with("image/") || mime.starts_with("video/")) {
        work_types.push(DeferredWorkType::Thumbnail);
    }
    if mime.starts_with("image/") {
        work_types.push(DeferredWorkType::DominantColors);
        work_types.push(DeferredWorkType::Phash);
    }
    db.enqueue_deferred_jobs(hash, &work_types).await
}

pub async fn enqueue_missing_deferred(
    db: &SqliteDatabase,
    blob_store: &Arc<BlobStore>,
    hashes: &[String],
) {
    for hash in hashes {
        let file = match db.get_file_by_hash(hash).await {
            Ok(Some(file)) => file,
            _ => continue,
        };

        let mut work_types = Vec::new();
        if (file.mime.starts_with("image/") || file.mime.starts_with("video/"))
            && blob_store
                .find_thumbnail_path(hash)
                .ok()
                .flatten()
                .is_none()
        {
            work_types.push(DeferredWorkType::Thumbnail);
        }
        if file.mime.starts_with("image/") && file.dominant_color_hex.is_none() {
            work_types.push(DeferredWorkType::DominantColors);
        }
        if file.mime.starts_with("image/") && file.phash.is_none() {
            work_types.push(DeferredWorkType::Phash);
        }

        if !work_types.is_empty() {
            let _ = db.enqueue_deferred_jobs(hash, &work_types).await;
        }
    }
}

pub async fn ensure_thumbnail(
    db: &SqliteDatabase,
    blob_store: &Arc<BlobStore>,
    hash: &str,
    force: bool,
) -> Result<EnsureThumbnailResult, String> {
    let file = match db.get_file_by_hash(hash).await? {
        Some(f) => f,
        None => {
            // File was deleted between enqueue and processing — skip gracefully
            return Ok(EnsureThumbnailResult {
                regenerated_thumbnail: false,
                has_thumbnail: false,
            });
        }
    };

    let ext = mime_to_extension(&file.mime).to_string();
    let effective_hash = file.hash.clone();
    let bs = blob_store.clone();

    let (regenerated_thumbnail, has_thumbnail) = tokio::spawn(async move {
        let result: Result<(bool, bool), String> = (async {
            let t0 = std::time::Instant::now();
            if force {
                bs.delete_thumbnail(&effective_hash)
                    .map_err(|e| format!("Delete thumbnail failed: {}", e))?;
            }

            let original = bs
                .find_original(&effective_hash, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", effective_hash))?;

            if !force {
                let thumb_exists = bs
                    .find_thumbnail_path(&effective_hash)
                    .map_err(|e| format!("Thumbnail lookup failed: {}", e))?
                    .is_some();
                if thumb_exists {
                    return Ok((false, true));
                }
            }
            let t_lookup = t0.elapsed().as_millis() as u64;

            let info = match crate::media_processing::get_file_info(&original.0, None).await {
                Ok(info) => info,
                Err(e) => {
                    debug!(hash = %effective_hash, error = %e, "thumbnail skipped: file info failed");
                    return Ok((false, false));
                }
            };
            let t_info = t0.elapsed().as_millis() as u64;

            let (thumb_bytes, thumb_ext) = match crate::media_processing::generate_thumbnail_bytes(
                &original.0,
                crate::media_processing::DEFAULT_THUMBNAIL_DIMENSIONS,
                info.mime,
                info.duration_ms,
                info.num_frames,
                35,
            )
            .await
            {
                Ok(result) => result,
                Err(e) => {
                    debug!(hash = %effective_hash, mime = ?info.mime, error = %e, "thumbnail skipped: no adapter");
                    return Ok((false, false));
                }
            };
            let t_generate = t0.elapsed().as_millis() as u64;

            bs.write_thumbnail(&effective_hash, &thumb_bytes, &thumb_ext)
                .map_err(|e| format!("Thumbnail write failed: {}", e))?;
            let t_write = t0.elapsed().as_millis() as u64;

            info!(
                hash = %effective_hash,
                lookup_ms = t_lookup,
                file_info_ms = t_info - t_lookup,
                generate_ms = t_generate - t_info,
                write_ms = t_write - t_generate,
                total_ms = t_write,
                thumb_size = thumb_bytes.len(),
                "thumbnail: timing breakdown"
            );
            Ok((true, true))
        })
        .await;
        result
    })
    .await
    .map_err(|e| format!("Thumbnail task failed: {}", e))??;

    Ok(EnsureThumbnailResult {
        regenerated_thumbnail,
        has_thumbnail,
    })
}

pub async fn reanalyze_file_colors(
    db: &SqliteDatabase,
    blob_store: &Arc<BlobStore>,
    hash: &str,
) -> Result<ReanalyzeFileColorsResult, String> {
    let file = match db.get_file_by_hash(hash).await? {
        Some(file) => file,
        None => {
            return Ok(ReanalyzeFileColorsResult {
                colors_extracted: 0,
                dominant_color_hex: None,
            });
        }
    };

    if !file.mime.starts_with("image/") {
        db.set_file_colors(hash, Vec::new(), None).await?;
        return Ok(ReanalyzeFileColorsResult {
            colors_extracted: 0,
            dominant_color_hex: None,
        });
    }

    let hash_owned = hash.to_string();
    let bs = blob_store.clone();
    let ext = mime_to_extension(&file.mime).to_string();
    let colors =
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, f32, f32, f32)>, String> {
            let t0 = std::time::Instant::now();
            let used_thumbnail;
            let bytes = if let Ok(Some(thumb_path)) = bs.find_thumbnail_path(&hash_owned) {
                used_thumbnail = true;
                std::fs::read(&thumb_path)
                    .map_err(|e| format!("Failed to read thumbnail: {}", e))?
            } else {
                used_thumbnail = false;
                let original = bs
                    .find_original(&hash_owned, Some(&ext))
                    .map_err(|e| format!("Blob error: {}", e))?
                    .ok_or_else(|| format!("Original file not found for hash {}", hash_owned))?;
                std::fs::read(&original.0)
                    .map_err(|e| format!("Failed to read original file: {}", e))?
            };
            let t_read = t0.elapsed().as_millis() as u64;
            let img = image::load_from_memory(&bytes)
                .map_err(|e| format!("Image decode failed: {}", e))?;
            let t_decode = t0.elapsed().as_millis() as u64;
            let extracted = crate::media_processing::colors::extract_dominant_colors(&img, 8);
            let t_extract = t0.elapsed().as_millis() as u64;
            tracing::info!(
                hash = %hash_owned,
                used_thumbnail,
                read_ms = t_read,
                decode_ms = t_decode - t_read,
                extract_ms = t_extract - t_decode,
                total_ms = t_extract,
                colors = extracted.len(),
                "colors: timing breakdown"
            );
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

    Ok(ReanalyzeFileColorsResult {
        colors_extracted,
        dominant_color_hex,
    })
}

pub async fn ensure_phash(
    db: &SqliteDatabase,
    blob_store: &Arc<BlobStore>,
    hash: &str,
    force: bool,
) -> Result<bool, String> {
    let file = match db.get_file_by_hash(hash).await? {
        Some(file) => file,
        None => return Ok(false),
    };

    if !file.mime.starts_with("image/") {
        return Ok(false);
    }
    if !force && file.phash.is_some() {
        return Ok(false);
    }

    let ext = mime_to_extension(&file.mime).to_string();
    let hash_owned = hash.to_string();
    let bs = blob_store.clone();
    let phash_b64 = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let original = bs
            .find_original(&hash_owned, Some(&ext))
            .map_err(|e| format!("{e}"))?
            .ok_or_else(|| format!("Original file not found for hash {}", hash_owned))?;
        let bytes = std::fs::read(&original.0).map_err(|e| format!("{e}"))?;
        crate::duplicates::phash::compute_phash_base64(&bytes).map_err(|e| format!("{e}"))
    })
    .await
    .map_err(|e| format!("Phash task failed: {}", e))??;

    db.set_phash(hash, &phash_b64).await?;
    Ok(true)
}

/// Process all deferred work for a single image, then emit immediately.
/// Returns the number of jobs processed (0 = nothing pending).
async fn drain_next_image(
    db: &SqliteDatabase,
    blob_store: &Arc<BlobStore>,
) -> Result<usize, String> {
    let jobs = db.claim_next_hash_jobs().await?;
    if jobs.is_empty() {
        return Ok(0);
    }

    let image_started = std::time::Instant::now();
    let hash = jobs[0].hash.clone();
    let job_count = jobs.len();

    // If the file/entity no longer exists, complete all jobs silently.
    let file_exists = db.get_file_by_hash(&hash).await.ok().flatten().is_some();
    if !file_exists {
        let entity_exists = db.resolve_hash(&hash).await.is_ok();
        if !entity_exists {
            debug!(hash = %hash, "Deferred work skipped: entity no longer exists");
            for job in &jobs {
                let _ = db.complete_deferred_work(job.work_id).await;
            }
            return Ok(job_count);
        }
    }

    let mut fields: Vec<MediaDerivativeField> = Vec::new();
    let mut any_changed = false;

    // Sort jobs: thumbnail first (so colors can use the thumbnail file),
    // then colors, then phash. This ensures the thumbnail exists on disk
    // before color extraction tries to read it.
    let mut jobs = jobs;
    jobs.sort_by_key(|j| match j.work_type {
        DeferredWorkType::Thumbnail => 0,
        DeferredWorkType::DominantColors => 1,
        DeferredWorkType::Phash => 2,
    });

    // Pre-load and decode the original image once if both thumbnail and phash
    // are in the job set. Avoids decoding the full original twice (~4s saved
    // per 22MP image).
    let has_thumbnail = jobs
        .iter()
        .any(|j| j.work_type == DeferredWorkType::Thumbnail);
    let has_phash = jobs.iter().any(|j| j.work_type == DeferredWorkType::Phash);
    let shared_decoded: Option<Arc<image::DynamicImage>> = if has_thumbnail && has_phash {
        let decode_started = std::time::Instant::now();
        let file = db.get_file_by_hash(&hash).await.ok().flatten();
        if let Some(ref file) = file {
            if file.mime.starts_with("image/") {
                let ext = mime_to_extension(&file.mime).to_string();
                let h = hash.clone();
                let bs = blob_store.clone();
                let decoded =
                    tokio::task::spawn_blocking(move || -> Option<image::DynamicImage> {
                        let original = bs.find_original(&h, Some(&ext)).ok()??;
                        let bytes = std::fs::read(&original.0).ok()?;
                        image::load_from_memory(&bytes).ok()
                    })
                    .await
                    .ok()
                    .flatten();
                info!(
                    hash = %hash,
                    elapsed_ms = decode_started.elapsed().as_millis() as u64,
                    success = decoded.is_some(),
                    "Deferred work: shared decode"
                );
                decoded.map(Arc::new)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    for job in &jobs {
        let started = std::time::Instant::now();
        let result: Result<Option<MediaDerivativeField>, String> =
            match job.work_type {
                DeferredWorkType::Thumbnail => {
                    // Thumbnail uses its own path-based pipeline (handles video too).
                    match ensure_thumbnail(db, blob_store, &job.hash, false).await {
                        Ok(r) => Ok(r
                            .regenerated_thumbnail
                            .then_some(MediaDerivativeField::Thumbnail)),
                        Err(e) => Err(e),
                    }
                }
                DeferredWorkType::DominantColors => {
                    // Colors already use the thumbnail file (fast).
                    match reanalyze_file_colors(db, blob_store, &job.hash).await {
                        Ok(r) => Ok((r.colors_extracted > 0)
                            .then_some(MediaDerivativeField::DominantColorHex)),
                        Err(e) => Err(e),
                    }
                }
                DeferredWorkType::Phash => {
                    // Use the shared pre-decoded image if available, otherwise fall back.
                    if let Some(ref img) = shared_decoded {
                        let img_ref = img.clone();
                        let job_hash = job.hash.clone();
                        let phash_result = tokio::task::spawn_blocking(move || {
                            let t0 = std::time::Instant::now();
                            let result =
                                crate::duplicates::phash::compute_phash_base64_from_image(&img_ref)
                                    .map_err(|e| format!("{e}"));
                            let hash_ms = t0.elapsed().as_millis() as u64;
                            tracing::info!(
                                hash = %job_hash,
                                hash_ms,
                                shared_decode = true,
                                "phash: timing breakdown"
                            );
                            result
                        })
                        .await
                        .map_err(|e| format!("Phash task failed: {e}"))?;
                        match phash_result {
                            Ok(phash_b64) => {
                                db.set_phash(&job.hash, &phash_b64).await?;
                                Ok(Some(MediaDerivativeField::Phash))
                            }
                            Err(e) => Err(e),
                        }
                    } else {
                        match ensure_phash(db, blob_store, &job.hash, false).await {
                            Ok(changed) => Ok(changed.then_some(MediaDerivativeField::Phash)),
                            Err(e) => Err(e),
                        }
                    }
                }
            };

        let elapsed_ms = started.elapsed().as_millis() as u64;
        let ok = result.is_ok();
        info!(
            hash = %job.hash,
            step = job.work_type.as_str(),
            elapsed_ms,
            ok,
            "Deferred work step"
        );

        match result {
            Ok(changed_field) => {
                db.complete_deferred_work(job.work_id).await?;
                if let Some(field) = changed_field {
                    any_changed = true;
                    if !fields.contains(&field) {
                        fields.push(field);
                    }
                }
            }
            Err(error) => {
                let next_attempt = job.attempt_count + 1;
                db.retry_deferred_work(job.work_id, next_attempt, &error)
                    .await?;
                warn!(
                    hash = %job.hash,
                    work_type = job.work_type.as_str(),
                    attempt = next_attempt,
                    error = %error,
                    "Deferred work failed"
                );
            }
        }
    }

    let image_elapsed_ms = image_started.elapsed().as_millis() as u64;
    info!(
        hash = %hash,
        jobs = job_count,
        elapsed_ms = image_elapsed_ms,
        changed = any_changed,
        "Deferred work: image complete"
    );

    // Emit immediately for this one image so the grid updates right away.
    if any_changed && !fields.is_empty() {
        db.emit_read_model_event(ReadModelEvent::RebuildAll);
        crate::events::emit_state_changed(
            "deferred_work_batch",
            ChangeImpact::new()
                .entity_hashes(vec![hash])
                .derivative_fields_changed(&fields)
                .smart_folder_scopes_changed_for_derivative_fields(&fields),
        );
    }

    Ok(job_count)
}

pub async fn start_deferred_work_loop(
    db: Arc<SqliteDatabase>,
    blob_store: Arc<BlobStore>,
    cancel: CancellationToken,
) {
    if let Err(error) = db.reset_running_deferred_work().await {
        warn!(error = %error, "Deferred work reset failed");
    }

    loop {
        match drain_next_image(&db, &blob_store).await {
            Ok(processed) if processed > 0 => continue, // More images may be waiting
            Ok(_) => {}
            Err(error) => warn!(error = %error, "Deferred work drain failed"),
        }

        tokio::select! {
            _ = tokio::time::sleep(DEFERRED_WORK_TICK) => {}
            _ = cancel.cancelled() => {
                debug!("Deferred work loop cancelled");
                return;
            }
        }
    }
}

pub fn enqueue_import_derivatives_canonical(
    db: &LibraryDatabase,
    entity_hash: &str,
    mime: &str,
    frame_count: Option<i64>,
    needs_thumbnail: bool,
) -> Result<(), String> {
    crate::background_work::enqueue_derivative_jobs(
        db,
        entity_hash,
        mime,
        frame_count,
        needs_thumbnail,
    )
}

async fn ensure_thumbnail_canonical(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
    force: bool,
) -> Result<EnsureThumbnailResult, String> {
    let file = match db.get_derivative_target_by_entity_hash(entity_hash)? {
        Some(file) => file,
        None => {
            return Ok(EnsureThumbnailResult {
                regenerated_thumbnail: false,
                has_thumbnail: false,
            });
        }
    };

    let ext = mime_to_extension(&file.mime_type).to_string();
    let file_hash = file.file_hash.clone();
    let bs = blob_store.clone();
    let (regenerated_thumbnail, has_thumbnail) = tokio::spawn(async move {
        let result: Result<(bool, bool), String> = (async {
            if force {
                bs.delete_thumbnail(&file_hash)
                    .map_err(|e| format!("Delete thumbnail failed: {}", e))?;
            }

            let original = bs
                .find_original(&file_hash, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", file_hash))?;

            if !force {
                let thumb_exists = bs
                    .find_thumbnail_path(&file_hash)
                    .map_err(|e| format!("Thumbnail lookup failed: {}", e))?
                    .is_some();
                if thumb_exists {
                    return Ok((false, true));
                }
            }

            let info = match crate::media_processing::get_file_info(&original.0, None).await {
                Ok(info) => info,
                Err(_) => return Ok((false, false)),
            };

            let (thumb_bytes, thumb_ext) = match crate::media_processing::generate_thumbnail_bytes(
                &original.0,
                crate::media_processing::DEFAULT_THUMBNAIL_DIMENSIONS,
                info.mime,
                info.duration_ms,
                info.num_frames,
                35,
            )
            .await
            {
                Ok(result) => result,
                Err(_) => return Ok((false, false)),
            };

            bs.write_thumbnail(&file_hash, &thumb_bytes, &thumb_ext)
                .map_err(|e| format!("Thumbnail write failed: {}", e))?;
            Ok((true, true))
        })
        .await;
        result
    })
    .await
    .map_err(|e| format!("Thumbnail task failed: {}", e))??;

    Ok(EnsureThumbnailResult {
        regenerated_thumbnail,
        has_thumbnail,
    })
}

async fn reanalyze_file_colors_canonical(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
) -> Result<ReanalyzeFileColorsResult, String> {
    let file = match db.get_derivative_target_by_entity_hash(entity_hash)? {
        Some(file) => file,
        None => {
            return Ok(ReanalyzeFileColorsResult {
                colors_extracted: 0,
                dominant_color_hex: None,
            });
        }
    };

    if !file.mime_type.starts_with("image/") {
        db.set_file_colors_for_entity_hash(entity_hash, &[], None)?;
        return Ok(ReanalyzeFileColorsResult {
            colors_extracted: 0,
            dominant_color_hex: None,
        });
    }

    let hash_owned = file.file_hash.clone();
    let ext = mime_to_extension(&file.mime_type).to_string();
    let bs = blob_store.clone();
    let colors =
        tokio::task::spawn_blocking(move || -> Result<Vec<(String, f32, f32, f32)>, String> {
            let bytes = if let Ok(Some(thumb_path)) = bs.find_thumbnail_path(&hash_owned) {
                std::fs::read(&thumb_path)
                    .map_err(|e| format!("Failed to read thumbnail: {}", e))?
            } else {
                let original = bs
                    .find_original(&hash_owned, Some(&ext))
                    .map_err(|e| format!("Blob error: {}", e))?
                    .ok_or_else(|| format!("Original file not found for hash {}", hash_owned))?;
                std::fs::read(&original.0)
                    .map_err(|e| format!("Failed to read original file: {}", e))?
            };
            let img = image::load_from_memory(&bytes)
                .map_err(|e| format!("Image decode failed: {}", e))?;
            Ok(
                crate::media_processing::colors::extract_dominant_colors(&img, 8)
                    .iter()
                    .map(|c| (c.hex.clone(), c.l as f32, c.a as f32, c.b as f32))
                    .collect(),
            )
        })
        .await
        .map_err(|e| format!("Color extraction task failed: {}", e))??;

    let dominant_color_hex = colors.first().map(|(hex, _, _, _)| hex.clone());
    db.set_file_colors_for_entity_hash(entity_hash, &colors, dominant_color_hex.as_deref())?;
    Ok(ReanalyzeFileColorsResult {
        colors_extracted: colors.len(),
        dominant_color_hex,
    })
}

async fn ensure_phash_canonical(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
    force: bool,
) -> Result<bool, String> {
    let file = match db.get_derivative_target_by_entity_hash(entity_hash)? {
        Some(file) => file,
        None => return Ok(false),
    };
    if !file.mime_type.starts_with("image/") {
        return Ok(false);
    }
    if !force && file.perceptual_hash.is_some() {
        return Ok(false);
    }

    let ext = mime_to_extension(&file.mime_type).to_string();
    let file_hash = file.file_hash.clone();
    let bs = blob_store.clone();
    let phash_b64 = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let original = bs
            .find_original(&file_hash, Some(&ext))
            .map_err(|e| format!("{e}"))?
            .ok_or_else(|| format!("Original file not found for hash {}", file_hash))?;
        let bytes = std::fs::read(&original.0).map_err(|e| format!("{e}"))?;
        crate::duplicates::phash::compute_phash_base64(&bytes).map_err(|e| format!("{e}"))
    })
    .await
    .map_err(|e| format!("Phash task failed: {}", e))??;

    db.set_phash_for_entity_hash(entity_hash, &phash_b64)?;
    Ok(true)
}

async fn drain_next_entity_canonical(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
) -> Result<usize, String> {
    let jobs = db.claim_next_deferred_work_items()?;
    if jobs.is_empty() {
        return Ok(0);
    }

    let hash = jobs[0].entity_hash.clone();
    let mut fields: Vec<MediaDerivativeField> = Vec::new();
    let mut any_changed = false;

    for job in &jobs {
        let result: Result<Option<MediaDerivativeField>, String> = match job.work_type.as_str() {
            "thumbnail" => {
                match ensure_thumbnail_canonical(db, blob_store, &job.entity_hash, false).await {
                    Ok(r) => Ok(r
                        .regenerated_thumbnail
                        .then_some(MediaDerivativeField::Thumbnail)),
                    Err(e) => Err(e),
                }
            }
            "dominant_colors" => {
                match reanalyze_file_colors_canonical(db, blob_store, &job.entity_hash).await {
                    Ok(r) => {
                        Ok((r.colors_extracted > 0)
                            .then_some(MediaDerivativeField::DominantColorHex))
                    }
                    Err(e) => Err(e),
                }
            }
            "perceptual_hash" => {
                match ensure_phash_canonical(db, blob_store, &job.entity_hash, false).await {
                    Ok(changed) => Ok(changed.then_some(MediaDerivativeField::Phash)),
                    Err(e) => Err(e),
                }
            }
            other => Err(format!("Unknown deferred work type: {other}")),
        };

        match result {
            Ok(changed_field) => {
                db.complete_deferred_work_item(job.work_id)?;
                if let Some(field) = changed_field {
                    any_changed = true;
                    if !fields.contains(&field) {
                        fields.push(field);
                    }
                }
            }
            Err(error) => {
                db.retry_deferred_work_item(job.work_id, job.attempt_count + 1, &error)?;
                warn!(
                    hash = %job.entity_hash,
                    work_type = %job.work_type,
                    attempt = job.attempt_count + 1,
                    error = %error,
                    "Canonical deferred work failed"
                );
            }
        }
    }

    if any_changed && !fields.is_empty() {
        crate::events::emit_state_changed(
            "deferred_work_batch",
            ChangeImpact::new()
                .entity_hashes(vec![hash])
                .derivative_fields_changed(&fields)
                .smart_folder_scopes_changed_for_derivative_fields(&fields),
        );
    }

    Ok(jobs.len())
}

pub async fn start_deferred_work_loop_canonical(
    db: Arc<LibraryDatabase>,
    blob_store: Arc<BlobStore>,
    cancel: CancellationToken,
) {
    if let Err(error) = db.reset_running_deferred_work_items() {
        warn!(error = %error, "Canonical deferred work reset failed");
    }

    loop {
        match drain_next_entity_canonical(&db, &blob_store).await {
            Ok(processed) if processed > 0 => continue,
            Ok(_) => {}
            Err(error) => warn!(error = %error, "Canonical deferred work drain failed"),
        }

        tokio::select! {
            _ = tokio::time::sleep(DEFERRED_WORK_TICK) => {}
            _ = cancel.cancelled() => {
                debug!("Canonical deferred work loop cancelled");
                return;
            }
        }
    }
}

impl SqliteDatabase {
    pub async fn enqueue_deferred_jobs(
        &self,
        hash: &str,
        work_types: &[DeferredWorkType],
    ) -> Result<(), String> {
        let hash = hash.to_string();
        let work_types = work_types.to_vec();
        self.with_conn_labeled("deferred_work/enqueue", move |conn| {
            enqueue_deferred_jobs_sync(conn, &hash, &work_types)
        })
        .await
    }

    /// Enqueue deferred jobs for multiple hashes in a single DB connection.
    pub async fn enqueue_deferred_jobs_batch(
        &self,
        items: Vec<(String, Vec<DeferredWorkType>)>,
    ) -> Result<(), String> {
        if items.is_empty() {
            return Ok(());
        }
        self.with_conn_labeled("deferred_work/enqueue_batch", move |conn| {
            for (hash, work_types) in &items {
                enqueue_deferred_jobs_sync(conn, hash, work_types)?;
            }
            Ok(())
        })
        .await
    }

    pub async fn reset_running_deferred_work(&self) -> Result<usize, String> {
        self.with_conn_labeled(
            "deferred_work/reset_running",
            reset_running_deferred_work_sync,
        )
        .await
    }

    async fn claim_next_hash_jobs(&self) -> Result<Vec<DeferredWorkItem>, String> {
        self.with_conn_mut_labeled("deferred_work/claim_next_hash", move |conn| {
            claim_next_hash_jobs_sync(conn)
        })
        .await
    }

    async fn complete_deferred_work(&self, work_id: i64) -> Result<(), String> {
        self.with_conn_labeled("deferred_work/complete", move |conn| {
            complete_deferred_work_sync(conn, work_id)
        })
        .await
    }

    async fn retry_deferred_work(
        &self,
        work_id: i64,
        next_attempt: i64,
        error: &str,
    ) -> Result<(), String> {
        let error = error.to_string();
        self.with_conn_labeled("deferred_work/retry", move |conn| {
            retry_deferred_work_sync(conn, work_id, next_attempt, &error)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::schema::{apply_pragmas, init_schema};

    #[test]
    fn deferred_work_enqueue_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();

        enqueue_deferred_jobs_sync(
            &conn,
            "abc",
            &[DeferredWorkType::Thumbnail, DeferredWorkType::Thumbnail],
        )
        .unwrap();
        enqueue_deferred_jobs_sync(&conn, "abc", &[DeferredWorkType::Thumbnail]).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM deferred_work", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn reset_running_jobs_marks_them_pending() {
        let conn = Connection::open_in_memory().unwrap();
        apply_pragmas(&conn).unwrap();
        init_schema(&conn).unwrap();

        conn.execute(
            "INSERT INTO deferred_work
                 (hash, work_type, status, attempt_count, available_at, created_at, updated_at)
             VALUES (?1, ?2, 'running', 0, ?3, ?3, ?3)",
            params!["abc", "thumbnail", Utc::now().to_rfc3339()],
        )
        .unwrap();

        reset_running_deferred_work_sync(&conn).unwrap();

        let status: String = conn
            .query_row("SELECT status FROM deferred_work LIMIT 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(status, "pending");
    }
}
