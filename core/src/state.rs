//! Global application state and initialization.
//!
//! Supports library switching: `open_library()` closes the previous library
//! (if any), opens a new one, and spawns background tasks. `close_library()`
//! shuts everything down cleanly via a `CancellationToken`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use tokio_util::sync::CancellationToken;

use tokio::sync::mpsc;

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::sqlite::{CompilerEvent, SqliteDatabase};
use crate::ptr::db::PtrSqliteDatabase;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

/// Shared application state, accessible to all command handlers.
pub struct AppState {
    pub db: Arc<SqliteDatabase>,
    pub ptr_db: Arc<PtrSqliteDatabase>,
    pub blob_store: Arc<BlobStore>,
    pub settings: SettingsStore,
    pub rate_limiter: RateLimiter,
    pub running_subscriptions: RunningSubscriptions,
    pub sub_terminal_statuses: SubTerminalStatuses,
    pub library_root: PathBuf,
    pub cancel: CancellationToken,
    /// Join handles for long-running background workers (bitmap flush, scheduler, etc.)
    /// Used by shutdown to deterministically await completion instead of sleeping.
    pub worker_handles: tokio::sync::Mutex<Vec<(&'static str, tokio::task::JoinHandle<()>)>>,
}

fn is_dir_writable(path: &Path) -> bool {
    let probe = path.join(".picto_write_probe");
    match std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

static STATE: OnceLock<RwLock<Option<Arc<AppState>>>> = OnceLock::new();

fn state_lock() -> &'static RwLock<Option<Arc<AppState>>> {
    STATE.get_or_init(|| RwLock::new(None))
}

/// Open a library, closing any previously open library first.
///
/// `library_root` is the path to the library directory
/// (e.g. `~/.local/share/picto/library` or a `.library/` folder).
pub async fn open_library(library_root: PathBuf) -> Result<Arc<AppState>, String> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "picto=info".parse().unwrap()),
        )
        .try_init();

    close_library_inner().await;

    std::fs::create_dir_all(&library_root)
        .map_err(|e| format!("Failed to create library directory: {}", e))?;

    let library_db: Arc<SqliteDatabase> = SqliteDatabase::open(&library_root)
        .await
        .map_err(|e| format!("Failed to open library database: {}", e))?;

    tracing::info!(
        epoch = library_db.manifest.published_epoch(),
        path = %library_root.display(),
        "Library database initialized"
    );

    let settings = SettingsStore::load(&library_root);

    // Fail fast if PTR path is invalid/unwritable rather than silently falling
    // back to temp storage. Temp fallback only allowed with PICTO_PTR_TEMP_FALLBACK=1.
    let preferred_ptr_root = match &settings.get().ptr_data_path {
        Some(custom) if !custom.is_empty() => PathBuf::from(custom),
        _ => library_root.parent().unwrap_or(&library_root).join("ptr"),
    };
    let allow_temp_fallback = std::env::var("PICTO_PTR_TEMP_FALLBACK")
        .map(|v| v == "1")
        .unwrap_or(false);

    let ptr_candidates = if allow_temp_fallback {
        tracing::warn!("PICTO_PTR_TEMP_FALLBACK=1: temp-path fallback enabled (dev/test only)");
        vec![
            preferred_ptr_root.clone(),
            library_root.join("ptr"),
            std::env::temp_dir().join("picto_ptr"),
        ]
    } else {
        vec![preferred_ptr_root.clone(), library_root.join("ptr")]
    };

    let mut ptr_open_error: Option<String> = None;
    let mut ptr_db_opt: Option<Arc<PtrSqliteDatabase>> = None;
    for candidate in &ptr_candidates {
        if let Err(e) = std::fs::create_dir_all(candidate) {
            ptr_open_error = Some(format!(
                "Failed to create PTR directory {}: {}",
                candidate.display(),
                e
            ));
            continue;
        }
        if !is_dir_writable(candidate) {
            tracing::warn!(path = %candidate.display(), "PTR directory is not writable");
            ptr_open_error = Some(format!(
                "PTR directory is not writable: {}",
                candidate.display()
            ));
            continue;
        }
        match PtrSqliteDatabase::open(candidate).await {
            Ok(db) => {
                tracing::info!(path = %candidate.display(), "PTR database opened");
                ptr_db_opt = Some(db);
                break;
            }
            Err(e) => {
                tracing::warn!(path = %candidate.display(), error = %e, "Failed to open PTR database");
                ptr_open_error = Some(e);
            }
        }
    }
    let ptr_db: Arc<PtrSqliteDatabase> = ptr_db_opt.ok_or_else(|| {
        let attempted = ptr_candidates
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "Failed to open PTR database.\n  Attempted paths: {}\n  Last error: {}\n  \
             Suggested fix: Ensure the PTR data directory exists and is writable, \
             or set a custom path in Settings > PTR > Data Path.",
            attempted,
            ptr_open_error.unwrap_or_else(|| "unknown error".into())
        )
    })?;

    let blob_store: Arc<BlobStore> = Arc::new(
        BlobStore::open(&library_root).map_err(|e| format!("Failed to open blob store: {}", e))?,
    );

    crate::constants::init_groupings();

    let rate_limiter = RateLimiter::new();
    let running_subscriptions: RunningSubscriptions =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));
    let sub_terminal_statuses: SubTerminalStatuses =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    let cancel = CancellationToken::new();
    let mut worker_handles: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();

    let compiler_db = library_db.clone();
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
        worker_handles.push(("compiler_loop", handle));
    }

    library_db.emit_compiler_event(crate::sqlite::CompilerEvent::RebuildAll);

    {
        let flush_db = library_db.clone();
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
        worker_handles.push(("bitmap_flush", handle));
    }

    {
        let sched_db = library_db.clone();
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
            if let Ok(state) = get_state() {
                check_scheduled_ptr_sync(
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
                if let Ok(state) = get_state() {
                    check_scheduled_flows(
                        &sched_db,
                        &sched_blob,
                        &sched_rl,
                        &sched_running,
                        &sched_terminal,
                        &state.settings,
                    )
                    .await;
                    check_scheduled_ptr_sync(
                        &sched_ptr_db,
                        &state.settings,
                        state.db.compiler_tx.clone(),
                    )
                    .await;
                }
            }
        });
        worker_handles.push(("flow_scheduler", handle));
    }

    let state = Arc::new(AppState {
        db: library_db,
        ptr_db,
        blob_store,
        settings,
        rate_limiter,
        running_subscriptions,
        sub_terminal_statuses,
        library_root,
        cancel,
        worker_handles: tokio::sync::Mutex::new(worker_handles),
    });

    {
        let mut guard = state_lock()
            .write()
            .map_err(|_| "State lock poisoned".to_string())?;
        *guard = Some(state.clone());
    }

    crate::ptr::controller::PtrController::start_background_startup_maintenance(
        state.ptr_db.clone(),
    );

    Ok(state)
}

