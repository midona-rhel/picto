//! Subscription run orchestration.
//!
//! This isolates execution, cancellation, and runtime publication from the
//! CRUD controller so subscription behavior can evolve behind one service.

use std::sync::Arc;

use crate::blob_store::BlobStore;
use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::SettingsStore;
use crate::subscriptions::job_queue::{
    activate_subscription_guard, enqueue_retry_job, enqueue_single_query,
    enqueue_subscription_bundle,
};
use crate::subscriptions::policy::resolve_query_name;
use crate::subscriptions::progress::{list_runtime_progress_from_tasks, SubscriptionProgressEvent};
use crate::subscriptions::runtime_tasks::{publish_cancelling, publish_start};
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

pub struct SubscriptionRunOrchestrator;

/// Undo `activate_subscription_guard` + `publish_start` after a failed
/// enqueue — otherwise the token and the Running task leak and the
/// subscription reads as permanently running.
async fn release_guard_and_task(running_subs: &RunningSubscriptions, id: &str) {
    let mut map = running_subs.lock().await;
    map.remove(id);
    drop(map);
    crate::runtime_state::remove_task(&format!("sub:{id}"));
}

/// When no executor is alive for this subscription (nothing was leased, or the
/// process that leased it is gone), stop must do the executor's cleanup itself.
async fn finalize_stopped_subscription(
    runtime: &crate::subscriptions::runtime_service::SubscriptionRuntimeService<'_>,
    running_subs: &RunningSubscriptions,
    id: &str,
    name: &str,
) {
    let sub_id = id.parse::<i64>().unwrap_or_default();
    let _ = runtime
        .finalize_open_runs_for_subscription(sub_id, "cancelled", Some("cancelled"), None)
        .await;
    crate::subscriptions::runtime_tasks::publish_finished(
        id,
        name,
        "subscription",
        None,
        None,
        0,
        0,
        0,
        0,
        None,
        "cancelled",
        "Cancelled",
        None,
        None,
    );
    crate::subscriptions::runtime_tasks::schedule_progress_snapshot_clear(
        running_subs.clone(),
        id.to_string(),
    );
}

