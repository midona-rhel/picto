//! Subscription group run/stop orchestration.

use std::sync::Arc;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator;
use crate::subscriptions::runtime_tasks::{
    publish_cancelling, publish_group_failed, publish_group_finished, publish_group_progress,
    publish_group_start,
};
use crate::types::RunningSubscriptions;

pub struct SubscriptionGroupOrchestrator;

impl SubscriptionGroupOrchestrator {
    pub async fn run_group(
        db: &Arc<LibraryDatabase>,
        library_root: &std::path::Path,
        blob_store: &Arc<BlobStore>,
        rate_limiter: &RateLimiter,
        running_subs: &RunningSubscriptions,
        id: String,
        settings: &SettingsStore,
    ) -> Result<(), String> {
        let group_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid group id: {}", id))?;

        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            library_root,
        );
        let subs = runtime.list_subscriptions_for_group(group_id).await?;
        if subs.is_empty() {
            return Err("Group has no subscriptions".to_string());
        }

        publish_group_start(&id);

        let mut started = 0u32;
        let mut last_err = String::new();
        let mut started_runs = Vec::new();

        for sub in subs {
            if sub.paused {
                continue;
            }
            let sub_id_str = sub.subscription_id.to_string();
            {
                let map = running_subs.lock().await;
                if map.contains_key(&sub_id_str) {
                    continue;
                }
            }
            match SubscriptionRunOrchestrator::run_subscription(
                db,
                library_root,
                blob_store,
                rate_limiter,
                running_subs,
                sub_id_str.clone(),
                settings,
            )
            .await
            {
                Ok(run_id) => {
                    started += 1;
                    started_runs.push((sub_id_str, run_id));
                }
                Err(e) => {
                    tracing::warn!(
                        subscription_id = sub.subscription_id,
                        "Group run: failed to start subscription: {e}"
                    );
                    last_err = e;
                }
            }
        }

        if started == 0 && !last_err.is_empty() {
            publish_group_failed(&id);
            return Err(format!("Failed to start: {last_err}"));
        }

        if started == 0 {
            publish_group_finished(&id, false, 0, 0);
            return Ok(());
        }

        let group_id_str = id.clone();
        let group_id_guard = id.clone();
        let running_subs_clone = running_subs.clone();
        let monitor_db = db.clone();
        let monitor_root = library_root.to_path_buf();

        tokio::spawn(async move {
            let inner = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let map = running_subs_clone.lock().await;
                    let still_running_count = started_runs
                        .iter()
                        .filter(|(sub_id, _)| map.contains_key(sub_id))
                        .count();
                    if still_running_count == 0 {
                        break;
                    }
                    drop(map);

                    let done = started as usize - still_running_count;
                    publish_group_progress(&group_id_str, done as u64, started as u64);
                }

                let runtime =
                    crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
                        monitor_db.as_ref(),
                        &monitor_root,
                    );
                let mut has_failed = false;
                for (subscription_id, run_id) in &started_runs {
                    let Ok(subscription_id) = subscription_id.parse::<i64>() else {
                        continue;
                    };
                    let failed = runtime
                        .list_subscription_runs(subscription_id, 20)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .any(|run| run.run_id == *run_id && run.status == "failed");
                    has_failed |= failed;
                }
                publish_group_finished(&group_id_str, has_failed, started as u64, started as u64);
            });

            if let Err(e) = inner.await {
                tracing::error!(group_id = %group_id_guard, "Group monitor panicked: {e}");
                publish_group_failed(&group_id_guard);
            }
        });

        Ok(())
    }

    pub async fn stop_group(
        db: &LibraryDatabase,
        library_root: &std::path::Path,
        running_subs: &RunningSubscriptions,
        id: String,
    ) -> Result<(), String> {
        let group_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid group id: {}", id))?;

        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db,
            library_root,
        );
        let subscriptions = runtime.list_subscriptions_for_group(group_id).await?;
        let mut names_by_id = std::collections::HashMap::new();
        for sub in &subscriptions {
            names_by_id.insert(sub.subscription_id.to_string(), sub.name.clone());
        }

        let sub_ids: Vec<i64> = subscriptions
            .iter()
            .map(|sub| sub.subscription_id)
            .collect();
        let mut cancelled_ids = Vec::new();
        for sub_id in sub_ids {
            let sub_id_str = sub_id.to_string();
            if crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
                db,
                library_root,
                running_subs,
                sub_id_str.clone(),
            )
            .await
            .is_ok()
            {
                cancelled_ids.push(sub_id_str);
            }
        }

        for sub_id in cancelled_ids {
            let sub_name = names_by_id
                .get(&sub_id)
                .cloned()
                .unwrap_or_else(|| format!("Subscription {sub_id}"));
            publish_cancelling(&sub_id, &sub_name);
        }
        Ok(())
    }
}
