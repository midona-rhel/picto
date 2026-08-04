use std::sync::Arc;

use crate::db::LibraryDatabase;
use crate::rate_limiter::RateLimiter;
use crate::settings::store::AppSettings;
use crate::subscriptions::gallery_dl_runner::FailureKind;
use crate::subscriptions::job_queue::clear_subscription_guard_if_idle;
use crate::subscriptions::policy::{
    effective_query_post_limit, resolve_finished_status_text, resolve_query_name,
};
use crate::subscriptions::runtime_tasks::{
    publish_finished, publish_panic, schedule_progress_snapshot_clear,
};
use crate::subscriptions::source_adapter::runner_key_for_site;
use crate::subscriptions::sync_engine::{SubscriptionSyncEngine, SyncProgress};
use crate::subscriptions::types::SubscriptionQueryJob;
use crate::types::{RunningSubscriptions, SubTerminalStatuses};

pub async fn execute_query_job(
    db: Arc<LibraryDatabase>,
    library_root: std::path::PathBuf,
    rate_limiter: RateLimiter,
    running_subs: RunningSubscriptions,
    sub_terminal_statuses: SubTerminalStatuses,
    app_settings: AppSettings,
    job: SubscriptionQueryJob,
) {
    let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
        db.as_ref(),
        &library_root,
    );
    let sub_id_str = job.subscription_id.to_string();

    // Early failures must still run finalize_if_idle — if this was the run's
    // only job, nothing else clears the guard, run row, and runtime task.
    macro_rules! fail_job_and_finalize {
        ($kind:expr, $message:expr) => {{
            let failure_kind = $kind;
            let message = $message;
            if failure_kind.creates_issue() {
                let _ = runtime
                    .upsert_subscription_issue(
                        job.subscription_id,
                        Some(job.query_id),
                        failure_kind,
                        &message,
                        None,
                    )
                    .await;
            }
            let _ = runtime
                .finish_subscription_query_job(
                    job.job_id,
                    "failed",
                    Some(failure_kind.as_str().to_string()),
                    Some(message),
                )
                .await;
            let _ = finalize_if_idle(
                &runtime,
                &running_subs,
                &sub_terminal_statuses,
                &format!("Subscription {}", job.subscription_id),
                &sub_id_str,
                job.run_id,
                "subscription",
                Some(job.query_id.to_string()),
                None,
            )
            .await;
            return;
        }};
    }

    let sub = match runtime.get_subscription(job.subscription_id).await {
        Ok(Some(sub)) => sub,
        Ok(None) => fail_job_and_finalize!(
            FailureKind::MissingSubscription,
            format!("Subscription {} no longer exists", job.subscription_id)
        ),
        Err(error) => fail_job_and_finalize!(FailureKind::Runtime, error),
    };

    let query = match runtime.get_subscription_query(job.query_id).await {
        Ok(Some(query)) => query,
        Ok(None) => fail_job_and_finalize!(
            FailureKind::MissingQuery,
            format!("Query {} no longer exists", job.query_id)
        ),
        Err(error) => fail_job_and_finalize!(FailureKind::Runtime, error),
    };

    let runner_key = runner_key_for_site(&query.site_id);
    let _site_guard = rate_limiter.acquire_domain_run(&runner_key).await;

    let cancel = {
        let map = running_subs.lock().await;
        map.get(&sub_id_str).cloned().unwrap_or_else(|| {
            let token = tokio_util::sync::CancellationToken::new();
            token.cancel();
            token
        })
    };

    let group_name = match sub.group_id {
        Some(gid) => runtime
            .get_group(gid)
            .await
            .ok()
            .flatten()
            .map(|group| group.name),
        None => None,
    };
    let mode = if job.requested_by == "query" || job.requested_by == "retry" {
        "query"
    } else {
        "subscription"
    };
    let query_name = if job.job_kind == "retry_post" {
        job.post_id
            .as_ref()
            .map(|post_id| format!("Retry post {post_id}"))
            .unwrap_or_else(|| format!("Retry query {}", job.query_id))
    } else {
        resolve_query_name(
            query.query_id,
            &query.query_text,
            query.display_name.as_deref(),
        )
    };

    let result = tokio::spawn(run_job_inner(
        db.clone(),
        library_root.clone(),
        rate_limiter.clone(),
        app_settings.clone(),
        sub.clone(),
        query.clone(),
        job.clone(),
        group_name.clone(),
        query_name.clone(),
        cancel.clone(),
    ))
    .await;

    let result = match result {
        Ok(result) => result,
        Err(error) => {
            let message = format!("Task panicked: {error}");
            let _ = runtime
                .upsert_subscription_issue(
                    job.subscription_id,
                    Some(job.query_id),
                    FailureKind::Panic,
                    "Subscription executor panicked",
                    Some(&message),
                )
                .await;
            let _ = runtime
                .finish_subscription_query_job(
                    job.job_id,
                    "failed",
                    Some(FailureKind::Panic.as_str().to_string()),
                    Some(message.clone()),
                )
                .await;
            publish_panic(
                &sub_id_str,
                &sub.name,
                mode,
                Some(job.query_id.to_string()),
                Some(query_name.clone()),
                message,
            );
            let _ = finalize_if_idle(
                &runtime,
                &running_subs,
                &sub_terminal_statuses,
                &sub.name,
                &sub_id_str,
                job.run_id,
                mode,
                Some(job.query_id.to_string()),
                Some(query_name),
            )
            .await;
            return;
        }
    };

    let status = if result.cancelled {
        "cancelled"
    } else if result.errors.is_empty() {
        "succeeded"
    } else {
        "failed"
    };
    let failure_kind = result.failure_kind.clone();
    let last_error = result.errors.last().cloned();
    let _ = runtime
        .finish_subscription_query_job(job.job_id, status, failure_kind.clone(), last_error.clone())
        .await;
    if let Some(run_id) = job.run_id {
        let _ = runtime
            .accumulate_subscription_run_counters(
                run_id,
                result.files_downloaded_delta as i64,
                result.files_skipped as i64,
                result.metadata_validated as i64,
                result.metadata_invalid as i64,
            )
            .await;
    }
    let _ = finalize_if_idle(
        &runtime,
        &running_subs,
        &sub_terminal_statuses,
        &sub.name,
        &sub_id_str,
        job.run_id,
        mode,
        Some(job.query_id.to_string()),
        Some(query_name),
    )
    .await;
}

