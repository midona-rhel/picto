//! Background worker lifecycle — spawning and shutdown.
//!
//! `start_workers()` spawns all library-scoped background tasks
//! (compiler loop, bitmap flush, flow scheduler, PTR startup maintenance).
//!
//! `stop_workers()` joins all handles with a timeout for clean shutdown.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::sqlite::SqliteDatabase;
use crate::ptr::db::PtrSqliteDatabase;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

/// Shutdown timeout for joining background workers.
const SHUTDOWN_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Spawn all library-scoped background workers.
///
/// Returns a vector of named join handles that should be stored in `AppState.worker_handles`
/// and passed to `stop_workers()` on shutdown.
pub async fn start_workers(
    db: &Arc<SqliteDatabase>,
    ptr_db: &Arc<PtrSqliteDatabase>,
    blob_store: &Arc<BlobStore>,
    rate_limiter: &RateLimiter,
    running_subscriptions: &RunningSubscriptions,
    sub_terminal_statuses: &SubTerminalStatuses,
    cancel: &CancellationToken,
) -> Vec<(&'static str, tokio::task::JoinHandle<()>)> {
    let mut handles: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();

    // ── Compiler loop ──────────────────────────────────
    {
        let compiler_db = db.clone();
        let compiler_ptr = ptr_db.clone();
        if let Some(rx) = compiler_db.take_compiler_rx().await {
            let handle = tokio::spawn(crate::sqlite::compilers::start_compiler_loop(
                compiler_db.clone(),
                Some(compiler_ptr),
                rx,
                |result| {
                    let mut domains = Vec::new();
                    if result.sidebar_affected {
                        domains.push(crate::events::Domain::Sidebar);
                    }
                    if result.smart_folders_rebuilt {
                        domains.push(crate::events::Domain::SmartFolders);
                    }
                    let mut impact = crate::events::MutationImpact::new()
                        .domains(&domains);
                    impact.compiler_batch_done = Some(true);
                    if result.smart_folders_rebuilt {
                        impact = impact.extra_grid_scopes(vec!["system:all".into()]);
                    }
                    crate::events::emit_mutation("compiler_batch_done", impact);
                },
            ));
            handles.push(("compiler_loop", handle));
        }

        compiler_db.emit_compiler_event(crate::sqlite::CompilerEvent::RebuildAll);
    }

    // ── Bitmap flush worker ────────────────────────────
    {
        let flush_db = db.clone();
        let flush_cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(30)) => {}
                    _ = flush_cancel.cancelled() => {
                        tracing::info!("Bitmap flush loop cancelled");
                        // Final flush before exiting
                        let _ = flush_db.flush().await;
                        return;
                    }
                }
                if let Err(e) = flush_db.flush().await {
                    tracing::warn!("Periodic flush failed: {e}");
                }
            }
        });
        handles.push(("bitmap_flush", handle));
    }

    // ── Flow scheduler ─────────────────────────────────
    {
        let sched_db = db.clone();
        let sched_blob = blob_store.clone();
        let sched_rl = rate_limiter.clone();
        let sched_running = running_subscriptions.clone();
        let sched_terminal = sub_terminal_statuses.clone();
        let sched_ptr_db = ptr_db.clone();
        let sched_cancel = cancel.clone();
        let handle = tokio::spawn(async move {
            // Startup delay — let the app settle before checking schedules
            tokio::select! {
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {}
                _ = sched_cancel.cancelled() => {
                    tracing::info!("Flow scheduler cancelled during startup delay");
                    return;
                }
            }

            // Immediate PTR check on startup — start initial population ASAP
            if let Ok(state) = crate::state::get_state() {
                crate::scheduler::check_scheduled_ptr_sync(
                    &sched_ptr_db,
                    &state.settings,
                    state.db.compiler_tx.clone(),
                )
                .await;
            }

            loop {
                tokio::select! {
                    _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {}
                    _ = sched_cancel.cancelled() => {
                        tracing::info!("Flow scheduler cancelled");
                        return;
                    }
                }
                if let Ok(state) = crate::state::get_state() {
                    crate::scheduler::check_scheduled_flows(
                        &sched_db,
                        &sched_blob,
                        &sched_rl,
                        &sched_running,
                        &sched_terminal,
                        &state.settings,
                    )
                    .await;
                    crate::scheduler::check_scheduled_ptr_sync(
                        &sched_ptr_db,
                        &state.settings,
                        state.db.compiler_tx.clone(),
                    )
                    .await;
                }
            }
        });
        handles.push(("flow_scheduler", handle));
    }

    // ── PTR startup maintenance ────────────────────────
    crate::ptr::controller::PtrController::start_background_startup_maintenance(
        ptr_db.clone(),
    );

    handles
}

/// Join all background worker handles with a timeout for clean shutdown.
pub async fn stop_workers(
    handles: Vec<(&'static str, tokio::task::JoinHandle<()>)>,
) {
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
