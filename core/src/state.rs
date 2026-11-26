//! The library that this process has open. At most one library is open at a
//! time.
//!
//! This file opens and closes the library and sends each command to it.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use lru::LruCache;
use picto_library::OpenCancellation;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
type WorkerHandle = (&'static str, tokio::task::JoinHandle<()>);

/// The error message of an open that was cancelled by a later open, a close
/// or process shutdown. The shell matches it literally.
pub const OPEN_CANCELLED_MESSAGE: &str = "Library open was cancelled";

/// The file in a library's folder that every application holding the
/// library open keeps under an exclusive lock, so that a second application
/// (another Picto, or another frontend on the same core) cannot open it at
/// the same time. The system lets the lock go with the process, so a crash
/// leaves nothing that blocks the next open.
pub const LIBRARY_LOCK_FILE: &str = ".picto-library.lock";

/// The start of the error message of an open refused because another
/// application holds the library's lock. The shell may match it literally.
pub const LIBRARY_IN_USE_MESSAGE: &str = "This library is open in another application";

/// An exclusive lock on a library's [`LIBRARY_LOCK_FILE`], held for as long
/// as the library is open and let go when dropped.
pub struct LibraryLock {
    _file: std::fs::File,
}

/// Takes the lock of the library at `root`, creating its folder and the
/// lock file when they are missing (as creating a library does). Fails with
/// [`LIBRARY_IN_USE_MESSAGE`] while another holder, in this process or
/// another, has it.
pub fn lock_library(root: &std::path::Path) -> Result<LibraryLock, String> {
    std::fs::create_dir_all(root).map_err(|error| format!("{}: {error}", root.display()))?;
    let path = root.join(LIBRARY_LOCK_FILE);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| format!("Could not lock the library ({}): {error}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(LibraryLock { _file: file }),
        Err(std::fs::TryLockError::WouldBlock) => Err(format!(
            "{LIBRARY_IN_USE_MESSAGE} ({}). Close it there, then open it again.",
            root.display()
        )),
        Err(std::fs::TryLockError::Error(error)) => Err(format!(
            "Could not lock the library ({}): {error}",
            path.display()
        )),
    }
}

/// What the in-flight open is doing, as the shell shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
enum OpenStage {
    Opening = 0,
    Migrating = 1,
    Loading = 2,
    Starting = 3,
}

impl OpenStage {
    fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::Migrating,
            2 => Self::Loading,
            3 => Self::Starting,
            _ => Self::Opening,
        }
    }
}

/// The one open in flight, if any. A later open, a close or shutdown cancels
/// it; `library.open_status` reads it without waiting for the open.
struct OpeningLibrary {
    root: PathBuf,
    cancel: OpenCancellation,
    stage: Arc<AtomicU8>,
}

static OPENING: OnceLock<parking_lot::Mutex<Option<OpeningLibrary>>> = OnceLock::new();

fn opening_lock() -> &'static parking_lot::Mutex<Option<OpeningLibrary>> {
    OPENING.get_or_init(|| parking_lot::Mutex::new(None))
}

/// Cancel the open in flight, if any, before queueing behind it.
fn cancel_in_flight_open() {
    if let Some(opening) = opening_lock().lock().as_ref() {
        opening.cancel.cancel();
    }
}

/// Clears the in-flight record on every exit from `open_library_inner`.
struct OpeningGuard;

impl Drop for OpeningGuard {
    fn drop(&mut self) {
        *opening_lock().lock() = None;
    }
}

#[derive(serde::Serialize)]
struct OpenStatus {
    path: String,
    stage: OpenStage,
}

fn open_status() -> Option<OpenStatus> {
    opening_lock().lock().as_ref().map(|opening| OpenStatus {
        path: opening.root.to_string_lossy().into_owned(),
        stage: OpenStage::from_u8(opening.stage.load(Ordering::Relaxed)),
    })
}

pub struct BackendState {
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
    workers: tokio::sync::Mutex<Vec<WorkerHandle>>,
    read_cache: parking_lot::Mutex<LruCache<String, (u64, String)>>,
    /// The library's lock, held until the library is closed.
    lock: parking_lot::Mutex<Option<LibraryLock>>,
}

impl BackendState {
    pub fn application(&self) -> &LibraryApplication {
        &self.application
    }
}

static STATE: OnceLock<RwLock<Option<Arc<BackendState>>>> = OnceLock::new();
static LIFECYCLE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
static INVOCATIONS: OnceLock<tokio::sync::RwLock<()>> = OnceLock::new();
static APPLICATION_DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();

