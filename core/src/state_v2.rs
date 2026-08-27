//! Process-level ownership for the replacement backend.
//!
//! Exactly one library is open at a time. Opening another library first
//! cancels and joins the previous library's durable workers.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use lru::LruCache;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::app::{Application, ItemId, MutationReceipt};
use crate::store::Store;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
type WorkerHandle = (&'static str, tokio::task::JoinHandle<()>);

pub struct BackendState {
    application: Arc<Application>,
    cancel: CancellationToken,
    workers: tokio::sync::Mutex<Vec<WorkerHandle>>,
    read_cache: parking_lot::Mutex<LruCache<String, (u64, String)>>,
}

impl BackendState {
    pub fn application(&self) -> &Application {
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

/// Initialize process logging and native event forwarding once.
pub fn init_tracing() {
    use std::sync::Once;
    use tracing_subscriber::prelude::*;

    static TRACING_INIT: Once = Once::new();
    TRACING_INIT.call_once(|| {
        let env_filter =
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "picto=info,picto::ipc=info"
                    .parse()
                    .expect("valid default tracing filter")
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
    let _lifecycle = lifecycle_lock().lock().await;
    let _invocations = invocation_lock().write().await;
    open_library_inner(library_root, None).await
}

/// Open an isolated tutorial library. Only the maintenance worker and the
/// deterministic bundled-fixture subscription runner are started; cloud,
/// watched folders, schedules, credentials, and network-capable runners stay
/// inactive for the lifetime of this backend.
pub async fn open_tutorial_library(
    library_root: PathBuf,
    fixture_root: PathBuf,
) -> Result<Arc<BackendState>, String> {
    let _lifecycle = lifecycle_lock().lock().await;
    let _invocations = invocation_lock().write().await;
    open_library_inner(library_root, Some(fixture_root)).await
}

async fn open_library_inner(
    library_root: PathBuf,
    tutorial_fixture_root: Option<PathBuf>,
) -> Result<Arc<BackendState>, String> {
    close_library_inner().await?;

    let store = Arc::new(Store::open(&library_root)?);
    let application = Arc::new(Application::try_new(store)?);
    if let Err(error) = crate::ai_models_v2::migrate_legacy_storage(&application) {
        tracing::warn!(%error, "Could not migrate legacy AI model storage");
    }
    let cancel = CancellationToken::new();
    let state = Arc::new(BackendState {
        application,
        cancel,
        workers: tokio::sync::Mutex::new(Vec::new()),
        read_cache: parking_lot::Mutex::new(LruCache::new(
            NonZeroUsize::new(16).expect("nonzero query cache capacity"),
        )),
    });

    {
        let mut guard = state_lock()
            .write()
            .map_err(|_| "Replacement state lock poisoned".to_string())?;
        *guard = Some(Arc::clone(&state));
    }

    let workers = if let Some(fixture_root) = tutorial_fixture_root {
        crate::runtime_v2::start_tutorial(
            Arc::clone(&state.application),
            state.cancel.clone(),
            fixture_root,
        )?
    } else {
        reconcile_cloud_before_mount(state.application()).await;
        crate::runtime_v2::start(Arc::clone(&state.application), state.cancel.clone())?
    };
    *state.workers.lock().await = workers;
    Ok(state)
}

async fn reconcile_cloud_before_mount(application: &Application) {
    let configured = application
        .store()
        .read(|connection| {
            connection.query_row(
                "SELECT provider IS NOT NULL AND paused = 0 FROM cloud_state WHERE singleton = 1",
                [],
                |row| row.get::<_, bool>(0),
            )
        })
        .unwrap_or(false);
    if !configured {
        return;
    }
    let provider = match crate::cloud::directory_provider(application) {
        Ok(provider) => provider,
        Err(error) => {
            tracing::warn!(error = %error, "Cloud folder is unavailable during startup");
            return;
        }
    };
    let pending = match crate::cloud::reconcile::metadata_pending(application, &provider).await {
        Ok(pending) => pending,
        Err(error) => {
            tracing::warn!(error = %error, "Checking startup cloud frontier failed");
            return;
        }
    };
    if !pending {
        return;
    }
    let shown_at = Instant::now();
    if let Err(error) = crate::cloud::reconcile::reconcile(
        application,
        &provider,
        crate::cloud::reconcile::ReconcileMode::Startup,
    )
    .await
    {
        // The active local database remains untouched by a rejected artifact.
        // Persisted cloud status exposes the failure after the workspace opens.
        tracing::warn!(error = %error, "Startup cloud reconciliation failed");
    }
    let minimum = Duration::from_secs(1);
    if let Some(remaining) = minimum.checked_sub(shown_at.elapsed()) {
        tokio::time::sleep(remaining).await;
    }
}

pub fn get_state() -> Result<Arc<BackendState>, String> {
    state_lock()
        .read()
        .map_err(|_| "Replacement state lock poisoned".to_string())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "No library is open. Call open_library() first.".to_string())
}

pub async fn invoke(command: &str, args_json: &str) -> Result<String, String> {
    if command == "cloud.providers.detect" {
        return serde_json::to_string(&crate::cloud::provider::detect_roots())
            .map_err(|error| error.to_string());
    }
    if command == "cloud.libraries.discover" {
        #[derive(serde::Deserialize)]
        struct Input {
            root_path: String,
        }
        let input: Input = serde_json::from_str(args_json)
            .map_err(|error| format!("Invalid command arguments: {error}"))?;
        return serde_json::to_string(&crate::cloud::discover_libraries(&input.root_path).await?)
            .map_err(|error| error.to_string());
    }
    if matches!(command, "cloud.restore.start" | "cloud.library.join") {
        let _invocations = invocation_lock().write().await;
        return match command {
            "cloud.restore.start" => restore_cloud_snapshot(args_json).await,
            "cloud.library.join" => join_cloud_library(args_json).await,
            _ => unreachable!(),
        };
    }
    let _invocations = invocation_lock().read().await;
    let state = get_state()?;
    let started = Instant::now();
    let cache_key = matches!(command, "items.query" | "sidebar.counts")
        .then(|| format!("{command}\0{args_json}"));
    let query_revision = if let Some(cache_key) = cache_key.as_ref() {
        let revision = state.application.store().revision()?;
        if let Some((cached_revision, cached_result)) = state.read_cache.lock().get(cache_key) {
            if *cached_revision == revision {
                return Ok(cached_result.clone());
            }
        }
        Some(revision)
    } else {
        None
    };
    let result = crate::ipc_v2::dispatch_async(state.application(), command, args_json).await;
    if let Some(revision_before) = query_revision {
        if let Ok(serialized) = result.as_ref() {
            let revision_after = state.application.store().revision()?;
            if revision_before == revision_after {
                state.read_cache.lock().put(
                    cache_key.expect("cached reads have a cache key"),
                    (revision_after, serialized.clone()),
                );
            }
        }
    }
    let duration_ms = started.elapsed().as_secs_f64() * 1_000.0;
    if let Ok(serialized) = result.as_ref() {
        audit_successful_mutation(command, args_json, serialized, duration_ms);
    }
    if duration_ms >= 100.0 {
        tracing::warn!(
            target: "picto::ipc",
            command,
            duration_ms,
            succeeded = result.is_ok(),
            "Core command completed"
        );
    } else if duration_ms >= 16.0 {
        tracing::info!(
            target: "picto::ipc",
            command,
            duration_ms,
            succeeded = result.is_ok(),
            "Core command completed"
        );
    } else {
        tracing::debug!(
            target: "picto::ipc",
            command,
            duration_ms,
            succeeded = result.is_ok(),
            "Core command completed"
        );
    }
    result
}

#[derive(serde::Deserialize)]
struct RestoreCloudSnapshotInput {
    snapshot_id: String,
}

#[derive(serde::Deserialize)]
struct JoinCloudLibraryInput {
    provider: String,
    account_label: String,
    root_path: String,
    library_id: String,
    target_root: String,
}

#[derive(serde::Serialize)]
struct JoinedCloudLibrary {
    library_id: String,
    snapshot_id: String,
    path: String,
}

struct LocalCloudConfiguration {
    device_id: String,
    provider: Option<String>,
    account_label: Option<String>,
    remote_root: Option<String>,
    paused: bool,
    retention_json: String,
}

async fn restore_cloud_snapshot(args_json: &str) -> Result<String, String> {
    let input: RestoreCloudSnapshotInput = serde_json::from_str(args_json)
        .map_err(|error| format!("Invalid command arguments: {error}"))?;
    let state = get_state()?;
    let local = state.application.store().read(|connection| {
        connection.query_row(
            "SELECT device_id, provider, account_label, remote_root, paused, retention_json
             FROM cloud_state WHERE singleton = 1",
            [],
            |row| {
                Ok(LocalCloudConfiguration {
                    device_id: row.get(0)?,
                    provider: row.get(1)?,
                    account_label: row.get(2)?,
                    remote_root: row.get(3)?,
                    paused: row.get::<_, i64>(4)? != 0,
                    retention_json: row.get(5)?,
                })
            },
        )
    })?;
    let provider = crate::cloud::directory_provider(state.application())?;
    let prepared = crate::cloud::snapshot::prepare_restore(
        state.application.store(),
        &provider,
        &input.snapshot_id,
    )
    .await?;
    let library_root = state.application.store().library_root().to_path_buf();
    drop(state);
    close_library_inner().await?;

    let database_path = library_root.join(crate::store::DATABASE_FILE);
    if let Err(error) = consolidate_closed_database(&database_path) {
        let _ = open_library_inner(library_root, None).await;
        return Err(error);
    }
    if let Some(parent) = prepared.emergency_copy_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create emergency restore directory: {error}"))?;
    }
    std::fs::rename(&database_path, &prepared.emergency_copy_path)
        .map_err(|error| format!("Failed to preserve the active database: {error}"))?;
    if let Err(error) = std::fs::rename(&prepared.database_path, &database_path) {
        let _ = remove_sqlite_sidecars(&database_path);
        let _ = std::fs::rename(&prepared.emergency_copy_path, &database_path);
        let _ = open_library_inner(library_root, None).await;
        return Err(format!("Failed to activate the restored database: {error}"));
    }

    let activation = (|| -> Result<(), String> {
        let connection = rusqlite::Connection::open(&database_path)
            .map_err(|error| format!("Failed to open the restored database: {error}"))?;
        crate::store::schema::validate(&connection)?;
        connection
            .execute(
                "UPDATE cloud_state SET device_id = ?1, provider = ?2, account_label = ?3,
                        remote_root = ?4, paused = ?5, retention_json = ?6,
                        state = CASE WHEN ?5 THEN 'paused' ELSE 'idle' END,
                        phase = 'idle', blocking = 0, completed_units = 0,
                        total_units = NULL, message = ''
                 WHERE singleton = 1",
                rusqlite::params![
                    local.device_id,
                    local.provider,
                    local.account_label,
                    local.remote_root,
                    i64::from(local.paused),
                    local.retention_json,
                ],
            )
            .map_err(|error| format!("Failed to restore device-local cloud settings: {error}"))?;
        Ok(())
    })();
    if let Err(error) = activation {
        let failed =
            database_path.with_extension(format!("failed-restore-{}.sqlite", uuid::Uuid::new_v4()));
        let _ = remove_sqlite_sidecars(&database_path);
        let _ = std::fs::rename(&database_path, failed);
        let _ = std::fs::rename(&prepared.emergency_copy_path, &database_path);
        let _ = open_library_inner(library_root, None).await;
        return Err(error);
    }
    if let Err(error) = open_library_inner(library_root.clone(), None).await {
        let failed =
            database_path.with_extension(format!("failed-restore-{}.sqlite", uuid::Uuid::new_v4()));
        let _ = remove_sqlite_sidecars(&database_path);
        let _ = std::fs::rename(&database_path, failed);
        let _ = std::fs::rename(&prepared.emergency_copy_path, &database_path);
        let _ = open_library_inner(library_root, None).await;
        return Err(format!("Restored database failed to open: {error}"));
    }
    serde_json::to_string(&crate::ipc_v2::CloudRestorePrepared {
        snapshot_id: prepared.snapshot_id,
        restored: true,
    })
    .map_err(|error| error.to_string())
}

/// Turn a closed WAL database into one self-contained main database before it
/// is moved aside. A restore must never leave an old generation's WAL/SHM at
/// the canonical path: snapshots share the same SQLite lineage, so those WAL
/// frames can otherwise be validly replayed over the restored main database.
fn consolidate_closed_database(database_path: &std::path::Path) -> Result<(), String> {
    let connection = rusqlite::Connection::open(database_path)
        .map_err(|error| format!("Failed to open the active database for restore: {error}"))?;
    connection
        .busy_timeout(Duration::from_secs(5))
        .map_err(|error| format!("Failed to configure restore checkpoint: {error}"))?;
    let (busy, remaining): (i64, i64) = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)? - row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|error| {
            format!("Failed to checkpoint the active database for restore: {error}")
        })?;
    if busy != 0 || remaining != 0 {
        return Err(format!(
            "Active database could not be checkpointed safely (busy={busy}, remaining={remaining})"
        ));
    }
    drop(connection);
    remove_sqlite_sidecars(database_path)
}

fn remove_sqlite_sidecars(database_path: &std::path::Path) -> Result<(), String> {
    for suffix in ["-wal", "-shm"] {
        let mut sidecar = database_path.as_os_str().to_os_string();
        sidecar.push(suffix);
        let sidecar = PathBuf::from(sidecar);
        match std::fs::remove_file(&sidecar) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Failed to remove stale SQLite sidecar {}: {error}",
                    sidecar.display()
                ));
            }
        }
    }
    Ok(())
}

