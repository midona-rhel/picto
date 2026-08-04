use tokio_util::sync::CancellationToken;

use crate::subscriptions::runtime_service::{
    RunnableQuery, RunnableSubscription, SubscriptionRuntimeService,
};
use crate::types::RunningSubscriptions;

pub async fn activate_subscription_guard(
    running_subs: &RunningSubscriptions,
    subscription_id: &str,
) -> Result<CancellationToken, String> {
    let mut map = running_subs.lock().await;
    if map.contains_key(subscription_id) {
        return Err(format!(
            "Subscription {} is already running",
            subscription_id
        ));
    }
    let cancel = CancellationToken::new();
    map.insert(subscription_id.to_string(), cancel.clone());
    Ok(cancel)
}

pub async fn current_subscription_token(
    running_subs: &RunningSubscriptions,
    subscription_id: &str,
) -> Option<CancellationToken> {
    let map = running_subs.lock().await;
    map.get(subscription_id).cloned()
}

/// Restore the transient cancellation handle for durable work claimed after a
/// restart. The database owns whether work exists; this map only delivers Stop.
pub async fn ensure_subscription_guard(
    running_subs: &RunningSubscriptions,
    subscription_id: i64,
) -> CancellationToken {
    let mut map = running_subs.lock().await;
    map.entry(subscription_id.to_string())
        .or_insert_with(CancellationToken::new)
        .clone()
}

pub async fn clear_subscription_guard_if_idle(
    runtime: &SubscriptionRuntimeService<'_>,
    running_subs: &RunningSubscriptions,
    subscription_id: i64,
) -> Result<bool, String> {
    if runtime
        .count_active_subscription_query_jobs(subscription_id)
        .await?
        != 0
    {
        return Ok(false);
    }

    let mut map = running_subs.lock().await;
    map.remove(&subscription_id.to_string());
    Ok(true)
}

/// A run whose enqueue created no new jobs (everything deduplicated against
/// in-flight work) must be finalized immediately — otherwise it sits as a
/// 'running' row nothing ever completes. Only the just-created run is touched;
/// the run that owns the in-flight jobs keeps running.
async fn finalize_empty_run(
    runtime: &SubscriptionRuntimeService<'_>,
    run_id: i64,
) -> Result<i64, String> {
    let _ = runtime
        .finalize_subscription_run_status(
            run_id,
            "cancelled",
            Some("duplicate".to_string()),
            Some("All queries already had an active job".to_string()),
        )
        .await;
    Err("A run for this subscription is already in progress".to_string())
}

pub async fn enqueue_subscription_bundle(
    runtime: &SubscriptionRuntimeService<'_>,
    bundle: &RunnableSubscription,
    requested_by: &str,
) -> Result<i64, String> {
    let run_id = runtime
        .create_subscription_run(bundle.subscription.subscription_id)
        .await?;
    let mut created_any = false;
    for query in bundle.queries.iter().filter(|query| !query.paused) {
        let (_job_id, created) = runtime
            .enqueue_subscription_query_job(
                Some(run_id),
                bundle.subscription.subscription_id,
                query.query_id,
                &query.site_id,
                "query_sync",
                requested_by,
                None,
            )
            .await?;
        created_any |= created;
    }
    if !created_any {
        return finalize_empty_run(runtime, run_id).await;
    }
    Ok(run_id)
}

pub async fn enqueue_single_query(
    runtime: &SubscriptionRuntimeService<'_>,
    runnable: &RunnableQuery,
    requested_by: &str,
) -> Result<(), String> {
    let (_job_id, created) = runtime
        .enqueue_subscription_query_job(
            None,
            runnable.subscription.subscription_id,
            runnable.query.query_id,
            &runnable.query.site_id,
            "query_sync",
            requested_by,
            None,
        )
        .await?;
    if !created {
        return Err("This query already has an active job".to_string());
    }
    Ok(())
}

pub async fn enqueue_retry_job(
    runtime: &SubscriptionRuntimeService<'_>,
    subscription_id: i64,
    query_id: i64,
    site_id: &str,
    post_id: &str,
) -> Result<(), String> {
    let (_job_id, created) = runtime
        .enqueue_subscription_query_job(
            None,
            subscription_id,
            query_id,
            site_id,
            "retry_post",
            "retry",
            Some(post_id),
        )
        .await?;
    if !created {
        return Err("This post already has an active retry job".to_string());
    }
    Ok(())
}