/// Get the current library state. Returns an `Arc` (cheap clone).
pub fn get_state() -> Result<Arc<AppState>, String> {
    let guard = state_lock()
        .read()
        .map_err(|_| "State lock poisoned".to_string())?;
    guard
        .as_ref()
        .cloned()
        .ok_or_else(|| "No library is open. Call open_library() first.".to_string())
}

/// Close the current library, cancelling background tasks.
pub async fn close_library() -> Result<(), String> {
    close_library_inner().await;
    Ok(())
}

/// Shutdown timeout for joining background workers.
const SHUTDOWN_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

async fn close_library_inner() {
    let old_state = {
        let lock = state_lock();
        let mut guard = match lock.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        guard.take()
    };

    crate::runtime_state::reset();

    if let Some(state) = old_state {
        tracing::info!(path = %state.library_root.display(), "Closing library");

        state.cancel.cancel();

        let handles = {
            let mut guard = state.worker_handles.lock().await;
            std::mem::take(&mut *guard)
        };

        if !handles.is_empty() {
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

        if let Err(e) = state.db.flush().await {
            tracing::warn!("Final flush on close failed: {e}");
        }

        tracing::info!("Library closed");
    }
}

/// Check all flows for overdue scheduled runs and trigger them.
async fn check_scheduled_flows(
    db: &Arc<SqliteDatabase>,
    blob_store: &Arc<BlobStore>,
    rate_limiter: &RateLimiter,
    running_subs: &RunningSubscriptions,
    sub_terminal_statuses: &SubTerminalStatuses,
    settings: &crate::settings::store::SettingsStore,
) {
    let flows = match db.list_flows().await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Scheduler: failed to list flows: {e}");
            return;
        }
    };

    for flow in flows {
        if flow.schedule == "manual" {
            continue;
        }

        let interval_secs: i64 = match flow.schedule.as_str() {
            "daily" => 86_400,
            "weekly" => 604_800,
            "monthly" => 2_592_000, // 30 days
            _ => continue,
        };

        let subs = match db.list_subscriptions_for_flow(flow.flow_id).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        if subs.is_empty() {
            continue;
        }

        let mut latest_check: Option<chrono::DateTime<chrono::Utc>> = None;
        let mut has_any_queries = false;
        for sub in &subs {
            let queries = match db.get_subscription_queries(sub.subscription_id).await {
                Ok(q) => q,
                Err(_) => continue,
            };
            for q in &queries {
                has_any_queries = true;
                if let Some(ref t) = q.last_check_time {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t) {
                        let utc = dt.with_timezone(&chrono::Utc);
                        latest_check = Some(
                            latest_check
                                .map_or(utc, |prev: chrono::DateTime<chrono::Utc>| prev.max(utc)),
                        );
                    }
                }
            }
        }

        if !has_any_queries {
            continue;
        }

        let now = chrono::Utc::now();
        let is_overdue = match latest_check {
            None => true, // Never ran
            Some(last) => (now - last).num_seconds() >= interval_secs,
        };

        if is_overdue {
            let flow_id_str = flow.flow_id.to_string();
            tracing::info!(
                flow_id = flow.flow_id,
                name = %flow.name,
                schedule = %flow.schedule,
                "Scheduler: running overdue flow"
            );
            if let Err(e) = crate::subscriptions::flow_controller::FlowController::run_flow(
                db,
                blob_store,
                rate_limiter,
                running_subs,
                sub_terminal_statuses,
                flow_id_str,
                settings,
            )
            .await
            {
                tracing::warn!(
                    flow_id = flow.flow_id,
                    "Scheduler: failed to start flow: {e}"
                );
            }
        }
    }
}