impl SubscriptionRunOrchestrator {
    pub async fn stop_subscription(
        db: &LibraryDatabase,
        library_root: &std::path::Path,
        running_subs: &RunningSubscriptions,
        id: String,
    ) -> Result<(), String> {
        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db,
            library_root,
        );
        let sub_id = id.parse::<i64>().unwrap_or_default();
        let resolved_name = if let Ok(sub_id) = id.parse::<i64>() {
            runtime
                .get_subscription(sub_id)
                .await
                .ok()
                .flatten()
                .map(|sub| sub.name)
                .unwrap_or_else(|| format!("Subscription {id}"))
        } else {
            format!("Subscription {id}")
        };
        let mut map = running_subs.lock().await;
        match map.remove(&id) {
            Some(token) => {
                drop(map);
                token.cancel();
                let _ = runtime
                    .cancel_pending_subscription_jobs_for_subscription(sub_id)
                    .await;
                publish_cancelling(&id, &resolved_name);
                // If nothing is actually executing (jobs were still queued, or
                // the run row was an orphan), no executor will ever finalize —
                // do it here so the UI clears without a restart.
                let active = runtime
                    .count_active_subscription_query_jobs(sub_id)
                    .await
                    .unwrap_or(1);
                if active == 0 {
                    finalize_stopped_subscription(&runtime, running_subs, &id, &resolved_name)
                        .await;
                }
                Ok(())
            }
            None => {
                drop(map);
                // Not running in-memory, but the DB or task registry may hold
                // stuck state (orphaned rows, leaked Cancelling task). Make
                // Stop an idempotent reconciler instead of an error so a stuck
                // card can be cleared in-app.
                let _ = runtime
                    .cancel_pending_subscription_jobs_for_subscription(sub_id)
                    .await;
                let active = runtime
                    .count_active_subscription_query_jobs(sub_id)
                    .await
                    .unwrap_or(1);
                if active == 0 {
                    let _ = runtime
                        .finalize_open_runs_for_subscription(
                            sub_id,
                            "cancelled",
                            Some("cancelled"),
                            None,
                        )
                        .await;
                    crate::runtime_state::remove_task(&format!("sub:{id}"));
                }
                Ok(())
            }
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

    pub async fn stop_subscription_query(
        db: &LibraryDatabase,
        library_root: &std::path::Path,
        running_subs: &RunningSubscriptions,
        subscription_id: String,
        query_id: String,
    ) -> Result<(), String> {
        let progress = list_runtime_progress_from_tasks()
            .into_iter()
            .find(|event| event.subscription_id == subscription_id);
        let event =
            progress.ok_or_else(|| format!("Subscription {} is not running", subscription_id))?;
        if event.mode != "query" || event.query_id.as_deref() != Some(query_id.as_str()) {
            return Err(format!(
                "Query {} is not running independently and cannot be stopped separately",
                query_id
            ));
        }
        Self::stop_subscription(db, library_root, running_subs, subscription_id).await
    }

    pub async fn run_subscription(
        db: &Arc<LibraryDatabase>,
        library_root: &std::path::Path,
        blob_store: &Arc<BlobStore>,
        rate_limiter: &RateLimiter,
        running_subs: &RunningSubscriptions,
        id: String,
        sub_terminal_statuses: Option<SubTerminalStatuses>,
        settings: &SettingsStore,
    ) -> Result<(), String> {
        let sub_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", id))?;

        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            library_root,
        );
        let bundle = runtime
            .get_runnable_subscription(sub_id)
            .await?
            .ok_or_else(|| format!("Subscription {} not found", id))?;
        let sub = bundle.subscription.clone();

        if sub.paused {
            return Err(format!("Subscription {} is paused", id));
        }

        if bundle.queries.is_empty() {
            return Err("Subscription has no queries".to_string());
        }
        for query in &bundle.queries {
            if query.site_id.is_empty() {
                return Err(format!("Query {} has no site configured", query.query_id));
            }
            if crate::subscriptions::gallery_dl_runner::site_by_id(&query.site_id).is_none() {
                return Err(format!("Unknown site: {}", query.site_id));
            }
        }

        check_credential_preflight(db, sub_id, &bundle.queries).await?;

        let _cancel = activate_subscription_guard(running_subs, &id).await?;

        publish_start(&id, &sub.name, "subscription", None, None);
        if let Err(error) = enqueue_subscription_bundle(&runtime, &bundle, "subscription").await {
            release_guard_and_task(running_subs, &id).await;
            return Err(error);
        }
        let _ = sub_terminal_statuses;
        let _ = blob_store;
        let _ = rate_limiter;
        let _ = settings;
        Ok(())
    }

    pub async fn run_subscription_query(
        db: &Arc<LibraryDatabase>,
        library_root: &std::path::Path,
        blob_store: &Arc<BlobStore>,
        rate_limiter: &RateLimiter,
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

        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            library_root,
        );
        let bundle = runtime
            .get_runnable_query(sub_id, qid)
            .await?
            .ok_or_else(|| format!("Query {} not found", query_id))?;
        let sub = bundle.subscription.clone();
        let query = bundle.query.clone();
        let query_name = resolve_query_name(
            query.query_id,
            &query.query_text,
            query.display_name.as_deref(),
        );

        if query.paused {
            return Err(format!("Query {} is paused", query_id));
        }

        if query.site_id.is_empty() {
            return Err("Query has no site configured".to_string());
        }
        if crate::subscriptions::gallery_dl_runner::site_by_id(&query.site_id).is_none() {
            return Err(format!("Unknown site: {}", query.site_id));
        }

        check_credential_preflight(db, sub_id, std::slice::from_ref(&query)).await?;

        let _cancel = activate_subscription_guard(running_subs, &subscription_id).await?;

        publish_start(
            &subscription_id,
            &sub.name,
            "query",
            Some(query_id.clone()),
            Some(query_name.clone()),
        );

