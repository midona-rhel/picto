use crate::subscriptions::job_queue::clear_subscription_guard_if_idle;
use crate::subscriptions::policy::resolve_finished_status_text;
use crate::subscriptions::runtime_service::SubscriptionRuntimeService;
use crate::subscriptions::runtime_tasks::{publish_finished, schedule_progress_snapshot_clear};
use crate::types::RunningSubscriptions;

fn settled_impact(
    subscription_id: i64,
    query_id: Option<i64>,
) -> crate::runtime_contract::change_builder::ChangeImpact {
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::new()
        .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
        .subscription_ids(vec![subscription_id]);
    if let Some(query_id) = query_id {
        impact = impact.query_ids(vec![query_id]);
    }
    impact
}

fn emit_settled(subscription_id: i64, query_id: Option<i64>) {
    crate::events::emit_state_changed(
        "subscription_run_settled",
        settled_impact(subscription_id, query_id),
    );
}

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
    emit_settled(run.subscription_id, None);
    schedule_progress_snapshot_clear(running_subscriptions.clone(), subscription_id);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::settled_impact;
    use crate::runtime_contract::state_change::Domain;

    #[test]
    fn settlement_invalidates_the_subscription_and_optional_query() {
        let subscription = settled_impact(7, None);
        assert_eq!(subscription.domains, vec![Domain::Subscriptions]);
        assert_eq!(subscription.subscription_ids, Some(vec![7]));
        assert_eq!(subscription.query_ids, None);

        let query = settled_impact(7, Some(11));
        assert_eq!(query.domains, vec![Domain::Subscriptions]);
        assert_eq!(query.subscription_ids, Some(vec![7]));
        assert_eq!(query.query_ids, Some(vec![11]));
    }
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
        return match settle_run(runtime, running_subscriptions, parent_run_id).await {
            Ok(true) => Ok(true),
            Ok(false) => {
                emit_settled(run.subscription_id, Some(run.query_id));
                Ok(true)
            }
            Err(error) => {
                emit_settled(run.subscription_id, Some(run.query_id));
                Err(error)
            }
        };
    }

    emit_settled(run.subscription_id, Some(run.query_id));
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
    let cumulative = runtime
        .count_current_query_run_progress(run.query_id)
        .await
        .unwrap_or(
            crate::subscriptions::runtime_service::CurrentQueryRunProgress {
                posts_processed: run.posts_processed.max(0) as usize,
                files_downloaded: run.files_downloaded.max(0) as usize,
                files_skipped: run.files_skipped.max(0) as usize,
                metadata_validated: run.metadata_validated.max(0) as usize,
                metadata_invalid: run.metadata_invalid.max(0) as usize,
                current_posts_processed: run.posts_processed.max(0) as usize,
                current_files_downloaded: run.files_downloaded.max(0) as usize,
                current_files_skipped: run.files_skipped.max(0) as usize,
                current_metadata_validated: run.metadata_validated.max(0) as usize,
                current_metadata_invalid: run.metadata_invalid.max(0) as usize,
            },
        );
    publish_finished(
        &subscription_id,
        &subscription_name,
        "query",
        Some(run.query_id.to_string()),
        query_name,
        cumulative.files_downloaded,
        cumulative.files_skipped,
        cumulative.metadata_validated,
        cumulative.metadata_invalid,
        None,
        &run.status,
        resolve_finished_status_text(&run.status, run.failure_kind.as_deref()),
        run.failure_kind,
        run.error_message,
    );
    schedule_progress_snapshot_clear(running_subscriptions.clone(), subscription_id);
    Ok(true)
}
