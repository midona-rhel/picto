//! Flow controller — orchestrates flow-level CRUD, execution, and scheduling.

use std::sync::Arc;

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::settings::SettingsStore;
use crate::sqlite::SqliteDatabase;
use crate::subscription_controller::SubscriptionController;
use crate::types::{
    FlowInfo, RunningSubscriptions, SubTerminalStatuses, SubscriptionInfo, SubscriptionQueryInfo,
};

pub struct FlowController;

impl FlowController {
    // PBI-040: Bulk read — constant query count per flow instead of O(N) per subscription.
    pub async fn get_flows(db: &SqliteDatabase) -> Result<Vec<FlowInfo>, String> {
        let start = std::time::Instant::now();
        let flows = db.list_flows().await?;
        let mut result = Vec::with_capacity(flows.len());

        for flow in flows {
            let fid = flow.flow_id;

            let subs_with_counts = db.list_subscriptions_for_flow_with_file_counts(fid).await?;
            let all_queries = db.list_subscription_queries_for_flow(fid).await?;

            let mut queries_map: std::collections::HashMap<i64, Vec<SubscriptionQueryInfo>> =
                std::collections::HashMap::new();
            for q in all_queries {
                queries_map
                    .entry(q.subscription_id)
                    .or_default()
                    .push(SubscriptionQueryInfo {
                        id: q.query_id.to_string(),
                        query_text: q.query_text.clone(),
                        display_name: q.display_name.or(Some(q.query_text)),
                        paused: q.paused,
                        last_check_time: q.last_check_time,
                        files_found: q.files_found as u64,
                        completed_initial_run: q.completed_initial_run,
                        resume_cursor: q.resume_cursor,
                        resume_strategy: q.resume_strategy,
                    });
            }

            let mut flow_total: u64 = 0;
            let sub_infos: Vec<SubscriptionInfo> = subs_with_counts
                .into_iter()
                .map(|(sub, file_count)| {
                    let sub_id = sub.subscription_id;
                    flow_total += file_count as u64;
                    let canonical_site_id =
                        crate::gallery_dl_runner::canonical_site_id(&sub.site_id);
                    SubscriptionInfo {
                        id: sub_id.to_string(),
                        name: sub.name,
                        site_id: canonical_site_id.to_string(),
                        paused: sub.paused,
                        flow_id: sub.flow_id.map(|id| id.to_string()),
                        initial_file_limit: sub.initial_file_limit as u32,
                        periodic_file_limit: sub.periodic_file_limit as u32,
                        created_at: sub.created_at,
                        total_files: file_count as u64,
                        queries: queries_map.remove(&sub_id).unwrap_or_default(),
                    }
                })
                .collect();

            result.push(FlowInfo {
                id: flow.flow_id.to_string(),
                name: flow.name,
                schedule: flow.schedule,
                created_at: flow.created_at,
                total_files: flow_total,
                subscriptions: sub_infos,
            });
        }

        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis() as u64,
            count = result.len(),
            "get_flows bulk read"
        );

