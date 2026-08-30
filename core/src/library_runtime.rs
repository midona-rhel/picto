use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;

pub type LibraryWorkerHandle = (&'static str, tokio::task::JoinHandle<()>);

pub fn start(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> Result<Vec<LibraryWorkerHandle>, String> {
    crate::library_subscription_state::recover(&application, &chrono::Utc::now().to_rfc3339())?;
    crate::library_ingest_runtime::recover(&application)?;
    crate::library_media_runtime::recover(&application)?;
    let mut workers = vec![
        (
            "library-publication",
            application.start_publication_worker(cancel.child_token()),
        ),
        (
            "library-ingest",
            start_ingest_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-watched-folders",
            start_watched_folder_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-fts",
            start_fts_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-checkpoint",
            start_checkpoint_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-derivatives",
            start_derivative_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-ai-tagging",
            start_ai_tag_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-cloud-snapshot",
            start_cloud_snapshot_worker(Arc::clone(&application), cancel.child_token()),
        ),
    ];
    workers.extend(start_subscription_workers(
        Arc::clone(&application),
        cancel.child_token(),
    ));
    Ok(workers)
}

pub fn start_tutorial(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
    fixture_root: std::path::PathBuf,
) -> Result<Vec<LibraryWorkerHandle>, String> {
    crate::library_subscription_state::recover(&application, &chrono::Utc::now().to_rfc3339())?;
    crate::library_ingest_runtime::recover(&application)?;
    crate::library_media_runtime::recover(&application)?;
    let mut workers = vec![
        (
            "library-publication",
            application.start_publication_worker(cancel.child_token()),
        ),
        (
            "library-ingest",
            start_ingest_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-fts",
            start_fts_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-derivatives",
            start_derivative_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-ai-tagging",
            start_ai_tag_worker(Arc::clone(&application), cancel.child_token()),
        ),
    ];
    let source_application = Arc::clone(&application);
    let source_cancel = cancel.child_token();
    workers.push((
        "library-tutorial-subscription",
        tokio::spawn(async move {
            let worker = crate::subscription_runtime::SubscriptionWorker::with_cancellation(
                &source_application,
                crate::tutorial_source::TutorialSourceRunner::new(fixture_root),
                source_cancel.clone(),
            );
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = source_cancel.cancelled() => return,
                    _ = interval.tick() => {
                        if let Err(error) = worker.tick(&chrono::Utc::now().to_rfc3339()).await {
                            tracing::warn!(%error, "Tutorial subscription worker failed");
                        }
                    }
                }
            }
        }),
    ));
    Ok(workers)
}

fn start_subscription_workers(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> Vec<LibraryWorkerHandle> {
    let scheduler_application = Arc::clone(&application);
    let scheduler_cancel = cancel.child_token();
    let mut workers = vec![(
        "library-subscription-scheduler",
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = scheduler_cancel.cancelled() => return,
                    _ = interval.tick() => {
                        if let Err(error) = crate::library_subscription_state::schedule_due_runs(
                            &scheduler_application,
                            &chrono::Utc::now().to_rfc3339(),
                        ) {
                            tracing::warn!(%error, "Canonical subscription scheduling failed");
                        }
                    }
                }
            }
        }),
    )];
    let schedule = Arc::new(Mutex::new(crate::subscriptions::DomainSchedule::new()));
    for index in 0..4 {
        let worker_application = Arc::clone(&application);
        let worker_cancel = cancel.child_token();
        let worker_schedule = Arc::clone(&schedule);
        workers.push((
            match index {
                0 => "library-subscription-1",
                1 => "library-subscription-2",
                2 => "library-subscription-3",
                _ => "library-subscription-4",
            },
            tokio::spawn(async move {
                let runner = crate::native_source::NativeSourceRunner::open(&worker_application);
                let worker = crate::subscription_runtime::SubscriptionWorker::with_shared_schedule(
                    &worker_application,
                    runner,
                    worker_schedule,
                    worker_cancel.clone(),
                );
                let mut interval = tokio::time::interval(Duration::from_secs(1));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = worker_cancel.cancelled() => return,
                        _ = interval.tick() => {
                            if let Err(error) = worker.tick(&chrono::Utc::now().to_rfc3339()).await {
                                tracing::warn!(%error, "Canonical subscription worker failed");
                            }
                        }
                    }
                }
            }),
        ));
    }
    workers
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

