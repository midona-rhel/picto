//! Background loops for the replacement backend.
//!
//! Durable queue tables own state. These loops only wake, execute bounded
//! work, publish compact invalidations, and honor application shutdown.

use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::app::{resources, Application, MutationReceipt};
use crate::background_runtime_v2::{self, DrainBatchResult};
use crate::gallery_dl_source_v2::GalleryDlSourceRunner;
use crate::ingest_queue_v2::{self, IngestRunReport};
use crate::subscription_runtime_v2::{SourceRunner, SubscriptionWorker};
use crate::subscriptions_v2::{self, RecoveryCounts};

const SUBSCRIPTION_TICK: StdDuration = StdDuration::from_secs(1);
const MAINTENANCE_TICK: StdDuration = StdDuration::from_millis(250);
const WATCH_TICK: StdDuration = StdDuration::from_secs(30);
const INGEST_BATCH_SIZE: usize = 8;
const WORK_BATCH_SIZE: usize = 8;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StartupRecovery {
    pub subscription_runs: usize,
    pub subscription_query_runs: usize,
    pub ingest_jobs: usize,
    pub work_items: usize,
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
    let work_items = crate::workers_v2::reset_running(application.store())?;
    Ok(StartupRecovery {
        subscription_runs: runs,
        subscription_query_runs: query_runs,
        ingest_jobs,
        work_items,
    })
}

pub async fn subscription_tick<R: SourceRunner>(
    application: &Application,
    worker: &mut SubscriptionWorker<'_, R>,
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
    let work = background_runtime_v2::drain_batch(application, WORK_BATCH_SIZE).await?;
    if let Some(receipt) = &work.receipt {
        application.publish(receipt);
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
    let runner = GalleryDlSourceRunner::open(application.store().library_root())?;

    let subscription_application = Arc::clone(&application);
    let subscription_cancel = cancel.clone();
    let subscription_handle = tokio::spawn(async move {
        let mut worker = SubscriptionWorker::with_cancellation(
            &subscription_application,
            runner,
            subscription_cancel.clone(),
        );
        let mut interval = tokio::time::interval(SUBSCRIPTION_TICK);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = subscription_cancel.cancelled() => return,
                _ = interval.tick() => {
                    let now = Utc::now().to_rfc3339();
                    if let Err(error) = subscription_tick(
                        &subscription_application,
                        &mut worker,
                        &now,
                    ).await {
                        tracing::warn!(error = %error, "Replacement subscription tick failed");
                    }
                }
            }
        }
    });

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

    Ok(vec![
        ("replacement_subscriptions", subscription_handle),
        ("replacement_maintenance", maintenance_handle),
        ("replacement_folder_watches", watch_handle),
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
    use crate::subscription_runtime_v2::{DownloadedItem, RunnerFailure, RunnerSuccess};
    use crate::subscriptions_v2::{QueryInput, SubscriptionInput};

    struct EmptyRunner;

    impl SourceRunner for EmptyRunner {
        fn run<'a>(
            &'a self,
            _query: &'a crate::subscriptions_v2::ClaimedQueryRun,
            _output: mpsc::Sender<DownloadedItem>,
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
        let mut worker = SubscriptionWorker::new(&application, EmptyRunner);

        let result = subscription_tick(&application, &mut worker, "2026-01-02T00:00:00Z")
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
