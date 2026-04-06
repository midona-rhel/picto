//! Legacy compatibility shim for media derivative helpers.
//!
//! Active runtime code uses `crate::background_work` and `crate::media_analysis`.
//! This module remains only as a thin canonical wrapper so any leftover internal
//! references do not pull sqlite-era behavior back into the build.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;

pub use crate::background_work::DeferredWorkType;
pub use crate::media_analysis::{EnsureThumbnailResult, ReanalyzeFileColorsResult};

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

pub async fn ensure_thumbnail_canonical(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
    force: bool,
) -> Result<EnsureThumbnailResult, String> {
    crate::background_work::ensure_thumbnail_now(db, blob_store, entity_hash, force).await
}

pub async fn reanalyze_file_colors_canonical(
    db: &LibraryDatabase,
    blob_store: &Arc<BlobStore>,
    entity_hash: &str,
) -> Result<ReanalyzeFileColorsResult, String> {
    crate::background_work::reanalyze_file_colors_now(db, blob_store, entity_hash).await
}

pub async fn start_deferred_work_loop_canonical(
    db: Arc<LibraryDatabase>,
    blob_store: Arc<BlobStore>,
    cancel: CancellationToken,
) {
    crate::background_work::start_worker_loop(db, blob_store, cancel).await;
}
