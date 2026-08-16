use std::collections::HashSet;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;

const DEFERRED_WORK_TICK: std::time::Duration = std::time::Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferredWorkType {
    Thumbnail,
    DominantColors,
    PerceptualHash,
    BlobDelete,
    AiTag,
}

impl DeferredWorkType {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumbnail",
            Self::DominantColors => "dominant_colors",
            Self::PerceptualHash => "perceptual_hash",
            Self::BlobDelete => "blob_delete",
            Self::AiTag => "ai_tag",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "thumbnail" => Some(Self::Thumbnail),
            "dominant_colors" => Some(Self::DominantColors),
            "perceptual_hash" => Some(Self::PerceptualHash),
            "blob_delete" => Some(Self::BlobDelete),
            "ai_tag" => Some(Self::AiTag),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeferredWorkStatus {
    Pending,
    Running,
}

impl DeferredWorkStatus {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeferredWorkFilter {
    pub entity_hash: Option<String>,
    pub work_type: Option<DeferredWorkType>,
    pub status: Option<DeferredWorkStatus>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeferredWorkItemInfo {
    pub entity_hash: String,
    pub work_type: DeferredWorkType,
    pub status: DeferredWorkStatus,
    pub attempt_count: i64,
    pub available_at: String,
    pub queued_at: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub last_error: Option<String>,
    pub last_error_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeferredWorkSummary {
    pub pending_count: i64,
    pub running_count: i64,
    pub failed_count: i64,
    pub dominant_colors_pending_count: i64,
    pub dominant_colors_running_count: i64,
    pub dominant_colors_failed_count: i64,
}

// Facade re-exports: callers use `background_work::` as the single entry
// point for derivative work; the implementations live in media_analysis.
pub use crate::media_analysis::{
    enqueue_derivative_jobs, enqueue_missing_derivative_jobs, ensure_missing_color_analysis_jobs,
    ensure_thumbnail_now, reanalyze_file_colors_now, EnsureThumbnailResult,
    ReanalyzeFileColorsResult,
};

async fn drain_next_entity(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
) -> Result<usize, String> {
    let jobs = db.claim_next_deferred_work_items()?;
    if jobs.is_empty() {
        return Ok(0);
    }
    let processed = jobs.len();
    let blob_jobs: Vec<_> = jobs
        .iter()
        .filter(|job| job.work_type == DeferredWorkType::BlobDelete.as_db_str())
        .collect();
    let derivative_jobs: Vec<_> = jobs
        .iter()
        .filter(|job| {
            job.work_type != DeferredWorkType::BlobDelete.as_db_str()
                && job.work_type != DeferredWorkType::AiTag.as_db_str()
        })
        .collect();
    let ai_jobs: Vec<_> = jobs
        .iter()
        .filter(|job| job.work_type == DeferredWorkType::AiTag.as_db_str())
        .cloned()
        .collect();

    let mut deleted_hashes = HashSet::new();
    let mut cleanup_errors = Vec::new();
    if !blob_jobs.is_empty() {
        for job in &blob_jobs {
            match db.cleanup_blob_delete_if_unreferenced(blob_store, &job.entity_hash) {
                Ok(crate::db::types::BlobCleanupResult::Deleted) => {
                    deleted_hashes.insert(job.entity_hash.clone());
                }
                Ok(crate::db::types::BlobCleanupResult::CancelledReferenced) => {}
                Err(error) => cleanup_errors.push(format!("{}: {error}", job.entity_hash)),
            }
        }
        if !cleanup_errors.is_empty() {
            let error = cleanup_errors.join("; ");
            if let Err(settlement_error) = db.retry_deferred_work_batch(&jobs, &error) {
                return Err(format!(
                    "{error}; deferred work settlement failed: {settlement_error}"
                ));
            }
            return Err(error);
        }
    }

    let mut processing_errors = Vec::new();

    if !derivative_jobs.is_empty() {
        let derivative_jobs: Vec<_> = derivative_jobs
            .into_iter()
            .filter(|job| !deleted_hashes.contains(&job.entity_hash))
            .cloned()
            .collect();
        if let Err(error) =
            crate::media_analysis::process_deferred_batch(db, blob_store, &derivative_jobs).await
        {
            processing_errors.push(error);
        }
    }

    if !ai_jobs.is_empty() {
        let mut hashes = Vec::new();
        let mut seen = HashSet::new();
        for job in &ai_jobs {
            if !deleted_hashes.contains(&job.entity_hash) && seen.insert(job.entity_hash.as_str()) {
                hashes.push(job.entity_hash.clone());
            }
        }

        if hashes.is_empty() {
            db.complete_deferred_work_batch(&ai_jobs)?;
        } else {
            let state = crate::state::get_state().map_err(|error| {
                format!("AI tagging worker cannot access application state: {error}")
            });
            match state {
                Ok(state) => {
                    match crate::dispatch::typed::ai_tagger::process_auto_tag_jobs(
                        state.as_ref(),
                        &hashes,
                    )
                    .await
                    {
                        Ok(()) => db.complete_deferred_work_batch(&ai_jobs)?,
                        Err(error) => {
                            db.retry_deferred_work_batch(&ai_jobs, &error)?;
                            processing_errors.push(error);
                        }
                    }
                }
                Err(error) => {
                    db.retry_deferred_work_batch(&ai_jobs, &error)?;
                    processing_errors.push(error);
                }
            }
        }
    }

    if !processing_errors.is_empty() {
        return Err(processing_errors.join("; "));
    }
    Ok(processed)
}

pub async fn start_worker_loop(
    db: Arc<LibraryDatabase>,
    blob_store: Arc<BlobStore>,
    cancel: CancellationToken,
) {
    if let Err(error) = db.reset_running_deferred_work_items() {
        warn!(error = %error, "Deferred work reset failed");
    }

    loop {
        match drain_next_entity(&db, &blob_store).await {
            Ok(processed) if processed > 0 => continue,
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

#[cfg(test)]
mod tests {
    use super::drain_next_entity;
    use crate::background_work::{DeferredWorkFilter, DeferredWorkType};
    use crate::blob_store::BlobStore;
    use crate::db::LibraryDatabase;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use std::collections::HashSet;
    use std::fs;
    use std::io::Cursor;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn deferred_batch_settlement_handles_every_claimed_job() {
        let dir = TempDir::new().unwrap();
        let db = LibraryDatabase::open(dir.path()).unwrap();
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        db.enqueue_deferred_jobs(
            hash,
            &[
                DeferredWorkType::Thumbnail,
                DeferredWorkType::DominantColors,
                DeferredWorkType::PerceptualHash,
                DeferredWorkType::AiTag,
            ],
        )
        .unwrap();

        let jobs = db.claim_next_deferred_work_items().unwrap();
        assert_eq!(jobs.len(), 4);

        db.retry_deferred_work_batch(&jobs, "simulated processing failure")
            .unwrap();
        let retried = db
            .list_deferred_work_items(DeferredWorkFilter::default())
            .unwrap();
        assert_eq!(retried.len(), 4);
        assert!(retried.iter().all(|job| {
            job.status == crate::background_work::DeferredWorkStatus::Pending
                && job.attempt_count == 1
                && job.last_error.as_deref() == Some("simulated processing failure")
        }));

        db.complete_deferred_work_batch(&jobs).unwrap();
        assert!(db
            .list_deferred_work_items(DeferredWorkFilter::default())
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn blob_delete_work_removes_blob_and_queue_row() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(LibraryDatabase::open(dir.path()).unwrap());
        let blob_store = Arc::new(BlobStore::open(dir.path()).unwrap());
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        blob_store
            .write_original(hash, b"data", Some("png"))
            .unwrap();
        db.enqueue_deferred_jobs(
            hash,
            &[
                DeferredWorkType::BlobDelete,
                DeferredWorkType::Thumbnail,
                DeferredWorkType::DominantColors,
                DeferredWorkType::PerceptualHash,
            ],
        )
        .unwrap();

        assert_eq!(drain_next_entity(&db, &blob_store).await.unwrap(), 4);
        assert!(blob_store.read_original(hash, Some("png")).is_err());
        assert!(db
            .list_deferred_work_items(DeferredWorkFilter {
                entity_hash: Some(hash.to_string()),
                ..Default::default()
            })
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn blob_delete_failure_is_persisted_for_retry() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(LibraryDatabase::open(dir.path()).unwrap());
        let blob_store = Arc::new(BlobStore::open(dir.path()).unwrap());
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let path = blob_store
            .original_path_with_ext(hash, Some("png"))
            .unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::create_dir(path).unwrap();
        db.enqueue_deferred_jobs(hash, &[DeferredWorkType::BlobDelete])
            .unwrap();

        let error = drain_next_entity(&db, &blob_store)
            .await
            .expect_err("cleanup failure must reach the worker");
        assert!(!error.is_empty());
        let jobs = db
            .list_deferred_work_items(DeferredWorkFilter {
                work_type: Some(DeferredWorkType::BlobDelete),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].attempt_count, 1);
        assert_eq!(
            jobs[0].status,
            crate::background_work::DeferredWorkStatus::Pending
        );
        assert!(jobs[0].last_error.is_some());
    }

    #[tokio::test]
    async fn blob_delete_is_cancelled_when_hash_is_referenced_again() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(LibraryDatabase::open(dir.path()).unwrap());
        let blob_store = Arc::new(BlobStore::open(dir.path()).unwrap());
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let lease = blob_store.acquire_hash_lease(hash).await;
        blob_store
            .write_original(hash, b"reimported", Some("png"))
            .unwrap();
        db.enqueue_deferred_jobs(
            hash,
            &[
                DeferredWorkType::BlobDelete,
                DeferredWorkType::Thumbnail,
                DeferredWorkType::DominantColors,
            ],
        )
        .unwrap();

        let cleanup_error = db
            .cleanup_blob_delete_if_unreferenced(&blob_store, hash)
            .expect_err("cleanup must defer while reimport owns the hash");
        assert!(cleanup_error.contains("being imported"));

        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO media_file
                    (file_id, file_hash, mime_type, size_bytes, date_added)
                 VALUES (1, ?1, 'image/png', 10, '2026-08-13')",
                [hash],
            )?;
            Ok(())
        })
        .unwrap();
        drop(lease);
        assert_eq!(
            db.cleanup_blob_delete_if_unreferenced(&blob_store, hash)
                .unwrap(),
            crate::db::types::BlobCleanupResult::CancelledReferenced
        );
        assert!(blob_store.read_original(hash, Some("png")).is_ok());
        let remaining = db
            .list_deferred_work_items(DeferredWorkFilter {
                entity_hash: Some(hash.to_string()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining
            .iter()
            .all(|job| job.work_type != DeferredWorkType::BlobDelete));
    }

    #[tokio::test]
    async fn referenced_blob_delete_preserves_and_processes_derivative_work() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(LibraryDatabase::open(dir.path()).unwrap());
        let blob_store = Arc::new(BlobStore::open(dir.path()).unwrap());
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let mut bytes = Vec::new();
        DynamicImage::ImageRgba8(ImageBuffer::from_pixel(8, 8, Rgba([120, 80, 40, 255])))
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        blob_store
            .write_original(hash, &bytes, Some("png"))
            .unwrap();
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO media_file
                    (file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count, date_added)
                 VALUES (1, ?1, 'image/png', 100, 8, 8, 1, '2026-08-13')",
                [hash],
            )?;
            conn.execute(
                "INSERT INTO media_entity
                    (entity_id, entity_hash, file_id, status, date_created, date_added, date_modified)
                 VALUES (1, ?1, 1, 1, '2026-08-13', '2026-08-13', '2026-08-13')",
                [hash],
            )?;
            Ok(())
        })
        .unwrap();
        db.enqueue_deferred_jobs(
            hash,
            &[DeferredWorkType::BlobDelete, DeferredWorkType::Thumbnail],
        )
        .unwrap();

        assert_eq!(drain_next_entity(&db, &blob_store).await.unwrap(), 2);
        assert!(blob_store.read_original(hash, Some("png")).is_ok());
        assert!(blob_store.read_thumbnail(hash).unwrap().is_some());
        assert!(db
            .list_deferred_work_items(DeferredWorkFilter {
                entity_hash: Some(hash.to_string()),
                ..Default::default()
            })
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn ordinary_delete_request_cannot_remove_blob_during_reimport() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(LibraryDatabase::open(dir.path()).unwrap());
        let blob_store = Arc::new(BlobStore::open(dir.path()).unwrap());
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let lease = blob_store.acquire_hash_lease(hash).await;
        blob_store
            .write_original(hash, b"reimported", Some("png"))
            .unwrap();

        let cleanup_db = db.clone();
        let cleanup_store = blob_store.clone();
        let cleanup = tokio::task::spawn_blocking(move || {
            cleanup_db.enqueue_blob_delete_and_attempt(&cleanup_store, hash)
        });
        let cleanup_error = tokio::time::timeout(Duration::from_secs(1), cleanup)
            .await
            .expect("ordinary cleanup request hung")
            .unwrap()
            .expect_err("cleanup should defer while reimport owns the hash");
        assert!(cleanup_error.contains("being imported"));

        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO media_file
                    (file_id, file_hash, mime_type, size_bytes, date_added)
                 VALUES (1, ?1, 'image/png', 10, '2026-08-13')",
                [hash],
            )?;
            Ok(())
        })
        .unwrap();
        drop(lease);

