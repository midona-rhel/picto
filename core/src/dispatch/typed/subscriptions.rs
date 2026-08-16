//! Handler functions for subscription operations.
//!
//! ## State-change classification (PBI-566 audit)
//!
//! Every handler in this module is classified as either:
//! - **state-changed**: emits `runtime/state_changed` with entity IDs after completing
//! - **task-only**: spawns async work; state changes emitted by the sync engine, not this handler
//! - **read-only**: returns data, no state mutation
//!
//! ### state-changed handlers (emit entity IDs)
//! - `set_subscription_schedule` → subscription_ids
//! - `create_subscription` → subscription_ids
//! - `delete_subscription` → subscription_ids
//! - `pause_subscription` → subscription_ids
//! - `rename_subscription` → subscription_ids
//! - `reset_subscription` → subscription_ids
//! - `add_subscription_query` → query_ids
//! - `delete_subscription_query` → query_ids
//! - `edit_subscription_query` → query_ids
//! - `pause_subscription_query` → query_ids
//! - `reset_subscription_query` → query_ids
//! - `set_credential` → credential_categories
//! - `delete_credential` → credential_categories
//! - `pixiv_oauth_exchange` → credential_categories
//!
//! ### task-only handlers (no state_changed emit — sync engine owns lifecycle)
//! - `run_subscription`
//! - `stop_subscription`
//! - `run_subscription_query`
//! - `retry_subscription_failed_post`
//!
//! ### read-only handlers
//! - `get_sites`
//! - `get_subscriptions`
//! - `get_running_subscriptions`
//! - `get_running_subscription_progress`
//! - `list_subscription_runs`
//! - `list_subscription_issues`
//! - `list_subscription_download_attempts`
//! - `list_credentials`
//! - `list_credential_health`
//! - `pixiv_oauth_start`

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

