//! Background scheduler for subscription-owned recurring schedules.
//!
//! Called by the subscription scheduler worker spawned in `workers.rs`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::types::RunningSubscriptions;

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

fn schedule_interval_seconds(schedule: &str) -> Option<i64> {
    match schedule {
        "daily" => Some(86_400),
        "weekly" => Some(604_800),
        "monthly" => Some(2_592_000),
        _ => None,
    }
}

fn is_due(
    schedule: &str,
    last_full_run_at: Option<&str>,
    now: chrono::DateTime<chrono::Utc>,
) -> bool {
    let Some(interval_secs) = schedule_interval_seconds(schedule) else {
        return false;
    };
    let Some(last_run) = last_full_run_at else {
        return true;
    };
    let Ok(last_run) = chrono::DateTime::parse_from_rfc3339(last_run) else {
        return true;
    };
    (now - last_run.with_timezone(&chrono::Utc)).num_seconds() >= interval_secs
}

/// Trigger each overdue subscription through the normal full-run path.
pub async fn check_scheduled_subscriptions(
    db: &Arc<LibraryDatabase>,
    library_root: &std::path::Path,
    blob_store: &Arc<BlobStore>,
    rate_limiter: &RateLimiter,
    running_subs: &RunningSubscriptions,
    settings: &SettingsStore,
) {
    let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
        db.as_ref(),
        library_root,
    );
    let subscriptions = match runtime.list_scheduled_subscriptions().await {
        Ok(f) => f,
        Err(e) => {
            warn_scheduler_failure(
                "list_subscriptions",
                format!("Scheduler: failed to list scheduled subscriptions: {e}"),
            );
            return;
        }
    };

    let now = chrono::Utc::now();
    for subscription in subscriptions {
        if is_due(
            &subscription.schedule,
            subscription.last_full_run_at.as_deref(),
            now,
        ) {
            let subscription_id = subscription.subscription_id.to_string();
            tracing::info!(
                subscription_id = subscription.subscription_id,
                name = %subscription.name,
                schedule = %subscription.schedule,
                "Scheduler: running overdue subscription"
            );
            if let Err(e) = crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::run_scheduled_subscription(
                    db,
                    library_root,
                    blob_store,
                    rate_limiter,
                    running_subs,
                    subscription_id,
                    settings,
                )
                .await
            {
                warn_scheduler_failure(
                    "run_subscription",
                    format!(
                        "Scheduler: failed to start subscription {}: {}",
                        subscription.subscription_id, e
                    ),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_due;
    use chrono::{TimeZone, Utc};

    #[test]
    fn only_recurring_schedules_become_due() {
        let now = Utc.with_ymd_and_hms(2026, 8, 5, 12, 0, 0).unwrap();
        assert!(!is_due("manual", None, now));
        assert!(is_due("daily", None, now));
        assert!(is_due("weekly", None, now));
        assert!(is_due("monthly", None, now));
    }

    #[test]
    fn schedule_intervals_use_the_latest_full_run() {
        let now = Utc.with_ymd_and_hms(2026, 8, 5, 12, 0, 0).unwrap();
        assert!(!is_due("daily", Some("2026-08-05T11:59:59Z"), now));
        assert!(is_due("daily", Some("2026-08-04T12:00:00Z"), now));
        assert!(!is_due("weekly", Some("2026-07-30T12:00:01Z"), now));
        assert!(is_due("weekly", Some("2026-07-29T12:00:00Z"), now));
        assert!(!is_due("monthly", Some("2026-07-06T12:00:01Z"), now));
        assert!(is_due("monthly", Some("2026-07-06T12:00:00Z"), now));
    }
}
