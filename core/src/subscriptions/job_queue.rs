use tokio_util::sync::CancellationToken;

use crate::subscriptions::runtime_service::{RunnableQuery, RunnableSubscription, SubscriptionRuntimeService};
use crate::types::RunningSubscriptions;

pub async fn activate_subscription_guard(
    running_subs: &RunningSubscriptions,
    subscription_id: &str,
) -> Result<CancellationToken, String> {
    let mut map = running_subs.lock().await;
    if map.contains_key(subscription_id) {
        return Err(format!("Subscription {} is already running", subscription_id));
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

pub async fn enqueue_subscription_bundle(
    runtime: &SubscriptionRuntimeService<'_>,
    bundle: &RunnableSubscription,
    requested_by: &str,
) -> Result<i64, String> {
    let run_id = runtime
        .create_subscription_run(bundle.subscription.subscription_id)
        .await?;
    for query in bundle.queries.iter().filter(|query| !query.paused) {
        runtime
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
    }
    Ok(run_id)
}

pub async fn enqueue_single_query(
    runtime: &SubscriptionRuntimeService<'_>,
    runnable: &RunnableQuery,
    requested_by: &str,
) -> Result<i64, String> {
    let run_id = runtime
        .create_subscription_run(runnable.subscription.subscription_id)
        .await?;
    runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            runnable.subscription.subscription_id,
            runnable.query.query_id,
            &runnable.query.site_id,
            "query_sync",
            requested_by,
            None,
        )
        .await?;
    Ok(run_id)
}

pub async fn enqueue_retry_job(
    runtime: &SubscriptionRuntimeService<'_>,
    subscription_id: i64,
    query_id: i64,
    site_id: &str,
    post_id: &str,
) -> Result<i64, String> {
    let run_id = runtime.create_subscription_run(subscription_id).await?;
    runtime
        .enqueue_subscription_query_job(
            Some(run_id),
            subscription_id,
            query_id,
            site_id,
            "retry_post",
            "retry",
            Some(post_id),
        )
        .await?;
    Ok(run_id)
}
