use std::sync::Arc;

use rusqlite::params;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;

use crate::blob_store::BlobStore;
use crate::events;
use crate::rate_limiter::RateLimiter;
use crate::settings::SettingsStore;
use crate::sqlite::subscriptions::{get_subscription, get_subscription_query};
use crate::sqlite::SqliteDatabase;
use crate::subscription_sync::SubscriptionSyncEngine;
use crate::types::{
    RunningSubscriptions, SubTerminalStatuses, SubscriptionInfo, SubscriptionQueryInfo,
};

fn schedule_progress_snapshot_clear(running_subs: RunningSubscriptions, subscription_id: String) {
    tokio::spawn(async move {
        sleep(Duration::from_millis(3000)).await;
        let still_running = {
            let map = running_subs.lock().await;
            map.contains_key(&subscription_id)
        };
        if !still_running {
            crate::subscription_sync::clear_runtime_progress_snapshot(&subscription_id);
        }
    });
}

fn resolve_query_name(query_id: i64, query_text: &str, display_name: Option<&str>) -> String {
    if let Some(name) = display_name.map(str::trim).filter(|name| !name.is_empty()) {
        // Legacy migration guard: numeric-only display names ("1", "2", ...)
        // are treated as placeholders when a real query text exists.
        let is_numeric_placeholder = name.chars().all(|c| c.is_ascii_digit());
        if !is_numeric_placeholder || query_text.trim().is_empty() {
            return name.to_string();
        }
    }
    let trimmed = query_text.trim();
    if !trimmed.is_empty() {
        return trimmed.to_string();
    }
    format!("Query {query_id}")
}

fn effective_query_file_limit(global_batch_size: u32, subscription_limit: u32) -> Option<u32> {
    if global_batch_size == 0 {
        // "Unlimited" global cap means "do not clamp globally", not
        // "force every query to run unbounded in one subprocess".
        if subscription_limit == 0 {
            return None;
        }
        return Some(subscription_limit.max(1));
    }
    let local = if subscription_limit == 0 {
        global_batch_size
    } else {
        subscription_limit
    };
    Some(local.min(global_batch_size).max(1))
}

async fn clear_subscription_archive_entries(
    db: &SqliteDatabase,
    archive_prefixes: &[String],
) -> Result<(), String> {
    if archive_prefixes.is_empty() {
        return Ok(());
    }

    let archive_path = db
        .db_dir()
        .parent()
        .map(|r| r.join("gdl-archive.sqlite3"))
        .unwrap_or_else(|| std::path::PathBuf::from("gdl-archive.sqlite3"));
    if !archive_path.exists() {
        return Ok(());
    }

    let prefixes = archive_prefixes.to_vec();
    let (deleted_rows, remaining_rows) =
        tokio::task::spawn_blocking(move || -> Result<(usize, usize), String> {
            let conn = rusqlite::Connection::open(&archive_path)
                .map_err(|e| format!("Failed to open gallery-dl archive: {e}"))?;
            conn.execute_batch("PRAGMA busy_timeout = 5000;")
                .map_err(|e| format!("Failed to configure gallery-dl archive connection: {e}"))?;
            let mut deleted = 0usize;
            let mut stmt = match conn.prepare("DELETE FROM archive WHERE entry LIKE ?1 ESCAPE '\\'")
            {
                Ok(stmt) => stmt,
                Err(e) => {
                    // Legacy/empty archive edge: table may not exist yet.
                    if e.to_string().contains("no such table: archive") {
                        return Ok((0, 0));
                    }
                    return Err(format!("Failed to prepare gallery-dl archive delete: {e}"));
                }
            };
            let mut count_stmt = match conn
                .prepare("SELECT COUNT(*) FROM archive WHERE entry LIKE ?1 ESCAPE '\\'")
            {
                Ok(stmt) => stmt,
                Err(e) => {
                    if e.to_string().contains("no such table: archive") {
                        return Ok((0, 0));
                    }
                    return Err(format!("Failed to prepare gallery-dl archive count: {e}"));
                }
            };

            let mut remaining = 0usize;
            for prefix in prefixes {
                let escaped_prefix = prefix
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_");
                let pattern = format!("{escaped_prefix}%");
                let removed = stmt
                    .execute([pattern])
                    .map_err(|e| format!("Failed to clear gallery-dl archive entries: {e}"))?;
                deleted += removed;

                let still: i64 = count_stmt
                    .query_row([format!("{escaped_prefix}%")], |row| row.get(0))
                    .map_err(|e| {
                        format!("Failed to count remaining gallery-dl archive entries: {e}")
                    })?;
                if still > 0 {
                    remaining += still as usize;
                }
            }
            Ok((deleted, remaining))
        })
        .await
        .map_err(|e| format!("Gallery-dl archive reset task failed: {e}"))??;

    tracing::info!(
        deleted_rows,
        remaining_rows,
        prefixes = archive_prefixes.len(),
        "Subscription reset: cleared gallery-dl archive rows"
    );

    if remaining_rows > 0 {
        tracing::warn!(
            remaining_rows,
            "Subscription reset: some archive rows still match reset prefixes"
        );
    }

    Ok(())
}