async fn join_cloud_library(args_json: &str) -> Result<String, String> {
    let input: JoinCloudLibraryInput = serde_json::from_str(args_json)
        .map_err(|error| format!("Invalid command arguments: {error}"))?;
    if !matches!(input.provider.as_str(), "google_drive" | "dropbox") {
        return Err(format!(
            "Unsupported cloud folder provider: {}",
            input.provider
        ));
    }
    let target_root = PathBuf::from(&input.target_root);
    let database_path = target_root.join(crate::store::DATABASE_FILE);
    if database_path.exists() {
        return Err("The destination already contains a Picto library".to_string());
    }
    let provider = crate::cloud::provider::DirectoryProvider::open_provider_root(
        &input.provider,
        &input.root_path,
    )?;
    let prepared =
        crate::cloud::snapshot::prepare_join(&provider, &input.library_id, &target_root).await?;

    // Keep the current library mounted until the remote database has passed
    // checksum, schema, quick-check, and foreign-key validation.
    let previous_root = get_state()
        .ok()
        .map(|state| state.application().store().library_root().to_path_buf());
    close_library_inner().await?;
    std::fs::create_dir_all(target_root.join("blobs"))
        .map_err(|error| format!("Failed to create joined library directory: {error}"))?;
    if let Err(error) = std::fs::rename(&prepared.database_path, &database_path) {
        if let Some(previous_root) = previous_root {
            let _ = open_library_inner(previous_root, None).await;
        }
        return Err(format!("Failed to activate joined cloud library: {error}"));
    }

    let initialize = initialize_joined_database(
        &database_path,
        &input.provider,
        &input.account_label,
        &input.root_path,
    );
    if let Err(error) = initialize {
        let failed = target_root.join(format!("failed-cloud-join-{}.sqlite", uuid::Uuid::new_v4()));
        let _ = std::fs::rename(&database_path, failed);
        if let Some(previous_root) = previous_root {
            let _ = open_library_inner(previous_root, None).await;
        }
        return Err(error);
    }
    if let Err(error) = open_library_inner(target_root.clone(), None).await {
        let failed = target_root.join(format!("failed-cloud-join-{}.sqlite", uuid::Uuid::new_v4()));
        let _ = std::fs::rename(&database_path, failed);
        if let Some(previous_root) = previous_root {
            let _ = open_library_inner(previous_root, None).await;
        }
        return Err(format!("Joined cloud library failed to open: {error}"));
    }
    serde_json::to_string(&JoinedCloudLibrary {
        library_id: prepared.library_id,
        snapshot_id: prepared.snapshot_id,
        path: target_root.to_string_lossy().into_owned(),
    })
    .map_err(|error| error.to_string())
}