fn start_watched_folder_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut idle = tokio::time::interval(Duration::from_secs(30));
        idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = idle.tick() => {
                    match crate::library_import::scan_watched_folders(&application).await {
                        Ok(_) => {}
                        Err(error) => tracing::warn!(%error, "Canonical watched-folder scan failed"),
                    }
                }
            }
        }
    })
}

fn start_fts_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut idle = tokio::time::interval(Duration::from_millis(100));
        idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = idle.tick() => {
                    let library = Arc::clone(application.library());
                    match tokio::task::spawn_blocking(move || library.settle_fts(256)).await {
                        Ok(Ok(_)) => {}
                        Ok(Err(error)) => tracing::warn!(%error, "Canonical FTS settlement failed"),
                        Err(error) => tracing::warn!(%error, "Canonical FTS worker stopped"),
                    }
                }
            }
        }
    })
}

fn start_checkpoint_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let interval = Duration::from_secs(5 * 60);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(interval) => {
                    let library = Arc::clone(application.library());
                    match tokio::task::spawn_blocking(move || library.write_projection_checkpoint()).await {
                        Ok(Ok(_)) => {}
                        Ok(Err(error)) => tracing::warn!(%error, "Canonical projection checkpoint failed"),
                        Err(error) => tracing::warn!(%error, "Canonical checkpoint worker stopped"),
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
        let mut duplicate_scan_dirty = false;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = idle.tick() => {
                    if let Err(error) = crate::library_media_runtime::drain_blob_cleanup(&application, 8) {
                        tracing::warn!(%error, "Canonical blob cleanup failed");
                    }
                    match crate::library_media_runtime::drain_batch(&application, 8).await {
                        Err(error) => tracing::warn!(%error, "Canonical derivative batch failed"),
                        Ok(report) => {
                            duplicate_scan_dirty |= report.perceptual_hashes_updated != 0;
                            if duplicate_scan_dirty {
                                match crate::library_media_runtime::settle_new_perceptual_hashes(
                                    Arc::clone(&application),
                                    1,
                                ).await {
                                    Ok(Some(_)) => duplicate_scan_dirty = false,
                                    Ok(None) => {}
                                    Err(error) => tracing::warn!(%error, "Automatic duplicate scan failed"),
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

fn start_ai_tag_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut idle = tokio::time::interval(Duration::from_millis(100));
        idle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = idle.tick() => {
                    if let Err(error) = crate::ai_runtime::drain_auto_tag_work(&application, 1).await {
                        tracing::warn!(%error, "Canonical AI-tag worker failed");
                    }
                }
            }
        }
    })
}

fn start_cloud_snapshot_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    const CLOUD_IDLE_DELAY: Duration = Duration::from_secs(30);
    tokio::spawn(async move {
        if let Err(error) = crate::cloud::recover_interrupted_sync_library(&application) {
            tracing::warn!(%error, "Canonical cloud recovery failed");
        }
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = interval.tick() => {
                    let now_ms = chrono::Utc::now().timestamp_millis();
                    let due = crate::cloud::snapshot_due_library(
                        &application,
                        now_ms,
                        CLOUD_IDLE_DELAY.as_millis() as i64,
                    );
                    match due {
                        Ok(false) => continue,
                        Err(error) => {
                            tracing::warn!(%error, "Canonical cloud schedule check failed");
                            continue;
                        }
                        Ok(true) => {}
                    }
                    let provider = match crate::cloud::directory_provider_library(&application) {
                        Ok(provider) => provider,
                        Err(error) => {
                            tracing::warn!(%error, "Canonical cloud provider is unavailable");
                            continue;
                        }
                    };
                    if let Err(error) = crate::cloud::snapshot::publish_library(&application, &provider).await {
                        tracing::warn!(%error, "Canonical cloud snapshot failed");
                    }
                }
            }
        }
    })
}
