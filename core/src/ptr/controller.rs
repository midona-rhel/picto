//! PTR sync orchestration — manages bootstrap, delta sync, scheduling,
//! and cancellation for the Public Tag Repository.
//!
//! Runs periodic sync on a 60-second schedule with back-off on failure.
//! Exposes global state flags (`PTR_SYNCING`) for UI status indication.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::events;
use crate::ptr_client::PtrClient;
use crate::ptr_sync::PtrSyncEngine;
use crate::ptr_types::PtrSyncProgress;
use crate::runtime_contract::task::{RuntimeTask, TaskKind, TaskProgress, TaskStatus};
use crate::settings::SettingsStore;
use crate::sqlite::CompilerEvent;
use crate::sqlite_ptr::tags::PtrResolvedTag;
use crate::sqlite_ptr::PtrSqliteDatabase;

/// Global flag: is a PTR sync currently running?
pub static PTR_SYNCING: AtomicBool = AtomicBool::new(false);

/// Global cancellation token for the running PTR sync.
static PTR_CANCEL: Mutex<Option<CancellationToken>> = Mutex::new(None);

/// Latest sync progress — pollable fallback when events are lost.
static PTR_PROGRESS: Mutex<Option<PtrSyncProgress>> = Mutex::new(None);

/// Tracks the last sync attempt time so the scheduler can back off on failure.
/// Cleared on successful sync. Set on every attempt.
static LAST_ATTEMPT: Mutex<Option<Instant>> = Mutex::new(None);

/// Global flag: is a PTR bootstrap currently running?
pub static PTR_BOOTSTRAP_RUNNING: AtomicBool = AtomicBool::new(false);
/// Global flag: is compact index build currently running?
pub static PTR_COMPACT_BUILD_RUNNING: AtomicBool = AtomicBool::new(false);

/// Cancellation token for bootstrap/compact build phases (PBI-016).
static PTR_BOOTSTRAP_CANCEL: Mutex<Option<CancellationToken>> = Mutex::new(None);

