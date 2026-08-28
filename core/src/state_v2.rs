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
    if let Err(error) = crate::ai_models_v2::migrate_legacy_storage(application.as_ref()) {
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
            root_path: String,
        }
        let input: Input = serde_json::from_str(args_json)
            .map_err(|error| format!("Invalid command arguments: {error}"))?;
        return serde_json::to_string(&crate::cloud::discover_libraries(&input.root_path).await?)
            .map_err(|error| error.to_string());
    }
    if matches!(command, "cloud.restore.start" | "cloud.library.join") {
        return Err("Cloud restore and join require a schema-1 snapshot; create a fresh cloud copy from this library first".into());
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

    let result = crate::ipc_v2::dispatch_library_async(state.application(), command, args_json)
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