pub struct SubscriptionController;

impl SubscriptionController {
    // PBI-040: Bulk read — 2 queries instead of O(N).
    pub async fn get_subscriptions(db: &SqliteDatabase) -> Result<Vec<SubscriptionInfo>, String> {
        let start = std::time::Instant::now();

        let subs_with_counts = db.list_subscriptions_with_file_counts().await?;
        let all_queries = db.list_all_subscription_queries().await?;

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

        let result: Vec<SubscriptionInfo> = subs_with_counts
            .into_iter()
            .map(|(sub, total_files)| {
                let sub_id = sub.subscription_id;
                let canonical_site_id = crate::gallery_dl_runner::canonical_site_id(&sub.site_id);
                SubscriptionInfo {
                    id: sub_id.to_string(),
                    name: sub.name,
                    site_id: canonical_site_id.to_string(),
                    paused: sub.paused,
                    flow_id: sub.flow_id.map(|id| id.to_string()),
                    initial_file_limit: sub.initial_file_limit as u32,
                    periodic_file_limit: sub.periodic_file_limit as u32,
                    created_at: sub.created_at,
                    total_files: total_files as u64,
                    queries: queries_map.remove(&sub_id).unwrap_or_default(),
                }
            })
            .collect();

        tracing::debug!(
            elapsed_ms = start.elapsed().as_millis() as u64,
            count = result.len(),
            "get_subscriptions bulk read"
        );

        Ok(result)
    }

    pub async fn create_subscription(
        db: &SqliteDatabase,
        name: String,
        site_id: String,
        queries: Vec<String>,
        flow_id: Option<i64>,
        initial_file_limit: Option<u32>,
        periodic_file_limit: Option<u32>,
    ) -> Result<SubscriptionInfo, String> {
        if crate::gallery_dl_runner::site_by_id(&site_id).is_none() {
            return Err(format!("Unknown site: {site_id}"));
        }
        let canonical_site_id = crate::gallery_dl_runner::canonical_site_id(&site_id).to_string();
        let sub = db
            .create_subscription(&name, &canonical_site_id, flow_id)
            .await?;
        let sub_id = sub.subscription_id;

        if initial_file_limit.is_some() || periodic_file_limit.is_some() {
            let il = initial_file_limit.unwrap_or(100) as i64;
            let pl = periodic_file_limit.unwrap_or(50) as i64;
            db.with_conn(move |conn| {
                conn.execute(
                    "UPDATE subscription SET initial_file_limit = ?1, periodic_file_limit = ?2
                     WHERE subscription_id = ?3",
                    params![il, pl, sub_id],
                )?;
                Ok(())
            })
            .await?;
        }

        let mut query_infos = Vec::new();
        for query_text in queries {
            let trimmed = query_text.trim().to_string();
            if trimmed.is_empty() {
                continue;
            }
            let q = db
                .add_subscription_query(sub_id, &trimmed, Some(trimmed.as_str()))
                .await?;
            query_infos.push(SubscriptionQueryInfo {
                id: q.query_id.to_string(),
                query_text: q.query_text,
                display_name: q.display_name,
                paused: q.paused,
                last_check_time: q.last_check_time,
                files_found: q.files_found as u64,
                completed_initial_run: q.completed_initial_run,
                resume_cursor: q.resume_cursor,
                resume_strategy: q.resume_strategy,
            });
        }

        Ok(SubscriptionInfo {
            id: sub_id.to_string(),
            name,
            site_id: canonical_site_id,
            paused: false,
            flow_id: flow_id.map(|id| id.to_string()),
            initial_file_limit: initial_file_limit.unwrap_or(100),
            periodic_file_limit: periodic_file_limit.unwrap_or(50),
            created_at: sub.created_at,
            total_files: 0,
            queries: query_infos,
        })
    }

