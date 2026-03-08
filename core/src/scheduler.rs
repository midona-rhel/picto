//! Background scheduler — checks for overdue flows and PTR sync.
//!
//! Called by the flow_scheduler worker spawned in `workers.rs`.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::sqlite::{CompilerEvent, SqliteDatabase};
use crate::ptr::db::PtrSqliteDatabase;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

/// Check all flows for overdue scheduled runs and trigger them.
pub async fn check_scheduled_flows(
    db: &Arc<SqliteDatabase>,
    blob_store: &Arc<BlobStore>,
    rate_limiter: &RateLimiter,
    running_subs: &RunningSubscriptions,
    sub_terminal_statuses: &SubTerminalStatuses,
    settings: &SettingsStore,
) {
    let flows = match db.list_flows().await {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!("Scheduler: failed to list flows: {e}");
            return;
        }
    };

    for flow in flows {
        if flow.schedule == "manual" {
            continue;
        }

        let interval_secs: i64 = match flow.schedule.as_str() {
            "daily" => 86_400,
            "weekly" => 604_800,
            "monthly" => 2_592_000, // 30 days
            _ => continue,
        };

        let subs = match db.list_subscriptions_for_flow(flow.flow_id).await {
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
            let flow_id_str = flow.flow_id.to_string();
            tracing::info!(
                flow_id = flow.flow_id,
                name = %flow.name,
                schedule = %flow.schedule,
                "Scheduler: running overdue flow"
            );
            if let Err(e) = crate::subscriptions::flow_controller::FlowController::run_flow(
                db,
                blob_store,
                rate_limiter,
                running_subs,
                sub_terminal_statuses,
                flow_id_str,
                settings,
            )
            .await
            {
                tracing::warn!(
                    flow_id = flow.flow_id,
                    "Scheduler: failed to start flow: {e}"
                );
            }
        }
    }
}

/// Check if PTR needs syncing — either initial population or scheduled auto-sync.
pub async fn check_scheduled_ptr_sync(
    ptr_db: &Arc<PtrSqliteDatabase>,
    settings: &SettingsStore,
    compiler_tx: mpsc::UnboundedSender<CompilerEvent>,
) {
    let s = settings.get();

    if !s.ptr_enabled {
        return;
    }

    // Short-circuit when any PTR heavy phase is running (PBI-024).
    if crate::ptr::controller::PtrController::is_ptr_busy_for_scheduler() {
        return;
    }

    // Don't hammer the server — back off after failed attempts
    if crate::ptr::controller::PtrController::is_auto_sync_cooling_down() {
        return;
    }

    // Force sync if PTR has never completed initial population,
    // regardless of auto_sync or schedule settings.
    if s.ptr_last_sync_time.is_none() {
        tracing::info!("PTR has never completed initial population — starting sync");
        if let Err(e) =
            crate::ptr::controller::PtrController::sync(ptr_db, settings, compiler_tx).await
        {
            tracing::warn!("Failed to start PTR initial population sync: {e}");
        }
        return;
    }

    // Regular auto-sync: requires auto_sync enabled + valid schedule
    if !s.ptr_auto_sync {
        return;
    }

    let interval_secs: i64 = match s.ptr_sync_schedule.as_str() {
        "daily" => 86_400,
        "weekly" => 604_800,
        "monthly" => 2_592_000,
        _ => return,
    };

    let now = chrono::Utc::now();
    let is_overdue = match &s.ptr_last_sync_time {
        Some(t) => match chrono::DateTime::parse_from_rfc3339(t) {
            Ok(last) => (now - last.with_timezone(&chrono::Utc)).num_seconds() >= interval_secs,
            Err(_) => true,
        },
        None => unreachable!(), // Handled above
    };

    if is_overdue {
        tracing::info!(
            schedule = %s.ptr_sync_schedule,
            "Scheduler: running overdue PTR sync"
        );
        if let Err(e) =
            crate::ptr::controller::PtrController::sync(ptr_db, settings, compiler_tx).await
        {
            tracing::warn!("Scheduler: failed to start PTR sync: {e}");
        }
    }
}
