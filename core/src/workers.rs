//! Background worker lifecycle — spawning and shutdown.
//!
//! `start_workers()` spawns all library-scoped background tasks
//! (compiler loop, bitmap flush, group scheduler).
//!
//! `stop_workers()` joins all handles with a timeout for clean shutdown.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::folders::watch::FolderWatchCommand;
use crate::rate_limiter::RateLimiter;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

/// Shutdown timeout for joining background workers.
const SHUTDOWN_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Spawn all library-scoped background workers.
///
/// Returns a vector of named join handles that should be stored in `AppState.worker_handles`
/// and passed to `stop_workers()` on shutdown.
pub async fn start_workers(
    canonical_db: &Arc<LibraryDatabase>,
    blob_store: &Arc<BlobStore>,
    rate_limiter: &RateLimiter,
    running_subscriptions: &RunningSubscriptions,
    sub_terminal_statuses: &SubTerminalStatuses,
    folder_watch_rx: tokio::sync::mpsc::UnboundedReceiver<FolderWatchCommand>,
    cancel: &CancellationToken,
) -> Vec<(&'static str, tokio::task::JoinHandle<()>)> {
    let mut handles: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();

    // ── Group scheduler ────────────────────────────────
    {
        let sched_db = canonical_db.clone();
        let sched_blob = blob_store.clone();
        let sched_rl = rate_limiter.clone();
        let sched_running = running_subscriptions.clone();
        let sched_terminal = sub_terminal_statuses.clone();
        let sched_cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            // Startup delay — let the app settle before checking schedules
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {}
                _ = sched_cancel.cancelled() => {
                    tracing::info!("Group scheduler cancelled during startup delay");
                    return;
                }
            }

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
                    _ = sched_cancel.cancelled() => {
                        tracing::info!("Group scheduler cancelled");
                        return;
                    }
                }
                if let Ok(state) = crate::state::get_state() {
                    crate::scheduler::check_scheduled_groups(
                        &sched_db,
                        &state.library_root,
                        &sched_blob,
                        &sched_rl,
                        &sched_running,
                        &sched_terminal,
                        &state.settings,
                    )
                    .await;
                }
            }
        });
        handles.push(("group_scheduler", handle));
    }

    // ── Ingest queue cleanup + worker ──────────────────
    {
        let cleanup_db = canonical_db.clone();
        tokio::spawn(async move {
            if let Err(e) = cleanup_db.cleanup_ingest_queue().await {
                tracing::warn!(error = %e, "Ingest queue cleanup failed");
            } else {
                tracing::debug!("Ingest queue cleanup complete");
            }
        });

        let ingest_db = canonical_db.clone();
        let ingest_blob = blob_store.clone();
        let ingest_cancel = cancel.clone();
        let handle = tokio::spawn(crate::ingest_queue::start_worker_loop(
            ingest_db,
            ingest_blob,
            ingest_cancel,
        ));
        handles.push(("ingest_queue", handle));
    }

    // ── Deferred media work queue ─────────────────────
    {
        let deferred_db = canonical_db.clone();
        let deferred_blob = blob_store.clone();
        let deferred_cancel = cancel.clone();
        let handle = tokio::spawn(crate::background_work::start_worker_loop(
            deferred_db,
            deferred_blob,
            deferred_cancel,
        ));
        handles.push(("deferred_work", handle));
    }

    // ── Folder watch worker ────────────────────────────
    {
        let watch_canonical_db = canonical_db.clone();
        let watch_blob = blob_store.clone();
        let watch_cancel = cancel.clone();
        let handle = crate::folders::watch::spawn_worker(
            watch_canonical_db,
            watch_blob,
            folder_watch_rx,
            watch_cancel,
        );
        handles.push(("folder_watch", handle));
    }

    handles
}

/// Join all background worker handles with a timeout for clean shutdown.
pub async fn stop_workers(handles: Vec<(&'static str, tokio::task::JoinHandle<()>)>) {
    if handles.is_empty() {
        return;
    }

    tracing::info!(count = handles.len(), "Awaiting background worker shutdown");
    let join_all = async {
        for (name, handle) in handles {
            match handle.await {
                Ok(()) => tracing::debug!(worker = name, "Worker shut down cleanly"),
                Err(e) => tracing::warn!(worker = name, error = %e, "Worker join failed"),
            }
        }
    };

    if tokio::time::timeout(SHUTDOWN_JOIN_TIMEOUT, join_all)
        .await
        .is_err()
    {
        tracing::warn!(
            timeout_secs = SHUTDOWN_JOIN_TIMEOUT.as_secs(),
            "Some workers did not shut down within timeout"
        );
    }
}
