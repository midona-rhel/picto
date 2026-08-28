use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;

pub type LibraryWorkerHandle = (&'static str, tokio::task::JoinHandle<()>);

pub fn start(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> Result<Vec<LibraryWorkerHandle>, String> {
    crate::library_ingest_runtime::recover(&application)?;
    crate::library_media_runtime::recover(&application)?;
    Ok(vec![
        (
            "library-publication",
            application.start_publication_worker(cancel.child_token()),
        ),
        (
            "library-ingest",
            start_ingest_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-derivatives",
            start_derivative_worker(application, cancel.child_token()),
        ),
    ])
}

fn start_ingest_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut idle = tokio::time::interval(Duration::from_millis(25));
        idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = idle.tick() => {
                    let application = Arc::clone(&application);
                    match tokio::task::spawn_blocking(move || {
                        crate::library_ingest_runtime::run_batch(&application, 64)
                    }).await {
                        Ok(Ok(_)) => {}
                        Ok(Err(error)) => tracing::warn!(%error, "Canonical ingest batch failed"),
                        Err(error) => tracing::warn!(%error, "Canonical ingest worker stopped"),
                    }
                }
            }
        }
    })
}

fn start_derivative_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut idle = tokio::time::interval(Duration::from_millis(50));
        idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = idle.tick() => {
                    if let Err(error) = crate::library_media_runtime::drain_batch(&application, 8).await {
                        tracing::warn!(%error, "Canonical derivative batch failed");
                    }
                }
            }
        }
    })
}