pub fn set_application_data_root(root: PathBuf) -> Result<(), String> {
    APPLICATION_DATA_ROOT
        .set(root)
        .map_err(|_| "Application data root is already configured".to_string())
}

pub(crate) fn application_data_root() -> Option<&'static PathBuf> {
    APPLICATION_DATA_ROOT.get()
}

fn state_lock() -> &'static RwLock<Option<Arc<BackendState>>> {
    STATE.get_or_init(|| RwLock::new(None))
}

fn lifecycle_lock() -> &'static tokio::sync::Mutex<()> {
    LIFECYCLE.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn invocation_lock() -> &'static tokio::sync::RwLock<()> {
    INVOCATIONS.get_or_init(|| tokio::sync::RwLock::new(()))
}

pub fn init_tracing() {
    use std::sync::Once;
    use tracing_subscriber::prelude::*;

    static TRACING_INIT: Once = Once::new();
    TRACING_INIT.call_once(|| {
        let env_filter =
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "picto=info,picto::ipc=info"
                    .parse()
                    .expect("valid log filter")
            });
        #[cfg(debug_assertions)]
        let env_filter = [
            "picto_core::native_source=debug",
            "picto_sources::http=debug",
            "picto_sources::providers::ehentai=debug",
            "picto_sources::providers::onlyfans=debug",
        ]
        .into_iter()
        .fold(env_filter, |filter, directive| {
            filter.add_directive(directive.parse().expect("valid debug directive"))
        });
        let _ = tracing_subscriber::registry()
            .with(env_filter)
            .with(tracing_subscriber::fmt::layer())
            .with(crate::events::EventEmitLayer)
            .try_init();
        crate::events::enable_log_forwarding();
    });
}

pub async fn open_library(library_root: PathBuf) -> Result<Arc<BackendState>, String> {
    cancel_in_flight_open();
    let _lifecycle = lifecycle_lock().lock().await;
    let _invocations = invocation_lock().write().await;
    open_library_inner(library_root, None).await
}

/// Opens a library as [`open_library`] does, and hands its `Library` to
/// `opened` as soon as the database is open: before the recovery passes run
/// and the workers start, which on a large library takes many seconds. The
/// caller can read from it meanwhile; commands still wait for the whole
/// open. If the open is then cancelled or fails, that library is never
/// published.
pub async fn open_library_reporting(
    library_root: PathBuf,
    opened: impl FnOnce(Arc<picto_library::Library>) + Send + 'static,
) -> Result<Arc<BackendState>, String> {
    cancel_in_flight_open();
    let _lifecycle = lifecycle_lock().lock().await;
    let _invocations = invocation_lock().write().await;
    open_library_stages(library_root, None, Some(Box::new(opened))).await
}

pub async fn open_tutorial_library(
    library_root: PathBuf,
    fixture_root: PathBuf,
) -> Result<Arc<BackendState>, String> {
    cancel_in_flight_open();
    let _lifecycle = lifecycle_lock().lock().await;
    let _invocations = invocation_lock().write().await;
    open_library_inner(library_root, Some(fixture_root)).await
}

/// Opens one library. The in-flight record is set before the previous
/// library is closed and is cleared when the new library is published, so a
/// cancel at any point stops the open when it reaches its next stage. The
/// synchronous open runs on the blocking thread pool, so that the runtime
/// keeps answering `library.open_status` meanwhile.
async fn open_library_inner(
    library_root: PathBuf,
    tutorial_fixture_root: Option<PathBuf>,
) -> Result<Arc<BackendState>, String> {
    open_library_stages(library_root, tutorial_fixture_root, None).await
}

/// Called with the library as soon as its database is open.
type LibraryOpened = Box<dyn FnOnce(Arc<picto_library::Library>) + Send>;