async fn finalize_if_idle(
    runtime: &crate::subscriptions::runtime_service::SubscriptionRuntimeService<'_>,
    running_subs: &RunningSubscriptions,
    sub_terminal_statuses: &SubTerminalStatuses,
    subscription_name: &str,
    subscription_id: &str,
    run_id: Option<i64>,
    mode: &str,
    query_id: Option<String>,
    query_name: Option<String>,
) -> Result<(), String> {
    let subscription_id_num: i64 = subscription_id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {subscription_id}"))?;
    if !clear_subscription_guard_if_idle(runtime, running_subs, subscription_id_num).await? {
        return Ok(());
    }

    let (status, failure_kind, error_message) = if let Some(run_id) = run_id {
        let jobs = runtime.list_subscription_query_jobs_for_run(run_id).await?;
        let failure = jobs
            .iter()
            .rev()
            .find(|job| job.status == "failed")
            .or_else(|| jobs.iter().rev().find(|job| job.status == "cancelled"));
        let status = if jobs.iter().any(|job| job.status == "failed") {
            "failed"
        } else if jobs.iter().any(|job| job.status == "cancelled") {
            "cancelled"
        } else {
            "succeeded"
        };
        let failure_kind = failure.and_then(|job| job.failure_kind.clone());
        let error_message = failure.and_then(|job| job.error_message.clone());
        runtime
            .finalize_subscription_run_status(
                run_id,
                status,
                failure_kind.clone(),
                error_message.clone(),
            )
            .await?;
        (status.to_string(), failure_kind, error_message)
    } else {
        ("succeeded".to_string(), None, None)
    };

    if mode == "subscription" {
        sub_terminal_statuses
            .lock()
            .await
            .insert(subscription_id.to_string(), status.clone());
    }

    let run_record = runtime
        .list_subscription_runs(subscription_id_num, 20)
        .await?
        .into_iter()
        .find(|run| Some(run.run_id) == run_id);
    let (downloaded, skipped, metadata_validated, metadata_invalid) = run_record
        .map(|run| {
            (
                run.files_downloaded as usize,
                run.files_skipped as usize,
                run.metadata_validated as usize,
                run.metadata_invalid as usize,
            )
        })
        .unwrap_or((0, 0, 0, 0));

    publish_finished(
        subscription_id,
        subscription_name,
        mode,
        query_id,
        query_name,
        downloaded,
        skipped,
        metadata_validated,
        metadata_invalid,
        None,
        &status,
        resolve_finished_status_text(&status, failure_kind.as_deref()),
        failure_kind.clone(),
        error_message.clone(),
    );
    schedule_progress_snapshot_clear(running_subs.clone(), subscription_id.to_string());
    Ok(())
}