        Ok(result)
    }

    /// Create a new flow with optional schedule.
    pub async fn create_flow(
        db: &SqliteDatabase,
        name: String,
        schedule: Option<String>,
    ) -> Result<FlowInfo, String> {
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Flow name cannot be empty".to_string());
        }
        let flow = db.create_flow(&trimmed).await?;
        let flow_id = flow.flow_id;

        if let Some(ref sched) = schedule {
            validate_schedule(sched)?;
            db.set_flow_schedule(flow_id, sched).await?;
        }

        let final_flow = db
            .get_flow(flow_id)
            .await?
            .ok_or_else(|| "Flow not found after creation".to_string())?;

        Ok(FlowInfo {
            id: final_flow.flow_id.to_string(),
            name: final_flow.name,
            schedule: final_flow.schedule,
            created_at: final_flow.created_at,
            total_files: 0,
            subscriptions: vec![],
        })
    }

    /// Delete a flow (CASCADE deletes subscriptions). Optionally delete associated files.
    pub async fn delete_flow(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        id: String,
        delete_files: Option<bool>,
    ) -> Result<(), String> {
        let flow_id: i64 = id.parse().map_err(|_| format!("Invalid flow id: {}", id))?;

        if delete_files.unwrap_or(false) {
            let sub_ids = db.get_flow_subscription_ids(flow_id).await?;
            for sub_id in sub_ids {
                let file_ids = db
                    .with_read_conn(move |conn| {
                        crate::sqlite::subscriptions::get_subscription_entity_ids(conn, sub_id)
                    })
                    .await?;
                if !file_ids.is_empty() {
                    let resolved = db.resolve_ids_batch(&file_ids).await?;
                    let hashes: Vec<String> = resolved.into_iter().map(|(_, hash)| hash).collect();
                    crate::lifecycle_controller::LifecycleController::delete_files(
                        db, blob_store, hashes,
                    )
                    .await?;
                }
            }
        }

        db.delete_flow(flow_id).await?;
        Ok(())
    }

    /// Rename a flow.
    pub async fn rename_flow(db: &SqliteDatabase, id: String, name: String) -> Result<(), String> {
        let flow_id: i64 = id.parse().map_err(|_| format!("Invalid flow id: {}", id))?;
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        db.rename_flow(flow_id, &trimmed).await
    }

    /// Set a flow's schedule.
    pub async fn set_flow_schedule(
        db: &SqliteDatabase,
        id: String,
        schedule: String,
    ) -> Result<(), String> {
        let flow_id: i64 = id.parse().map_err(|_| format!("Invalid flow id: {}", id))?;
        validate_schedule(&schedule)?;
        db.set_flow_schedule(flow_id, &schedule).await
    }

    /// Run all non-paused subscriptions in a flow.
    ///
    /// Emits `flow-started` immediately. Emits `flow-finished` only when all
    /// child subscriptions reach a terminal state (success, failure, or cancel).
    pub async fn run_flow(
        db: &Arc<SqliteDatabase>,
        blob_store: &Arc<BlobStore>,
        rate_limiter: &RateLimiter,
        running_subs: &RunningSubscriptions,
        sub_terminal_statuses: &SubTerminalStatuses,
        id: String,
        settings: &SettingsStore,
    ) -> Result<(), String> {
        let flow_id: i64 = id.parse().map_err(|_| format!("Invalid flow id: {}", id))?;

        let subs = db.list_subscriptions_for_flow(flow_id).await?;
        if subs.is_empty() {
            return Err("Flow has no subscriptions".to_string());
        }

        {
            let mut statuses = sub_terminal_statuses.lock().await;
            statuses.clear();
        }

        crate::events::emit(
            crate::events::event_names::FLOW_STARTED,
            &crate::events::FlowStartedEvent {
                flow_id: id.clone(),
                subscription_count: subs.iter().filter(|s| !s.paused).count(),
            },
        );

        let mut started = 0u32;
        let mut last_err = String::new();
        // PBI-034: Track only the IDs we actually started, not all non-paused subs.
        let mut started_sub_ids: Vec<String> = Vec::new();

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
            match SubscriptionController::run_subscription(
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
                        "Flow run: failed to start subscription: {e}"
                    );
                    last_err = e;
                }
            }
        }
        if started == 0 && !last_err.is_empty() {
            crate::events::emit(
                crate::events::event_names::FLOW_FINISHED,
                &crate::events::FlowFinishedEvent {
                    flow_id: id.clone(),
                    status: "failed".to_string(),
                    started_count: None,
                    error: Some(last_err.clone()),
                },
            );
            return Err(format!("Failed to start: {last_err}"));
        }

        if started == 0 {
            crate::events::emit(
                crate::events::event_names::FLOW_FINISHED,
                &crate::events::FlowFinishedEvent {
                    flow_id: id.clone(),
                    status: "succeeded".to_string(),
                    started_count: Some(0),
                    error: None,
                },
            );
            return Ok(());
        }

        // PBI-034: Monitor only the subscriptions we actually started.
        let flow_id_str = id.clone();
        let flow_id_guard = id.clone();
        let running_subs_clone = running_subs.clone();
        let terminal_statuses_clone = sub_terminal_statuses.clone();

        // Panic-safe outer/inner pattern (matches subscription_controller).
        tokio::spawn(async move {
            let inner = tokio::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    let map = running_subs_clone.lock().await;
                    let still_running_count = started_sub_ids
                        .iter()
                        .filter(|id| map.contains_key(*id))
                        .count();
                    if still_running_count == 0 {
                        break;
                    }
                    drop(map);

                    let done = started as usize - still_running_count;
                    crate::events::emit(
                        crate::events::event_names::FLOW_PROGRESS,
                        &crate::events::FlowProgressEvent {
                            flow_id: flow_id_str.clone(),
                            total: started,
                            done,
                            remaining: still_running_count,
                        },
                    );
                }

                // PBI-002: Aggregate terminal statuses from child subscriptions.
                let statuses = terminal_statuses_clone.lock().await;
                let has_failed = started_sub_ids
                    .iter()
                    .any(|id| statuses.get(id).map(|s| s == "failed").unwrap_or(false));
                let has_cancelled = started_sub_ids
                    .iter()
                    .any(|id| statuses.get(id).map(|s| s == "cancelled").unwrap_or(false));
                let final_status = if has_failed {
                    "failed"
                } else if has_cancelled {
                    "cancelled"
                } else {
                    "succeeded"
                };

                crate::events::emit(
                    crate::events::event_names::FLOW_FINISHED,
                    &crate::events::FlowFinishedEvent {
                        flow_id: flow_id_str.clone(),
                        status: final_status.to_string(),
                        started_count: Some(started),
                        error: None,
                    },
                );
            });

            if let Err(e) = inner.await {
                tracing::error!(flow_id = %flow_id_guard, "Flow monitor panicked: {e}");
                crate::events::emit(
                    crate::events::event_names::FLOW_FINISHED,
                    &crate::events::FlowFinishedEvent {
                        flow_id: flow_id_guard,
                        status: "failed".to_string(),
                        started_count: None,
                        error: Some(format!("Monitor panicked: {e}")),
                    },
                );
            }
        });

        Ok(())
    }

    /// Stop all running subscriptions belonging to a flow.
    pub async fn stop_flow(
        db: &SqliteDatabase,
        running_subs: &RunningSubscriptions,
        id: String,
    ) -> Result<(), String> {
        let flow_id: i64 = id.parse().map_err(|_| format!("Invalid flow id: {}", id))?;

        let subscriptions = db.list_subscriptions_for_flow(flow_id).await?;
        let mut names_by_id = std::collections::HashMap::new();
        for sub in &subscriptions {
            names_by_id.insert(sub.subscription_id.to_string(), sub.name.clone());
        }

        let sub_ids = db.get_flow_subscription_ids(flow_id).await?;
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
            let progress = crate::subscription_sync::SubscriptionProgressEvent {
                subscription_id: sub_id.clone(),
                subscription_name: names_by_id
                    .get(&sub_id)
                    .cloned()
                    .unwrap_or_else(|| format!("Subscription {sub_id}")),
                mode: "subscription".to_string(),
                query_id: None,
                query_name: None,
                files_downloaded: 0,
                files_skipped: 0,
                pages_fetched: 0,
                metadata_validated: 0,
                metadata_invalid: 0,
                last_metadata_error: None,
                status_text: "Cancelling…".to_string(),
            };
            crate::subscription_sync::update_runtime_progress_snapshot(progress.clone());
            crate::events::emit(
                crate::events::event_names::SUBSCRIPTION_PROGRESS,
                &progress,
            );
        }
        Ok(())
    }
}

fn validate_schedule(schedule: &str) -> Result<(), String> {
    match schedule {
        "manual" | "daily" | "weekly" | "monthly" => Ok(()),
        _ => Err(format!(
            "Invalid schedule: {}. Must be one of: manual, daily, weekly, monthly",
            schedule
        )),
    }
}