fn initialize_joined_database(
    database_path: &std::path::Path,
    provider: &str,
    account_label: &str,
    remote_root: &str,
) -> Result<(), String> {
    let mut connection = rusqlite::Connection::open(database_path)
        .map_err(|error| format!("Failed to initialize joined cloud library: {error}"))?;
    crate::store::schema::validate(&connection)?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to initialize joined cloud library: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    transaction
        .execute(
            "UPDATE cloud_state SET device_id = ?1, provider = ?2, account_label = ?3,
                    remote_root = ?4, paused = 0, state = 'idle', phase = 'idle',
                    blocking = 0, completed_units = 0, total_units = NULL, message = '',
                    pending_blobs = (SELECT COUNT(*) FROM media_file), missing_blobs = 0
             WHERE singleton = 1",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                provider,
                account_label,
                remote_root
            ],
        )
        .map_err(|error| format!("Failed to assign joined device identity: {error}"))?;
    transaction
        .execute(
            "UPDATE folder SET watch_path = NULL, watch_enabled = 0, watch_subfolders = 0",
            [],
        )
        .map_err(|error| format!("Failed to clear device-local folder watches: {error}"))?;
    transaction
        .execute("DELETE FROM view_pref", [])
        .map_err(|error| format!("Failed to clear device-local views: {error}"))?;
    transaction
        .execute(
            "INSERT INTO cloud_blob_state (file_hash, state, updated_at)
             SELECT file_hash, 'queued', ?1 FROM media_file WHERE 1
             ON CONFLICT(file_hash) DO UPDATE SET
                 state = 'queued', priority = 0, last_error = NULL, updated_at = excluded.updated_at",
            [&now],
        )
        .map_err(|error| format!("Failed to queue cloud media recovery: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit joined cloud library: {error}"))?;
    Ok(())
}

