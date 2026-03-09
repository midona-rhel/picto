//! Background scheduler — checks for overdue subscription groups.
//!
//! Called by the group_scheduler worker spawned in `workers.rs`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::sqlite::SqliteDatabase;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

const SCHEDULER_WARN_WINDOW: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
struct SchedulerWarnWindow {
    started_at: Instant,
    count: u64,
    message: String,
}

static SCHEDULER_WARNINGS: OnceLock<Mutex<HashMap<&'static str, SchedulerWarnWindow>>> =
    OnceLock::new();

fn warn_scheduler_failure(kind: &'static str, message: String) {
    let warnings = SCHEDULER_WARNINGS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = crate::poison::mutex_or_recover(warnings, "scheduler::warnings");
    let now = Instant::now();

    match guard.get_mut(kind) {
        Some(window)
            if window.message == message
                && now.duration_since(window.started_at) <= SCHEDULER_WARN_WINDOW =>
        {
            window.count += 1;
        }
        Some(window) => {
            if window.count > 1 {
                tracing::warn!(
                    scheduler_kind = kind,
                    suppressed = window.count - 1,
                    message = %window.message,
                    window_secs = SCHEDULER_WARN_WINDOW.as_secs(),
                    "Scheduler: suppressed repeated failure"
                );
            }
            *window = SchedulerWarnWindow {
                started_at: now,
                count: 1,
                message: message.clone(),
            };
            tracing::warn!(scheduler_kind = kind, "{message}");
        }
        None => {
            guard.insert(
                kind,
                SchedulerWarnWindow {
                    started_at: now,
                    count: 1,
                    message: message.clone(),
                },
            );
            tracing::warn!(scheduler_kind = kind, "{message}");
        }
    }
}

/// Check all subscription groups for overdue scheduled runs and trigger them.
pub async fn check_scheduled_groups(
    db: &Arc<SqliteDatabase>,
    blob_store: &Arc<BlobStore>,
    rate_limiter: &RateLimiter,
    running_subs: &RunningSubscriptions,
    sub_terminal_statuses: &SubTerminalStatuses,
    settings: &SettingsStore,
) {
    let groups = match db.list_groups().await {
        Ok(f) => f,
        Err(e) => {
            warn_scheduler_failure("list_groups", format!("Scheduler: failed to list groups: {e}"));
            return;
        }
    };

    for group in groups {
        if group.schedule == "manual" {
            continue;
        }

        let interval_secs: i64 = match group.schedule.as_str() {
            "daily" => 86_400,
            "weekly" => 604_800,
            "monthly" => 2_592_000, // 30 days
            _ => continue,
        };

        let subs = match db.list_subscriptions_for_group(group.group_id).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        if subs.is_empty() {
            continue;
        }

        let mut latest_check: Option<chrono::DateTime<chrono::Utc>> = None;
        let mut has_any_queries = false;
        for sub in &subs {
            let queries = match db.get_subscription_queries(sub.subscription_id).await {
                Ok(q) => q,
                Err(_) => continue,
            };
            for q in &queries {
                has_any_queries = true;
                if let Some(ref t) = q.last_check_time {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(t) {
                        let utc = dt.with_timezone(&chrono::Utc);
                        latest_check = Some(
                            latest_check
                                .map_or(utc, |prev: chrono::DateTime<chrono::Utc>| prev.max(utc)),
                        );
                    }
                }
            }
        }

        if !has_any_queries {
            continue;
        }

        let now = chrono::Utc::now();
        let is_overdue = match latest_check {
            None => true, // Never ran
            Some(last) => (now - last).num_seconds() >= interval_secs,
        };

        if is_overdue {
            let group_id_str = group.group_id.to_string();
            tracing::info!(
                group_id = group.group_id,
                name = %group.name,
                schedule = %group.schedule,
                "Scheduler: running overdue group"
            );
            if let Err(e) = crate::subscriptions::subscription_group_controller::SubscriptionGroupController::run_group(
                db,
                blob_store,
                rate_limiter,
                running_subs,
                sub_terminal_statuses,
                group_id_str,
                settings,
            )
            .await
            {
                warn_scheduler_failure(
                    "run_group",
                    format!(
                        "Scheduler: failed to start group {}: {}",
                        group.group_id, e
                    ),
                );
            }
        }
    }
}
