//! Subscription run orchestration.
//!
//! This isolates execution, cancellation, and runtime publication from the
//! CRUD controller so subscription behavior can evolve behind one service.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::sqlite::SqliteDatabase;
use crate::subscriptions::db::{get_subscription, get_subscription_query};
use crate::subscriptions::policy::{
    effective_query_file_limit, resolve_finished_status_text, resolve_query_name,
};
use crate::subscriptions::progress::{SubscriptionProgressEvent, list_runtime_progress_from_tasks};
use crate::subscriptions::runtime_tasks::{
    publish_cancelling, publish_finished, publish_panic, publish_start,
    schedule_progress_snapshot_clear,
};
use crate::subscriptions::sync_engine::SubscriptionSyncEngine;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

pub struct SubscriptionRunOrchestrator;

impl SubscriptionRunOrchestrator {
    pub async fn stop_subscription(
        db: &SqliteDatabase,
        running_subs: &tokio::sync::Mutex<std::collections::HashMap<String, CancellationToken>>,
        id: String,
    ) -> Result<(), String> {
        let resolved_name = if let Ok(sub_id) = id.parse::<i64>() {
            db.with_read_conn(move |conn| get_subscription(conn, sub_id))
                .await
                .ok()
                .flatten()
                .map(|sub| sub.name)
                .unwrap_or_else(|| format!("Subscription {id}"))
        } else {
            format!("Subscription {id}")
        };
        let map = running_subs.lock().await;
        match map.get(&id) {
            Some(token) => {
                token.cancel();
                drop(map);
                publish_cancelling(&id, &resolved_name);
                Ok(())
            }
            None => Err(format!("Subscription {} is not running", id)),
        }
    }

    pub async fn get_running_subscriptions(
        running_subs: &RunningSubscriptions,
    ) -> Result<Vec<String>, String> {
        let map = running_subs.lock().await;
        Ok(map.keys().cloned().collect())
    }

    pub fn get_running_subscription_progress() -> Vec<SubscriptionProgressEvent> {
        list_runtime_progress_from_tasks()
    }

