//! Background loops for the replacement backend.
//!
//! Durable queue tables own state. These loops only wake, execute bounded
//! work, publish compact invalidations, and honor application shutdown.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use chrono::Utc;
use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::app::{resources, Application, MutationReceipt};
use crate::background_runtime_v2::{self, DrainBatchResult};
use crate::ingest_queue_v2::{self, IngestRunReport};
use crate::onlyfans_source_v2::SubscriptionSourceRouter;
use crate::subscription_runtime_v2::{SourceRunner, SubscriptionWorker};
use crate::subscriptions_v2::{self, RecoveryCounts};

const SUBSCRIPTION_TICK: StdDuration = StdDuration::from_secs(1);
const SUBSCRIPTION_WORKER_COUNT: usize = 4;
const MAINTENANCE_TICK: StdDuration = StdDuration::from_millis(250);
const WATCH_TICK: StdDuration = StdDuration::from_secs(30);
const CLOUD_TICK: StdDuration = StdDuration::from_secs(2);
const INGEST_BATCH_SIZE: usize = 8;
const WORK_BATCH_SIZE: usize = 8;
const THUMBNAIL_CHANGED_EVENT: &str = "picto:thumbnail-changed";
const DOMINANT_COLOR_CHANGED_EVENT: &str = "picto:dominant-color-changed";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThumbnailChanged<'a> {
    file_hash: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DominantColorChanged<'a> {
    file_hash: &'a str,
    dominant_color_hex: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StartupRecovery {
    pub subscription_runs: usize,
    pub subscription_query_runs: usize,
    pub ingest_jobs: usize,
    pub work_items: usize,
    pub pruned_thumbnail_work: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubscriptionTickResult {
    pub scheduled_runs: usize,
    pub ran_query: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MaintenanceTickResult {
    pub ingest: IngestRunReport,
    pub work: DrainBatchResult,
}

pub fn recover(application: &Application, now: &str) -> Result<StartupRecovery, String> {
    let RecoveryCounts { runs, query_runs } =
        subscriptions_v2::recover_startup(application.store(), now)?;
    let ingest_jobs = ingest_queue_v2::reset_running(application)?;
    ingest_queue_v2::recover_settled_provisional_collections(application)?;
    let work_items = crate::workers_v2::reset_running(application.store())?;
    let pruned_thumbnail_work =
        crate::workers_v2::prune_deferred_thumbnail_work(application.store())?;
    crate::media_processing_v2::enqueue_missing_dominant_color_work(application.store(), now)?;
    crate::media_processing_v2::reconcile_perceptual_hash_work(application.store(), now)?;
    Ok(StartupRecovery {
        subscription_runs: runs,
        subscription_query_runs: query_runs,
        ingest_jobs,
        work_items,
        pruned_thumbnail_work,
    })
}

pub async fn subscription_tick<R: SourceRunner>(
    application: &Application,
    worker: &SubscriptionWorker<'_, R>,
    now: &str,
) -> Result<SubscriptionTickResult, String> {
    let scheduled = subscriptions_v2::schedule_due_runs(application.store(), now)?;
    if !scheduled.is_empty() {
        let receipt = MutationReceipt {
            revision: application.store().revision()?,
            resources: vec![
                resources::SUBSCRIPTIONS.to_string(),
                resources::TASKS.to_string(),
            ],
            item_ids: Vec::new(),
        };
        application.publish(&receipt);
    }
    let ran_query = worker.tick(now).await?.is_some();
    Ok(SubscriptionTickResult {
        scheduled_runs: scheduled.len(),
        ran_query,
    })
}

pub async fn maintenance_tick(application: &Application) -> Result<MaintenanceTickResult, String> {
    let ingest = ingest_queue_v2::run_batch(application, INGEST_BATCH_SIZE)?;
    // User-visible ingest thumbnails win over colors, pHash, and other
    // derivatives. A subscription worker may be draining the same queue, so
    // checking both ready and running jobs prevents derivative contention.
    let work = if ingest_queue_v2::has_ready_or_running(application)? {
        DrainBatchResult::default()
    } else {
        background_runtime_v2::drain_batch(application, WORK_BATCH_SIZE).await?
    };
    if let Some(receipt) = &work.receipt {
        application.publish(receipt);
    }
    for file_hash in &work.thumbnail_file_hashes {
        crate::events::emit(THUMBNAIL_CHANGED_EVENT, &ThumbnailChanged { file_hash });
    }
    for change in &work.dominant_color_changes {
        crate::events::emit(
            DOMINANT_COLOR_CHANGED_EVENT,
            &DominantColorChanged {
                file_hash: &change.file_hash,
                dominant_color_hex: change.dominant_color_hex.as_deref(),
            },
        );
    }
    Ok(MaintenanceTickResult { ingest, work })
}

/// Start replacement workers after the replacement store becomes the active
/// application backend. The caller owns and joins the returned handles.
pub fn start(
    application: Arc<Application>,
    cancel: CancellationToken,
) -> Result<Vec<(&'static str, tokio::task::JoinHandle<()>)>, String> {
    recover(&application, &Utc::now().to_rfc3339())?;
    let scheduler_application = Arc::clone(&application);
    let scheduler_cancel = cancel.clone();
    let scheduler_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(SUBSCRIPTION_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = scheduler_cancel.cancelled() => return,
                _ = interval.tick() => {
                    let now = Utc::now().to_rfc3339();
                    match subscriptions_v2::schedule_due_runs(scheduler_application.store(), &now) {
                        Ok(scheduled) if !scheduled.is_empty() => {
                            match scheduler_application.store().revision() {
                                Ok(revision) => {
                                    let receipt = MutationReceipt {
                                        revision,
                                        resources: vec![
                                            resources::SUBSCRIPTIONS.to_string(),
                                            resources::TASKS.to_string(),
                                        ],
                                        item_ids: Vec::new(),
                                    };
                                    scheduler_application.publish(&receipt);
                                }
                                Err(error) => tracing::warn!(
                                    error = %error,
                                    "Reading revision after subscription scheduling failed"
                                ),
                            }
                        }
                        Ok(_) => {}
                        Err(error) => tracing::warn!(error = %error, "Subscription scheduling failed"),
                    }
                }
            }
        }
    });

    let shared_schedule = Arc::new(Mutex::new(subscriptions_v2::DomainSchedule::new()));
    let mut subscription_handles = Vec::with_capacity(SUBSCRIPTION_WORKER_COUNT);
    for _ in 0..SUBSCRIPTION_WORKER_COUNT {
        let subscription_application = Arc::clone(&application);
        let subscription_cancel = cancel.clone();
        let schedule = Arc::clone(&shared_schedule);
        let runner = SubscriptionSourceRouter::open(application.store().library_root());
        subscription_handles.push(tokio::spawn(async move {
            let worker = SubscriptionWorker::with_shared_schedule(
                &subscription_application,
                runner,
                schedule,
                subscription_cancel.clone(),
            );
            let mut interval = tokio::time::interval(SUBSCRIPTION_TICK);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = subscription_cancel.cancelled() => return,
                    _ = interval.tick() => {
                        let now = Utc::now().to_rfc3339();
                        if let Err(error) = worker.tick(&now).await {
                            tracing::warn!(error = %error, "Replacement subscription worker failed");
                        }
                    }
                }
            }
        }));
    }

    let maintenance_application = Arc::clone(&application);
    let maintenance_cancel = cancel.clone();
    let maintenance_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(MAINTENANCE_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = maintenance_cancel.cancelled() => return,
                _ = interval.tick() => {
                    if let Err(error) = maintenance_tick(&maintenance_application).await {
                        tracing::warn!(error = %error, "Replacement maintenance tick failed");
                    }
                }
            }
        }
    });

    let watch_application = Arc::clone(&application);
    let watch_cancel = cancel.clone();
    let watch_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(WATCH_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = watch_cancel.cancelled() => return,
                _ = interval.tick() => {
                    if let Err(error) = crate::import_v2::scan_watched_folders(&watch_application).await {
                        tracing::warn!(error = %error, "Replacement watched-folder scan failed");
                    }
                }
            }
        }
    });

    let cloud_application = Arc::clone(&application);
    let cloud_cancel = cancel.clone();
    let cloud_handle = tokio::spawn(async move {
        let mut state = crate::cloud::worker::WorkerState::default();
        let mut interval = tokio::time::interval(CLOUD_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cloud_cancel.cancelled() => return,
                _ = interval.tick() => {
                    match crate::cloud::worker::tick(&cloud_application, &mut state).await {
                        Ok(result) if result.state_changed => {
                            match cloud_application.store().revision() {
                                Ok(revision) => cloud_application.publish(&MutationReceipt {
                                    revision,
                                    resources: vec![
                                        resources::CLOUD.to_string(),
                                        resources::TASKS.to_string(),
                                    ],
                                    item_ids: Vec::new(),
                                }),
                                Err(error) => tracing::warn!(error = %error, "Reading revision after cloud sync failed"),
                            }
                        }
                        Ok(_) => {}
                        Err(error) => tracing::warn!(error = %error, "Cloud folder sync failed"),
                    }
                }
            }
        }
    });

    let mut handles = vec![
        ("replacement_subscription_scheduler", scheduler_handle),
        ("replacement_maintenance", maintenance_handle),
        ("replacement_folder_watches", watch_handle),
        ("replacement_cloud_sync", cloud_handle),
    ];
    handles.extend(
        subscription_handles
            .into_iter()
            .map(|handle| ("replacement_subscription_worker", handle)),
    );
    Ok(handles)
}

