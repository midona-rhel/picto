use crate::subscriptions::job_queue::clear_subscription_guard_if_idle;
use crate::subscriptions::policy::resolve_finished_status_text;
use crate::subscriptions::runtime_service::SubscriptionRuntimeService;
use crate::subscriptions::runtime_tasks::{publish_finished, schedule_progress_snapshot_clear};
use crate::types::RunningSubscriptions;

/// Finalize and publish a run only after both downloading and ingest are terminal.
/// Safe to call from either worker; the database transition succeeds once.
pub async fn settle_run(
    runtime: &SubscriptionRuntimeService<'_>,
    running_subscriptions: &RunningSubscriptions,
    run_id: i64,
) -> Result<bool, String> {
    let Some(run) = runtime
        .finalize_subscription_run_if_terminal(run_id)
        .await?
    else {
        return Ok(false);
    };
    let subscription_id = run.subscription_id.to_string();
    let subscription_name = runtime
        .get_subscription(run.subscription_id)
        .await?
        .map(|subscription| subscription.name)
        .unwrap_or_else(|| format!("Subscription {}", run.subscription_id));

    let _ = clear_subscription_guard_if_idle(runtime, running_subscriptions, run.subscription_id)
        .await?;
    publish_finished(
        &subscription_id,
        &subscription_name,
        "subscription",
        None,
        None,
        run.files_downloaded.max(0) as usize,
        run.files_skipped.max(0) as usize,
        run.metadata_validated.max(0) as usize,
        run.metadata_invalid.max(0) as usize,
        None,
        &run.status,
        resolve_finished_status_text(&run.status, run.failure_kind.as_deref()),
        run.failure_kind,
        run.error_message,
    );
    schedule_progress_snapshot_clear(running_subscriptions.clone(), subscription_id);
    Ok(true)
}

/// Finalize one query only after its downloader outcome is known and every
/// ingest queue created by that query is terminal.
pub async fn settle_query_run(
    runtime: &SubscriptionRuntimeService<'_>,
    running_subscriptions: &RunningSubscriptions,
    query_run_id: i64,
) -> Result<bool, String> {
    let Some(run) = runtime
        .finalize_subscription_query_run_if_terminal(query_run_id)
        .await?
    else {
        return Ok(false);
    };

    if let Some(parent_run_id) = run.run_id {
        settle_run(runtime, running_subscriptions, parent_run_id).await?;
        return Ok(true);
    }

    if !clear_subscription_guard_if_idle(runtime, running_subscriptions, run.subscription_id)
        .await?
    {
        return Ok(true);
    }

    let subscription_name = runtime
        .get_subscription(run.subscription_id)
        .await?
        .map(|subscription| subscription.name)
        .unwrap_or_else(|| format!("Subscription {}", run.subscription_id));
    let query_name = runtime
        .get_subscription_query(run.query_id)
        .await?
        .map(|query| query.display_name.unwrap_or(query.query_text));
    let subscription_id = run.subscription_id.to_string();
    publish_finished(
        &subscription_id,
        &subscription_name,
        "query",
        Some(run.query_id.to_string()),
        query_name,
        run.files_downloaded.max(0) as usize,
        run.files_skipped.max(0) as usize,
        run.metadata_validated.max(0) as usize,
        run.metadata_invalid.max(0) as usize,
        None,
        &run.status,
        resolve_finished_status_text(&run.status, run.failure_kind.as_deref()),
        run.failure_kind,
        run.error_message,
    );
    schedule_progress_snapshot_clear(running_subscriptions.clone(), subscription_id);
    Ok(true)
}