    pub async fn run_subscription(
        db: &Arc<SqliteDatabase>,
        blob_store: &Arc<BlobStore>,
        _rate_limiter: &RateLimiter,
        running_subs: &RunningSubscriptions,
        id: String,
        sub_terminal_statuses: Option<SubTerminalStatuses>,
        settings: &SettingsStore,
    ) -> Result<(), String> {
        let sub_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", id))?;

        {
            let map = running_subs.lock().await;
            if map.contains_key(&id) {
                return Err(format!("Subscription {} is already running", id));
            }
        }

        let sub = db
            .with_read_conn(move |conn| get_subscription(conn, sub_id))
            .await?
            .ok_or_else(|| format!("Subscription {} not found", id))?;

        if sub.paused {
            return Err(format!("Subscription {} is paused", id));
        }

        let queries = db.get_subscription_queries(sub_id).await?;
        if queries.is_empty() {
            return Err("Subscription has no queries".to_string());
        }

        if sub.site_id.is_empty() {
            return Err("Subscription has no site configured".to_string());
        }
        if crate::subscriptions::gallery_dl_runner::site_by_id(&sub.site_id).is_none() {
            return Err(format!("Unknown site: {}", sub.site_id));
        }

        let cancel = CancellationToken::new();
        {
            let mut map = running_subs.lock().await;
            map.insert(id.clone(), cancel.clone());
        }

        publish_start(&id, &sub.name, "subscription", None, None);
        tracing::info!(subscription_id = %id, name = %sub.name, queries = queries.len(), "subscription run starting");

        let db = db.clone();
        let blob_store = blob_store.clone();
        let running_subs = running_subs.clone();
        let sub_name = sub.name.clone();
        let sub_id_str = id.clone();
        let site_id =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(&sub.site_id).to_string();
        let terminal_statuses = sub_terminal_statuses;

        let app_settings = settings.get();
        let auto_merge_enabled = app_settings.duplicate_auto_merge_enabled;
        let auto_merge_distance = if auto_merge_enabled {
            crate::settings::store::similarity_pct_to_distance(
                app_settings.duplicate_auto_merge_similarity_pct,
            )
        } else {
            0
        };
        let auto_merge_require_matching_dimensions =
            app_settings.duplicate_auto_merge_require_matching_dimensions;

        let running_subs_guard = running_subs.clone();
        let sub_id_guard = sub_id_str.clone();
        let sub_id_for_inner_clear = sub_id_guard.clone();
        let sub_name_guard = sub_name.clone();

        tokio::spawn(async move {
            let inner = tokio::spawn(async move {
                let mut total_errors = 0usize;
                let mut last_error: Option<String> = None;
                let mut last_failure_kind: Option<String> = None;
                let mut was_cancelled = false;
                let mut total_downloaded = 0usize;
                let mut total_skipped = 0usize;
                let mut total_metadata_validated = 0usize;
                let mut total_metadata_invalid = 0usize;
                let mut last_metadata_error: Option<String> = None;

                let engine_result = SubscriptionSyncEngine::new(&db, &blob_store, &app_settings);
                match engine_result {
                    Ok(engine) => {
                        let mut engine = engine
                            .with_name(sub_name.clone())
                            .with_auto_merge(
                                auto_merge_enabled,
                                auto_merge_distance,
                                auto_merge_require_matching_dimensions,
                            );
                        for query in &queries {
                            if cancel.is_cancelled() {
                                was_cancelled = true;
                                break;
                            }
                            if query.paused {
                                continue;
                            }
                            let subscription_limit = if query.completed_initial_run {
                                sub.periodic_file_limit as u32
                            } else {
                                sub.initial_file_limit as u32
                            };
                            let file_limit = effective_query_file_limit(
                                app_settings.sub_batch_size,
                                subscription_limit,
                            );
                            let result = engine
                                .sync_query(
                                    sub_id,
                                    query.query_id,
                                    &query.query_text,
                                    query.display_name.as_deref(),
                                    &site_id,
                                    file_limit,
                                    query.completed_initial_run,
                                    query.resume_cursor.as_deref(),
                                    query.resume_strategy.as_deref(),
                                    cancel.clone(),
                                )
                                .await;
                            total_downloaded += result.files_downloaded;
                            total_skipped += result.files_skipped;
                            total_metadata_validated += result.metadata_validated;
                            total_metadata_invalid += result.metadata_invalid;
                            total_errors += result.errors.len();
                            if let Some(e) = result.errors.last() {
                                last_error = Some(e.clone());
                            }
                            if let Some(e) = result.last_metadata_error {
                                last_metadata_error = Some(e);
                            }
                            if let Some(kind) = result.failure_kind {
                                last_failure_kind = Some(kind);
                            }
                            if result.cancelled {
                                was_cancelled = true;
                            }
                        }
                    }
                    Err(e) => {
                        last_error = Some(e);
                        total_errors = 1;
                        last_failure_kind = Some("unknown".to_string());
                    }
                }

                {
                    let mut map = running_subs.lock().await;
                    map.remove(&sub_id_str);
                }

                let status = if was_cancelled {
                    "cancelled"
                } else if total_errors > 0 {
                    "failed"
                } else {
                    "succeeded"
                };

                tracing::info!(
                    subscription_id = %sub_id_str,
                    status,
                    downloaded = total_downloaded,
                    skipped = total_skipped,
                    errors = total_errors,
                    "subscription run finished"
                );

                if let Some(ref statuses) = terminal_statuses {
                    statuses
                        .lock()
                        .await
                        .insert(sub_id_str.clone(), status.to_string());
                }
                let final_status_text =
                    resolve_finished_status_text(status, last_failure_kind.as_deref());

                publish_finished(
                    &sub_id_for_inner_clear,
                    &sub_name,
                    "subscription",
                    None,
                    None,
                    total_downloaded,
                    total_skipped,
                    total_metadata_validated,
                    total_metadata_invalid,
                    last_metadata_error.clone(),
                    status,
                    final_status_text,
                    last_failure_kind.clone(),
                    last_error.clone(),
                );
                schedule_progress_snapshot_clear(
                    running_subs.clone(),
                    sub_id_for_inner_clear.clone(),
                );
            });

            if let Err(e) = inner.await {
                tracing::error!(
                    subscription_id = %sub_id_guard,
                    "Subscription task panicked — cleaning up running key: {e}"
                );
                let mut map = running_subs_guard.lock().await;
                map.remove(&sub_id_guard);
                publish_panic(
                    &sub_id_guard,
                    &sub_name_guard,
                    "subscription",
                    None,
                    None,
                    format!("{e}"),
                );
                schedule_progress_snapshot_clear(running_subs_guard.clone(), sub_id_guard.clone());
            }
        });

        Ok(())
    }