        assert_eq!(
            db.enqueue_blob_delete_and_attempt(&blob_store, hash)
                .unwrap(),
            crate::db::types::BlobCleanupResult::CancelledReferenced
        );
        assert!(blob_store.read_original(hash, Some("png")).is_ok());
    }

    #[tokio::test]
    async fn orphan_sweep_candidate_cannot_remove_blob_during_reimport() {
        let dir = TempDir::new().unwrap();
        let db = Arc::new(LibraryDatabase::open(dir.path()).unwrap());
        let blob_store = Arc::new(BlobStore::open(dir.path()).unwrap());
        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        let lease = blob_store.acquire_hash_lease(hash).await;
        blob_store
            .write_original(hash, b"reimported", Some("png"))
            .unwrap();

        let candidates = blob_store
            .orphan_candidates(&HashSet::new(), Duration::ZERO)
            .unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].hash, hash);

        let cleanup_error = db
            .enqueue_blob_delete_and_attempt(&blob_store, hash)
            .expect_err("sweep cleanup should defer while reimport owns the hash");
        assert!(cleanup_error.contains("being imported"));
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO media_file
                    (file_id, file_hash, mime_type, size_bytes, date_added)
                 VALUES (1, ?1, 'image/png', 10, '2026-08-13')",
                [hash],
            )?;
            Ok(())
        })
        .unwrap();
        drop(lease);

        assert_eq!(
            db.enqueue_blob_delete_and_attempt(&blob_store, hash)
                .unwrap(),
            crate::db::types::BlobCleanupResult::CancelledReferenced
        );
        assert!(blob_store.read_original(hash, Some("png")).is_ok());
    }
}
