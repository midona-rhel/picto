use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::blob_store::{mime_to_extension, BlobStore};
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::MediaDerivativeField;
use crate::sqlite::{ReadModelEvent, SqliteDatabase};

const DEFERRED_WORK_TICK: std::time::Duration = std::time::Duration::from_secs(5);
const DEFERRED_WORK_BATCH_SIZE: i64 = 32;
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

fn claim_deferred_work_batch_sync(
    conn: &mut Connection,
    limit: i64,
) -> rusqlite::Result<Vec<DeferredWorkItem>> {
    let tx = conn.transaction()?;
    let now = Utc::now().to_rfc3339();
    let mut stmt = tx.prepare(
        "SELECT work_id, hash, work_type, attempt_count
         FROM deferred_work
         WHERE status = 'pending'
           AND available_at <= ?1
         ORDER BY available_at ASC, work_id ASC
         LIMIT ?2",
    )?;
    let raw_items = stmt
        .query_map(params![now, limit], |row| {
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
             SET status = 'running',
                 updated_at = ?2
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
            && blob_store.find_thumbnail_path(hash).ok().flatten().is_none()
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
    let effective_hash;
    let file = match db.get_file_by_hash(hash).await? {
        Some(f) => f,
        None => {
            let entity_id = db
                .resolve_hash(hash)
                .await
                .map_err(|_| format!("Entity not found for hash: {}", hash))?;
            let cover_hash = db
                .with_read_conn(move |conn| {
                    conn.query_row(
                        "SELECT f.hash FROM media_entity me
                         JOIN file f ON f.file_id = me.cover_file_id
                         WHERE me.entity_id = ?1 AND me.kind = 'collection'",
                        [entity_id],
                        |row| row.get::<_, String>(0),
                    )
                })
                .await
                .map_err(|_| format!("Collection has no cover file: {}", hash))?;
            effective_hash = cover_hash;
            db.get_file_by_hash(&effective_hash)
                .await?
                .ok_or_else(|| format!("Cover file not found: {}", effective_hash))?
        }
    };

    let ext = mime_to_extension(&file.mime).to_string();
    let effective_hash = file.hash.clone();
    let bs = blob_store.clone();

    let (regenerated_thumbnail, has_thumbnail) = tokio::spawn(async move {
        let result: Result<(bool, bool), String> = (async {
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

            let info = match crate::media_processing::get_file_info(&original.0, None).await {
                Ok(info) => info,
                Err(e) => {
                    debug!(hash = %effective_hash, error = %e, "thumbnail skipped: file info failed");
                    return Ok((false, false));
                }
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
                Err(e) => {
                    debug!(hash = %effective_hash, mime = ?info.mime, error = %e, "thumbnail skipped: no adapter");
                    return Ok((false, false));
                }
            };

            bs.write_thumbnail(&effective_hash, &thumb_bytes, &thumb_ext)
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

    let ext = mime_to_extension(&file.mime).to_string();
    let hash_owned = hash.to_string();
    let bs = blob_store.clone();
    let colors = tokio::task::spawn_blocking(
        move || -> Result<Vec<(String, f32, f32, f32)>, String> {
            let original = bs
                .find_original(&hash_owned, Some(&ext))
                .map_err(|e| format!("Blob error: {}", e))?
                .ok_or_else(|| format!("Original file not found for hash {}", hash_owned))?;
            let bytes = std::fs::read(&original.0)
                .map_err(|e| format!("Failed to read original file: {}", e))?;
            let img =
                image::load_from_memory(&bytes).map_err(|e| format!("Image decode failed: {}", e))?;
            let extracted = crate::media_processing::colors::extract_dominant_colors(&img, 8);
            Ok(extracted
                .iter()
                .map(|c| (c.hex.clone(), c.l as f32, c.a as f32, c.b as f32))
                .collect())
        },
    )
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

async fn process_deferred_work_item(
    db: &SqliteDatabase,
    blob_store: &Arc<BlobStore>,
    item: &DeferredWorkItem,
) -> Result<Option<MediaDerivativeField>, String> {
    // If the file/entity no longer exists (deleted between enqueue and processing),
    // skip silently — returning Ok(None) so the job is completed and removed.
    let file_exists = db.get_file_by_hash(&item.hash).await.ok().flatten().is_some();
    if !file_exists {
        // Could be a collection hash — check entity table too.
        let entity_exists = db.resolve_hash(&item.hash).await.is_ok();
        if !entity_exists {
            debug!(hash = %item.hash, work_type = item.work_type.as_str(), "Deferred work skipped: entity no longer exists");
            return Ok(None);
        }
    }

    match item.work_type {
        DeferredWorkType::Thumbnail => {
            let result = ensure_thumbnail(db, blob_store, &item.hash, false).await?;
            Ok(result.regenerated_thumbnail.then_some(MediaDerivativeField::Thumbnail))
        }
        DeferredWorkType::DominantColors => {
            let result = reanalyze_file_colors(db, blob_store, &item.hash).await?;
            Ok((result.colors_extracted > 0).then_some(MediaDerivativeField::DominantColorHex))
        }
        DeferredWorkType::Phash => {
            let changed = ensure_phash(db, blob_store, &item.hash, false).await?;
            Ok(changed.then_some(MediaDerivativeField::Phash))
        }
    }
}

async fn drain_deferred_work_batch(
    db: &SqliteDatabase,
    blob_store: &Arc<BlobStore>,
) -> Result<usize, String> {
    let jobs = db.claim_deferred_work_batch(DEFERRED_WORK_BATCH_SIZE).await?;
    if jobs.is_empty() {
        return Ok(0);
    }

    let mut changed_hashes = BTreeSet::new();
    let mut fields: Vec<MediaDerivativeField> = Vec::new();

    for job in &jobs {
        match process_deferred_work_item(db, blob_store, job).await {
            Ok(changed_field) => {
                db.complete_deferred_work(job.work_id).await?;
                if let Some(field) = changed_field {
                    changed_hashes.insert(job.hash.clone());
                    if !fields.contains(&field) {
                        fields.push(field);
                    }
                }
            }
            Err(error) => {
                let next_attempt = job.attempt_count + 1;
                db.retry_deferred_work(job.work_id, next_attempt, &error).await?;
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

    if !changed_hashes.is_empty() && !fields.is_empty() {
        let hashes: Vec<String> = changed_hashes.into_iter().collect();
        db.emit_read_model_event(ReadModelEvent::RebuildAll);
        crate::events::emit_state_changed(
            "deferred_work_batch",
            ChangeImpact::new()
                .file_hashes(hashes)
                .derivative_fields_changed(&fields)
                .smart_folder_scopes_changed_for_derivative_fields(&fields),
        );
    }

    Ok(jobs.len())
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
        match drain_deferred_work_batch(&db, &blob_store).await {
            Ok(processed) if processed as i64 >= DEFERRED_WORK_BATCH_SIZE => continue,
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

    pub async fn reset_running_deferred_work(&self) -> Result<usize, String> {
        self.with_conn_labeled("deferred_work/reset_running", reset_running_deferred_work_sync)
            .await
    }

    async fn claim_deferred_work_batch(&self, limit: i64) -> Result<Vec<DeferredWorkItem>, String> {
        self.with_conn_mut_labeled("deferred_work/claim_batch", move |conn| {
            claim_deferred_work_batch_sync(conn, limit)
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
            .query_row("SELECT status FROM deferred_work LIMIT 1", [], |row| row.get(0))
            .unwrap();
        assert_eq!(status, "pending");
    }
}
