use std::collections::HashSet;
use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use futures_util::FutureExt;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::subscriptions::executor::execute_query_job;
use crate::subscriptions::gallery_dl_runner::FailureKind;
use crate::subscriptions::job_queue::{
    clear_subscription_guard_if_idle, ensure_subscription_guard,
};
use crate::subscriptions::source_adapter::runner_key_for_site;
use crate::subscriptions::types::SubscriptionQueryJob;
use crate::types::RunningSubscriptions;

async fn settle_panicked_job(
    db: &LibraryDatabase,
    library_root: &std::path::Path,
    running_subs: &RunningSubscriptions,
    job: &SubscriptionQueryJob,
) {
    let runtime =
        crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(db, library_root);
    let message = "Subscription executor panicked";
    let _ = runtime
        .upsert_subscription_issue(
            job.subscription_id,
            Some(job.query_id),
            FailureKind::Panic,
            message,
            None,
        )
        .await;
    let _ = runtime
        .finish_subscription_query_job(
            job.job_id,
            "failed",
            Some(FailureKind::Panic.as_str().to_string()),
            Some(message.to_string()),
        )
        .await;
    if let Some(run_id) = job.run_id {
        let _ = runtime.finalize_subscription_run_if_terminal(run_id).await;
    }
    let _ = clear_subscription_guard_if_idle(&runtime, running_subs, job.subscription_id).await;
    crate::runtime_state::remove_task(&format!("sub:{}", job.subscription_id));
}

pub async fn start_worker_loop(
    db: Arc<LibraryDatabase>,
    library_root: std::path::PathBuf,
    rate_limiter: RateLimiter,
    running_subs: RunningSubscriptions,
    cancel: CancellationToken,
) {
    let mut active_sites = HashSet::<String>::new();
    let mut executors = JoinSet::<String>::new();
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(250));

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                let tokens = running_subs.lock().await.values().cloned().collect::<Vec<_>>();
                for token in tokens {
                    token.cancel();
                }
                let drained = tokio::time::timeout(
                    std::time::Duration::from_secs(5),
                    async {
                        while executors.join_next().await.is_some() {}
                    },
                ).await;
                if drained.is_err() {
                    tracing::warn!("Subscription executors exceeded shutdown timeout; aborting them");
                    executors.shutdown().await;
                }
                tracing::info!("Subscription site runner cancelled after executor cleanup");
                return;
            }
            completed = executors.join_next(), if !executors.is_empty() => {
                match completed {
                    Some(Ok(runner_key)) => {
                        active_sites.remove(&runner_key);
                    }
                    Some(Err(error)) => {
                        tracing::error!(error = %error, "Subscription executor task failed");
                    }
                    None => {}
                }
            }
            _ = tick.tick() => {
                let app_settings = match crate::state::get_state() {
                    Ok(state) => state.settings.get(),
                    Err(_) => continue,
                };
                let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
                    db.as_ref(),
                    &library_root,
                );
                let queued_jobs = match runtime.list_queued_subscription_query_jobs(64).await {
                    Ok(jobs) => jobs,
                    Err(error) => {
                        tracing::warn!(error = %error, "subscription site runner: failed to list queued jobs");
                        continue;
                    }
                };

                for job in queued_jobs {
                    let runner_key = runner_key_for_site(&job.site_id);
                    if active_sites.contains(&runner_key) {
                        continue;
                    }
                    let leased = match runtime.lease_subscription_query_job(job.job_id).await {
                        Ok(Some(job)) => job,
                        Ok(None) => continue,
                        Err(error) => {
                            tracing::warn!(job_id = job.job_id, error = %error, "subscription site runner: failed to lease job");
                            continue;
                        }
                    };

                    ensure_subscription_guard(&running_subs, leased.subscription_id).await;
                    active_sites.insert(runner_key.clone());
                    let task_db = db.clone();
                    let task_root = library_root.clone();
                    let task_rate_limiter = rate_limiter.clone();
                    let task_running_subs = running_subs.clone();
                    let task_settings = app_settings.clone();
                    let task_shutdown = cancel.clone();
                    executors.spawn(async move {
                        let outcome = AssertUnwindSafe(execute_query_job(
                            task_db.clone(),
                            task_root.clone(),
                            task_rate_limiter,
                            task_running_subs.clone(),
                            task_shutdown,
                            task_settings,
                            leased.clone(),
                        ))
                        .catch_unwind()
                        .await;
                        if outcome.is_err() {
                            tracing::error!(job_id = leased.job_id, "Subscription executor panicked");
                            settle_panicked_job(
                                task_db.as_ref(),
                                &task_root,
                                &task_running_subs,
                                &leased,
                            )
                            .await;
                        }
                        runner_key
                    });
                }
            }
        }
    }
}
