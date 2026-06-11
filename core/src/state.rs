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
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

/// Shared application state, accessible to all command handlers.
pub struct AppState {
    pub blob_store: Arc<BlobStore>,
    /// The new application engine boundary. All new code should call
    /// `engine` methods instead of going through `db` directly.
    pub engine: Arc<crate::engine::ApplicationEngine>,
    pub settings: SettingsStore,
    pub rate_limiter: RateLimiter,
    pub running_subscriptions: RunningSubscriptions,
    pub sub_terminal_statuses: SubTerminalStatuses,
    pub library_root: PathBuf,
    pub cancel: CancellationToken,
    pub folder_watch_commands: tokio::sync::mpsc::UnboundedSender<FolderWatchCommand>,
    /// Join handles for long-running background workers (bitmap flush, scheduler, etc.)
    /// Used by shutdown to deterministically await completion instead of sleeping.
    pub worker_handles: tokio::sync::Mutex<Vec<(&'static str, tokio::task::JoinHandle<()>)>>,
    /// AI tagger sessions — one per enabled model, lazily initialised.
    pub ai_taggers: crate::ai_tagger::inference::SharedTaggerSessions,
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

    let worker_handles = crate::workers::start_workers(
        &new_db,
        &library_root,
        &blob_store,
        &rate_limiter,
        &running_subscriptions,
        &sub_terminal_statuses,
        folder_watch_rx,
        &cancel,
    )
    .await;
    let engine = Arc::new(crate::engine::ApplicationEngine::new(new_db));

    let state = Arc::new(AppState {
        blob_store,
        engine,
        settings,
        rate_limiter,
        running_subscriptions,
        sub_terminal_statuses,
        library_root,
        cancel,
        folder_watch_commands,
        worker_handles: tokio::sync::Mutex::new(worker_handles),
        ai_taggers: crate::ai_tagger::inference::new_shared_sessions(),
    });

    {
        let mut guard = state_lock()
            .write()
            .map_err(|_| "State lock poisoned".to_string())?;
        *guard = Some(state.clone());
    }

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

        tracing::info!("Library closed");
    }
}
