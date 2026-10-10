use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use crate::library_application::LibraryApplication;
use crate::work_wake::WorkWake;

pub type LibraryWorkerHandle = (&'static str, tokio::task::JoinHandle<()>);

pub fn start(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> Result<Vec<LibraryWorkerHandle>, String> {
    crate::library_subscription_state::recover(&application, &chrono::Utc::now().to_rfc3339())?;
    crate::library_ingest_runtime::recover(&application)?;
    crate::library_media_runtime::recover(&application)?;
    crate::cloud::normalize_retention_library(&application)?;
    let heavy_jobs = Arc::new(Semaphore::new(1));
    let wake = WorkWake::subscribe(&application);
    let mut workers = vec![
        (
            "library-publication",
            application.start_publication_worker(cancel.child_token()),
        ),
        (
            "library-ingest",
            start_ingest_worker(
                Arc::clone(&application),
                cancel.child_token(),
                Arc::clone(&heavy_jobs),
                wake.clone(),
            ),
        ),
        (
            "library-watched-folders",
            start_watched_folder_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-fts",
            start_fts_worker(Arc::clone(&application), cancel.child_token(), wake.clone()),
        ),
        (
            "library-checkpoint",
            start_checkpoint_worker(Arc::clone(&application), cancel.child_token()),
        ),
        (
            "library-derivatives",
            start_derivative_worker(Arc::clone(&application), cancel.child_token(), wake.clone()),
        ),
        (
            "library-ai-tagging",
            start_ai_tag_worker(Arc::clone(&application), cancel.child_token(), wake.clone()),
        ),
        (
            "library-cloud-snapshot",
            start_cloud_snapshot_worker(
                Arc::clone(&application),
                cancel.child_token(),
                Arc::clone(&heavy_jobs),
            ),
        ),
    ];
    workers.extend(start_subscription_workers(
        Arc::clone(&application),
        cancel.child_token(),
        wake,
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
    let heavy_jobs = Arc::new(Semaphore::new(1));
    let wake = WorkWake::subscribe(&application);
    let mut workers = vec![
        (
            "library-publication",
            application.start_publication_worker(cancel.child_token()),
        ),
        (
            "library-ingest",
            start_ingest_worker(Arc::clone(&application), cancel.child_token(), heavy_jobs, wake.clone()),
        ),
        (
            "library-fts",
            start_fts_worker(Arc::clone(&application), cancel.child_token(), wake.clone()),
        ),
        (
            "library-derivatives",
            start_derivative_worker(Arc::clone(&application), cancel.child_token(), wake.clone()),
        ),
        (
            "library-ai-tagging",
            start_ai_tag_worker(Arc::clone(&application), cancel.child_token(), wake.clone()),
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

/// Subscription workers re-check on any publication (a run was queued, an
/// inbox item was reviewed) and otherwise every few seconds, which bounds how
/// late a scheduled run or a rate-limited domain can start.
const SUBSCRIPTION_PACE: Duration = Duration::from_secs(1);
const SUBSCRIPTION_FALLBACK: Duration = Duration::from_secs(5);

fn start_subscription_workers(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
    wake: WorkWake,
) -> Vec<LibraryWorkerHandle> {
    let scheduler_application = Arc::clone(&application);
    let scheduler_cancel = cancel.child_token();
    let mut scheduler_wake = wake.clone();
    let mut workers = vec![(
        "library-subscription-scheduler",
        tokio::spawn(async move {
            loop {
                scheduler_wake.mark_seen();
                if let Err(error) = crate::library_subscription_state::schedule_due_runs(
                    &scheduler_application,
                    &chrono::Utc::now().to_rfc3339(),
                ) {
                    tracing::warn!(%error, "Canonical subscription scheduling failed");
                }
                if !scheduler_wake
                    .next(&scheduler_cancel, SUBSCRIPTION_PACE, SUBSCRIPTION_FALLBACK)
                    .await
                {
                    return;
                }
            }
        }),
    )];
    let schedule = Arc::new(Mutex::new(crate::subscriptions::DomainSchedule::new()));
    for index in 0..4 {
        let worker_application = Arc::clone(&application);
        let worker_cancel = cancel.child_token();
        let worker_schedule = Arc::clone(&schedule);
        let mut worker_wake = wake.clone();
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
                loop {
                    worker_wake.mark_seen();
                    if let Err(error) = worker.tick(&chrono::Utc::now().to_rfc3339()).await {
                        tracing::warn!(%error, "Canonical subscription worker failed");
                    }
                    if !worker_wake
                        .next(&worker_cancel, SUBSCRIPTION_PACE, SUBSCRIPTION_FALLBACK)
                        .await
                    {
                        return;
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
    heavy_jobs: Arc<Semaphore>,
    mut wake: WorkWake,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            let permit = tokio::select! {
                _ = cancel.cancelled() => return,
                permit = Arc::clone(&heavy_jobs).acquire_owned() => match permit {
                    Ok(permit) => permit,
                    Err(_) => return,
                },
            };
            wake.mark_seen();
            let batch_application = Arc::clone(&application);
            match tokio::task::spawn_blocking(move || {
                crate::library_ingest_runtime::run_batch(&batch_application, 64)
            }).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::warn!(%error, "Canonical ingest batch failed"),
                Err(error) => tracing::warn!(%error, "Canonical ingest worker stopped"),
            }
            drop(permit);
            if !wake.next(&cancel, Duration::from_millis(25), Duration::from_secs(2)).await {
                return;
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
    mut wake: WorkWake,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            wake.mark_seen();
            let library = Arc::clone(application.library());
            match tokio::task::spawn_blocking(move || library.settle_fts(256)).await {
                Ok(Ok(_)) => {}
                Ok(Err(error)) => tracing::warn!(%error, "Canonical FTS settlement failed"),
                Err(error) => tracing::warn!(%error, "Canonical FTS worker stopped"),
            }
            // Text only goes dirty through a published write.
            if !wake.next(&cancel, Duration::from_millis(100), Duration::from_secs(30)).await {
                return;
            }
        }
    })
}

fn start_checkpoint_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        // WAL pages are folded back every minute; the projection checkpoint is
        // considered every fifth minute and skipped when nothing changed.
        let interval = Duration::from_secs(60);
        let mut tick = 0_u32;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(interval) => {
                    tick = tick.wrapping_add(1);
                    let library = Arc::clone(application.library());
                    let projection = tick.is_multiple_of(5);
                    let result = tokio::task::spawn_blocking(move || {
                        if projection {
                            library.write_projection_checkpoint()?;
                            // Without a cloud provider no snapshot will ever
                            // consume the change journal; keep it from growing.
                            library.database().maintenance_write(
                                picto_library::database::WorkPriority::Maintenance,
                                |transaction| {
                                    transaction.execute(
                                        "DELETE FROM cloud_journal WHERE NOT EXISTS (
                                             SELECT 1 FROM cloud_state WHERE provider IS NOT NULL)",
                                        [],
                                    )?;
                                    Ok(())
                                },
                            )?;
                        }
                        library.database().checkpoint_wal_passive()
                    })
                    .await;
                    match result {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) => tracing::warn!(%error, "Canonical checkpoint failed"),
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
    mut wake: WorkWake,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut duplicate_scan_dirty = false;
        loop {
            wake.mark_seen();
            if let Err(error) = crate::library_media_runtime::drain_blob_cleanup(&application, 8) {
                tracing::warn!(%error, "Canonical blob cleanup failed");
            }
            match crate::library_media_runtime::drain_batch(&application, 8).await {
                Err(error) => tracing::warn!(%error, "Canonical derivative batch failed"),
                Ok(report) => {
                    for file_hash in &report.thumbnails_changed {
                        crate::events::emit(
                            "picto:thumbnail-changed",
                            &serde_json::json!({ "fileHash": file_hash }),
                        );
                    }
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
            // Retries become ready with time; a short fallback picks them up.
            if !wake.next(&cancel, Duration::from_millis(50), Duration::from_secs(2)).await {
                return;
            }
        }
    })
}

fn start_ai_tag_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
    mut wake: WorkWake,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            wake.mark_seen();
            if let Err(error) = crate::ai_runtime::drain_auto_tag_work(&application, 1).await {
                tracing::warn!(%error, "Canonical AI-tag worker failed");
            }
            if !wake.next(&cancel, Duration::from_millis(100), Duration::from_secs(3)).await {
                return;
            }
        }
    })
}

fn start_cloud_snapshot_worker(
    application: Arc<LibraryApplication>,
    cancel: CancellationToken,
    heavy_jobs: Arc<Semaphore>,
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
                    let permit = tokio::select! {
                        _ = cancel.cancelled() => return,
                        permit = Arc::clone(&heavy_jobs).acquire_owned() => match permit {
                            Ok(permit) => permit,
                            Err(_) => return,
                        },
                    };
                    if let Err(error) = crate::cloud::snapshot::publish_library(&application, &provider).await {
                        tracing::warn!(%error, "Canonical cloud snapshot failed");
                    }
                    drop(permit);
                }
            }
        }
    })
}