        if let Err(error) = enqueue_single_query(&runtime, &bundle, "query").await {
            release_guard_and_task(running_subs, &subscription_id).await;
            return Err(error);
        }
        let _ = blob_store;
        let _ = rate_limiter;
        let _ = settings;
        Ok(())
    }

    pub async fn retry_failed_post(
        db: &Arc<LibraryDatabase>,
        library_root: &std::path::Path,
        blob_store: &Arc<BlobStore>,
        rate_limiter: &RateLimiter,
        running_subs: &RunningSubscriptions,
        subscription_id: String,
        query_id: String,
        site_id: String,
        post_id: String,
        settings: &SettingsStore,
    ) -> Result<(), String> {
        let sub_id: i64 = subscription_id
            .parse()
            .map_err(|_| format!("Invalid subscription id: {}", subscription_id))?;
        let qid: i64 = query_id
            .parse()
            .map_err(|_| format!("Invalid query id: {}", query_id))?;

        let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
            db.as_ref(),
            library_root,
        );
        let sub = runtime
            .get_subscription(sub_id)
            .await?
            .ok_or_else(|| format!("Subscription {} not found", subscription_id))?;
        let query = runtime
            .get_subscription_query(qid)
            .await?
            .ok_or_else(|| format!("Query {} not found", query_id))?;

        let canonical_site_id =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(&site_id).to_string();
        if canonical_site_id
            != crate::subscriptions::gallery_dl_runner::canonical_site_id(&query.site_id)
        {
            return Err("retry site_id does not match the query site".to_string());
        }

        let matching = runtime
            .find_unresolved_subscription_download_attempts(
                sub_id,
                qid,
                &canonical_site_id,
                &post_id,
            )
            .await?;
        if matching.is_empty() {
            return Err(format!(
                "No failed download attempts found for post {} on query {}",
                post_id, query_id
            ));
        }
        let retry_url = matching
            .iter()
            .find_map(|attempt| attempt.retry_url.clone())
            .or_else(|| {
                matching
                    .iter()
                    .find_map(|attempt| attempt.canonical_post_url.clone())
            })
            .ok_or_else(|| format!("No retry URL recorded for post {post_id}"))?;
        let _cancel = activate_subscription_guard(running_subs, &subscription_id).await?;

        let query_name = format!("Retry post {post_id}");
        publish_start(
            &subscription_id,
            &sub.name,
            "query",
            Some(query_id.clone()),
            Some(query_name.clone()),
        );

        if let Err(error) =
            enqueue_retry_job(&runtime, sub_id, qid, &canonical_site_id, &post_id).await
        {
            release_guard_and_task(running_subs, &subscription_id).await;
            return Err(error);
        }
        let _ = retry_url;
        let _ = blob_store;
        let _ = rate_limiter;
        let _ = settings;
        let _ = query;
        Ok(())
    }
}

/// Block a run up front when a query's site needs credentials that are
/// missing or known-bad — an actionable error beats a mid-run failure.
async fn check_credential_preflight(
    db: &LibraryDatabase,
    subscription_id: i64,
    queries: &[crate::subscriptions::types::SubscriptionQuery],
) -> Result<(), String> {
    use crate::subscriptions::credential_service::{
        CredentialPreflight, SubscriptionCredentialService,
    };

    let service = SubscriptionCredentialService::new(db);
    for query in queries {
        let Some(site) = crate::subscriptions::gallery_dl_runner::site_by_id(&query.site_id) else {
            continue;
        };
        let url =
            crate::subscriptions::gallery_dl_runner::build_url(&query.site_id, &query.query_text)
                .unwrap_or_default();
        match service.preflight_for_run(&query.site_id, &url).await {
            CredentialPreflight::Ready => {}
            CredentialPreflight::MissingOptional => {
                // Run proceeds — sync engine already warns and records the
                // credential_missing issue for auth-recommended sites.
            }
            CredentialPreflight::MissingRequired => {
                let message = format!(
                    "{} requires a login — open Accounts, add one for {}, then run again",
                    site.name, site.domain
                );
                service
                    .note_preflight_block(
                        subscription_id,
                        Some(query.query_id),
                        &query.site_id,
                        crate::subscriptions::gallery_dl_runner::FailureKind::CredentialMissing,
                        &message,
                    )
                    .await;
                return Err(message);
            }
            CredentialPreflight::Blocked { status } => {
                let reason = match status.as_str() {
                    "expired" => "session has expired".to_string(),
                    "unauthorized" => "login was rejected by the site".to_string(),
                    other => format!("credential is {other}"),
                };
                let message = format!(
                    "Your {} {reason} — open Accounts and log in again, then run this subscription",
                    site.name
                );
                service
                    .note_preflight_block(
                        subscription_id,
                        Some(query.query_id),
                        &query.site_id,
                        crate::subscriptions::gallery_dl_runner::FailureKind::CredentialBlocked,
                        &message,
                    )
                    .await;
                return Err(message);
            }
        }
    }
    Ok(())
}
