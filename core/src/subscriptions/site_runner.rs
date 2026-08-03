use std::collections::HashSet;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::subscriptions::executor::execute_query_job;
use crate::subscriptions::source_adapter::runner_key_for_site;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

pub async fn start_worker_loop(
    db: Arc<LibraryDatabase>,
    library_root: std::path::PathBuf,
    rate_limiter: RateLimiter,
    running_subs: RunningSubscriptions,
    sub_terminal_statuses: SubTerminalStatuses,
    cancel: CancellationToken,
) {
    let active_sites = Arc::new(tokio::sync::Mutex::new(HashSet::<String>::new()));

    loop {
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(250)) => {}
            _ = cancel.cancelled() => {
                tracing::info!("Subscription site runner cancelled");
                return;
            }
        }

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
            {
                let active = active_sites.lock().await;
                if active.contains(&runner_key) {
                    continue;
                }
            }

            let leased = match runtime.lease_subscription_query_job(job.job_id).await {
                Ok(Some(job)) => job,
                Ok(None) => continue,
                Err(error) => {
                    tracing::warn!(job_id = job.job_id, error = %error, "subscription site runner: failed to lease job");
                    continue;
                }
            };

            {
                let mut active = active_sites.lock().await;
                active.insert(runner_key.clone());
            }

            let db = db.clone();
            let rate_limiter = rate_limiter.clone();
            let running_subs = running_subs.clone();
            let sub_terminal_statuses = sub_terminal_statuses.clone();
            let library_root = library_root.clone();
            let active_sites = active_sites.clone();
            tokio::spawn(async move {
                let job_id = leased.job_id;
                let subscription_id = leased.subscription_id;

                // The job is already leased ('running') — every exit from this
                // task must either execute it or fail it, and must release
                // active_sites, or the domain wedges until restart.
                let fail_leased_job = |kind: &'static str, message: String| {
                    let db = db.clone();
                    let library_root = library_root.clone();
                    let running_subs = running_subs.clone();
                    async move {
                        let runtime =
                            crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
                                db.as_ref(),
                                &library_root,
                            );
                        let _ = runtime
                            .finish_subscription_query_job(
                                job_id,
                                "failed",
                                Some(kind.to_string()),
                                Some(message),
                            )
                            .await;
                        let _ = crate::subscriptions::job_queue::clear_subscription_guard_if_idle(
                            &runtime,
                            &running_subs,
                            subscription_id,
                        )
                        .await;
                        let _ = runtime
                            .finalize_open_runs_for_subscription(
                                subscription_id,
                                "failed",
                                Some(kind),
                                None,
                            )
                            .await;
                        crate::runtime_state::remove_task(&format!("sub:{subscription_id}"));
                    }
                };

                match crate::state::get_state() {
                    Ok(state) => {
                        let app_settings = state.settings.get();
                        let exec = tokio::spawn(execute_query_job(
                            db.clone(),
                            library_root.clone(),
                            rate_limiter,
                            running_subs.clone(),
                            sub_terminal_statuses,
                            app_settings,
                            leased,
                        ));
                        if let Err(join_error) = exec.await {
                            tracing::error!(job_id, error = %join_error, "subscription executor panicked");
                            fail_leased_job("panic", format!("Executor panicked: {join_error}"))
                                .await;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(job_id, error = %error, "subscription site runner: missing state");
                        fail_leased_job("environment", "App state unavailable".to_string()).await;
                    }
                }
                let mut active = active_sites.lock().await;
                active.remove(&runner_key);
            });
        }
    }
}