async fn open_library_stages(
    library_root: PathBuf,
    tutorial_fixture_root: Option<PathBuf>,
    readable: Option<LibraryOpened>,
) -> Result<Arc<BackendState>, String> {
    let cancel = OpenCancellation::new();
    let stage = Arc::new(AtomicU8::new(OpenStage::Opening as u8));
    *opening_lock().lock() = Some(OpeningLibrary {
        root: library_root.clone(),
        cancel: cancel.clone(),
        stage: Arc::clone(&stage),
    });
    let _opening = OpeningGuard;
    let cancelled = || -> Result<(), String> {
        if cancel.is_cancelled() {
            Err(OPEN_CANCELLED_MESSAGE.to_string())
        } else {
            Ok(())
        }
    };
    close_library_inner().await?;
    cancelled()?;
    let database_path = library_root.join("library.sqlite");
    let opened = {
        let root = library_root.clone();
        let handle = cancel.clone();
        let stage = Arc::clone(&stage);
        tokio::task::spawn_blocking(move || {
            // Taken before the database is touched, and kept while it is
            // open; another application holding it refuses this open.
            let lock = lock_library(&root)?;
            let application = if database_path.exists() {
                let mut progress = |reported: picto_library::OpenStage| {
                    let value = match reported {
                        picto_library::OpenStage::Migrating => OpenStage::Migrating,
                        picto_library::OpenStage::Loading(_) => OpenStage::Loading,
                        picto_library::OpenStage::Ready => OpenStage::Starting,
                    };
                    stage.store(value as u8, Ordering::Relaxed);
                };
                LibraryApplication::open_with(&root, &handle, &mut progress)
            } else {
                LibraryApplication::create(&root)
            }?;
            Ok::<_, String>((application, lock))
        })
        .await
        .map_err(|error| format!("Library open task failed: {error}"))?
    };
    let (application, lock) = match opened {
        Ok((application, lock)) => (Arc::new(application), lock),
        Err(_) if cancel.is_cancelled() => return Err(OPEN_CANCELLED_MESSAGE.to_string()),
        Err(error) => return Err(error),
    };
    if let Some(readable) = readable {
        readable(Arc::clone(application.library()));
    }
    if let Err(error) = crate::ai_models::migrate_legacy_storage(application.as_ref()) {
        tracing::warn!(%error, "Could not migrate legacy AI model storage");
    }
    stage.store(OpenStage::Starting as u8, Ordering::Relaxed);
    cancelled()?;
    let workers_cancel = CancellationToken::new();
    let state = Arc::new(BackendState {
        application,
        cancel: workers_cancel,
        workers: tokio::sync::Mutex::new(Vec::new()),
        read_cache: parking_lot::Mutex::new(LruCache::new(
            NonZeroUsize::new(16).expect("nonzero query cache capacity"),
        )),
        lock: parking_lot::Mutex::new(Some(lock)),
    });
    let workers = if let Some(fixture_root) = tutorial_fixture_root {
        crate::library_runtime::start_tutorial(
            Arc::clone(&state.application),
            state.cancel.clone(),
            fixture_root,
        )?
    } else {
        crate::library_runtime::start(Arc::clone(&state.application), state.cancel.clone())?
    };
    *state.workers.lock().await = workers;
    if cancel.is_cancelled() {
        // Stop the workers that were just started, through their own
        // cancellation token. No checkpoint is written for a library that
        // was never published.
        state.cancel.cancel();
        let workers = std::mem::take(&mut *state.workers.lock().await);
        join_workers(workers).await;
        return Err(OPEN_CANCELLED_MESSAGE.to_string());
    }
    *state_lock()
        .write()
        .map_err(|_| "Backend state lock poisoned".to_string())? = Some(Arc::clone(&state));
    Ok(state)
}

pub fn get_state() -> Result<Arc<BackendState>, String> {
    state_lock()
        .read()
        .map_err(|_| "Backend state lock poisoned".to_string())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "No library is open. Call open_library() first.".to_string())
}

pub async fn invoke(command: &str, args_json: &str) -> Result<String, String> {
    if command == "library.open_status" {
        // Answered from the in-flight record without waiting for the open.
        return serde_json::to_string(&open_status()).map_err(|error| error.to_string());
    }
    let _invocations = invocation_lock().read().await;
    let state = get_state()?;
    let started = Instant::now();
    // Only the commands that read nothing but the in-memory snapshot are
    // cached. A cached answer is used while the library's query epoch is
    // unchanged. The query epoch changes when anything these commands read
    // changes, and not when only the work queues change. `sidebar.counts`
    // also reads duplicate pairs from SQLite, so it is not cached; it is
    // fast enough without a cache because it does not count every tag.
    let cache_key = matches!(command, "items.query" | "items.mime_facets")
        .then(|| format!("{command}\0{args_json}"));
    let revision_before = cache_key
        .as_ref()
        .map(|_| state.application.library().query_epoch());
    if let Some(cache_key) = cache_key.as_ref() {
        if let Some((revision, result)) = state.read_cache.lock().get(cache_key) {
            if Some(*revision) == revision_before {
                return Ok(result.clone());
            }
        }
    }

    let result = crate::ipc::dispatch_library_async(&state.application, command, args_json)
        .await?
        .ok_or_else(|| format!("Unknown schema-1 command: {command}"));
    if let (Some(cache_key), Ok(serialized)) = (cache_key, result.as_ref()) {
        let revision_after = state.application.library().query_epoch();
        if revision_before == Some(revision_after) {
            state
                .read_cache
                .lock()
                .put(cache_key, (revision_after, serialized.clone()));
        }
    }
    let duration_ms = started.elapsed().as_secs_f64() * 1_000.0;
    if let Ok(serialized) = result.as_ref() {
        audit_successful_mutation(command, args_json, serialized, duration_ms);
    }
    let succeeded = result.is_ok();
    if duration_ms >= 100.0 {
        tracing::warn!(target: "picto::ipc", command, duration_ms, succeeded, "Core command completed");
    } else if duration_ms >= 16.0 {
        tracing::info!(target: "picto::ipc", command, duration_ms, succeeded, "Core command completed");
    } else {
        tracing::debug!(target: "picto::ipc", command, duration_ms, succeeded, "Core command completed");
    }
    result
}

