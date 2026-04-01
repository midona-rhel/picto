//! Runtime task helpers for subscription execution.
//!
//! The subscription domain should publish progress through one adapter instead
//! of duplicating `RuntimeTask` shaping in every controller and monitor loop.

use tokio::time::{sleep, Duration};

use crate::runtime_contract::task::{RuntimeTask, TaskProgress, TaskStatus};
use crate::subscriptions::progress::SubscriptionProgressEvent;
use crate::types::RunningSubscriptions;

fn make_subscription_task(
    subscription_id: &str,
    subscription_name: &str,
    status: TaskStatus,
    progress: Option<TaskProgress>,
    detail: Option<serde_json::Value>,
) -> RuntimeTask {
    let now = chrono::Utc::now().to_rfc3339();
    RuntimeTask {
        task_id: format!("sub:{subscription_id}"),
        kind: crate::runtime_contract::task::TaskKind::Subscription,
        status,
        label: subscription_name.to_string(),
        parent_task_id: None,
        progress,
        detail,
        started_at: now.clone(),
        updated_at: now,
    }
}

fn make_group_task(
    group_id: &str,
    status: TaskStatus,
    progress: Option<TaskProgress>,
) -> RuntimeTask {
    let now = chrono::Utc::now().to_rfc3339();
    RuntimeTask {
        task_id: format!("group:{group_id}"),
        kind: crate::runtime_contract::task::TaskKind::SubscriptionGroup,
        status,
        label: format!("Group {group_id}"),
        parent_task_id: None,
        progress,
        detail: None,
        started_at: now.clone(),
        updated_at: now,
    }
}

pub fn schedule_progress_snapshot_clear(
    running_subs: RunningSubscriptions,
    subscription_id: String,
) {
    let task_id = format!("sub:{subscription_id}");
    tokio::spawn(async move {
        sleep(Duration::from_millis(3000)).await;
        let still_running = {
            let map = running_subs.lock().await;
            map.contains_key(&subscription_id)
        };
        if !still_running {
            crate::runtime_state::remove_task(&task_id);
        }
    });
}

pub fn publish_subscription_task(
    subscription_id: &str,
    subscription_name: &str,
    status: TaskStatus,
    progress: Option<TaskProgress>,
    event: &SubscriptionProgressEvent,
) {
    crate::runtime_state::upsert_task(make_subscription_task(
        subscription_id,
        subscription_name,
        status,
        progress,
        serde_json::to_value(event).ok(),
    ));
}