fn runtime_service(
    state: &AppState,
) -> crate::subscriptions::runtime_service::SubscriptionRuntimeService<'_> {
    crate::subscriptions::runtime_service::SubscriptionRuntimeService::new(
        state.engine.db(),
        &state.library_root,
    )
}

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SuggestSiteTagsInput {
    pub site_id: String,
    pub prefix: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetSubscriptionScheduleInput {
    pub id: String,
    pub schedule: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateSubscriptionInput {
    pub name: String,
    #[ts(type = "number | null")]
    pub initial_post_limit: Option<u32>,
    #[ts(type = "number | null")]
    pub periodic_post_limit: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteSubscriptionInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct PauseSubscriptionInput {
    pub id: String,
    pub paused: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct AddSubscriptionQueryInput {
    pub subscription_id: String,
    pub site_id: String,
    pub query_kind: Option<String>,
    pub query_text: String,
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteSubscriptionQueryInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct PauseSubscriptionQueryInput {
    pub id: String,
    pub paused: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RunSubscriptionInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct StopSubscriptionInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ResetSubscriptionInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ResetSubscriptionQueryInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RenameSubscriptionInput {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RunSubscriptionQueryInput {
    pub subscription_id: String,
    pub query_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct StopSubscriptionQueryInput {
    pub subscription_id: String,
    pub query_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RetrySubscriptionFailedPostInput {
    pub subscription_id: String,
    pub query_id: String,
    pub post_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RetrySubscriptionFailedPostsInput {
    pub subscription_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionRunsInput {
    pub subscription_id: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionIssuesInput {
    pub subscription_id: String,
    pub query_id: Option<String>,
    pub cursor: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionDownloadAttemptsInput {
    pub subscription_id: String,
    pub query_id: Option<String>,
    pub cursor: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetCredentialInput {
    pub site_category: String,
    pub credential_type: String,
    pub display_name: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub cookies: Option<std::collections::HashMap<String, String>>,
    pub oauth_token: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteCredentialInput {
    pub site_category: String,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn set_subscription_schedule(
    state: &AppState,
    input: SetSubscriptionScheduleInput,
) -> Result<(), String> {
    let sid: i64 = input.id.parse().unwrap_or(0);
    runtime_service(state)
        .set_subscription_schedule(input.id, input.schedule)
        .await?;
    crate::events::emit_state_changed(
        "set_subscription_schedule",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .subscription_ids(vec![sid]),
    );
    Ok(())
}

pub async fn get_sites(
    _state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let sites: Vec<_> = crate::subscriptions::gallery_dl_runner::SITES
        .iter()
        .collect();
    serde_json::to_value(sites).map_err(|e| e.to_string())
}

pub async fn get_subscription_covers(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = runtime_service(state).get_subscription_covers().await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

/// Read-only: booru tag autocomplete. Empty list for unsupported sites or
/// on any network/parse failure — never an error.
pub async fn suggest_site_tags(
    _state: &AppState,
    input: SuggestSiteTagsInput,
) -> Result<serde_json::Value, String> {
    let suggestions = crate::subscriptions::tag_autocomplete::suggest_tags(
        &input.site_id,
        &input.prefix,
        input.limit,
    )
    .await;
    Ok(serde_json::to_value(&suggestions).map_err(|e| e.to_string())?)
}

pub async fn get_subscriptions(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = runtime_service(state).get_subscriptions().await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn create_subscription(
    state: &AppState,
    input: CreateSubscriptionInput,
) -> Result<serde_json::Value, String> {
    let sub = runtime_service(state)
        .create_subscription(
            input.name,
            input.initial_post_limit,
            input.periodic_post_limit,
        )
        .await?;
    let sid: i64 = sub.id.parse().unwrap_or(0);
    crate::events::emit_state_changed(
        "create_subscription",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Subscriptions,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .subscription_ids(vec![sid]),
    );
    Ok(serde_json::to_value(&sub).map_err(|e| e.to_string())?)
}

pub async fn delete_subscription(
    state: &AppState,
    input: DeleteSubscriptionInput,
) -> Result<serde_json::Value, String> {
    let sid: i64 = input.id.parse().unwrap_or(0);
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        state.engine.db(),
        &state.library_root,
        &state.running_subscriptions,
        input.id.clone(),
    )
    .await?;
    let count = runtime_service(state).delete_subscription(input.id).await?;
    crate::events::emit_state_changed(
        "delete_subscription",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Subscriptions,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .subscription_ids(vec![sid]),
    );
    Ok(serde_json::to_value(&count).map_err(|e| e.to_string())?)
}

pub async fn pause_subscription(
    state: &AppState,
    input: PauseSubscriptionInput,
) -> Result<(), String> {
    let sid: i64 = input.id.parse().unwrap_or(0);
    runtime_service(state)
        .pause_subscription(input.id, input.paused)
        .await?;
    crate::events::emit_state_changed(
        "pause_subscription",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .subscription_ids(vec![sid]),
    );
    Ok(())
}

pub async fn add_subscription_query(
    state: &AppState,
    input: AddSubscriptionQueryInput,
) -> Result<serde_json::Value, String> {
    let query = runtime_service(state)
        .add_subscription_query(
            input.subscription_id,
            input.site_id,
            input.query_kind,
            input.query_text,
            input.notes,
        )
        .await?;
    let qid: i64 = query.id.parse().unwrap_or(0);
    crate::events::emit_state_changed(
        "add_subscription_query",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .query_ids(vec![qid]),
    );
    Ok(serde_json::to_value(&query).map_err(|e| e.to_string())?)
}

pub async fn delete_subscription_query(
    state: &AppState,
    input: DeleteSubscriptionQueryInput,
) -> Result<(), String> {
    let qid: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid query id: {}", input.id))?;
    let query = runtime_service(state)
        .get_subscription_query(qid)
        .await?
        .ok_or_else(|| format!("Query {qid} not found"))?;
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        state.engine.db(),
        &state.library_root,
        &state.running_subscriptions,
        query.subscription_id.to_string(),
    )
    .await?;
    runtime_service(state)
        .delete_subscription_query(input.id)
        .await?;
    crate::events::emit_state_changed(
        "delete_subscription_query",
        crate::runtime_contract::change_builder::ChangeImpact::new().query_ids(vec![qid]),
    );
    Ok(())
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct EditSubscriptionQueryInput {
    pub id: i64,
    pub site_id: String,
    pub query_kind: Option<String>,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
}

pub async fn edit_subscription_query(
    state: &AppState,
    input: EditSubscriptionQueryInput,
) -> Result<(), String> {
    runtime_service(state)
        .edit_subscription_query(
            input.id,
            input.site_id,
            input.query_kind,
            input.query_text,
            input.display_name,
            input.notes,
        )
        .await?;
    crate::events::emit_state_changed(
        "edit_subscription_query",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .query_ids(vec![input.id]),
    );
    Ok(())
}

pub async fn pause_subscription_query(
    state: &AppState,
    input: PauseSubscriptionQueryInput,
) -> Result<(), String> {
    let qid: i64 = input.id.parse().unwrap_or(0);
    runtime_service(state)
        .pause_subscription_query(input.id, input.paused)
        .await?;
    crate::events::emit_state_changed(
        "pause_subscription_query",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .query_ids(vec![qid]),
    );
    Ok(())
}

/// Task-only: spawns an async subscription run. State changes are emitted by
/// the sync engine as files are imported, not by this dispatch handler.
/// Progress is tracked via runtime/task_upserted events.
pub async fn run_subscription(state: &AppState, input: RunSubscriptionInput) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::run_subscription(
        &state.engine.db_arc(),
        &state.library_root,
        &state.running_subscriptions,
        input.id,
    )
    .await?;
    Ok(())
}

/// Task-only: cancels a running subscription via CancellationToken.
/// The sync engine emits its own terminal state change when it finishes.
pub async fn stop_subscription(
    state: &AppState,
    input: StopSubscriptionInput,
) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        state.engine.db(),
        &state.library_root,
        &state.running_subscriptions,
        input.id,
    )
    .await?;
    Ok(())
}

pub async fn reset_subscription(
    state: &AppState,
    input: ResetSubscriptionInput,
) -> Result<(), String> {
    let sid: i64 = input.id.parse().unwrap_or(0);
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        state.engine.db(),
        &state.library_root,
        &state.running_subscriptions,
        input.id.clone(),
    )
    .await?;
    runtime_service(state).reset_subscription(input.id).await?;
    crate::events::emit_state_changed(
        "reset_subscription",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .subscription_ids(vec![sid]),
    );
    Ok(())
}

pub async fn reset_subscription_query(
    state: &AppState,
    input: ResetSubscriptionQueryInput,
) -> Result<(), String> {
    let qid: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid query id: {}", input.id))?;
    let query = runtime_service(state)
        .get_subscription_query(qid)
        .await?
        .ok_or_else(|| format!("Query {qid} not found"))?;
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        state.engine.db(),
        &state.library_root,
        &state.running_subscriptions,
        query.subscription_id.to_string(),
    )
    .await?;
    runtime_service(state)
        .reset_subscription_query(input.id)
        .await?;
    crate::events::emit_state_changed(
        "reset_subscription_query",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .query_ids(vec![qid]),
    );
    Ok(())
}

pub async fn get_running_subscriptions(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::get_running_subscriptions(
        &state.running_subscriptions,
    ).await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn get_running_subscription_progress(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let mut result = crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::get_running_subscription_progress();
    for event in &mut result {
        if let Some(Ok(query_id)) = event.query_id.as_deref().map(str::parse::<i64>) {
            if let Ok(progress) = runtime_service(state)
                .count_current_query_run_progress(query_id)
                .await
            {
                event.files_downloaded = merge_current_attempt_progress(
                    progress.files_downloaded,
                    progress.current_files_downloaded,
                    event.files_downloaded,
                );
                event.files_skipped = merge_current_attempt_progress(
                    progress.files_skipped,
                    progress.current_files_skipped,
                    event.files_skipped,
                );
                event.posts_processed = merge_current_attempt_progress(
                    progress.posts_processed,
                    progress.current_posts_processed,
                    event.posts_processed,
                );
                event.metadata_validated = merge_current_attempt_progress(
                    progress.metadata_validated,
                    progress.current_metadata_validated,
                    event.metadata_validated,
                );
                event.metadata_invalid = merge_current_attempt_progress(
                    progress.metadata_invalid,
                    progress.current_metadata_invalid,
                    event.metadata_invalid,
                );
            }
            if let Ok(counts) = runtime_service(state)
                .count_current_ingest_queue(query_id)
                .await
            {
                event.queued_for_ingest = counts.queued;
                event.ingesting = counts.ingesting;
                event.ingested = counts.ingested;
                event.reused = counts.reused;
                event.failed_ingest = counts.failed;
            }
        }
    }
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

fn merge_current_attempt_progress(total: usize, current: usize, live: usize) -> usize {
    total.saturating_sub(current) + current.max(live)
}

#[cfg(test)]
mod progress_tests {
    use super::merge_current_attempt_progress;

    #[test]
    fn live_progress_extends_prior_attempts_without_double_counting() {
        assert_eq!(merge_current_attempt_progress(50, 35, 36), 51);
        assert_eq!(merge_current_attempt_progress(50, 35, 35), 50);
        assert_eq!(merge_current_attempt_progress(50, 35, 34), 50);
    }
}

pub async fn list_subscription_runs(
    state: &AppState,
    input: ListSubscriptionRunsInput,
) -> Result<serde_json::Value, String> {
    let subscription_id: i64 = input
        .subscription_id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", input.subscription_id))?;
    let result = runtime_service(state)
        .list_subscription_runs(subscription_id, input.limit.unwrap_or(20).max(1))
        .await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn list_subscription_issues(
    state: &AppState,
    input: ListSubscriptionIssuesInput,
) -> Result<serde_json::Value, String> {
    let subscription_id: i64 = input
        .subscription_id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", input.subscription_id))?;
    let query_id = input
        .query_id
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| "Invalid query id".to_string())?;
    let result = runtime_service(state)
        .list_subscription_issues_page(
            subscription_id,
            query_id,
            input.cursor,
            input.limit.unwrap_or(50).clamp(1, 200),
        )
        .await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn list_subscription_download_attempts(
    state: &AppState,
    input: ListSubscriptionDownloadAttemptsInput,
) -> Result<serde_json::Value, String> {
    let subscription_id: i64 = input
        .subscription_id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", input.subscription_id))?;
    let query_id = input
        .query_id
        .map(|value| value.parse::<i64>())
        .transpose()
        .map_err(|_| "Invalid query id".to_string())?;
    let result = runtime_service(state)
        .list_subscription_download_attempts_page(
            subscription_id,
            query_id,
            input.cursor,
            input.limit.unwrap_or(50).clamp(1, 200),
        )
        .await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn rename_subscription(
    state: &AppState,
    input: RenameSubscriptionInput,
) -> Result<(), String> {
    let sid: i64 = input.id.parse().unwrap_or(0);
    runtime_service(state)
        .rename_subscription(input.id, input.name)
        .await?;
    crate::events::emit_state_changed(
        "rename_subscription",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Subscriptions,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .subscription_ids(vec![sid]),
    );
    Ok(())
}

/// Task-only: spawns an async query run. State changes emitted by sync engine.
pub async fn run_subscription_query(
    state: &AppState,
    input: RunSubscriptionQueryInput,
) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::run_subscription_query(
        &state.engine.db_arc(),
        &state.library_root,
        &state.running_subscriptions,
        input.subscription_id,
        input.query_id,
    )
    .await?;
    Ok(())
}

pub async fn stop_subscription_query(
    state: &AppState,
    input: StopSubscriptionQueryInput,
) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription_query(
        state.engine.db(),
        &state.library_root,
        &state.running_subscriptions,
        input.subscription_id,
        input.query_id,
    )
    .await?;
    Ok(())
}

pub async fn retry_subscription_failed_post(
    state: &AppState,
    input: RetrySubscriptionFailedPostInput,
) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::retry_failed_post(
        &state.engine.db_arc(),
        &state.library_root,
        &state.running_subscriptions,
        input.subscription_id,
        input.query_id,
        input.post_id,
    )
    .await?;
    Ok(())
}

pub async fn retry_subscription_failed_posts(
    state: &AppState,
    input: RetrySubscriptionFailedPostsInput,
) -> Result<serde_json::Value, String> {
    let result =
        crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::retry_failed_posts(
            &state.engine.db_arc(),
            &state.library_root,
            &state.running_subscriptions,
            input.subscription_id,
        )
        .await?;
    serde_json::to_value(result).map_err(|error| error.to_string())
}

pub async fn list_credentials(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = crate::subscriptions::credential_service::SubscriptionCredentialService::new(
        state.engine.db(),
    )
    .list_credentials()
    .await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn list_credential_health(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = crate::subscriptions::credential_service::SubscriptionCredentialService::new(
        state.engine.db(),
    )
    .list_credential_health()
    .await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn set_credential(state: &AppState, input: SetCredentialInput) -> Result<(), String> {
    let site_category =
        crate::subscriptions::credential_service::SubscriptionCredentialService::new(
            state.engine.db(),
        )
        .store_captured_credential(
            crate::subscriptions::credential_service::SetCapturedCredentialRequest {
                site_category: input.site_category,
                credential_type: input.credential_type,
                username: input.username,
                password: input.password,
                cookies: input.cookies,
                oauth_token: input.oauth_token,
                display_name: input.display_name,
            },
        )
        .await?;

    crate::events::emit_state_changed(
        "set_credential",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .credential_categories(vec![site_category]),
    );

    Ok(())
}

pub async fn delete_credential(
    state: &AppState,
    input: DeleteCredentialInput,
) -> Result<(), String> {
    let canonical = crate::subscriptions::credential_service::SubscriptionCredentialService::new(
        state.engine.db(),
    )
    .delete_credential(&input.site_category)
    .await?;
    crate::events::emit_state_changed(
        "delete_credential",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .credential_categories(vec![canonical]),
    );
    Ok(())
}

// ─── Pixiv OAuth ──────────────────────────────────────────────────────────

pub async fn pixiv_oauth_start(
    _state: &AppState,
    _args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let challenge = crate::subscriptions::pixiv_oauth::generate_challenge();
    serde_json::to_value(&challenge).map_err(|e| format!("Serialize error: {e}"))
}

#[derive(Debug, Deserialize)]
pub struct PixivOAuthExchangeInput {
    pub code: String,
    pub code_verifier: String,
    pub phpsessid: Option<String>,
}

pub async fn pixiv_oauth_exchange(
    state: &AppState,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let input: PixivOAuthExchangeInput =
        serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;

    let refresh_token =
        crate::subscriptions::pixiv_oauth::exchange_code(&input.code, &input.code_verifier).await?;

    crate::subscriptions::credential_service::SubscriptionCredentialService::new(state.engine.db())
        .store_pixiv_oauth_credential(refresh_token, input.phpsessid)
        .await?;

    crate::events::emit_state_changed(
        "pixiv_oauth_exchange",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .credential_categories(vec!["pixiv".to_string()]),
    );

    Ok(serde_json::json!({ "ok": true }))
}
