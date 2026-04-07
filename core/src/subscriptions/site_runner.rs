use std::collections::HashSet;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::subscriptions::executor::execute_query_job;
use crate::subscriptions::source_adapter::runner_key_for_site;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

pub async fn start_worker_loop(
    db: Arc<LibraryDatabase>,
    library_root: std::path::PathBuf,
    blob_store: Arc<BlobStore>,
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

        let runtime =
            crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(db.as_ref(), &library_root);
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
            let blob_store = blob_store.clone();
            let rate_limiter = rate_limiter.clone();
            let running_subs = running_subs.clone();
            let sub_terminal_statuses = sub_terminal_statuses.clone();
            let library_root = library_root.clone();
            let active_sites = active_sites.clone();
            tokio::spawn(async move {
                let app_settings = match crate::state::get_state() {
                    Ok(state) => state.settings.get(),
                    Err(error) => {
                        tracing::warn!(job_id = leased.job_id, error = %error, "subscription site runner: missing state");
                        let mut active = active_sites.lock().await;
                        active.remove(&runner_key);
                        return;
                    }
                };
                execute_query_job(
                    db,
                    library_root,
                    blob_store,
                    rate_limiter,
                    running_subs,
                    sub_terminal_statuses,
                    app_settings,
                    leased,
                )
                .await;
                let mut active = active_sites.lock().await;
                active.remove(&runner_key);
            });
        }
    }
}
