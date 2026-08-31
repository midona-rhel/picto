//! Process ownership for the schema-1 media-library backend.
//!
//! Exactly one canonical library is open. No legacy Store or Application is
//! constructed on this path.

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::{Duration, Instant};

use lru::LruCache;
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
type WorkerHandle = (&'static str, tokio::task::JoinHandle<()>);

pub struct BackendState {
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
    workers: tokio::sync::Mutex<Vec<WorkerHandle>>,
    read_cache: parking_lot::Mutex<LruCache<String, (u64, String)>>,
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
        let mut env_filter =
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                "picto=info,picto::ipc=info"
                    .parse()
                    .expect("valid log filter")
            });
        #[cfg(debug_assertions)]
        for directive in [
            "picto_core::native_source=debug",
            "picto_sources::http=debug",
            "picto_sources::providers::ehentai=debug",
        ] {
            env_filter =
                env_filter.add_directive(directive.parse().expect("valid debug directive"));
        }
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
    let database_path = library_root.join("library.sqlite");
    let application = Arc::new(if database_path.exists() {
        LibraryApplication::open(&library_root)?
    } else {
        LibraryApplication::create(&library_root)?
    });
    if let Err(error) = crate::ai_models::migrate_legacy_storage(application.as_ref()) {
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
    if command == "cloud.providers.detect" {
        return serde_json::to_string(&crate::cloud::provider::detect_roots())
            .map_err(|error| error.to_string());
    }
    if command == "cloud.libraries.discover" {
        #[derive(serde::Deserialize)]
        struct Input {
            provider: String,
            root_path: String,
        }
        let input: Input = serde_json::from_str(args_json)
            .map_err(|error| format!("Invalid command arguments: {error}"))?;
        return serde_json::to_string(
            &crate::cloud::discover_libraries(&input.provider, &input.root_path).await?,
        )
        .map_err(|error| error.to_string());
    }
    if command == "cloud.provider.validate" {
        #[derive(serde::Deserialize)]
        struct Input {
            provider: String,
            root_path: String,
        }
        let input: Input = serde_json::from_str(args_json)
            .map_err(|error| format!("Invalid command arguments: {error}"))?;
        return serde_json::to_string(&crate::cloud::provider::validate_root(
            &input.provider,
            input.root_path,
        )?)
        .map_err(|error| error.to_string());
    }
    if command == "cloud.restore.start" || command == "cloud.library.join" {
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
    let revision_before = state
        .application
        .library()
        .database()
        .revision()
        .map_err(|error| error.to_string())?;
    if let Some(cache_key) = cache_key.as_ref() {
        if let Some((revision, result)) = state.read_cache.lock().get(cache_key) {
            if *revision == revision_before {
                return Ok(result.clone());
            }
        }
    }

    let result = crate::ipc::dispatch_library_async(state.application(), command, args_json)
        .await?
        .ok_or_else(|| format!("Unknown schema-1 command: {command}"));
    if let (Some(cache_key), Ok(serialized)) = (cache_key, result.as_ref()) {
        let revision_after = state
            .application
            .library()
            .database()
            .revision()
            .map_err(|error| error.to_string())?;
        if revision_before == revision_after {
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

#[derive(serde::Serialize)]
struct RestoredCloudSnapshot {
    snapshot_id: String,
    restored: bool,
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
    let local = state
        .application()
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                Ok(connection.query_row(
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
                )?)
            },
        )
        .map_err(|error| error.to_string())?;
    let provider = crate::cloud::directory_provider_library(state.application())?;
    let prepared =
        crate::cloud::snapshot::prepare_restore(state.application(), &provider, &input.snapshot_id)
            .await?;
    let library_root = state.application().root().to_path_buf();
    drop(state);
    close_library_inner().await?;

    let database_path = library_root.join("library.sqlite");
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

    let activation = initialize_restored_database(&database_path, &local);
    if let Err(error) = activation {
        rollback_database_activation(
            &database_path,
            &prepared.emergency_copy_path,
            "failed-restore",
        );
        let _ = open_library_inner(library_root, None).await;
        return Err(error);
    }
    if let Err(error) = open_library_inner(library_root.clone(), None).await {
        rollback_database_activation(
            &database_path,
            &prepared.emergency_copy_path,
            "failed-restore",
        );
        let _ = open_library_inner(library_root, None).await;
        return Err(format!("Restored database failed to open: {error}"));
    }
    serde_json::to_string(&RestoredCloudSnapshot {
        snapshot_id: prepared.snapshot_id,
        restored: true,
    })
    .map_err(|error| error.to_string())
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
    let database_path = target_root.join("library.sqlite");
    if database_path.exists() {
        return Err("The destination already contains a Picto library".to_string());
    }
    let validated = crate::cloud::provider::validate_root(&input.provider, &input.root_path)?;
    let provider = crate::cloud::provider::DirectoryProvider::open_existing(&validated.path)?;
    let prepared =
        crate::cloud::snapshot::prepare_join(&provider, &input.library_id, &target_root).await?;

    let previous_root = get_state()
        .ok()
        .map(|state| state.application().root().to_path_buf());
    close_library_inner().await?;
    std::fs::create_dir_all(target_root.join("blobs"))
        .map_err(|error| format!("Failed to create joined library directory: {error}"))?;
    if let Err(error) = std::fs::rename(&prepared.database_path, &database_path) {
        if let Some(previous_root) = previous_root {
            let _ = open_library_inner(previous_root, None).await;
        }
        return Err(format!("Failed to activate joined cloud library: {error}"));
    }

    if let Err(error) = initialize_joined_database(
        &database_path,
        &input.provider,
        &input.account_label,
        &validated.path,
    ) {
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
    picto_library::schema::validate(&connection).map_err(|error| error.to_string())?;
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to initialize joined cloud library: {error}"))?;
    let now = chrono::Utc::now().to_rfc3339();
    transaction
        .execute(
            "UPDATE cloud_state SET device_id = ?1, provider = ?2, account_label = ?3,
                    remote_root = ?4, paused = 0, state = 'idle', phase = 'blobs',
                    blocking = CASE WHEN EXISTS (SELECT 1 FROM media_file) THEN 1 ELSE 0 END,
                    completed_units = 0,
                    total_units = CASE WHEN EXISTS (SELECT 1 FROM media_file)
                        THEN (SELECT COALESCE(SUM(MAX(size_bytes, 1)), 0) FROM media_file) ELSE NULL END,
                    message = CASE WHEN EXISTS (SELECT 1 FROM media_file)
                        THEN 'Restoring library media' ELSE '' END,
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
            "UPDATE folder_definition
             SET watch_path = NULL, watch_enabled = 0, watch_subfolders = 0",
            [],
        )
        .map_err(|error| format!("Failed to clear device-local folder watches: {error}"))?;
    transaction
        .execute("DELETE FROM view_pref", [])
        .map_err(|error| format!("Failed to clear device-local views: {error}"))?;
    transaction
        .execute(QUEUE_CLOUD_MEDIA_RECOVERY_SQL, [&now])
        .map_err(|error| format!("Failed to queue cloud media recovery: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("Failed to commit joined cloud library: {error}"))
}

// SQLite otherwise parses `ON` as a possible JOIN clause after `FROM media_file`.
// The always-true predicate unambiguously terminates the SELECT before the UPSERT.
const QUEUE_CLOUD_MEDIA_RECOVERY_SQL: &str =
    "INSERT INTO cloud_blob_state (file_hash, state, updated_at)
     SELECT content_hash, 'queued', ?1 FROM media_file WHERE 1
     ON CONFLICT(file_hash) DO UPDATE SET
         state = 'queued', priority = 0, last_error = NULL, updated_at = excluded.updated_at";

fn initialize_restored_database(
    database_path: &std::path::Path,
    local: &LocalCloudConfiguration,
) -> Result<(), String> {
    let connection = rusqlite::Connection::open(database_path)
        .map_err(|error| format!("Failed to open the restored database: {error}"))?;
    picto_library::schema::validate(&connection).map_err(|error| error.to_string())?;
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
}

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

fn rollback_database_activation(
    database_path: &std::path::Path,
    emergency_copy_path: &std::path::Path,
    failed_prefix: &str,
) {
    let failed =
        database_path.with_extension(format!("{failed_prefix}-{}.sqlite", uuid::Uuid::new_v4()));
    let _ = remove_sqlite_sidecars(database_path);
    let _ = std::fs::rename(database_path, failed);
    let _ = std::fs::rename(emergency_copy_path, database_path);
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
    use super::{initialize_joined_database, QUEUE_CLOUD_MEDIA_RECOVERY_SQL};

    #[test]
    fn cloud_join_recovery_upsert_is_valid_sqlite() {
        let mut connection = rusqlite::Connection::open_in_memory().unwrap();
        picto_library::schema::create(&mut connection).unwrap();
        connection
            .execute(QUEUE_CLOUD_MEDIA_RECOVERY_SQL, ["2026-08-31T00:00:00Z"])
            .unwrap();
    }

    #[test]
    fn cloud_join_starts_blocking_byte_accurate_media_recovery() {
        let directory = tempfile::tempdir().unwrap();
        let database_path = directory.path().join("library.sqlite");
        let mut connection = rusqlite::Connection::open(&database_path).unwrap();
        picto_library::schema::create(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO media_file
                     (file_id, content_hash, file_path, mime, size_bytes)
                 VALUES (1, ?1, 'blobs/a', 'image/png', 12000)",
                ["a".repeat(64)],
            )
            .unwrap();
        drop(connection);

        initialize_joined_database(
            &database_path,
            "google_drive",
            "Personal",
            "/Cloud/My Drive",
        )
        .unwrap();

        let connection = rusqlite::Connection::open(database_path).unwrap();
        let state = connection
            .query_row(
                "SELECT state, phase, blocking, completed_units, total_units,
                        message, pending_blobs
                 FROM cloud_state WHERE singleton = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, i64>(6)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            state,
            (
                "idle".into(),
                "blobs".into(),
                1,
                0,
                12000,
                "Restoring library media".into(),
                1,
            )
        );
    }
}
