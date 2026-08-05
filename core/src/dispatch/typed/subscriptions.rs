//! Handler functions for subscription and group operations.
//!
//! ## State-change classification (PBI-566 audit)
//!
//! Every handler in this module is classified as either:
//! - **state-changed**: emits `runtime/state_changed` with entity IDs after completing
//! - **task-only**: spawns async work; state changes emitted by the sync engine, not this handler
//! - **read-only**: returns data, no state mutation
//!
//! ### state-changed handlers (emit entity IDs)
//! - `create_group` → group_ids
//! - `delete_group` → group_ids
//! - `rename_group` → group_ids
//! - `set_subscription_schedule` → subscription_ids
//! - `create_subscription` → subscription_ids
//! - `delete_subscription` → subscription_ids
//! - `pause_subscription` → subscription_ids
//! - `rename_subscription` → subscription_ids
//! - `reset_subscription` → subscription_ids
//! - `set_subscription_auto_collections` → subscription_ids
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
//! - `run_group`
//! - `stop_group`
//! - `run_subscription`
//! - `stop_subscription`
//! - `run_subscription_query`
//! - `retry_subscription_failed_post`
//!
//! ### read-only handlers
//! - `get_sites`
//! - `get_site_metadata_schema`
//! - `validate_site_metadata`
//! - `get_subscriptions`
//! - `get_running_subscriptions`
//! - `get_running_subscription_progress`
//! - `list_subscription_runs`
//! - `list_subscription_query_runs`
//! - `list_subscription_issues`
//! - `list_subscription_download_attempts`
//! - `list_credentials`
//! - `list_credential_health`
//! - `pixiv_oauth_start`
//! - `pixiv_oauth_popup`

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
pub struct VerifySubscriptionSiteInput {
    pub site_id: String,
    pub query: Option<String>,
    pub post_limit: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SuggestSiteTagsInput {
    pub site_id: String,
    pub prefix: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetSubscriptionGroupInput {
    pub subscription_id: String,
    pub group_id: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionCollectionsInput {
    pub subscription_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateGroupInput {
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteGroupInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RenameGroupInput {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetSubscriptionScheduleInput {
    pub id: String,
    pub schedule: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RunGroupInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct StopGroupInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GetSiteMetadataSchemaInput {
    pub site_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ValidateSiteMetadataInput {
    pub site_id: String,
    #[serde(default)]
    pub sample_url: Option<String>,
    #[ts(type = "Record<string, unknown> | null")]
    pub sample_metadata_json: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateSubscriptionInput {
    pub name: String,
    #[ts(type = "number | null")]
    pub group_id: Option<i64>,
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
    pub site_id: String,
    pub post_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionRunsInput {
    pub subscription_id: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionQueryRunsInput {
    pub query_id: String,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionIssuesInput {
    pub subscription_id: String,
    pub query_id: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ListSubscriptionDownloadAttemptsInput {
    pub subscription_id: String,
    pub query_id: Option<String>,
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
    pub expires_at: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteCredentialInput {
    pub site_category: String,
}

// ─── Handlers ──────────────────────────────────────────────────────────────

pub async fn get_groups(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = runtime_service(state).get_groups().await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn create_group(
    state: &AppState,
    input: CreateGroupInput,
) -> Result<serde_json::Value, String> {
    let group = runtime_service(state).create_group(input.name).await?;
    let gid: i64 = group.id.parse().unwrap_or(0);
    crate::events::emit_state_changed(
        "create_group",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Subscriptions,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .group_ids(vec![gid]),
    );
    Ok(serde_json::to_value(&group).map_err(|e| e.to_string())?)
}

pub async fn delete_group(state: &AppState, input: DeleteGroupInput) -> Result<(), String> {
    let gid: i64 = input.id.parse().unwrap_or(0);
    runtime_service(state).delete_group(input.id).await?;
    crate::events::emit_state_changed(
        "delete_group",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Subscriptions,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .group_ids(vec![gid]),
    );
    Ok(())
}

pub async fn rename_group(state: &AppState, input: RenameGroupInput) -> Result<(), String> {
    let gid: i64 = input.id.parse().unwrap_or(0);
    runtime_service(state)
        .rename_group(input.id, input.name)
        .await?;
    crate::events::emit_state_changed(
        "rename_group",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domains(&[
                crate::runtime_contract::state_change::Domain::Subscriptions,
                crate::runtime_contract::state_change::Domain::Sidebar,
            ])
            .group_ids(vec![gid]),
    );
    Ok(())
}

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

/// Task-only: spawns async group run (all child subscriptions).
/// State changes emitted per-subscription by the sync engine.
pub async fn run_group(state: &AppState, input: RunGroupInput) -> Result<(), String> {
    crate::subscriptions::group_orchestrator::SubscriptionGroupOrchestrator::run_group(
        &state.engine.db_arc(),
        &state.library_root,
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        input.id,
        &state.settings,
    )
    .await?;
    Ok(())
}

/// Task-only: cancels all running subscriptions in a group.
pub async fn stop_group(state: &AppState, input: StopGroupInput) -> Result<(), String> {
    crate::subscriptions::group_orchestrator::SubscriptionGroupOrchestrator::stop_group(
        state.engine.db(),
        &state.library_root,
        &state.running_subscriptions,
        input.id,
    )
    .await?;
    Ok(())
}

pub async fn get_sites(
    _state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    Ok(
        serde_json::to_value(&crate::subscriptions::gallery_dl_runner::SITES)
            .map_err(|e| e.to_string())?,
    )
}

/// State-changed: moves a subscription into/out of a group → subscription_ids + group_ids.
pub async fn set_subscription_group(
    state: &AppState,
    input: SetSubscriptionGroupInput,
) -> Result<(), String> {
    let sid: i64 = input
        .subscription_id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", input.subscription_id))?;
    runtime_service(state)
        .set_subscription_group(sid, input.group_id)
        .await?;
    let mut impact = crate::runtime_contract::change_builder::ChangeImpact::new()
        .add_domains(&[
            crate::runtime_contract::state_change::Domain::Subscriptions,
            crate::runtime_contract::state_change::Domain::Sidebar,
        ])
        .subscription_ids(vec![sid]);
    if let Some(gid) = input.group_id {
        impact = impact.group_ids(vec![gid]);
    }
    crate::events::emit_state_changed("set_subscription_group", impact);
    Ok(())
}

/// Read-only: collections this subscription created from multi-image posts.
pub async fn get_subscription_covers(
    state: &AppState,
    _input: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let result = runtime_service(state).get_subscription_covers().await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn list_subscription_collections(
    state: &AppState,
    input: ListSubscriptionCollectionsInput,
) -> Result<serde_json::Value, String> {
    let sid: i64 = input
        .subscription_id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", input.subscription_id))?;
    let result = runtime_service(state)
        .list_subscription_collections(sid)
        .await?;
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

/// Read-only: live end-to-end probe of one site (downloads 1-3 posts to a
/// temp dir, never ingests, never writes credential health or issues).
pub async fn verify_subscription_site(
    state: &AppState,
    input: VerifySubscriptionSiteInput,
) -> Result<serde_json::Value, String> {
    let report = crate::subscriptions::site_verification::verify_site(
        state,
        &input.site_id,
        input.query.as_deref(),
        input.post_limit,
    )
    .await?;
    Ok(serde_json::to_value(&report).map_err(|e| e.to_string())?)
}

pub async fn get_site_metadata_schema(
    _state: &AppState,
    input: GetSiteMetadataSchemaInput,
) -> Result<serde_json::Value, String> {
    let schema = crate::subscriptions::gallery_dl_runner::get_site_metadata_schema(&input.site_id)
        .ok_or_else(|| format!("Unsupported site for metadata schema: {}", input.site_id))?;
    Ok(serde_json::to_value(&schema).map_err(|e| e.to_string())?)
}

pub async fn validate_site_metadata(
    _state: &AppState,
    input: ValidateSiteMetadataInput,
) -> Result<serde_json::Value, String> {
    let sample_url = input.sample_url.unwrap_or_default();
    let result = crate::subscriptions::gallery_dl_runner::validate_site_metadata(
        &input.site_id,
        &sample_url,
        input.sample_metadata_json.as_ref(),
    );
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
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
            input.group_id,
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
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .query_ids(vec![qid]),
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

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetSubscriptionAutoCollectionsInput {
    pub id: String,
    pub auto_collections: bool,
}

pub async fn set_subscription_auto_collections(
    state: &AppState,
    input: SetSubscriptionAutoCollectionsInput,
) -> Result<(), String> {
    let sub_id: i64 = input
        .id
        .parse()
        .map_err(|_| format!("Invalid subscription id: {}", input.id))?;
    runtime_service(state)
        .set_subscription_auto_collections(sub_id, input.auto_collections)
        .await?;
    crate::events::emit_state_changed(
        "set_subscription_auto_collections",
        crate::runtime_contract::change_builder::ChangeImpact::new()
            .add_domain(crate::runtime_contract::state_change::Domain::Subscriptions)
            .subscription_ids(vec![sub_id]),
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
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        input.id,
        &state.settings,
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

pub async fn list_subscription_query_runs(
    state: &AppState,
    input: ListSubscriptionQueryRunsInput,
) -> Result<serde_json::Value, String> {
    let query_id: i64 = input
        .query_id
        .parse()
        .map_err(|_| format!("Invalid query id: {}", input.query_id))?;
    let result = runtime_service(state)
        .list_subscription_query_runs(query_id, input.limit.unwrap_or(20).max(1))
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
        .list_subscription_issues(subscription_id, query_id, input.limit.unwrap_or(50).max(1))
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
        .list_subscription_download_attempts(
            subscription_id,
            query_id,
            input.limit.unwrap_or(50).max(1),
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
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        input.subscription_id,
        input.query_id,
        &state.settings,
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
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        input.subscription_id,
        input.query_id,
        input.site_id,
        input.post_id,
        &state.settings,
    )
    .await?;
    Ok(())
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
        .set_manual_credential(
            crate::subscriptions::credential_service::SetManualCredentialRequest {
                site_category: input.site_category,
                credential_type: input.credential_type,
                username: input.username,
                password: input.password,
                cookies: input.cookies,
                oauth_token: input.oauth_token,
                display_name: input.display_name,
                expires_at: input.expires_at,
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
