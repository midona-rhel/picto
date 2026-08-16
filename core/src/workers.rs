//! Background worker lifecycle — spawning and shutdown.
//!
//! `start_workers()` spawns all library-scoped background tasks
//! (compiler loop, bitmap flush, subscription scheduler).
//!
//! `stop_workers()` joins all handles with a timeout for clean shutdown.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::folders::watch::FolderWatchCommand;
use crate::rate_limiter::RateLimiter;
use crate::types::RunningSubscriptions;

/// Shutdown timeout for joining background workers.
const SHUTDOWN_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Spawn all library-scoped background workers.
///
/// Returns a vector of named join handles that should be stored in `AppState.worker_handles`
/// and passed to `stop_workers()` on shutdown.
pub async fn start_workers(
    canonical_db: &Arc<LibraryDatabase>,
    library_root: &std::path::Path,
    blob_store: &Arc<BlobStore>,
    rate_limiter: &RateLimiter,
    running_subscriptions: &RunningSubscriptions,
    sync_cycle_lock: &Arc<tokio::sync::Mutex<()>>,
    folder_watch_rx: tokio::sync::mpsc::UnboundedReceiver<FolderWatchCommand>,
    cancel: &CancellationToken,
) -> Vec<(&'static str, tokio::task::JoinHandle<()>)> {
    let mut handles: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();

    // ── Folder sync ─────────────────────────────────────
    {
        let sync_db = canonical_db.clone();
        let sync_blobs = blob_store.clone();
        let sync_lock = sync_cycle_lock.clone();
        let sync_cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            const MAX_LOCAL_BATCHES_PER_WAKE: usize = 32;
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = tick.tick() => {}
                    _ = sync_cancel.cancelled() => {
                        tracing::info!("Folder sync worker cancelled");
                        return;
                    }
                }
                match crate::oplog::sync::binding(&sync_db) {
                    Ok(Some(_)) => {
                        for cycle in 0..MAX_LOCAL_BATCHES_PER_WAKE {
                            let result = crate::oplog::sync::run_serialized_sync(
                                sync_db.clone(),
                                sync_blobs.clone(),
                                sync_lock.clone(),
                            )
                            .await;
                            match result {
                                Ok(report)
                                    if !report.waiting_for_prerequisites
                                        && cycle + 1 < MAX_LOCAL_BATCHES_PER_WAKE
                                        && (report.more_remote_work
                                            || sync_db.pending_op_count().unwrap_or(0) > 0) =>
                                {
                                    tokio::select! {
                                        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
                                        _ = sync_cancel.cancelled() => return,
                                    }
                                }
                                Ok(_) => break,
                                Err(error) => {
                                    tracing::warn!(error = %error, "Folder sync cycle failed");
                                    break;
                                }
                            }
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(error = %error, "Folder sync binding could not be read")
                    }
                }
            }
        });
        handles.push(("folder_sync", handle));
    }

    // ── Subscription scheduler ─────────────────────────
    {
        let sched_db = canonical_db.clone();
        let sched_root = library_root.to_path_buf();
        let sched_running = running_subscriptions.clone();
        let sched_cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            // Startup delay — let the app settle before checking schedules
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {}
                _ = sched_cancel.cancelled() => {
                    tracing::info!("Subscription scheduler cancelled during startup delay");
                    return;
                }
            }

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
                    _ = sched_cancel.cancelled() => {
                        tracing::info!("Subscription scheduler cancelled");
                        return;
                    }
                }
                crate::scheduler::check_scheduled_subscriptions(
                    &sched_db,
                    &sched_root,
                    &sched_running,
                )
                .await;
            }
        });
        handles.push(("subscription_scheduler", handle));
    }

    // Restore ingest leases before reconciling runs so queued work remains
    // authoritative after an interrupted shutdown.
    if let Err(error) = canonical_db.cleanup_ingest_queue().await {
        tracing::warn!(error = %error, "Ingest queue cleanup failed");
    }

    // ── Subscription runtime recovery (must precede the site runner) ──
    {
        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            canonical_db.as_ref(),
            library_root,
        );
        match runtime.reconcile_subscription_runtime_state().await {
            Ok(report) => {
                tracing::info!(?report, "Subscription runtime reconciled at startup")
            }
            Err(error) => {
                tracing::warn!(error = %error, "Subscription runtime reconcile failed")
            }
        }
        match runtime.list_unsettled_subscription_query_run_ids().await {
            Ok(query_run_ids) => {
                for query_run_id in query_run_ids {
                    if let Err(error) = crate::subscriptions::settlement::settle_query_run(
                        &runtime,
                        running_subscriptions,
                        query_run_id,
                    )
                    .await
                    {
                        tracing::warn!(query_run_id, error = %error, "Subscription query startup settlement failed");
                    }
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "Failed to list unsettled subscription query runs")
            }
        }
        match runtime.list_running_subscription_run_ids().await {
            Ok(run_ids) => {
                for run_id in run_ids {
                    if let Err(error) = crate::subscriptions::settlement::settle_run(
                        &runtime,
                        running_subscriptions,
                        run_id,
                    )
                    .await
                    {
                        tracing::warn!(run_id, error = %error, "Subscription startup settlement failed");
                    }
                }
            }
            Err(error) => {
                tracing::warn!(error = %error, "Failed to list unsettled subscription runs")
            }
        }
    }

    // ── Subscription site runner ──────────────────────
    {
        let sub_db = canonical_db.clone();
        let sub_root = library_root.to_path_buf();
        let sub_rl = rate_limiter.clone();
        let sub_running = running_subscriptions.clone();
        let sub_cancel = cancel.clone();
        let handle = tokio::spawn(crate::subscriptions::site_runner::start_worker_loop(
            sub_db,
            sub_root,
            sub_rl,
            sub_running,
            sub_cancel,
        ));
        handles.push(("subscription_site_runner", handle));
    }

    // ── Ingest queue worker ────────────────────────────
    {
        let ingest_db = canonical_db.clone();
        let ingest_blob = blob_store.clone();
        let ingest_root = library_root.to_path_buf();
        let ingest_running = running_subscriptions.clone();
        let ingest_cancel = cancel.clone();
        let handle = tokio::spawn(crate::ingest_queue::start_worker_loop(
            ingest_db,
            ingest_blob,
            ingest_root,
            ingest_running,
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