/// Start the deliberately small tutorial runtime. The real maintenance and
/// subscription pipelines remain in use, but the only source runner can read
/// bundled fixture files and no scheduler, watch, cloud, or network worker is
/// created.
pub fn start_tutorial(
    application: Arc<Application>,
    cancel: CancellationToken,
    fixture_root: PathBuf,
) -> Result<Vec<(&'static str, tokio::task::JoinHandle<()>)>, String> {
    recover(&application, &Utc::now().to_rfc3339())?;

    let maintenance_application = Arc::clone(&application);
    let maintenance_cancel = cancel.clone();
    let maintenance_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(MAINTENANCE_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = maintenance_cancel.cancelled() => return,
                _ = interval.tick() => {
                    if let Err(error) = maintenance_tick(&maintenance_application).await {
                        tracing::warn!(error = %error, "Tutorial maintenance tick failed");
                    }
                }
            }
        }
    });

    let source_application = Arc::clone(&application);
    let source_cancel = cancel.clone();
    let source_handle = tokio::spawn(async move {
        let worker = SubscriptionWorker::with_cancellation(
            &source_application,
            crate::tutorial_source_v2::TutorialSourceRunner::new(fixture_root),
            source_cancel.clone(),
        );
        let mut interval = tokio::time::interval(SUBSCRIPTION_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = source_cancel.cancelled() => return,
                _ = interval.tick() => {
                    let now = Utc::now().to_rfc3339();
                    if let Err(error) = worker.tick(&now).await {
                        tracing::warn!(error = %error, "Tutorial subscription worker failed");
                    }
                }
            }
        }
    });

    Ok(vec![
        ("tutorial_maintenance", maintenance_handle),
        ("tutorial_subscription_worker", source_handle),
    ])
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;

    use tokio::sync::mpsc;

    use super::*;
    use crate::store::Store;
    use crate::subscription_runtime_v2::{RunnerFailure, RunnerSuccess, SourceEvent};
    use crate::subscriptions_v2::{QueryInput, SubscriptionInput};

    struct EmptyRunner;

    impl SourceRunner for EmptyRunner {
        fn run<'a>(
            &'a self,
            _query: &'a crate::subscriptions_v2::ClaimedQueryRun,
            _output: mpsc::Sender<SourceEvent>,
            _cancel: CancellationToken,
        ) -> Pin<Box<dyn Future<Output = Result<RunnerSuccess, RunnerFailure>> + Send + 'a>>
        {
            Box::pin(async { Ok(RunnerSuccess::default()) })
        }
    }

    #[tokio::test]
    async fn due_schedule_runs_through_the_persisted_worker() {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        let subscription_id = subscriptions_v2::create_subscription(
            application.store(),
            &SubscriptionInput {
                subscription_key: "scheduled".into(),
                name: "Scheduled".into(),
                schedule: "daily".into(),
                paused: false,
                initial_post_limit: None,
                periodic_post_limit: None,
            },
            "2026-01-01T00:00:00Z",
        )
        .unwrap();
        subscriptions_v2::create_query(
            application.store(),
            subscription_id,
            &QueryInput {
                query_key: "query".into(),
                site_id: "example".into(),
                domain_key: "example.test".into(),
                query_kind: "search".into(),
                query_text: "artist".into(),
                display_name: None,
                notes: None,
            },
        )
        .unwrap();
        let worker = SubscriptionWorker::new(&application, EmptyRunner);

        let result = subscription_tick(&application, &worker, "2026-01-02T00:00:00Z")
            .await
            .unwrap();

        assert_eq!(
            result,
            SubscriptionTickResult {
                scheduled_runs: 1,
                ran_query: true,
            }
        );
        let state: String = application
            .store()
            .read(|connection| {
                connection.query_row("SELECT status FROM subscription_run", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(state, "succeeded");
    }

    #[test]
    fn idle_recovery_does_not_advance_revision() {
        let directory = tempfile::tempdir().unwrap();
        let application = Application::new(Arc::new(Store::open(directory.path()).unwrap()));
        assert_eq!(application.store().revision().unwrap(), 0);
        assert_eq!(
            recover(&application, "2026-01-01T00:00:00Z").unwrap(),
            StartupRecovery::default()
        );
        assert_eq!(application.store().revision().unwrap(), 0);
    }
}