fn audit_successful_mutation(command: &str, args_json: &str, result_json: &str, duration_ms: f64) {
    if !result_json.contains("\"resources\"") || !result_json.contains("\"item_ids\"") {
        return;
    }
    let Ok(result) = serde_json::from_str::<Value>(result_json) else {
        return;
    };
    let Some(receipt) = find_mutation_receipt(&result) else {
        return;
    };
    tracing::info!(
        target: "picto::audit",
        action = command,
        duration_ms,
        revision = receipt.revision,
        resources = ?receipt.resources,
        affected_item_ids = %summarize_item_ids(&receipt.item_ids),
        identifiers = %safe_identifiers(args_json),
        "Mutation completed"
    );
}

fn find_mutation_receipt(value: &Value) -> Option<picto_library::MutationReceipt> {
    if let Ok(receipt) = serde_json::from_value::<picto_library::MutationReceipt>(value.clone()) {
        return Some(receipt);
    }
    match value {
        Value::Object(object) => object.values().find_map(find_mutation_receipt),
        Value::Array(values) => values.iter().find_map(find_mutation_receipt),
        _ => None,
    }
}

fn safe_identifiers(args_json: &str) -> Value {
    const SAFE_KEYS: &[&str] = &[
        "item_id",
        "item_ids",
        "entity_hash",
        "entity_hashes",
        "file_hash",
        "file_hashes",
        "folder_id",
        "folder_ids",
        "folder_node_id",
        "folder_node_ids",
        "smart_folder_id",
        "smart_folder_ids",
        "tag_id",
        "tag_ids",
        "subscription_id",
        "query_id",
        "collection_id",
        "run_id",
        "site_id",
    ];
    fn collect(value: &Value, output: &mut Map<String, Value>) {
        match value {
            Value::Object(object) => {
                for (key, value) in object {
                    if SAFE_KEYS.contains(&key.as_str()) {
                        output.insert(key.clone(), summarize_identifier(value));
                    }
                    collect(value, output);
                }
            }
            Value::Array(values) => values.iter().for_each(|value| collect(value, output)),
            _ => {}
        }
    }
    let mut output = Map::new();
    if let Ok(args) = serde_json::from_str::<Value>(args_json) {
        collect(&args, &mut output);
    }
    Value::Object(output)
}

fn summarize_identifier(value: &Value) -> Value {
    let Value::Array(values) = value else {
        return value.clone();
    };
    if values.len() <= 12 {
        return value.clone();
    }
    serde_json::json!({"values": values.iter().take(12).cloned().collect::<Vec<_>>(), "total": values.len()})
}

fn summarize_item_ids(item_ids: &[picto_library::RootId]) -> Value {
    let values = item_ids
        .iter()
        .take(12)
        .map(|item_id| Value::from(item_id.0))
        .collect::<Vec<_>>();
    if item_ids.len() <= 12 {
        Value::Array(values)
    } else {
        serde_json::json!({"values": values, "total": item_ids.len()})
    }
}

pub async fn close_library() -> Result<(), String> {
    cancel_in_flight_open();
    let _lifecycle = lifecycle_lock().lock().await;
    let _invocations = invocation_lock().write().await;
    close_library_inner().await
}

async fn close_library_inner() -> Result<(), String> {
    let state = state_lock()
        .write()
        .map_err(|_| "Backend state lock poisoned".to_string())?
        .take();
    let Some(state) = state else {
        return Ok(());
    };
    // Let go once the library is checkpointed, or failed to be: the lock
    // leaves with this function whatever happens below.
    let _lock = state.lock.lock().take();
    state.cancel.cancel();
    state.application.cancel_ai_model_downloads().await;
    let workers = std::mem::take(&mut *state.workers.lock().await);
    join_workers(workers).await;
    state
        .application
        .library()
        .write_projection_checkpoint()
        .map_err(|error| error.to_string())?;
    state
        .application
        .library()
        .database()
        .checkpoint_wal()
        .map_err(|error| error.to_string())
}