fn audit_successful_mutation(command: &str, args_json: &str, result_json: &str, duration_ms: f64) {
    // Avoid parsing normal read responses, especially large grid pages.
    if !result_json.contains("\"resources\"") || !result_json.contains("\"item_ids\"") {
        return;
    }
    let Ok(result) = serde_json::from_str::<Value>(result_json) else {
        return;
    };
    let Some(receipt) = find_mutation_receipt(&result) else {
        return;
    };
    let identifiers = safe_identifiers(args_json);
    let affected_item_ids = summarize_item_ids(&receipt.item_ids);
    tracing::info!(
        target: "picto::audit",
        action = command,
        duration_ms,
        revision = receipt.revision,
        resources = ?receipt.resources,
        affected_item_ids = %affected_item_ids,
        identifiers = %identifiers,
        "Mutation completed"
    );
}

fn find_mutation_receipt(value: &Value) -> Option<MutationReceipt> {
    if let Ok(receipt) = serde_json::from_value::<MutationReceipt>(value.clone()) {
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
            Value::Array(values) => {
                for value in values {
                    collect(value, output);
                }
            }
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
    serde_json::json!({
        "values": values.iter().take(12).cloned().collect::<Vec<_>>(),
        "total": values.len(),
    })
}

fn summarize_item_ids(item_ids: &[ItemId]) -> Value {
    let values = item_ids
        .iter()
        .take(12)
        .map(|item_id| Value::from(item_id.0))
        .collect::<Vec<_>>();
    if item_ids.len() <= 12 {
        Value::Array(values)
    } else {
        serde_json::json!({ "values": values, "total": item_ids.len() })
    }
}

pub async fn close_library() -> Result<(), String> {
    let _lifecycle = lifecycle_lock().lock().await;
    let _invocations = invocation_lock().write().await;
    close_library_inner().await
}

async fn close_library_inner() -> Result<(), String> {
    let state = {
        let mut guard = state_lock()
            .write()
            .map_err(|_| "Replacement state lock poisoned".to_string())?;
        guard.take()
    };
    let Some(state) = state else {
        return Ok(());
    };

    state.cancel.cancel();
    state.application.cancel_ai_model_downloads().await;
    let workers = {
        let mut guard = state.workers.lock().await;
        std::mem::take(&mut *guard)
    };
    join_workers(workers).await;
    state.application.store().checkpoint()
}

async fn join_workers(workers: Vec<WorkerHandle>) {
    let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
    for (name, mut worker) in workers {
        match tokio::time::timeout_at(deadline, &mut worker).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(worker = name, error = %error, "Replacement worker join failed");
            }
            Err(_) => {
                tracing::warn!(
                    worker = name,
                    "Replacement worker exceeded shutdown timeout"
                );
                worker.abort();
                let _ = worker.await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[tokio::test]
    async fn opens_without_resolving_the_gallery_dl_bridge() {
        let _test = state_test_lock().lock().await;
        let directory = tempfile::tempdir().unwrap();
        let state = open_library(directory.path().to_path_buf()).await.unwrap();

        let settings = invoke("settings.get", "{}").await.unwrap();
        assert!(settings.contains("revision"));
        assert_eq!(state.application().store().library_root(), directory.path());

        close_library().await.unwrap();
        assert!(get_state().is_err());
    }

    #[tokio::test]
    async fn restore_replaces_the_database_and_preserves_device_configuration() {
        let _test = state_test_lock().lock().await;
        let directory = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let state = open_library(directory.path().to_path_buf()).await.unwrap();
        crate::cloud::configure(
            state.application(),
            &crate::cloud::ConfigureCloudInput {
                provider: "dropbox".into(),
                account_label: "local test".into(),
                root_path: remote.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        state
            .application()
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO setting (key, value_json) VALUES ('restore-marker', '\"before\"')",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let device_before = crate::cloud::configuration(state.application())
            .unwrap()
            .device_id;
        let provider = crate::cloud::directory_provider(state.application()).unwrap();
        let snapshot = crate::cloud::snapshot::publish(state.application().store(), &provider)
            .await
            .unwrap();
        state
            .application()
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE setting SET value_json = '\"after\"' WHERE key = 'restore-marker'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        drop(state);

        let output = invoke(
            "cloud.restore.start",
            &serde_json::json!({ "snapshot_id": snapshot.snapshot_id }).to_string(),
        )
        .await
        .unwrap();
        assert!(output.contains("\"restored\":true"));

        let restored = get_state().unwrap();
        let marker: String = restored
            .application()
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT value_json FROM setting WHERE key = 'restore-marker'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(marker, "\"before\"");
        assert_eq!(
            crate::cloud::configuration(restored.application())
                .unwrap()
                .device_id,
            device_before
        );
        let emergency_path = std::fs::read_dir(directory.path().join("cloud/emergency"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let emergency = rusqlite::Connection::open_with_flags(
            emergency_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let emergency_marker: String = emergency
            .query_row(
                "SELECT value_json FROM setting WHERE key = 'restore-marker'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(emergency_marker, "\"after\"");
        drop(restored);
        close_library().await.unwrap();
    }

    #[tokio::test]
    async fn joining_cloud_library_assigns_a_new_device_identity() {
        let _test = state_test_lock().lock().await;
        let _ = close_library().await;
        let source = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        let target_parent = tempfile::tempdir().unwrap();
        let target = target_parent.path().join("Joined.library");
        let store = std::sync::Arc::new(Store::open(source.path()).unwrap());
        let application = Application::try_new(std::sync::Arc::clone(&store)).unwrap();
        crate::cloud::configure(
            &application,
            &crate::cloud::ConfigureCloudInput {
                provider: "dropbox".into(),
                account_label: "source".into(),
                root_path: remote.path().to_string_lossy().into_owned(),
            },
        )
        .unwrap();
        let source_configuration = crate::cloud::configuration(&application).unwrap();
        let provider = crate::cloud::directory_provider(&application).unwrap();
        let snapshot = crate::cloud::snapshot::publish(&store, &provider)
            .await
            .unwrap();
        drop(application);
        drop(store);

        let output = invoke(
            "cloud.library.join",
            &serde_json::json!({
                "provider": "dropbox",
                "account_label": "joined",
                "root_path": remote.path(),
                "library_id": snapshot.library_id,
                "target_root": target,
            })
            .to_string(),
        )
        .await
        .unwrap();
        assert!(output.contains("Joined.library"));

        let joined = get_state().unwrap();
        let joined_configuration = crate::cloud::configuration(joined.application()).unwrap();
        assert_eq!(
            joined_configuration.library_id,
            source_configuration.library_id
        );
        assert_ne!(
            joined_configuration.device_id,
            source_configuration.device_id
        );
        assert_eq!(
            joined_configuration.account_label.as_deref(),
            Some("joined")
        );
        drop(joined);
        close_library().await.unwrap();
    }

    #[test]
    fn finds_direct_and_nested_mutation_receipts() {
        let direct = serde_json::json!({
            "revision": 7,
            "resources": ["items"],
            "item_ids": [4, 9]
        });
        let nested = serde_json::json!({ "state": {}, "receipt": direct });

        let receipt = find_mutation_receipt(&nested).expect("nested receipt");
        assert_eq!(receipt.revision, 7);
        assert_eq!(
            receipt
                .item_ids
                .iter()
                .map(|item_id| item_id.0)
                .collect::<Vec<_>>(),
            vec![4, 9]
        );
    }

    #[test]
    fn mutation_audit_keeps_identifiers_and_excludes_user_content() {
        let args = serde_json::json!({
            "item_ids": [1, 2],
            "file_hash": "abc123",
            "name": "private name",
            "path": "/private/file.jpg",
            "query": { "text": "private search", "folder_id": 8 },
            "credential": "secret"
        });

        assert_eq!(
            safe_identifiers(&args.to_string()),
            serde_json::json!({
                "file_hash": "abc123",
                "folder_id": 8,
                "item_ids": [1, 2]
            })
        );
    }

    #[test]
    fn mutation_audit_bounds_large_identifier_lists() {
        let args = serde_json::json!({ "item_ids": (0..20).collect::<Vec<_>>() });
        let identifiers = safe_identifiers(&args.to_string());

        assert_eq!(identifiers["item_ids"]["total"], 20);
        assert_eq!(
            identifiers["item_ids"]["values"].as_array().unwrap().len(),
            12
        );

        let item_ids = (0..20).map(ItemId).collect::<Vec<_>>();
        let affected = summarize_item_ids(&item_ids);
        assert_eq!(affected["total"], 20);
        assert_eq!(affected["values"].as_array().unwrap().len(), 12);
    }
}