struct JobOutcome {
    files_downloaded_delta: usize,
    files_skipped: usize,
    metadata_validated: usize,
    metadata_invalid: usize,
    cancelled: bool,
    failure_kind: Option<String>,
    errors: Vec<String>,
}

async fn failed_job_outcome(
    runtime: &crate::subscriptions::runtime_service::SubscriptionRuntimeService<'_>,
    subscription_id: i64,
    query_id: i64,
    failure_kind: FailureKind,
    message: String,
) -> JobOutcome {
    let _ = runtime
        .upsert_subscription_issue(
            subscription_id,
            Some(query_id),
            failure_kind,
            &message,
            None,
        )
        .await;
    JobOutcome {
        files_downloaded_delta: 0,
        files_skipped: 0,
        metadata_validated: 0,
        metadata_invalid: 0,
        cancelled: false,
        failure_kind: Some(failure_kind.as_str().to_string()),
        errors: vec![message],
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_job_inner(
    db: Arc<LibraryDatabase>,
    library_root: std::path::PathBuf,
    rate_limiter: RateLimiter,
    app_settings: AppSettings,
    sub: crate::subscriptions::types::Subscription,
    query: crate::subscriptions::types::SubscriptionQuery,
    job: SubscriptionQueryJob,
    group_name: Option<String>,
    _query_name: String,
    cancel: tokio_util::sync::CancellationToken,
) -> JobOutcome {
    let runtime = crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
        db.as_ref(),
        &library_root,
    );
    let engine_result = SubscriptionSyncEngine::new(&db, &app_settings, &library_root);
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

    let mut engine = match engine_result {
        Ok(engine) => engine
            .with_name(sub.name.clone())
            .with_progress_mode(
                if job.requested_by == "query" || job.requested_by == "retry" {
                    "query"
                } else {
                    "subscription"
                },
            )
            .with_group_name(group_name.clone())
            .with_rate_limiter(rate_limiter.clone())
            .with_auto_merge(
                auto_merge_enabled,
                auto_merge_distance,
                auto_merge_require_matching_dimensions,
            )
            .with_auto_collections(sub.auto_collections),
        Err(error) => {
            return failed_job_outcome(
                &runtime,
                sub.subscription_id,
                query.query_id,
                FailureKind::Environment,
                error,
            )
            .await;
        }
    };

    if job.job_kind == "retry_post" {
        let canonical_site_id =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(&query.site_id).to_string();
        let post_id = job.post_id.as_deref().unwrap_or_default();
        let matching = match runtime
            .find_unresolved_subscription_download_attempts(
                sub.subscription_id,
                query.query_id,
                &canonical_site_id,
                post_id,
            )
            .await
        {
            Ok(matching) => matching,
            Err(error) => {
                return failed_job_outcome(
                    &runtime,
                    sub.subscription_id,
                    query.query_id,
                    FailureKind::Runtime,
                    error,
                )
                .await;
            }
        };
        if matching.is_empty() {
            return failed_job_outcome(
                &runtime,
                sub.subscription_id,
                query.query_id,
                FailureKind::MissingRetry,
                format!(
                    "No failed download attempts found for post {} on query {}",
                    post_id, query.query_id
                ),
            )
            .await;
        }
        let retry_url = matching
            .iter()
            .find_map(|attempt| attempt.retry_url.clone())
            .or_else(|| {
                matching
                    .iter()
                    .find_map(|attempt| attempt.canonical_post_url.clone())
            });
        let Some(retry_url) = retry_url else {
            return failed_job_outcome(
                &runtime,
                sub.subscription_id,
                query.query_id,
                FailureKind::MissingRetry,
                format!("No retry URL recorded for post {}", post_id),
            )
            .await;
        };
        for attempt in &matching {
            let _ = runtime
                .mark_subscription_download_attempt_retrying(attempt.attempt_id)
                .await;
        }
        let progress = engine
            .retry_failed_post(
                sub.subscription_id,
                query.query_id,
                &canonical_site_id,
                &retry_url,
                post_id,
                cancel,
            )
            .await;
        return JobOutcome {
            files_downloaded_delta: progress.files_downloaded,
            files_skipped: progress.files_skipped,
            metadata_validated: progress.metadata_validated,
            metadata_invalid: progress.metadata_invalid,
            cancelled: progress.cancelled,
            failure_kind: progress.failure_kind,
            errors: progress.errors,
        };
    }

    let mut total_downloaded_delta = 0usize;
    let mut total_skipped = 0usize;
    let mut total_metadata_validated = 0usize;
    let mut total_metadata_invalid = 0usize;
    let mut total_errors = Vec::new();
    let mut failure_kind = None;
    let mut cancelled = false;

    loop {
        let current_query = match runtime.get_subscription_query(query.query_id).await {
            Ok(Some(query)) => query,
            _ => query.clone(),
        };
        let prior_files = current_query.files_found.max(0) as usize;
        let subscription_limit = if current_query.completed_initial_run {
            sub.periodic_post_limit as u32
        } else {
            sub.initial_post_limit as u32
        };
        // For the initial run the subscription limit is a TOTAL budget across
        // continuation batches, not a per-batch size — "first sync: up to N
        // posts" must stop at N, not crawl the site's entire history N at a
        // time. Each batch requests only what's left of the budget.
        let effective_limit = if current_query.completed_initial_run || subscription_limit == 0 {
            subscription_limit
        } else {
            let already_fetched = current_query.posts_found.max(0) as u32;
            let remaining = subscription_limit.saturating_sub(already_fetched);
            if remaining == 0 {
                tracing::info!(
                    query_id = current_query.query_id,
                    initial_post_limit = subscription_limit,
                    posts_found = already_fetched,
                    "initial post budget exhausted — marking initial run complete"
                );
                let _ = runtime
                    .set_query_completed_initial_run(current_query.query_id, true)
                    .await;
                let _ = runtime
                    .set_query_resume_state(current_query.query_id, None, None)
                    .await;
                break;
            }
            remaining
        };
        let post_limit = effective_query_post_limit(app_settings.sub_batch_size, effective_limit);
        let progress: SyncProgress = engine
            .sync_query(
                job.run_id,
                sub.subscription_id,
                current_query.query_id,
                &current_query.query_text,
                current_query.display_name.as_deref(),
                &current_query.site_id,
                post_limit,
                current_query.completed_initial_run,
                current_query.resume_cursor.as_deref(),
                current_query.resume_strategy.as_deref(),
                cancel.clone(),
            )
            .await;
        total_downloaded_delta += progress.files_downloaded.saturating_sub(prior_files);
        total_skipped += progress.files_skipped;
        total_metadata_validated += progress.metadata_validated;
        total_metadata_invalid += progress.metadata_invalid;
        if !progress.errors.is_empty() {
            total_errors.extend(progress.errors.clone());
        }
        if progress.failure_kind.is_some() {
            failure_kind = progress.failure_kind.clone();
        }
        if progress.cancelled {
            cancelled = true;
            break;
        }

        let refreshed = runtime
            .get_subscription_query(query.query_id)
            .await
            .ok()
            .flatten();
        let needs_continuation = refreshed.as_ref().is_some_and(|query| {
            !query.completed_initial_run
                && query
                    .resume_cursor
                    .as_ref()
                    .is_some_and(|cursor| !cursor.is_empty())
        });
        if !needs_continuation {
            break;
        }
    }

    JobOutcome {
        files_downloaded_delta: total_downloaded_delta,
        files_skipped: total_skipped,
        metadata_validated: total_metadata_validated,
        metadata_invalid: total_metadata_invalid,
        cancelled,
        failure_kind,
        errors: total_errors,
    }
}