    pub async fn delete_subscription(
        db: &SqliteDatabase,
        blob_store: &BlobStore,
        id: String,
        delete_files: Option<bool>,
    ) -> Result<usize, String> {
        let sub_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", id))?;

        if delete_files.unwrap_or(false) {
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

        db.delete_subscription(sub_id).await?;
        Ok(1)
    }

    pub async fn pause_subscription(
        db: &SqliteDatabase,
        id: String,
        paused: bool,
    ) -> Result<(), String> {
        let sub_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", id))?;
        db.set_subscription_paused(sub_id, paused).await
    }

    pub async fn add_subscription_query(
        db: &SqliteDatabase,
        subscription_id: String,
        query_text: String,
    ) -> Result<SubscriptionQueryInfo, String> {
        let sub_id: i64 = subscription_id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", subscription_id))?;
        let q = db
            .add_subscription_query(sub_id, query_text.trim(), Some(query_text.trim()))
            .await?;
        Ok(SubscriptionQueryInfo {
            id: q.query_id.to_string(),
            query_text: q.query_text,
            display_name: q.display_name,
            paused: q.paused,
            last_check_time: q.last_check_time,
            files_found: q.files_found as u64,
            completed_initial_run: q.completed_initial_run,
            resume_cursor: q.resume_cursor,
            resume_strategy: q.resume_strategy,
        })
    }

    pub async fn delete_subscription_query(db: &SqliteDatabase, id: String) -> Result<(), String> {
        let query_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid query id: {}", id))?;
        db.delete_subscription_query(query_id).await
    }

    pub async fn pause_subscription_query(
        db: &SqliteDatabase,
        id: String,
        paused: bool,
    ) -> Result<(), String> {
        let query_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid query id: {}", id))?;
        db.set_query_paused(query_id, paused).await
    }

    pub async fn rename_subscription(
        db: &SqliteDatabase,
        id: String,
        name: String,
    ) -> Result<(), String> {
        let sub_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", id))?;
        let trimmed = name.trim().to_string();
        if trimmed.is_empty() {
            return Err("Name cannot be empty".to_string());
        }
        db.rename_subscription(sub_id, &trimmed).await
    }

    pub async fn reset_subscription(db: &SqliteDatabase, id: String) -> Result<(), String> {
        let sub_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", id))?;
        let queries = db.get_subscription_queries(sub_id).await?;
        let mut archive_prefixes: Vec<String> = queries
            .iter()
            .map(|q| {
                crate::subscription_sync::subscription_query_archive_prefix(sub_id, q.query_id)
            })
            .collect();
        archive_prefixes.push(format!("picto_s{sub_id}_q"));

        let (queries_reset, entities_deleted, post_maps_deleted) =
            db.reset_subscription_state(sub_id).await?;
        clear_subscription_archive_entries(db, &archive_prefixes).await?;

        tracing::info!(
            subscription_id = sub_id,
            queries_reset,
            entities_deleted,
            post_maps_deleted,
            "Subscription reset: state cleared"
        );
        Ok(())
    }

    pub async fn reset_subscription_checked(
        db: &SqliteDatabase,
        running_subs: &RunningSubscriptions,
        id: String,
    ) -> Result<(), String> {
        {
            let map = running_subs.lock().await;
            if map.contains_key(&id) {
                return Err(format!(
                    "Subscription {} is running; stop it before reset",
                    id
                ));
            }
        }
        Self::reset_subscription(db, id).await
    }

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
                let progress = crate::subscription_sync::SubscriptionProgressEvent {
                    subscription_id: id.clone(),
                    subscription_name: resolved_name,
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
                events::emit(
                    events::event_names::SUBSCRIPTION_PROGRESS,
                    &progress,
                );
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

    pub fn get_running_subscription_progress() -> Vec<crate::subscription_sync::SubscriptionProgressEvent>
    {
        crate::subscription_sync::list_runtime_progress_snapshots()
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
        if crate::gallery_dl_runner::site_by_id(&sub.site_id).is_none() {
            return Err(format!("Unknown site: {}", sub.site_id));
        }

        let cancel = CancellationToken::new();
        {
            let mut map = running_subs.lock().await;
            map.insert(id.clone(), cancel.clone());
        }

        events::emit(
            events::event_names::SUBSCRIPTION_STARTED,
            &events::SubscriptionStartedEvent {
                subscription_id: id.clone(),
                subscription_name: sub.name.clone(),
                mode: "subscription".to_string(),
                query_id: None,
                query_name: None,
            },
        );
        crate::subscription_sync::update_runtime_progress_snapshot(
            crate::subscription_sync::SubscriptionProgressEvent {
                subscription_id: id.clone(),
                subscription_name: sub.name.clone(),
                mode: "subscription".to_string(),
                query_id: None,
                query_name: None,
                files_downloaded: 0,
                files_skipped: 0,
                pages_fetched: 0,
                metadata_validated: 0,
                metadata_invalid: 0,
                last_metadata_error: None,
                status_text: "Starting...".to_string(),
            },
        );

        let db = db.clone();
        let blob_store = blob_store.clone();
        let running_subs = running_subs.clone();
        let sub_name = sub.name.clone();
        let sub_id_str = id.clone();
        let site_id = crate::gallery_dl_runner::canonical_site_id(&sub.site_id).to_string();
        let terminal_statuses = sub_terminal_statuses;

        let app_settings = settings.get();
        let auto_merge_enabled = app_settings.duplicate_auto_merge_enabled;
        let auto_merge_distance = if auto_merge_enabled {
            crate::settings::similarity_pct_to_distance(
                app_settings.duplicate_auto_merge_similarity_pct,
            )
        } else {
            0
        };

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
                            .with_auto_merge(auto_merge_enabled, auto_merge_distance);
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

                if let Some(ref statuses) = terminal_statuses {
                    statuses
                        .lock()
                        .await
                        .insert(sub_id_str.clone(), status.to_string());
                }

                events::emit(
                    events::event_names::SUBSCRIPTION_FINISHED,
                    &events::SubscriptionFinishedEvent {
                        subscription_id: sub_id_str.clone(),
                        subscription_name: sub_name.clone(),
                        mode: "subscription".to_string(),
                        query_id: None,
                        query_name: None,
                        status: status.to_string(),
                        files_downloaded: total_downloaded,
                        files_skipped: total_skipped,
                        errors_count: total_errors,
                        error: last_error,
                        failure_kind: last_failure_kind,
                        metadata_validated: total_metadata_validated,
                        metadata_invalid: total_metadata_invalid,
                        last_metadata_error: last_metadata_error.clone(),
                    },
                );
                let final_status_text = match status {
                    "succeeded" => "Completed",
                    "cancelled" => "Cancelled",
                    _ => "Failed",
                };
                crate::subscription_sync::update_runtime_progress_snapshot(
                    crate::subscription_sync::SubscriptionProgressEvent {
                        subscription_id: sub_id_for_inner_clear.clone(),
                        subscription_name: sub_name.clone(),
                        mode: "subscription".to_string(),
                        query_id: None,
                        query_name: None,
                        files_downloaded: total_downloaded,
                        files_skipped: total_skipped,
                        pages_fetched: 0,
                        metadata_validated: total_metadata_validated,
                        metadata_invalid: total_metadata_invalid,
                        last_metadata_error: last_metadata_error.clone(),
                        status_text: final_status_text.to_string(),
                    },
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
                crate::subscription_sync::update_runtime_progress_snapshot(
                    crate::subscription_sync::SubscriptionProgressEvent {
                        subscription_id: sub_id_guard.clone(),
                        subscription_name: sub_name_guard.clone(),
                        mode: "subscription".to_string(),
                        query_id: None,
                        query_name: None,
                        files_downloaded: 0,
                        files_skipped: 0,
                        pages_fetched: 0,
                        metadata_validated: 0,
                        metadata_invalid: 0,
                        last_metadata_error: None,
                        status_text: "Failed".to_string(),
                    },
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
        if crate::gallery_dl_runner::site_by_id(&sub.site_id).is_none() {
            return Err(format!("Unknown site: {}", sub.site_id));
        }

        let cancel = CancellationToken::new();
        {
            let mut map = running_subs.lock().await;
            map.insert(subscription_id.clone(), cancel.clone());
        }

        events::emit(
            events::event_names::SUBSCRIPTION_STARTED,
            &events::SubscriptionStartedEvent {
                subscription_id: subscription_id.clone(),
                subscription_name: sub.name.clone(),
                mode: "query".to_string(),
                query_id: Some(query_id.clone()),
                query_name: Some(query_name.clone()),
            },
        );
        crate::subscription_sync::update_runtime_progress_snapshot(
            crate::subscription_sync::SubscriptionProgressEvent {
                subscription_id: subscription_id.clone(),
                subscription_name: sub.name.clone(),
                mode: "query".to_string(),
                query_id: Some(query_id.clone()),
                query_name: Some(query_name.clone()),
                files_downloaded: 0,
                files_skipped: 0,
                pages_fetched: 0,
                metadata_validated: 0,
                metadata_invalid: 0,
                last_metadata_error: None,
                status_text: "Starting...".to_string(),
            },
        );

        let db = db.clone();
        let blob_store = blob_store.clone();
        let running_subs = running_subs.clone();
        let sub_name = sub.name.clone();
        let sub_id_str = subscription_id.clone();
        let query_id_str = query_id.clone();
        let query_name_str = query_name.clone();
        let site_id = crate::gallery_dl_runner::canonical_site_id(&sub.site_id).to_string();
        let query_text = query.query_text.clone();
        let query_display_name = query.display_name.clone();
        let completed_initial_run = query.completed_initial_run;
        let resume_cursor = query.resume_cursor.clone();
        let resume_strategy = query.resume_strategy.clone();

        let app_settings = settings.get();
        let auto_merge_enabled = app_settings.duplicate_auto_merge_enabled;
        let auto_merge_distance = if auto_merge_enabled {
            crate::settings::similarity_pct_to_distance(
                app_settings.duplicate_auto_merge_similarity_pct,
            )
        } else {
            0
        };
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
                                .with_auto_merge(auto_merge_enabled, auto_merge_distance);
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
                events::emit(
                    events::event_names::SUBSCRIPTION_FINISHED,
                    &events::SubscriptionFinishedEvent {
                        subscription_id: sub_id_str.clone(),
                        subscription_name: sub_name.clone(),
                        mode: "query".to_string(),
                        query_id: Some(query_id_str.clone()),
                        query_name: Some(query_name_str.clone()),
                        status: status.to_string(),
                        files_downloaded: total_downloaded,
                        files_skipped: total_skipped,
                        errors_count: total_errors,
                        error: last_error,
                        failure_kind: failure_kind.clone(),
                        metadata_validated,
                        metadata_invalid,
                        last_metadata_error: last_metadata_error.clone(),
                    },
                );
                let final_status_text = match status {
                    "succeeded" => "Completed",
                    "cancelled" => "Cancelled",
                    _ => "Failed",
                };
                crate::subscription_sync::update_runtime_progress_snapshot(
                    crate::subscription_sync::SubscriptionProgressEvent {
                        subscription_id: sub_id_for_inner_clear.clone(),
                        subscription_name: sub_name.clone(),
                        mode: "query".to_string(),
                        query_id: Some(query_id_str.clone()),
                        query_name: Some(query_name_str.clone()),
                        files_downloaded: total_downloaded,
                        files_skipped: total_skipped,
                        pages_fetched: 0,
                        metadata_validated,
                        metadata_invalid,
                        last_metadata_error: last_metadata_error.clone(),
                        status_text: final_status_text.to_string(),
                    },
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
                events::emit(
                    events::event_names::SUBSCRIPTION_FINISHED,
                    &events::SubscriptionFinishedEvent {
                        subscription_id: sub_id_guard.clone(),
                        subscription_name: sub_name_guard.clone(),
                        mode: "query".to_string(),
                        query_id: Some(query_id_guard.clone()),
                        query_name: Some(query_name_guard.clone()),
                        status: "failed".to_string(),
                        files_downloaded: 0,
                        files_skipped: 0,
                        errors_count: 1,
                        error: Some(format!("Task panicked: {e}")),
                        failure_kind: Some("unknown".to_string()),
                        metadata_validated: 0,
                        metadata_invalid: 0,
                        last_metadata_error: None,
                    },
                );
                crate::subscription_sync::update_runtime_progress_snapshot(
                    crate::subscription_sync::SubscriptionProgressEvent {
                        subscription_id: sub_id_guard.clone(),
                        subscription_name: sub_name_guard.clone(),
                        mode: "query".to_string(),
                        query_id: Some(query_id_guard.clone()),
                        query_name: Some(query_name_guard.clone()),
                        files_downloaded: 0,
                        files_skipped: 0,
                        pages_fetched: 0,
                        metadata_validated: 0,
                        metadata_invalid: 0,
                        last_metadata_error: None,
                        status_text: "Failed".to_string(),
                    },
                );
                schedule_progress_snapshot_clear(running_subs_guard.clone(), sub_id_guard.clone());
            }
        });

        Ok(())
    }
}
