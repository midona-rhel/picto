//! Global application state and initialization.
//!
//! Supports library switching: `open_library()` closes the previous library
//! (if any), opens a new one, and spawns background tasks. `close_library()`
//! shuts everything down cleanly via a `CancellationToken`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::sqlite::SqliteDatabase;
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

    let worker_handles = crate::workers::start_workers(
        &library_db,
        &ptr_db,
        &blob_store,
        &rate_limiter,
        &running_subscriptions,
        &sub_terminal_statuses,
        &cancel,
    )
    .await;

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

        if let Err(e) = state.db.flush().await {
            tracing::warn!("Final flush on close failed: {e}");
        }

        tracing::info!("Library closed");
    }
}
