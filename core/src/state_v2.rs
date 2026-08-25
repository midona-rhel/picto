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
    query_cache: parking_lot::Mutex<LruCache<String, (u64, String)>>,
}

impl BackendState {
    pub fn application(&self) -> &Application {
        &self.application
    }
}

static STATE: OnceLock<RwLock<Option<Arc<BackendState>>>> = OnceLock::new();
static LIFECYCLE: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

fn state_lock() -> &'static RwLock<Option<Arc<BackendState>>> {
    STATE.get_or_init(|| RwLock::new(None))
}

fn lifecycle_lock() -> &'static tokio::sync::Mutex<()> {
    LIFECYCLE.get_or_init(|| tokio::sync::Mutex::new(()))
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
    close_library_inner().await?;

    let store = Arc::new(Store::open(&library_root)?);
    let application = Arc::new(Application::try_new(store)?);
    let cancel = CancellationToken::new();
    let workers = crate::runtime_v2::start(Arc::clone(&application), cancel.clone())?;
    let state = Arc::new(BackendState {
        application,
        cancel,
        workers: tokio::sync::Mutex::new(workers),
        query_cache: parking_lot::Mutex::new(LruCache::new(
            NonZeroUsize::new(16).expect("nonzero query cache capacity"),
        )),
    });

    let mut guard = state_lock()
        .write()
        .map_err(|_| "Replacement state lock poisoned".to_string())?;
    *guard = Some(Arc::clone(&state));
    Ok(state)
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
    let state = get_state()?;
    let started = Instant::now();
    let query_revision = if command == "items.query" {
        let revision = state.application.store().revision()?;
        if let Some((cached_revision, cached_result)) = state.query_cache.lock().get(args_json) {
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
                state
                    .query_cache
                    .lock()
                    .put(args_json.to_owned(), (revision_after, serialized.clone()));
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

    #[tokio::test]
    async fn opens_without_resolving_the_gallery_dl_bridge() {
        let directory = tempfile::tempdir().unwrap();
        let state = open_library(directory.path().to_path_buf()).await.unwrap();

        let settings = invoke("settings.get", "{}").await.unwrap();
        assert!(settings.contains("revision"));
        assert_eq!(state.application().store().library_root(), directory.path());

        close_library().await.unwrap();
        assert!(get_state().is_err());
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
