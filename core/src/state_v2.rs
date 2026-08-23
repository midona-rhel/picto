//! Process-level ownership for the replacement backend.
//!
//! Exactly one library is open at a time. Opening another library first
//! cancels and joins the previous library's durable workers.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::app::Application;
use crate::store::Store;

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
type WorkerHandle = (&'static str, tokio::task::JoinHandle<()>);

pub struct BackendState {
    application: Arc<Application>,
    cancel: CancellationToken,
    workers: tokio::sync::Mutex<Vec<WorkerHandle>>,
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
    crate::ipc_v2::dispatch_async(state.application(), command, args_json).await
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
}
