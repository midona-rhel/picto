//! Subscription group run/stop orchestration.

use std::sync::Arc;

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::sqlite::SqliteDatabase;
use crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator;
use crate::subscriptions::runtime_tasks::{
    publish_cancelling, publish_group_failed, publish_group_finished, publish_group_progress,
    publish_group_start,
};
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

pub struct SubscriptionGroupOrchestrator;

impl SubscriptionGroupOrchestrator {
    pub async fn run_group(
        db: &Arc<SqliteDatabase>,
        blob_store: &Arc<BlobStore>,
        rate_limiter: &RateLimiter,
        running_subs: &RunningSubscriptions,
        sub_terminal_statuses: &SubTerminalStatuses,
        id: String,
        settings: &SettingsStore,
    ) -> Result<(), String> {
        let group_id: i64 = id.parse().map_err(|_| format!("Invalid group id: {}", id))?;

        let subs = db.list_subscriptions_for_group(group_id).await?;
        if subs.is_empty() {
            return Err("Group has no subscriptions".to_string());
        }

        {
            let mut statuses = sub_terminal_statuses.lock().await;
            statuses.clear();
        }

        publish_group_start(&id);

        let mut started = 0u32;
        let mut last_err = String::new();
        let mut started_sub_ids = Vec::new();

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
                blob_store,
                rate_limiter,
                running_subs,
                sub_id_str.clone(),
                Some(sub_terminal_statuses.clone()),
                settings,
            )
            .await
            {
                Ok(()) => {
                    started += 1;
                    started_sub_ids.push(sub_id_str);
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
        let terminal_statuses_clone = sub_terminal_statuses.clone();

        tokio::spawn(async move {
            let inner = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let map = running_subs_clone.lock().await;
                    let still_running_count = started_sub_ids
                        .iter()
                        .filter(|sub_id| map.contains_key(*sub_id))
                        .count();
                    if still_running_count == 0 {
                        break;
                    }
                    drop(map);

                    let done = started as usize - still_running_count;
                    publish_group_progress(&group_id_str, done as u64, started as u64);
                }

                let statuses = terminal_statuses_clone.lock().await;
                let has_failed = started_sub_ids
                    .iter()
                    .any(|sub_id| statuses.get(sub_id).map(|status| status == "failed").unwrap_or(false));
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
        db: &SqliteDatabase,
        running_subs: &RunningSubscriptions,
        id: String,
    ) -> Result<(), String> {
        let group_id: i64 = id.parse().map_err(|_| format!("Invalid group id: {}", id))?;

        let subscriptions = db.list_subscriptions_for_group(group_id).await?;
        let mut names_by_id = std::collections::HashMap::new();
        for sub in &subscriptions {
            names_by_id.insert(sub.subscription_id.to_string(), sub.name.clone());
        }

        let sub_ids = db.get_group_subscription_ids(group_id).await?;
        let map = running_subs.lock().await;
        let mut cancelled_ids = Vec::new();
        for sub_id in sub_ids {
            let sub_id_str = sub_id.to_string();
            if let Some(token) = map.get(&sub_id_str) {
                token.cancel();
                cancelled_ids.push(sub_id_str);
            }
        }
        drop(map);

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
