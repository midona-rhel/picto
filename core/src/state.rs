//! Global application state and initialization.
//!
//! Supports library switching: `open_library()` closes the previous library
//! (if any), opens a new one, and spawns background tasks. `close_library()`
//! shuts everything down cleanly via a `CancellationToken`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::folders::watch::FolderWatchCommand;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::types::RunningSubscriptions;

/// Shared application state, accessible to all command handlers.
pub struct AppState {
    pub blob_store: Arc<BlobStore>,
    /// The new application engine boundary. All new code should call
    /// `engine` methods instead of going through `db` directly.
    pub engine: Arc<crate::engine::ApplicationEngine>,
    pub settings: SettingsStore,
    pub rate_limiter: RateLimiter,
    pub running_subscriptions: RunningSubscriptions,
    pub library_root: PathBuf,
    pub cancel: CancellationToken,
    pub folder_watch_commands: tokio::sync::mpsc::UnboundedSender<FolderWatchCommand>,
    /// Join handles for long-running background workers (bitmap flush, scheduler, etc.)
    /// Used by shutdown to deterministically await completion instead of sleeping.
    pub worker_handles: tokio::sync::Mutex<Vec<(&'static str, tokio::task::JoinHandle<()>)>>,
    /// AI tagger sessions — one per enabled model, lazily initialised.
    pub ai_taggers: crate::ai_tagger::inference::SharedTaggerSessions,
    /// Identity and cancellation token for the latest reviewed prediction.
    pub ai_tag_run: tokio::sync::Mutex<Option<(u64, CancellationToken)>>,
    /// Active model downloads keyed by registered model slug.
    pub ai_model_downloads: tokio::sync::Mutex<HashMap<String, CancellationToken>>,
    /// Serializes model activation/deletion/loading with inference.
    pub ai_model_lifecycle: tokio::sync::Mutex<()>,
}

static STATE: OnceLock<RwLock<Option<Arc<AppState>>>> = OnceLock::new();

fn state_lock() -> &'static RwLock<Option<Arc<AppState>>> {
    STATE.get_or_init(|| RwLock::new(None))
}

/// Initialize tracing subscriber once per process.
/// Safe to call multiple times — subsequent calls are no-ops.
pub fn init_tracing() {
    use std::sync::Once;
    static TRACING_INIT: Once = Once::new();
    TRACING_INIT.call_once(|| {
        use tracing_subscriber::prelude::*;
        let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "picto=info".parse().unwrap());
        let fmt_layer = tracing_subscriber::fmt::layer();
        let _ = tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(crate::events::EventEmitLayer)
            .try_init();
        crate::events::enable_log_forwarding();
    });
}

/// Open a library, closing any previously open library first.
///
/// `library_root` is the path to the library directory
/// (e.g. `~/.local/share/picto/library` or a `.library/` folder).
pub async fn open_library(library_root: PathBuf) -> Result<Arc<AppState>, String> {
    close_library_inner().await;

    std::fs::create_dir_all(&library_root)
        .map_err(|e| format!("Failed to create library directory: {}", e))?;

    let settings = SettingsStore::load(&library_root);
    let models_root = library_root
        .parent()
        .unwrap_or(&library_root)
        .join("models");
    crate::ai_tagger::download::recover_registered_bundles(&models_root).await?;

    let blob_store: Arc<BlobStore> = Arc::new(
        BlobStore::open(&library_root).map_err(|e| format!("Failed to open blob store: {}", e))?,
    );

    crate::constants::init_groupings();

    let rate_limiter = RateLimiter::new();
    let running_subscriptions: RunningSubscriptions =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    let cancel = CancellationToken::new();
    let (folder_watch_commands, folder_watch_rx) = crate::folders::watch::channel();

    let new_db = Arc::new(
        crate::db::LibraryDatabase::open(&library_root)
            .map_err(|e| format!("Failed to open new LibraryDatabase: {e}"))?,
    );

    match crate::media_analysis::enqueue_stale_color_backfill(&new_db) {
        Ok(count) if count > 0 => tracing::info!(count, "Queued stale color analysis backfill"),
        Ok(_) => {}
        Err(e) => tracing::warn!(error = %e, "Color analysis backfill enqueue failed"),
    }

    let engine = Arc::new(crate::engine::ApplicationEngine::new(new_db.clone()));

    // Reclaim orphaned blobs in the background — off the open critical path.
    // The 10-minute age guard keeps in-flight ingest staging safe.
    {
        let sweep_db = new_db.clone();
        let sweep_blobs = blob_store.clone();
        tokio::task::spawn_blocking(move || {
            let referenced = sweep_db.with_read(|conn| {
                let mut stmt = conn.prepare("SELECT file_hash FROM media_file")?;
                let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
                rows.collect::<rusqlite::Result<std::collections::HashSet<_>>>()
            });
            match referenced {
                Ok(referenced) => match sweep_blobs
                    .orphan_candidates(&referenced, std::time::Duration::from_secs(600))
                {
                    Ok(candidates) => {
                        let mut deleted = 0;
                        let mut freed = 0;
                        let mut errors = 0;
                        for candidate in candidates {
                            match sweep_db
                                .enqueue_blob_delete_and_attempt(&sweep_blobs, &candidate.hash)
                            {
                                Ok(crate::db::types::BlobCleanupResult::Deleted) => {
                                    deleted += candidate.file_count;
                                    freed += candidate.bytes;
                                }
                                Ok(crate::db::types::BlobCleanupResult::CancelledReferenced) => {}
                                Err(error) => {
                                    errors += 1;
                                    tracing::error!(
                                        file_hash = %candidate.hash,
                                        error = %error,
                                        "Startup orphan blob cleanup deferred"
                                    );
                                }
                            }
                        }
                        if deleted > 0 || errors > 0 {
                            tracing::info!(
                                deleted,
                                freed,
                                errors,
                                "startup orphaned blob sweep complete"
                            );
                        }
                    }
                    Err(error) => {
                        tracing::error!(error = %error, "Startup orphaned blob enumeration failed");
                    }
                },
                Err(error) => {
                    tracing::error!(error = %error, "Startup orphaned blob reference query failed");
                }
            }
        });
    }

    let state = Arc::new(AppState {
        blob_store,
        engine,
        settings,
        rate_limiter,
        running_subscriptions,
        library_root,
        cancel,
        folder_watch_commands,
        worker_handles: tokio::sync::Mutex::new(Vec::new()),
        ai_taggers: crate::ai_tagger::inference::new_shared_sessions(),
        ai_tag_run: tokio::sync::Mutex::new(None),
        ai_model_downloads: tokio::sync::Mutex::new(HashMap::new()),
        ai_model_lifecycle: tokio::sync::Mutex::new(()),
    });

    {
        let mut guard = state_lock()
            .write()
            .map_err(|_| "State lock poisoned".to_string())?;
        *guard = Some(state.clone());
    }

    let worker_handles = crate::workers::start_workers(
        &new_db,
        &state.library_root,
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        folder_watch_rx,
        &state.cancel,
    )
    .await;
    state.worker_handles.lock().await.extend(worker_handles);

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
        crate::workers::stop_workers(handles).await;

        // Checkpoint explicitly: a detached worker can hold an Arc to the
        // database past this point, so Drop alone would run too late (or,
        // on process exit, not at all).
        state.engine.db().checkpoint();

        tracing::info!("Library closed");
    }
}