pub fn publish_start(
    subscription_id: &str,
    subscription_name: &str,
    mode: &str,
    query_id: Option<String>,
    query_name: Option<String>,
) {
    let event = SubscriptionProgressEvent {
        subscription_id: subscription_id.to_string(),
        subscription_name: subscription_name.to_string(),
        mode: mode.to_string(),
        group_name: None,
        query_id,
        query_name,
        files_downloaded: 0,
        files_skipped: 0,
        queued_for_ingest: 0,
        ingesting: 0,
        ingested: 0,
        reused: 0,
        failed_ingest: 0,
        pages_fetched: 0,
        metadata_validated: 0,
        metadata_invalid: 0,
        last_metadata_error: None,
        status_text: "Starting...".to_string(),
        phase: Some("starting".to_string()),
        current_post_id: None,
        current_post_items: 0,
        posts_processed: 0,
        resume_cursor: None,
        last_error: None,
        finished_status: None,
        failure_kind: None,
        error: None,
    };
    publish_subscription_task(
        subscription_id,
        subscription_name,
        TaskStatus::Running,
        None,
        &event,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn publish_running_progress(
    subscription_id: &str,
    subscription_name: &str,
    mode: &str,
    group_name: Option<&str>,
    query_id: Option<String>,
    query_name: Option<String>,
    progress: &crate::subscriptions::sync_engine::SyncProgress,
    status_text: &str,
    phase: &str,
) {
    let event = SubscriptionProgressEvent {
        subscription_id: subscription_id.to_string(),
        subscription_name: subscription_name.to_string(),
        mode: mode.to_string(),
        group_name: group_name.map(|s| s.to_string()),
        query_id,
        query_name,
        files_downloaded: progress.files_downloaded,
        files_skipped: progress.files_skipped,
        queued_for_ingest: progress.queued_for_ingest,
        ingesting: 0,
        ingested: 0,
        reused: 0,
        failed_ingest: 0,
        pages_fetched: progress.pages_fetched,
        metadata_validated: progress.metadata_validated,
        metadata_invalid: progress.metadata_invalid,
        last_metadata_error: progress.last_metadata_error.clone(),
        status_text: status_text.to_string(),
        phase: Some(phase.to_string()),
        current_post_id: progress.current_post_id.clone(),
        current_post_items: progress.current_post_items,
        posts_processed: progress.posts_processed,
        resume_cursor: progress.resume_cursor.clone(),
        last_error: progress.errors.last().cloned(),
        finished_status: None,
        failure_kind: None,
        error: None,
    };
    publish_subscription_task(
        subscription_id,
        subscription_name,
        TaskStatus::Running,
        Some(TaskProgress {
            done: progress.files_downloaded as u64,
            total: (progress.files_downloaded + progress.files_skipped) as u64,
            status_text: Some(status_text.to_string()),
        }),
        &event,
    );
}

pub fn publish_cancelling(subscription_id: &str, subscription_name: &str) {
    let event = SubscriptionProgressEvent {
        subscription_id: subscription_id.to_string(),
        subscription_name: subscription_name.to_string(),
        mode: "subscription".to_string(),
        group_name: None,
        query_id: None,
        query_name: None,
        files_downloaded: 0,
        files_skipped: 0,
        queued_for_ingest: 0,
        ingesting: 0,
        ingested: 0,
        reused: 0,
        failed_ingest: 0,
        pages_fetched: 0,
        metadata_validated: 0,
        metadata_invalid: 0,
        last_metadata_error: None,
        status_text: "Cancelling…".to_string(),
        phase: Some("cancelling".to_string()),
        current_post_id: None,
        current_post_items: 0,
        posts_processed: 0,
        resume_cursor: None,
        last_error: None,
        finished_status: None,
        failure_kind: None,
        error: None,
    };
    publish_subscription_task(
        subscription_id,
        subscription_name,
        TaskStatus::Cancelling,
        None,
        &event,
    );
}

#[allow(clippy::too_many_arguments)]
pub fn publish_finished(
    subscription_id: &str,
    subscription_name: &str,
    mode: &str,
    query_id: Option<String>,
    query_name: Option<String>,
    downloaded: usize,
    skipped: usize,
    metadata_validated: usize,
    metadata_invalid: usize,
    last_metadata_error: Option<String>,
    finished_status: &str,
    status_text: &str,
    failure_kind: Option<String>,
    error: Option<String>,
) {
    let task_status = if finished_status == "succeeded" {
        TaskStatus::Finished
    } else {
        TaskStatus::Failed
    };
    let last_error = error.clone();
    let event = SubscriptionProgressEvent {
        subscription_id: subscription_id.to_string(),
        subscription_name: subscription_name.to_string(),
        mode: mode.to_string(),
        group_name: None,
        query_id,
        query_name,
        files_downloaded: downloaded,
        files_skipped: skipped,
        queued_for_ingest: 0,
        ingesting: 0,
        ingested: 0,
        reused: 0,
        failed_ingest: 0,
        pages_fetched: 0,
        metadata_validated,
        metadata_invalid,
        last_metadata_error,
        status_text: status_text.to_string(),
        phase: Some("finished".to_string()),
        current_post_id: None,
        current_post_items: 0,
        posts_processed: 0,
        resume_cursor: None,
        last_error,
        finished_status: Some(finished_status.to_string()),
        failure_kind,
        error,
    };
    publish_subscription_task(
        subscription_id,
        subscription_name,
        task_status,
        Some(TaskProgress {
            done: downloaded as u64,
            total: (downloaded + skipped) as u64,
            status_text: Some(status_text.to_string()),
        }),
        &event,
    );
}

pub fn publish_panic(
    subscription_id: &str,
    subscription_name: &str,
    mode: &str,
    query_id: Option<String>,
    query_name: Option<String>,
    error: String,
) {
    let last_error = Some(error.clone());
    let event = SubscriptionProgressEvent {
        subscription_id: subscription_id.to_string(),
        subscription_name: subscription_name.to_string(),
        mode: mode.to_string(),
        group_name: None,
        query_id,
        query_name,
        files_downloaded: 0,
        files_skipped: 0,
        queued_for_ingest: 0,
        ingesting: 0,
        ingested: 0,
        reused: 0,
        failed_ingest: 0,
        pages_fetched: 0,
        metadata_validated: 0,
        metadata_invalid: 0,
        last_metadata_error: None,
        status_text: "Failed".to_string(),
        phase: Some("finished".to_string()),
        current_post_id: None,
        current_post_items: 0,
        posts_processed: 0,
        resume_cursor: None,
        last_error,
        finished_status: Some("failed".to_string()),
        failure_kind: Some("panic".to_string()),
        error: Some(error),
    };
    publish_subscription_task(
        subscription_id,
        subscription_name,
        TaskStatus::Failed,
        None,
        &event,
    );
}

pub fn publish_group_start(group_id: &str) {
    crate::runtime_state::upsert_task(make_group_task(group_id, TaskStatus::Running, None));
}

pub fn publish_group_progress(group_id: &str, done: u64, total: u64) {
    crate::runtime_state::upsert_task(make_group_task(
        group_id,
        TaskStatus::Running,
        Some(TaskProgress {
            done,
            total,
            status_text: None,
        }),
    ));
}

pub fn publish_group_finished(group_id: &str, failed: bool, done: u64, total: u64) {
    crate::runtime_state::upsert_task(make_group_task(
        group_id,
        if failed {
            TaskStatus::Failed
        } else {
            TaskStatus::Finished
        },
        Some(TaskProgress {
            done,
            total,
            status_text: None,
        }),
    ));
}

pub fn publish_group_failed(group_id: &str) {
    crate::runtime_state::upsert_task(make_group_task(group_id, TaskStatus::Failed, None));
}