/// Check if PTR needs syncing — either initial population or scheduled auto-sync.
async fn check_scheduled_ptr_sync(
    ptr_db: &Arc<PtrSqliteDatabase>,
    settings: &SettingsStore,
    compiler_tx: mpsc::UnboundedSender<CompilerEvent>,
) {
    let s = settings.get();

    if !s.ptr_enabled {
        return;
    }

    // Short-circuit when any PTR heavy phase is running (PBI-024).
    if crate::ptr::controller::PtrController::is_ptr_busy_for_scheduler() {
        return;
    }

    // Don't hammer the server — back off after failed attempts
    if crate::ptr::controller::PtrController::is_auto_sync_cooling_down() {
        return;
    }

    // Force sync if PTR has never completed initial population,
    // regardless of auto_sync or schedule settings.
    if s.ptr_last_sync_time.is_none() {
        tracing::info!("PTR has never completed initial population — starting sync");
        if let Err(e) =
            crate::ptr::controller::PtrController::sync(ptr_db, settings, compiler_tx).await
        {
            tracing::warn!("Failed to start PTR initial population sync: {e}");
        }
        return;
    }

    // Regular auto-sync: requires auto_sync enabled + valid schedule
    if !s.ptr_auto_sync {
        return;
    }

    let interval_secs: i64 = match s.ptr_sync_schedule.as_str() {
        "daily" => 86_400,
        "weekly" => 604_800,
        "monthly" => 2_592_000,
        _ => return,
    };

    let now = chrono::Utc::now();
    let is_overdue = match &s.ptr_last_sync_time {
        Some(t) => match chrono::DateTime::parse_from_rfc3339(t) {
            Ok(last) => (now - last.with_timezone(&chrono::Utc)).num_seconds() >= interval_secs,
            Err(_) => true,
        },
        None => unreachable!(), // Handled above
    };

    if is_overdue {
        tracing::info!(
            schedule = %s.ptr_sync_schedule,
            "Scheduler: running overdue PTR sync"
        );
        if let Err(e) =
            crate::ptr::controller::PtrController::sync(ptr_db, settings, compiler_tx).await
        {
            tracing::warn!("Scheduler: failed to start PTR sync: {e}");
        }
    }
}