    pub async fn run_subscription_query(
        db: &Arc<SqliteDatabase>,
        blob_store: &Arc<BlobStore>,
        _rate_limiter: &RateLimiter,
        running_subs: &RunningSubscriptions,
        subscription_id: String,
        query_id: String,
        settings: &SettingsStore,
    ) -> Result<(), String> {
        let sub_id: i64 = subscription_id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", subscription_id))?;
        let qid: i64 = query_id
            .parse()
            .map_err(|_| format!("Invalid query id: {}", query_id))?;

        {
            let map = running_subs.lock().await;
            if map.contains_key(&subscription_id) {
                return Err(format!(
                    "Subscription {} is already running",
                    subscription_id
                ));
            }
        }

        let sub = db
            .with_read_conn(move |conn| get_subscription(conn, sub_id))
            .await?
            .ok_or_else(|| format!("Subscription {} not found", subscription_id))?;

        let query = db
            .with_read_conn(move |conn| get_subscription_query(conn, qid))
            .await?
            .ok_or_else(|| format!("Query {} not found", query_id))?;
        let query_name = resolve_query_name(
            query.query_id,
            &query.query_text,
            query.display_name.as_deref(),
        );

        if query.paused {
            return Err(format!("Query {} is paused", query_id));
        }

        if sub.site_id.is_empty() {
            return Err("Subscription has no site configured".to_string());
        }
        if crate::subscriptions::gallery_dl_runner::site_by_id(&sub.site_id).is_none() {
            return Err(format!("Unknown site: {}", sub.site_id));
        }

        let cancel = CancellationToken::new();
        {
            let mut map = running_subs.lock().await;
            map.insert(subscription_id.clone(), cancel.clone());
        }

        publish_start(
            &subscription_id,
            &sub.name,
            "query",
            Some(query_id.clone()),
            Some(query_name.clone()),
        );

        let db = db.clone();
        let blob_store = blob_store.clone();
        let running_subs = running_subs.clone();
        let sub_name = sub.name.clone();
        let sub_id_str = subscription_id.clone();
        let query_id_str = query_id.clone();
        let query_name_str = query_name.clone();
        let site_id =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(&sub.site_id).to_string();
        let query_text = query.query_text.clone();
        let query_display_name = query.display_name.clone();
        let completed_initial_run = query.completed_initial_run;
        let resume_cursor = query.resume_cursor.clone();
        let resume_strategy = query.resume_strategy.clone();

        let app_settings = settings.get();
        let auto_merge_enabled = app_settings.duplicate_auto_merge_enabled;
        let auto_merge_distance = if auto_merge_enabled {
            crate::settings::store::similarity_pct_to_distance(
                app_settings.duplicate_auto_merge_similarity_pct,
            )
        } else {
            0
        };
        let auto_merge_require_matching_dimensions =
            app_settings.duplicate_auto_merge_require_matching_dimensions;
        let subscription_limit = if completed_initial_run {
            sub.periodic_file_limit as u32
        } else {
            sub.initial_file_limit as u32
        };
        let file_limit =
            effective_query_file_limit(app_settings.sub_batch_size, subscription_limit);

        let running_subs_guard = running_subs.clone();
        let sub_id_guard = sub_id_str.clone();
        let sub_id_for_inner_clear = sub_id_guard.clone();
        let sub_name_guard = sub_name.clone();
        let query_id_guard = query_id_str.clone();
        let query_name_guard = query_name_str.clone();

        tokio::spawn(async move {
            let inner = tokio::spawn(async move {
                let (
                    total_downloaded,
                    total_skipped,
                    total_errors,
                    last_error,
                    was_cancelled,
                    failure_kind,
                    metadata_validated,
                    metadata_invalid,
                    last_metadata_error,
                ) = {
                    let engine_result =
                        SubscriptionSyncEngine::new(&db, &blob_store, &app_settings);
                    match engine_result {
                        Ok(engine) => {
                            let mut engine = engine
                                .with_name(sub_name.clone())
                                .with_auto_merge(
                                    auto_merge_enabled,
                                    auto_merge_distance,
                                    auto_merge_require_matching_dimensions,
                                );
                            let result = engine
                                .sync_query(
                                    sub_id,
                                    qid,
                                    &query_text,
                                    query_display_name.as_deref(),
                                    &site_id,
                                    file_limit,
                                    completed_initial_run,
                                    resume_cursor.as_deref(),
                                    resume_strategy.as_deref(),
                                    cancel,
                                )
                                .await;
                            let err = result.errors.last().cloned();
                            (
                                result.files_downloaded,
                                result.files_skipped,
                                result.errors.len(),
                                err,
                                result.cancelled,
                                result.failure_kind,
                                result.metadata_validated,
                                result.metadata_invalid,
                                result.last_metadata_error,
                            )
                        }
                        Err(e) => (
                            0,
                            0,
                            1,
                            Some(e),
                            false,
                            Some("unknown".to_string()),
                            0,
                            0,
                            None,
                        ),
                    }
                };

                {
                    let mut map = running_subs.lock().await;
                    map.remove(&sub_id_str);
                }

                let status = if was_cancelled {
                    "cancelled"
                } else if total_errors > 0 {
                    "failed"
                } else {
                    "succeeded"
                };
                let final_status_text =
                    resolve_finished_status_text(status, failure_kind.as_deref());
                publish_finished(
                    &sub_id_for_inner_clear,
                    &sub_name,
                    "query",
                    Some(query_id_str.clone()),
                    Some(query_name_str.clone()),
                    total_downloaded,
                    total_skipped,
                    metadata_validated,
                    metadata_invalid,
                    last_metadata_error.clone(),
                    status,
                    final_status_text,
                    failure_kind.clone(),
                    last_error.clone(),
                );
                schedule_progress_snapshot_clear(
                    running_subs.clone(),
                    sub_id_for_inner_clear.clone(),
                );
            });

            if let Err(e) = inner.await {
                tracing::error!(
                    subscription_id = %sub_id_guard,
                    "Subscription query task panicked — cleaning up running key: {e}"
                );
                let mut map = running_subs_guard.lock().await;
                map.remove(&sub_id_guard);
                publish_panic(
                    &sub_id_guard,
                    &sub_name_guard,
                    "query",
                    Some(query_id_guard.clone()),
                    Some(query_name_guard.clone()),
                    format!("Task panicked: {e}"),
                );
                schedule_progress_snapshot_clear(running_subs_guard.clone(), sub_id_guard.clone());
            }
        });

        Ok(())
    }
}
