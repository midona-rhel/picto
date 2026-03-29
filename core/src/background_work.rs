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
}

impl DeferredWorkType {
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumbnail",
            Self::DominantColors => "dominant_colors",
            Self::PerceptualHash => "perceptual_hash",
        }
    }

    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "thumbnail" => Some(Self::Thumbnail),
            "dominant_colors" => Some(Self::DominantColors),
            "perceptual_hash" => Some(Self::PerceptualHash),
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
}

pub use crate::media_analysis::{EnsureThumbnailResult, ReanalyzeFileColorsResult};

pub fn enqueue_derivative_jobs(
    db: &LibraryDatabase,
    entity_hash: &str,
    mime: &str,
    frame_count: Option<i64>,
    needs_thumbnail: bool,
) -> Result<(), String> {
    crate::media_analysis::enqueue_derivative_jobs(db, entity_hash, mime, frame_count, needs_thumbnail)
}

pub async fn enqueue_missing_derivative_jobs(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hashes: &[String],
) {
    crate::media_analysis::enqueue_missing_derivative_jobs(db, blob_store, entity_hashes).await;
}

pub async fn ensure_thumbnail_now(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
    force: bool,
) -> Result<EnsureThumbnailResult, String> {
    crate::media_analysis::ensure_thumbnail_now(db, blob_store, entity_hash, force).await
}

pub async fn reanalyze_file_colors_now(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
) -> Result<ReanalyzeFileColorsResult, String> {
    crate::media_analysis::reanalyze_file_colors_now(db, blob_store, entity_hash).await
}

async fn drain_next_entity(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
) -> Result<usize, String> {
    let jobs = db.claim_next_deferred_work_items()?;
    if jobs.is_empty() {
        return Ok(0);
    }
    let processed = jobs.len();
    crate::media_analysis::process_deferred_batch(db, blob_store, &jobs).await?;
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