#[derive(Debug, Clone, Default, Serialize)]
pub struct PtrBootstrapStatus {
    pub running: bool,
    pub phase: String,
    pub stage: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_result: Option<crate::sqlite_ptr::bootstrap::PtrBootstrapImportResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run_result: Option<crate::sqlite_ptr::bootstrap::PtrBootstrapResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_done: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_done_stage: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_total_stage: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_per_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<crate::sqlite_ptr::bootstrap::PtrCompactCheckpoint>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PtrBootstrapRequest {
    pub snapshot_dir: String,
    pub ptr_service_id: Option<i64>,
    pub mode: String,
}

static PTR_BOOTSTRAP_STATUS: Mutex<PtrBootstrapStatus> = Mutex::new(PtrBootstrapStatus {
    running: false,
    phase: String::new(),
    stage: String::new(),
    mode: String::new(),
    service_id: None,
    started_at: None,
    updated_at: None,
    last_error: None,
    last_result: None,
    dry_run_result: None,
    rows_total: None,
    rows_done: None,
    eta_seconds: None,
    rows_done_stage: None,
    rows_total_stage: None,
    rows_per_sec: None,
    checkpoint: None,
});

/// Minimum cooldown between auto-sync attempts (30 minutes).
/// Prevents hammering the server when it's down or unreachable.
const AUTO_SYNC_COOLDOWN_SECS: u64 = 1800;

pub struct PtrController;

impl PtrController {
    /// Run PTR startup maintenance in the background if a schema rebuild is needed.
    /// This must never block app launch.
    pub fn start_background_startup_maintenance(ptr_db: Arc<PtrSqliteDatabase>) {
        tokio::spawn(async move {
            let needs_schema_rebuild = match ptr_db.needs_schema_rebuild().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "PTR startup maintenance check failed");
                    return;
                }
            };
            let needs_index_rebuild = match ptr_db.needs_bulk_index_rebuild().await {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "PTR index-rebuild check failed");
                    false
                }
            };
            if !needs_schema_rebuild && !needs_index_rebuild {
                return;
            }

            if PTR_SYNCING
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_err()
            {
                tracing::info!("PTR startup maintenance skipped (sync already running)");
                return;
            }

            let startup_phase = if needs_schema_rebuild {
                "schema_rebuild"
            } else {
                "index_rebuild"
            };
            tracing::info!(phase = startup_phase, "PTR startup maintenance started");

            // Register cancellation token so cancel_sync can interrupt maintenance (PBI-025).
            let maintenance_cancel = CancellationToken::new();
            {
                if let Ok(mut guard) = PTR_CANCEL.lock() {
                    *guard = Some(maintenance_cancel.clone());
                }
            }

            events::emit_empty(events::event_names::PTR_SYNC_STARTED);
            {
                let now = chrono::Utc::now().to_rfc3339();
                crate::runtime_state::upsert_task(RuntimeTask {
                    task_id: "ptr:sync".to_string(),
                    kind: TaskKind::PtrSync,
                    status: TaskStatus::Running,
                    label: format!("PTR {startup_phase}"),
                    parent_task_id: None,
                    progress: None,
                    detail: None,
                    started_at: now.clone(),
                    updated_at: now,
                });
            }

            let mut progress = PtrSyncProgress {
                phase: startup_phase.into(),
                ..Default::default()
            };
            events::emit(
                events::event_names::PTR_SYNC_PHASE_CHANGED,
                &events::PtrSyncPhaseChangedEvent {
                    phase: startup_phase.into(),
                    current_update_index: None,
                    ts: None,
                },
            );
            Self::update_sync_progress(&progress);
            events::emit(events::event_names::PTR_SYNC_PROGRESS, &progress);

            let heartbeat_cancel = CancellationToken::new();
            let heartbeat_cancel_clone = heartbeat_cancel.clone();
            let heartbeat_start = Instant::now();
            let heartbeat_task = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
                interval.tick().await;
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let mut snap = PtrSyncProgress {
                                phase: startup_phase.into(),
                                heartbeat: true,
                                elapsed_ms: heartbeat_start.elapsed().as_millis() as u64,
                                ..Default::default()
                            };
                            PtrController::update_sync_progress(&snap);
                            events::emit(events::event_names::PTR_SYNC_PROGRESS, &snap);
                            // Avoid carrying heartbeat=true in poll fallback snapshots
                            snap.heartbeat = false;
                        }
                        _ = heartbeat_cancel_clone.cancelled() => break,
                    }
                }
            });

            let rebuild_res = if needs_schema_rebuild {
                ptr_db.run_schema_rebuild().await
            } else {
                ptr_db.run_bulk_index_rebuild().await
            };
            heartbeat_cancel.cancel();
            let _ = heartbeat_task.await;

            match rebuild_res {
                Ok(()) => {
                    progress.elapsed_ms = heartbeat_start.elapsed().as_millis() as u64;
                    Self::update_sync_progress(&progress);
                    events::emit(
                        events::event_names::PTR_SYNC_FINISHED,
                        &events::PtrSyncFinishedEvent {
                            success: true,
                            error: None,
                            updates_processed: None,
                            tags_added: None,
                            schema_rebuild: Some(needs_schema_rebuild),
                            index_rebuild: Some(needs_index_rebuild && !needs_schema_rebuild),
                            changed_hashes_truncated: None,
                        },
                    );
                    {
                        let now = chrono::Utc::now().to_rfc3339();
                        crate::runtime_state::upsert_task(RuntimeTask {
                            task_id: "ptr:sync".to_string(),
                            kind: TaskKind::PtrSync,
                            status: TaskStatus::Finished,
                            label: format!("PTR {startup_phase}"),
                            parent_task_id: None,
                            progress: None,
                            detail: None,
                            started_at: now.clone(),
                            updated_at: now,
                        });
                    }
                    tracing::info!(phase = startup_phase, "PTR startup maintenance finished");
                }
                Err(e) => {
                    events::emit(
                        events::event_names::PTR_SYNC_FINISHED,
                        &events::PtrSyncFinishedEvent {
                            success: false,
                            error: Some(e.clone()),
                            updates_processed: None,
                            tags_added: None,
                            schema_rebuild: Some(needs_schema_rebuild),
                            index_rebuild: Some(needs_index_rebuild && !needs_schema_rebuild),
                            changed_hashes_truncated: None,
                        },
                    );
                    {
                        let now = chrono::Utc::now().to_rfc3339();
                        crate::runtime_state::upsert_task(RuntimeTask {
                            task_id: "ptr:sync".to_string(),
                            kind: TaskKind::PtrSync,
                            status: TaskStatus::Failed,
                            label: format!("PTR {startup_phase}"),
                            parent_task_id: None,
                            progress: None,
                            detail: None,
                            started_at: now.clone(),
                            updated_at: now,
                        });
                    }
                    tracing::error!(phase = startup_phase, "PTR startup maintenance failed");
                }
            }

            PTR_SYNCING.store(false, Ordering::SeqCst);
            if let Ok(mut guard) = PTR_CANCEL.lock() {
                *guard = None;
            }
            if let Ok(mut guard) = PTR_PROGRESS.lock() {
                *guard = None;
            }
        });
    }

    pub async fn get_ptr_status(
        ptr_db: &PtrSqliteDatabase,
    ) -> Result<crate::sqlite_ptr::tags::PtrStats, String> {
        ptr_db.get_stats().await
    }

    pub fn is_ptr_syncing() -> bool {
        PTR_SYNCING.load(Ordering::SeqCst)
    }

    /// Get the latest sync progress snapshot (for polling).
    pub fn get_sync_progress() -> Option<PtrSyncProgress> {
        PTR_PROGRESS.lock().ok().and_then(|g| g.clone())
    }

    pub fn get_bootstrap_status() -> PtrBootstrapStatus {
        PTR_BOOTSTRAP_STATUS
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Called by the sync engine to update the pollable progress.
    pub fn update_sync_progress(progress: &PtrSyncProgress) {
        if let Ok(mut g) = PTR_PROGRESS.lock() {
            *g = Some(progress.clone());
        }
    }

    fn update_bootstrap_status(update: impl FnOnce(&mut PtrBootstrapStatus)) {
        if let Ok(mut g) = PTR_BOOTSTRAP_STATUS.lock() {
            update(&mut g);
            g.updated_at = Some(chrono::Utc::now().to_rfc3339());
        }
    }

    /// Returns true if the auto-sync cooldown is still active (last attempt was recent).
    /// Manual sync calls ignore this — it's only for the scheduler.
    pub fn is_auto_sync_cooling_down() -> bool {
        if let Ok(guard) = LAST_ATTEMPT.lock() {
            if let Some(last) = *guard {
                return last.elapsed().as_secs() < AUTO_SYNC_COOLDOWN_SECS;
            }
        }
        false
    }

    /// Returns true when any PTR heavy writer phase is active (PBI-017/024).
    /// Used by the scheduler to avoid useless sync attempts during busy phases.
    pub fn is_ptr_busy_for_scheduler() -> bool {
        PTR_SYNCING.load(Ordering::SeqCst)
            || PTR_BOOTSTRAP_RUNNING.load(Ordering::SeqCst)
            || PTR_COMPACT_BUILD_RUNNING.load(Ordering::SeqCst)
    }

    /// Cancel the currently running PTR sync, if any.
    pub fn cancel_sync(ptr_db: &PtrSqliteDatabase) -> Result<(), String> {
        if !PTR_SYNCING.load(Ordering::SeqCst) {
            return Err("No PTR sync is running".into());
        }
        let guard = PTR_CANCEL.lock().map_err(|_| "lock poisoned")?;
        if let Some(token) = guard.as_ref() {
            token.cancel();
            tracing::info!("PTR sync cancellation requested");
        } else {
            tracing::warn!("PTR sync cancellation requested without active token");
        }
        // Force-interrupt long SQLite statements so cancel isn't blocked on
        // a large in-flight writer transaction.
        ptr_db.interrupt_writer();
        drop(guard);

        if let Ok(mut progress_guard) = PTR_PROGRESS.lock() {
            if let Some(progress) = progress_guard.as_mut() {
                progress.phase = "cancelling".into();
                progress.heartbeat = false;
                events::emit(events::event_names::PTR_SYNC_PROGRESS, progress);
            }
        }
        events::emit(
            events::event_names::PTR_SYNC_PHASE_CHANGED,
            &events::PtrSyncPhaseChangedEvent {
                phase: "cancelling".into(),
                current_update_index: None,
                ts: None,
            },
        );
        Ok(())
    }

    /// Cancel the currently running PTR bootstrap/compact build, if any.
    pub fn cancel_bootstrap(ptr_db: &PtrSqliteDatabase) -> Result<(), String> {
        if !PTR_BOOTSTRAP_RUNNING.load(Ordering::SeqCst)
            && !PTR_COMPACT_BUILD_RUNNING.load(Ordering::SeqCst)
        {
            return Err("No PTR bootstrap or compact build is running".into());
        }
        let guard = PTR_BOOTSTRAP_CANCEL.lock().map_err(|_| "lock poisoned")?;
        if let Some(token) = guard.as_ref() {
            token.cancel();
            tracing::info!("PTR bootstrap/compact cancellation requested");
        } else {
            tracing::warn!("PTR bootstrap/compact cancellation requested without active token");
        }
        ptr_db.interrupt_writer();
        drop(guard);

        Self::update_bootstrap_status(|s| {
            s.phase = "cancelling".into();
        });
        events::emit(
            events::event_names::PTR_BOOTSTRAP_PROGRESS,
            &events::PtrBootstrapProgressEvent {
                phase: "cancelling".into(),
                stage: Some("cancelling".into()),
                ..Default::default()
            },
        );
        Ok(())
    }

    pub async fn bootstrap_from_hydrus_snapshot(
        ptr_db: &Arc<PtrSqliteDatabase>,
        req: PtrBootstrapRequest,
    ) -> Result<serde_json::Value, String> {
        let mode = crate::sqlite_ptr::bootstrap::PtrBootstrapMode::parse(&req.mode)?;
        if PTR_SYNCING.load(Ordering::SeqCst) {
            return Err("PTR sync is running; stop it before bootstrap".into());
        }
        if PTR_COMPACT_BUILD_RUNNING.load(Ordering::SeqCst) {
            return Err("PTR compact index build is already running".into());
        }
        if PTR_BOOTSTRAP_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("PTR bootstrap is already running".into());
        }
        let snapshot_dir = std::path::PathBuf::from(req.snapshot_dir.clone());
        let probe_result = tokio::task::spawn_blocking({
            let snapshot_dir = snapshot_dir.clone();
            let ptr_service_id = req.ptr_service_id;
            move || crate::sqlite_ptr::bootstrap::probe_snapshot(&snapshot_dir, ptr_service_id)
        })
        .await
        .map_err(|e| format!("PTR bootstrap probe join failed: {e}"))?;
        let probe = match probe_result {
            Ok(p) => p,
            Err(e) => {
                PTR_BOOTSTRAP_RUNNING.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };

        Self::update_bootstrap_status(|s| {
            s.running = true;
            s.phase = "probe".into();
            s.stage = "probe".into();
            s.mode = req.mode.clone();
            s.service_id = Some(probe.service_id);
            s.started_at = Some(chrono::Utc::now().to_rfc3339());
            s.last_error = None;
            s.last_result = None;
            s.rows_done = None;
            s.rows_total = None;
            s.eta_seconds = None;
            s.rows_done_stage = None;
            s.rows_total_stage = None;
            s.rows_per_sec = None;
            s.checkpoint = None;
        });
        events::emit(
            events::event_names::PTR_BOOTSTRAP_STARTED,
            &events::PtrBootstrapStartedEvent {
                snapshot_dir: req.snapshot_dir.clone(),
                service_id: probe.service_id,
                mode: req.mode.clone(),
            },
        );
        {
            let now = chrono::Utc::now().to_rfc3339();
            crate::runtime_state::upsert_task(RuntimeTask {
                task_id: "ptr:bootstrap".to_string(),
                kind: TaskKind::PtrBootstrap,
                status: TaskStatus::Running,
                label: format!("PTR bootstrap ({})", req.mode),
                parent_task_id: None,
                progress: None,
                detail: None,
                started_at: now.clone(),
                updated_at: now,
            });
        }

        let dry_run_result = tokio::task::spawn_blocking({
            let probe = probe.clone();
            move || crate::sqlite_ptr::bootstrap::dry_run_snapshot(&probe)
        })
        .await
        .map_err(|e| format!("PTR bootstrap dry-run join failed: {e}"))?;
        let dry_run = match dry_run_result {
            Ok(r) => r,
            Err(e) => {
                PTR_BOOTSTRAP_RUNNING.store(false, Ordering::SeqCst);
                Self::update_bootstrap_status(|s| {
                    s.running = false;
                    s.phase = "failed".into();
                    s.stage = "failed".into();
                    s.last_error = Some(e.clone());
                });
                return Err(e);
            }
        };

        Self::update_bootstrap_status(|s| {
            s.phase = "dry_run_complete".into();
            s.stage = "dry_run".into();
            s.dry_run_result = Some(dry_run.clone());
            let total = dry_run.counts.hash_defs
                + dry_run.counts.tag_defs
                + dry_run.counts.mappings
                + dry_run.counts.siblings
                + dry_run.counts.parents;
            s.rows_total = Some(total);
            s.rows_done = Some(0);
            s.eta_seconds = Some(dry_run.projected_import_seconds);
        });
        events::emit(
            events::event_names::PTR_BOOTSTRAP_PROGRESS,
            &events::PtrBootstrapProgressEvent {
                phase: "dry_run_complete".into(),
                service_id: Some(dry_run.service_id),
                counts: Some(serde_json::to_value(&dry_run.counts).unwrap_or_default()),
                ..Default::default()
            },
        );

        if mode == crate::sqlite_ptr::bootstrap::PtrBootstrapMode::DryRun {
            PTR_BOOTSTRAP_RUNNING.store(false, Ordering::SeqCst);
            Self::update_bootstrap_status(|s| {
                s.running = false;
                s.phase = "idle".into();
                s.stage = "dry_run".into();
                s.rows_done = Some(0);
            });
            events::emit(
                events::event_names::PTR_BOOTSTRAP_FINISHED,
                &events::PtrBootstrapFinishedEvent {
                    success: true,
                    dry_run: Some(true),
                    service_id: Some(dry_run.service_id),
                    result: None,
                    cursor_index: None,
                    cursor_source: None,
                    delta_sync_started: None,
                    counts: Some(serde_json::to_value(&dry_run.counts).unwrap_or_default()),
                },
            );
            {
                let now = chrono::Utc::now().to_rfc3339();
                crate::runtime_state::upsert_task(RuntimeTask {
                    task_id: "ptr:bootstrap".to_string(),
                    kind: TaskKind::PtrBootstrap,
                    status: TaskStatus::Finished,
                    label: "PTR bootstrap (dry_run)".to_string(),
                    parent_task_id: None,
                    progress: None,
                    detail: None,
                    started_at: now.clone(),
                    updated_at: now,
                });
            }
            return Ok(serde_json::json!({
                "started": false,
                "dry_run": true,
                "result": dry_run,
            }));
        }

        // Resolve the library DB path from global state so bootstrap import can
        // ATTACH it directly, rather than guessing via sibling-path heuristic (PBI-015).
        let library_db_path = crate::state::get_state()
            .ok()
            .map(|s| s.library_root.join("db").join("library.sqlite"));

        // Set up cancellation token for bootstrap/compact (PBI-016).
        let bootstrap_cancel = CancellationToken::new();
        {
            let mut guard = PTR_BOOTSTRAP_CANCEL.lock().map_err(|_| "lock poisoned")?;
            *guard = Some(bootstrap_cancel.clone());
        }

        // Bridge the async CancellationToken to a synchronous AtomicBool for use in
        // spawn_blocking (bootstrap runs synchronous rusqlite code).
        let cancel_flag: crate::sqlite_ptr::bootstrap::CancelCheck =
            Arc::new(std::sync::atomic::AtomicBool::new(false));
        {
            let flag = cancel_flag.clone();
            let token = bootstrap_cancel.clone();
            tokio::spawn(async move {
                token.cancelled().await;
                flag.store(true, std::sync::atomic::Ordering::SeqCst);
            });
        }

        let ptr_db = ptr_db.clone();
        tokio::spawn(async move {
            Self::update_bootstrap_status(|s| {
                s.phase = "importing".into();
                s.stage = "warmup_local".into();
            });
            let import_started = std::time::Instant::now();
            let progress_cb = std::sync::Arc::new(
                move |progress: crate::sqlite_ptr::bootstrap::PtrBootstrapProgress| {
                    let eta_seconds =
                        if progress.rows_done > 0 && progress.rows_total > progress.rows_done {
                            let elapsed_s = import_started.elapsed().as_secs_f64();
                            let per_row = elapsed_s / progress.rows_done as f64;
                            let remain = (progress.rows_total - progress.rows_done) as f64;
                            Some((per_row * remain).round() as i64)
                        } else {
                            None
                        };
                    let rows_per_sec = if progress.rows_done > 0 {
                        let elapsed_s = import_started.elapsed().as_secs_f64();
                        if elapsed_s > 0.0 {
                            progress.rows_done as f64 / elapsed_s
                        } else {
                            0.0
                        }
                    } else {
                        0.0
                    };
                    PtrController::update_bootstrap_status(|s| {
                        s.phase = progress.phase.clone();
                        s.stage = match progress.phase.as_str() {
                            "dry_run_complete" => "dry_run",
                            "warmup_done" | "warmup_local" => "warmup_local",
                            "compact_tags" | "compact_hashes" | "compact_postings"
                            | "compact_completed" => "compact_build",
                            _ => progress.phase.as_str(),
                        }
                        .to_string();
                        s.rows_done = Some(progress.rows_done);
                        s.rows_total = Some(progress.rows_total);
                        s.rows_done_stage = Some(progress.rows_done);
                        s.rows_total_stage = Some(progress.rows_total);
                        s.rows_per_sec = Some(rows_per_sec);
                        s.eta_seconds = eta_seconds;
                    });
                    events::emit(
                        events::event_names::PTR_BOOTSTRAP_PROGRESS,
                        &events::PtrBootstrapProgressEvent {
                            phase: progress.phase,
                            rows_done: Some(progress.rows_done),
                            rows_total: Some(progress.rows_total),
                            rows_done_stage: Some(progress.rows_done),
                            rows_total_stage: Some(progress.rows_total),
                            rows_per_sec: Some(rows_per_sec),
                            eta_seconds: eta_seconds.map(|s| s as f64),
                            ts: Some(progress.ts),
                            ..Default::default()
                        },
                    );
                },
            );
            let heartbeat_cancel = CancellationToken::new();
            let heartbeat_cancel_clone = heartbeat_cancel.clone();
            let heartbeat = tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
                interval.tick().await;
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let snap = PtrController::get_bootstrap_status();
                            events::emit(events::event_names::PTR_BOOTSTRAP_PROGRESS, &events::PtrBootstrapProgressEvent {
                                phase: snap.phase,
                                stage: Some(snap.stage),
                                service_id: snap.service_id,
                                running: Some(snap.running),
                                rows_done: snap.rows_done,
                                rows_total: snap.rows_total,
                                rows_done_stage: snap.rows_done_stage,
                                rows_total_stage: snap.rows_total_stage,
                                rows_per_sec: snap.rows_per_sec,
                                eta_seconds: snap.eta_seconds.map(|s| s as f64),
                                updated_at: snap.updated_at,
                                ..Default::default()
                            });
                        }
                        _ = heartbeat_cancel_clone.cancelled() => break,
                    }
                }
            });

            let imported = crate::sqlite_ptr::bootstrap::import_snapshot(
                &ptr_db,
                probe.clone(),
                Some(progress_cb),
                library_db_path,
                Some(cancel_flag.clone()),
            )
            .await;
            heartbeat_cancel.cancel();
            let _ = heartbeat.await;

            match imported {
                Ok(result) => {
                    // Warmup is complete, release bootstrap lock so sync can run.
                    PTR_BOOTSTRAP_RUNNING.store(false, Ordering::SeqCst);
                    Self::update_bootstrap_status(|s| {
                        s.phase = "finalize_cursor".into();
                        s.stage = "finalize_cursor".into();
                        s.last_result = Some(result.clone());
                        s.last_error = None;
                    });
                    let final_cursor = result.cursor_index;
                    // PBI-030: Require deterministic cursor from snapshot artifacts.
                    // Never fall back to server-latest index — that can skip deltas
                    // and produce silent PTR divergence.
                    if final_cursor <= 0
                        && (result.counts.mappings > 0
                            || result.counts.hash_defs > 0
                            || result.counts.tag_defs > 0)
                    {
                        tracing::warn!(
                            cursor = final_cursor,
                            mappings = result.counts.mappings,
                            "PTR bootstrap: snapshot produced no valid cursor index despite having data. \
                             Ensure the snapshot includes repository_updates table (client.db or client.caches.db). \
                             Delta sync will start from index 0."
                        );
                    }
                    let _ = ptr_db.set_cursor(final_cursor).await;
                    let delta_sync_started = if let Ok(state) = crate::state::get_state() {
                        Self::sync(&ptr_db, &state.settings, state.db.compiler_tx.clone())
                            .await
                            .is_ok()
                    } else {
                        false
                    };
                    Self::update_bootstrap_status(|s| {
                        s.phase = if delta_sync_started {
                            "delta_started".into()
                        } else {
                            "delta_start_failed".into()
                        };
                        s.stage = "delta_start".into();
                    });

                    // Continue with compact build in the background.
                    PTR_COMPACT_BUILD_RUNNING.store(true, Ordering::SeqCst);
                    let compact_started = std::time::Instant::now();
                    let compact_progress_cb = std::sync::Arc::new(
                        move |progress: crate::sqlite_ptr::bootstrap::PtrBootstrapProgress| {
                            let rows_per_sec = if progress.rows_done > 0 {
                                let elapsed_s = compact_started.elapsed().as_secs_f64();
                                if elapsed_s > 0.0 {
                                    progress.rows_done as f64 / elapsed_s
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            };
                            let eta_seconds = if rows_per_sec > 0.0
                                && progress.rows_total > progress.rows_done
                            {
                                Some(
                                    ((progress.rows_total - progress.rows_done) as f64
                                        / rows_per_sec) as i64,
                                )
                            } else {
                                None
                            };
                            PtrController::update_bootstrap_status(|s| {
                                s.running = true;
                                s.phase = progress.phase.clone();
                                s.stage = "compact_build".into();
                                s.rows_done = Some(progress.rows_done);
                                s.rows_total = Some(progress.rows_total);
                                s.rows_done_stage = Some(progress.rows_done);
                                s.rows_total_stage = Some(progress.rows_total);
                                s.rows_per_sec = Some(rows_per_sec);
                                s.eta_seconds = eta_seconds;
                            });
                            events::emit(
                                events::event_names::PTR_BOOTSTRAP_PROGRESS,
                                &events::PtrBootstrapProgressEvent {
                                    phase: progress.phase,
                                    stage: Some("compact_build".into()),
                                    running: Some(true),
                                    rows_done: Some(progress.rows_done),
                                    rows_total: Some(progress.rows_total),
                                    rows_done_stage: Some(progress.rows_done),
                                    rows_total_stage: Some(progress.rows_total),
                                    rows_per_sec: Some(rows_per_sec),
                                    eta_seconds: eta_seconds.map(|s| s as f64),
                                    updated_at: Some(chrono::Utc::now().to_rfc3339()),
                                    ts: Some(progress.ts),
                                    ..Default::default()
                                },
                            );
                        },
                    );
                    // 1s heartbeat ensures frontend receives events even between
                    // long batch boundaries (matches bootstrap import pattern).
                    let compact_hb_cancel = CancellationToken::new();
                    let compact_hb_cancel_clone = compact_hb_cancel.clone();
                    let compact_heartbeat = tokio::spawn(async move {
                        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
                        interval.tick().await;
                        loop {
                            tokio::select! {
                                _ = interval.tick() => {
                                    let snap = PtrController::get_bootstrap_status();
                                    events::emit(events::event_names::PTR_BOOTSTRAP_PROGRESS, &events::PtrBootstrapProgressEvent {
                                        phase: snap.phase,
                                        stage: Some(snap.stage),
                                        service_id: snap.service_id,
                                        running: Some(snap.running),
                                        rows_done: snap.rows_done,
                                        rows_total: snap.rows_total,
                                        rows_done_stage: snap.rows_done_stage,
                                        rows_total_stage: snap.rows_total_stage,
                                        rows_per_sec: snap.rows_per_sec,
                                        eta_seconds: snap.eta_seconds.map(|s| s as f64),
                                        updated_at: snap.updated_at,
                                        ..Default::default()
                                    });
                                }
                                _ = compact_hb_cancel_clone.cancelled() => break,
                            }
                        }
                    });
                    let compact_result = crate::sqlite_ptr::bootstrap::build_compact_index(
                        &ptr_db,
                        probe,
                        Some(compact_progress_cb),
                        Some(cancel_flag.clone()),
                    )
                    .await;
                    compact_hb_cancel.cancel();
                    let _ = compact_heartbeat.await;
                    PTR_COMPACT_BUILD_RUNNING.store(false, Ordering::SeqCst);

                    match compact_result {
                        Ok(_) => {
                            let compact_status =
                                crate::sqlite_ptr::bootstrap::get_compact_index_status(&ptr_db)
                                    .await
                                    .ok();
                            Self::update_bootstrap_status(|s| {
                                s.running = false;
                                s.phase = "completed".into();
                                s.stage = "completed".into();
                                s.last_result = Some(result.clone());
                                s.last_error = None;
                                if let Some(cs) = compact_status.clone() {
                                    s.rows_done_stage = Some(cs.rows_done_stage);
                                    s.rows_total_stage = Some(cs.rows_total_stage);
                                    s.rows_per_sec = Some(cs.rows_per_sec);
                                    s.checkpoint = Some(cs.checkpoint);
                                }
                            });
                            events::emit(
                                events::event_names::PTR_BOOTSTRAP_FINISHED,
                                &events::PtrBootstrapFinishedEvent {
                                    success: true,
                                    dry_run: None,
                                    service_id: None,
                                    result: Some(serde_json::to_value(&result).unwrap_or_default()),
                                    cursor_index: Some(final_cursor),
                                    cursor_source: Some("snapshot".into()),
                                    delta_sync_started: Some(delta_sync_started),
                                    counts: None,
                                },
                            );
                            {
                                let now = chrono::Utc::now().to_rfc3339();
                                crate::runtime_state::upsert_task(RuntimeTask {
                                    task_id: "ptr:bootstrap".to_string(),
                                    kind: TaskKind::PtrBootstrap,
                                    status: TaskStatus::Finished,
                                    label: "PTR bootstrap".to_string(),
                                    parent_task_id: None,
                                    progress: None,
                                    detail: None,
                                    started_at: now.clone(),
                                    updated_at: now,
                                });
                            }
                        }
                        Err(e) => {
                            Self::update_bootstrap_status(|s| {
                                s.running = false;
                                s.phase = "failed".into();
                                s.stage = "failed".into();
                                s.last_error = Some(e.clone());
                            });
                            events::emit(
                                events::event_names::PTR_BOOTSTRAP_FAILED,
                                &events::PtrBootstrapFailedEvent {
                                    success: false,
                                    error: e,
                                },
                            );
                            {
                                let now = chrono::Utc::now().to_rfc3339();
                                crate::runtime_state::upsert_task(RuntimeTask {
                                    task_id: "ptr:bootstrap".to_string(),
                                    kind: TaskKind::PtrBootstrap,
                                    status: TaskStatus::Failed,
                                    label: "PTR bootstrap".to_string(),
                                    parent_task_id: None,
                                    progress: None,
                                    detail: None,
                                    started_at: now.clone(),
                                    updated_at: now,
                                });
                            }
                        }
                    }
                }
                Err(e) => {
                    PTR_BOOTSTRAP_RUNNING.store(false, Ordering::SeqCst);
                    Self::update_bootstrap_status(|s| {
                        s.running = false;
                        s.phase = "failed".into();
                        s.stage = "failed".into();
                        s.last_error = Some(e.clone());
                    });
                    events::emit(
                        events::event_names::PTR_BOOTSTRAP_FAILED,
                        &events::PtrBootstrapFailedEvent {
                            success: false,
                            error: e,
                        },
                    );
                    {
                        let now = chrono::Utc::now().to_rfc3339();
                        crate::runtime_state::upsert_task(RuntimeTask {
                            task_id: "ptr:bootstrap".to_string(),
                            kind: TaskKind::PtrBootstrap,
                            status: TaskStatus::Failed,
                            label: "PTR bootstrap".to_string(),
                            parent_task_id: None,
                            progress: None,
                            detail: None,
                            started_at: now.clone(),
                            updated_at: now,
                        });
                    }
                }
            }

            // Clean up cancellation token (PBI-016).
            if let Ok(mut guard) = PTR_BOOTSTRAP_CANCEL.lock() {
                *guard = None;
            }
        });

        Ok(serde_json::json!({
            "started": true,
            "dry_run": false,
            "service_id": dry_run.service_id,
            "counts": dry_run.counts,
            "auto_finalize": true,
            "auto_delta_start": true,
            "background_compact_build": true,
        }))
    }

    pub async fn get_compact_index_status(
        ptr_db: &Arc<PtrSqliteDatabase>,
    ) -> Result<crate::sqlite_ptr::bootstrap::PtrCompactIndexStatus, String> {
        crate::sqlite_ptr::bootstrap::get_compact_index_status(ptr_db).await
    }

    /// Run a full PTR sync in the background.
    pub async fn sync(
        ptr_db: &Arc<PtrSqliteDatabase>,
        settings: &SettingsStore,
        compiler_tx: mpsc::UnboundedSender<CompilerEvent>,
    ) -> Result<(), String> {
        if PTR_BOOTSTRAP_RUNNING.load(Ordering::SeqCst) {
            return Err("PTR bootstrap is running".into());
        }
        if PTR_COMPACT_BUILD_RUNNING.load(Ordering::SeqCst) {
            return Err("PTR compact build is running".into());
        }
        if PTR_SYNCING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err("PTR sync is already running".into());
        }

        let settings = settings.get();
        let server_url = settings
            .ptr_server_url
            .as_deref()
            .unwrap_or(crate::ptr_client::DEFAULT_PTR_URL);
        let access_key = settings
            .ptr_access_key
            .as_deref()
            .unwrap_or(crate::ptr_client::DEFAULT_PTR_ACCESS_KEY);

        let client = PtrClient::new(server_url, access_key);
        let engine = PtrSyncEngine::new(client, ptr_db.clone());
        let cancel = CancellationToken::new();

        // Store the token so cancel_sync() can reach it
        {
            let mut guard = PTR_CANCEL.lock().map_err(|_| "lock poisoned")?;
            *guard = Some(cancel.clone());
        }

        // Record attempt time for scheduler backoff
        if let Ok(mut guard) = LAST_ATTEMPT.lock() {
            *guard = Some(Instant::now());
        }

        events::emit_empty(events::event_names::PTR_SYNC_STARTED);
        events::emit(
            events::event_names::PTR_SYNC_PHASE_CHANGED,
            &events::PtrSyncPhaseChangedEvent {
                phase: "starting".into(),
                current_update_index: None,
                ts: None,
            },
        );
        {
            let now = chrono::Utc::now().to_rfc3339();
            crate::runtime_state::upsert_task(RuntimeTask {
                task_id: "ptr:sync".to_string(),
                kind: TaskKind::PtrSync,
                status: TaskStatus::Running,
                label: "PTR sync".to_string(),
                parent_task_id: None,
                progress: None,
                detail: None,
                started_at: now.clone(),
                updated_at: now,
            });
        }

        let cancel_check = cancel.clone();
        tokio::spawn(async move {
            match engine.sync(cancel).await {
                Ok(progress) => {
                    if cancel_check.is_cancelled() {
                        tracing::info!("PTR sync was cancelled");
                        events::emit(
                            events::event_names::PTR_SYNC_FINISHED,
                            &events::PtrSyncFinishedEvent {
                                success: false,
                                error: Some("Cancelled".into()),
                                updates_processed: None,
                                tags_added: None,
                                schema_rebuild: None,
                                index_rebuild: None,
                                changed_hashes_truncated: None,
                            },
                        );
                        {
                            let now = chrono::Utc::now().to_rfc3339();
                            crate::runtime_state::upsert_task(RuntimeTask {
                                task_id: "ptr:sync".to_string(),
                                kind: TaskKind::PtrSync,
                                status: TaskStatus::Failed,
                                label: "PTR sync".to_string(),
                                parent_task_id: None,
                                progress: None,
                                detail: None,
                                started_at: now.clone(),
                                updated_at: now,
                            });
                        }
                    } else {
                        tracing::info!(
                            processed = progress.updates_processed,
                            tags = progress.tags_added,
                            "PTR sync finished successfully"
                        );
                        // Record last sync time in settings
                        if let Ok(state) = crate::state::get_state() {
                            let mut s = state.settings.get();
                            s.ptr_last_sync_time = Some(chrono::Utc::now().to_rfc3339());
                            state.settings.update(s);
                        }
                        // Clear cooldown so auto-sync uses normal schedule
                        if let Ok(mut guard) = LAST_ATTEMPT.lock() {
                            *guard = None;
                        }
                        if progress.changed_hashes_truncated || progress.tag_graph_changed {
                            // Force full overlay rebuild when hashes are truncated or
                            // when sibling/parent relations changed (PBI-028).
                            let _ = compiler_tx.send(CompilerEvent::PtrFullRebuild);
                        } else {
                            let _ = compiler_tx.send(CompilerEvent::PtrSyncComplete {
                                changed_hashes: progress.changed_hashes.clone(),
                            });
                        }
                        events::emit(
                            events::event_names::PTR_SYNC_FINISHED,
                            &events::PtrSyncFinishedEvent {
                                success: true,
                                error: None,
                                updates_processed: Some(progress.updates_processed),
                                tags_added: Some(progress.tags_added),
                                schema_rebuild: None,
                                index_rebuild: None,
                                changed_hashes_truncated: Some(progress.changed_hashes_truncated),
                            },
                        );
                        {
                            let now = chrono::Utc::now().to_rfc3339();
                            crate::runtime_state::upsert_task(RuntimeTask {
                                task_id: "ptr:sync".to_string(),
                                kind: TaskKind::PtrSync,
                                status: TaskStatus::Finished,
                                label: "PTR sync".to_string(),
                                parent_task_id: None,
                                progress: Some(TaskProgress {
                                    done: progress.updates_processed as u64,
                                    total: progress.updates_processed as u64,
                                    status_text: None,
                                }),
                                detail: None,
                                started_at: now.clone(),
                                updated_at: now,
                            });
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "PTR sync failed");
                    events::emit(
                        events::event_names::PTR_SYNC_FINISHED,
                        &events::PtrSyncFinishedEvent {
                            success: false,
                            error: Some(e.clone()),
                            updates_processed: None,
                            tags_added: None,
                            schema_rebuild: None,
                            index_rebuild: None,
                            changed_hashes_truncated: None,
                        },
                    );
                    {
                        let now = chrono::Utc::now().to_rfc3339();
                        crate::runtime_state::upsert_task(RuntimeTask {
                            task_id: "ptr:sync".to_string(),
                            kind: TaskKind::PtrSync,
                            status: TaskStatus::Failed,
                            label: "PTR sync".to_string(),
                            parent_task_id: None,
                            progress: None,
                            detail: None,
                            started_at: now.clone(),
                            updated_at: now,
                        });
                    }
                }
            }
            PTR_SYNCING.store(false, Ordering::SeqCst);
            // Clear the cancellation token and progress
            if let Ok(mut guard) = PTR_CANCEL.lock() {
                *guard = None;
            }
            if let Ok(mut guard) = PTR_PROGRESS.lock() {
                *guard = None;
            }
        });

        Ok(())
    }

    pub async fn lookup_tags_for_hash(
        ptr_db: &PtrSqliteDatabase,
        hash: &str,
    ) -> Result<Vec<PtrResolvedTag>, String> {
        ptr_db.lookup_tags_for_hash(hash).await
    }

    pub async fn batch_get_overlay(
        ptr_db: &PtrSqliteDatabase,
        hashes: Vec<String>,
    ) -> Result<Vec<(String, Vec<PtrResolvedTag>)>, String> {
        ptr_db.batch_get_overlay(hashes).await
    }

    pub async fn batch_check_negative(
        ptr_db: &PtrSqliteDatabase,
        hashes: Vec<String>,
    ) -> Result<Vec<String>, String> {
        ptr_db.batch_check_negative(hashes).await
    }
}