async fn join_workers(workers: Vec<WorkerHandle>) {
    let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
    for (name, mut worker) in workers {
        match tokio::time::timeout_at(deadline, &mut worker).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(worker = name, %error, "Backend worker join failed"),
            Err(_) => {
                tracing::warn!(worker = name, "Backend worker exceeded shutdown timeout");
                worker.abort();
                let _ = worker.await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    /// The tests that open the one global library take turns.
    static GLOBAL_LIBRARY: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_close_racing_an_open_leaves_nothing_open_and_no_in_flight_record() {
        let _turn = GLOBAL_LIBRARY.lock().await;
        assert_eq!(
            super::invoke("library.open_status", "{}").await.unwrap(),
            "null"
        );
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Racing.library");
        let opening = super::open_library(root.clone());
        tokio::pin!(opening);
        // Poll the open until it pauses with its in-flight record set. The
        // open's future stays in this test, so the open cannot publish the
        // library before the close below is polled.
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            tokio::select! {
                biased;
                _ = &mut opening => panic!("open completed before the cancellation boundary"),
                _ = async {
                    while super::open_status().is_none() {
                        tokio::task::yield_now().await;
                    }
                } => {}
            }
        })
        .await
        .expect("open did not reach its in-flight boundary");
        let (closed, opened) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            tokio::join!(biased; super::close_library(), &mut opening)
        })
        .await
        .expect("cancelled open did not settle");
        closed.unwrap();
        assert!(matches!(opened, Err(error) if error == super::OPEN_CANCELLED_MESSAGE));
        // This is checked before the test closes the library again, because
        // a second close would hide a first close that left a library open.
        assert!(super::get_state().is_err());
        assert_eq!(
            super::invoke("library.open_status", "{}").await.unwrap(),
            "null"
        );
        // The directory is still a valid library afterwards.
        super::open_library(root).await.unwrap();
        assert!(super::get_state().is_ok());
        super::close_library().await.unwrap();
        assert!(super::get_state().is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_reported_open_hands_over_the_library_before_it_is_published() {
        let _turn = GLOBAL_LIBRARY.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Reported.library");
        // The library is created and closed, so that the open below opens
        // an existing library.
        super::open_library(root.clone()).await.unwrap();
        super::close_library().await.unwrap();
        let (sender, handed) = std::sync::mpsc::channel();
        let state = super::open_library_reporting(root, move |library| {
            // The library is not published yet, because the recovery passes
            // and the workers have not started.
            let published = super::get_state().is_ok();
            sender.send((library, published)).unwrap();
        })
        .await
        .unwrap();
        let (library, published) = handed.try_recv().unwrap();
        assert!(!published);
        assert!(std::sync::Arc::ptr_eq(
            &library,
            state.application().library()
        ));
        assert!(library.sidebar_counts().is_ok());
        super::close_library().await.unwrap();
    }

    /// An open library's lock refuses a second holder, in this process or
    /// another; a library locked elsewhere is not opened; closing lets go,
    /// so the same library opens again.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_open_library_is_locked_until_it_is_closed() {
        let _turn = GLOBAL_LIBRARY.lock().await;
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("Locked.library");
        super::open_library(root.clone()).await.unwrap();
        assert!(root.join(super::LIBRARY_LOCK_FILE).is_file());
        let refused = super::lock_library(&root).err().unwrap();
        assert!(
            refused.starts_with(super::LIBRARY_IN_USE_MESSAGE),
            "{refused}"
        );
        super::close_library().await.unwrap();
        // Once the library is closed, its lock can be taken.
        drop(super::lock_library(&root).unwrap());
        // While another holder has the lock, the open fails with
        // `LIBRARY_IN_USE_MESSAGE` and no library is open.
        let held = super::lock_library(&root).unwrap();
        let error = super::open_library(root.clone()).await.err().unwrap();
        assert!(error.starts_with(super::LIBRARY_IN_USE_MESSAGE), "{error}");
        assert!(super::get_state().is_err());
        drop(held);
        super::open_library(root.clone()).await.unwrap();
        // Reopening the open library lets go of its own lock first.
        super::open_library(root).await.unwrap();
        super::close_library().await.unwrap();
    }
}
