//! Typed command implementations for subscription and flow operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;
use super::{TypedCommand, run_typed};

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct CreateFlowInput {
    pub name: String,
    pub schedule: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteFlowInput {
    pub id: String,
    pub delete_files: Option<bool>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RenameFlowInput {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct SetFlowScheduleInput {
    pub id: String,
    pub schedule: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RunFlowInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct StopFlowInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct GetSiteMetadataSchemaInput {
    pub site_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ValidateSiteMetadataInput {
    pub site_id: String,
    #[serde(default)]
    pub sample_url: Option<String>,
    #[ts(type = "Record<string, unknown> | null")]
    pub sample_metadata_json: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct CreateSubscriptionInput {
    pub name: String,
    pub site_id: String,
    pub queries: Vec<String>,
    #[ts(type = "number | null")]
    pub flow_id: Option<i64>,
    #[ts(type = "number | null")]
    pub initial_file_limit: Option<u32>,
    #[ts(type = "number | null")]
    pub periodic_file_limit: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteSubscriptionInput {
    pub id: String,
    pub delete_files: Option<bool>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct PauseSubscriptionInput {
    pub id: String,
    pub paused: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct AddSubscriptionQueryInput {
    pub subscription_id: String,
    pub query_text: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteSubscriptionQueryInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct PauseSubscriptionQueryInput {
    pub id: String,
    pub paused: bool,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RunSubscriptionInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct StopSubscriptionInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ResetSubscriptionInput {
    pub id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RenameSubscriptionInput {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct RunSubscriptionQueryInput {
    pub subscription_id: String,
    pub query_id: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
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
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct DeleteCredentialInput {
    pub site_category: String,
}

// ─── Command structs ───────────────────────────────────────────────────────

struct GetFlows;
struct CreateFlow;
struct DeleteFlow;
struct RenameFlow;
struct SetFlowSchedule;
struct RunFlow;
struct StopFlow;
struct GetSites;
struct GetSiteMetadataSchema;
struct ValidateSiteMetadata;
struct GetSubscriptions;
struct CreateSubscription;
struct DeleteSubscription;
struct PauseSubscription;
struct AddSubscriptionQuery;
struct DeleteSubscriptionQuery;
struct PauseSubscriptionQuery;
struct RunSubscription;
struct StopSubscription;
struct ResetSubscription;
struct GetRunningSubscriptions;
struct GetRunningSubscriptionProgress;
struct RenameSubscription;
struct RunSubscriptionQuery;
struct ListCredentials;
struct ListCredentialHealth;
struct SetCredential;
struct DeleteCredential;

// ─── TypedCommand impls ────────────────────────────────────────────────────

impl TypedCommand for GetFlows {
    const NAME: &'static str = "get_flows";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::subscriptions::flow_controller::FlowController::get_flows(&state.db).await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for CreateFlow {
    const NAME: &'static str = "create_flow";
    type Input = CreateFlowInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let flow = crate::subscriptions::flow_controller::FlowController::create_flow(
            &state.db,
            input.name,
            input.schedule,
        )
        .await?;
        crate::events::emit_mutation(
            "create_flow",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(serde_json::to_value(&flow).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for DeleteFlow {
    const NAME: &'static str = "delete_flow";
    type Input = DeleteFlowInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::flow_controller::FlowController::delete_flow(
            &state.db,
            &state.blob_store,
            input.id,
            input.delete_files,
        )
        .await?;
        crate::events::emit_mutation(
            "delete_flow",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions)
                .domains(&[
                    crate::events::Domain::Subscriptions,
                    crate::events::Domain::Sidebar,
                    crate::events::Domain::Files,
                ])
                .selection_summary(),
        );
        Ok(())
    }
}

impl TypedCommand for RenameFlow {
    const NAME: &'static str = "rename_flow";
    type Input = RenameFlowInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::flow_controller::FlowController::rename_flow(&state.db, input.id, input.name)
            .await?;
        crate::events::emit_mutation(
            "rename_flow",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for SetFlowSchedule {
    const NAME: &'static str = "set_flow_schedule";
    type Input = SetFlowScheduleInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::flow_controller::FlowController::set_flow_schedule(
            &state.db,
            input.id,
            input.schedule,
        )
        .await?;
        crate::events::emit_mutation(
            "set_flow_schedule",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for RunFlow {
    const NAME: &'static str = "run_flow";
    type Input = RunFlowInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::flow_controller::FlowController::run_flow(
            &state.db,
            &state.blob_store,
            &state.rate_limiter,
            &state.running_subscriptions,
            &state.sub_terminal_statuses,
            input.id,
            &state.settings,
        )
        .await?;
        crate::events::emit_mutation(
            "run_flow",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for StopFlow {
    const NAME: &'static str = "stop_flow";
    type Input = StopFlowInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::flow_controller::FlowController::stop_flow(
            &state.db,
            &state.running_subscriptions,
            input.id,
        )
        .await?;
        crate::events::emit_mutation(
            "stop_flow",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for GetSites {
    const NAME: &'static str = "get_sites";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        Ok(serde_json::to_value(&crate::subscriptions::gallery_dl_runner::SITES).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for GetSiteMetadataSchema {
    const NAME: &'static str = "get_site_metadata_schema";
    type Input = GetSiteMetadataSchemaInput;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let schema = crate::subscriptions::gallery_dl_runner::get_site_metadata_schema(&input.site_id)
            .ok_or_else(|| format!("Unsupported site for metadata schema: {}", input.site_id))?;
        Ok(serde_json::to_value(&schema).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for ValidateSiteMetadata {
    const NAME: &'static str = "validate_site_metadata";
    type Input = ValidateSiteMetadataInput;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let sample_url = input.sample_url.unwrap_or_default();
        let result = crate::subscriptions::gallery_dl_runner::validate_site_metadata(
            &input.site_id,
            &sample_url,
            input.sample_metadata_json.as_ref(),
        );
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for GetSubscriptions {
    const NAME: &'static str = "get_subscriptions";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::subscriptions::controller::SubscriptionController::get_subscriptions(&state.db)
                .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for CreateSubscription {
    const NAME: &'static str = "create_subscription";
    type Input = CreateSubscriptionInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let sub =
            crate::subscriptions::controller::SubscriptionController::create_subscription(
                &state.db,
                input.name,
                input.site_id,
                input.queries,
                input.flow_id,
                input.initial_file_limit,
                input.periodic_file_limit,
            )
            .await?;
        crate::events::emit_mutation(
            "create_subscription",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(serde_json::to_value(&sub).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for DeleteSubscription {
    const NAME: &'static str = "delete_subscription";
    type Input = DeleteSubscriptionInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let count =
            crate::subscriptions::controller::SubscriptionController::delete_subscription(
                &state.db,
                &state.blob_store,
                input.id,
                input.delete_files,
            )
            .await?;
        crate::events::emit_mutation(
            "delete_subscription",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions)
                .domains(&[
                    crate::events::Domain::Subscriptions,
                    crate::events::Domain::Sidebar,
                    crate::events::Domain::Files,
                ])
                .selection_summary(),
        );
        Ok(serde_json::to_value(&count).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for PauseSubscription {
    const NAME: &'static str = "pause_subscription";
    type Input = PauseSubscriptionInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::controller::SubscriptionController::pause_subscription(
            &state.db,
            input.id,
            input.paused,
        )
        .await?;
        crate::events::emit_mutation(
            "pause_subscription",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for AddSubscriptionQuery {
    const NAME: &'static str = "add_subscription_query";
    type Input = AddSubscriptionQueryInput;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let query =
            crate::subscriptions::controller::SubscriptionController::add_subscription_query(
                &state.db,
                input.subscription_id,
                input.query_text,
            )
            .await?;
        crate::events::emit_mutation(
            "add_subscription_query",
            crate::events::MutationImpact::domain_only(crate::events::Domain::Subscriptions),
        );
        Ok(serde_json::to_value(&query).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for DeleteSubscriptionQuery {
    const NAME: &'static str = "delete_subscription_query";
    type Input = DeleteSubscriptionQueryInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::controller::SubscriptionController::delete_subscription_query(
            &state.db,
            input.id,
        )
        .await?;
        crate::events::emit_mutation(
            "delete_subscription_query",
            crate::events::MutationImpact::domain_only(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for PauseSubscriptionQuery {
    const NAME: &'static str = "pause_subscription_query";
    type Input = PauseSubscriptionQueryInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::controller::SubscriptionController::pause_subscription_query(
            &state.db,
            input.id,
            input.paused,
        )
        .await?;
        crate::events::emit_mutation(
            "pause_subscription_query",
            crate::events::MutationImpact::domain_only(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for RunSubscription {
    const NAME: &'static str = "run_subscription";
    type Input = RunSubscriptionInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        // PBI-043: Fire-and-forget — return null, not fake results.
        crate::subscriptions::controller::SubscriptionController::run_subscription(
            &state.db,
            &state.blob_store,
            &state.rate_limiter,
            &state.running_subscriptions,
            input.id,
            Some(state.sub_terminal_statuses.clone()),
            &state.settings,
        )
        .await?;
        Ok(())
    }
}

impl TypedCommand for StopSubscription {
    const NAME: &'static str = "stop_subscription";
    type Input = StopSubscriptionInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::controller::SubscriptionController::stop_subscription(
            &state.db,
            &state.running_subscriptions,
            input.id,
        )
        .await?;
        Ok(())
    }
}

impl TypedCommand for ResetSubscription {
    const NAME: &'static str = "reset_subscription";
    type Input = ResetSubscriptionInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::controller::SubscriptionController::reset_subscription_checked(
            &state.db,
            &state.running_subscriptions,
            input.id,
        )
        .await?;
        crate::events::emit_mutation(
            "reset_subscription",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for GetRunningSubscriptions {
    const NAME: &'static str = "get_running_subscriptions";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result =
            crate::subscriptions::controller::SubscriptionController::get_running_subscriptions(
                &state.running_subscriptions,
            )
            .await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for GetRunningSubscriptionProgress {
    const NAME: &'static str = "get_running_subscription_progress";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(_state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = crate::subscriptions::controller::SubscriptionController::get_running_subscription_progress();
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for RenameSubscription {
    const NAME: &'static str = "rename_subscription";
    type Input = RenameSubscriptionInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        crate::subscriptions::controller::SubscriptionController::rename_subscription(
            &state.db,
            input.id,
            input.name,
        )
        .await?;
        crate::events::emit_mutation(
            "rename_subscription",
            crate::events::MutationImpact::sidebar(crate::events::Domain::Subscriptions),
        );
        Ok(())
    }
}

impl TypedCommand for RunSubscriptionQuery {
    const NAME: &'static str = "run_subscription_query";
    type Input = RunSubscriptionQueryInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        // PBI-043: Fire-and-forget — return null, not fake results.
        crate::subscriptions::controller::SubscriptionController::run_subscription_query(
            &state.db,
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
}

impl TypedCommand for ListCredentials {
    const NAME: &'static str = "list_credentials";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = state.db.list_credential_domains().await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for ListCredentialHealth {
    const NAME: &'static str = "list_credential_health";
    type Input = serde_json::Value;
    type Output = serde_json::Value;

    async fn execute(state: &AppState, _input: Self::Input) -> Result<Self::Output, String> {
        let result = state.db.list_credential_health().await?;
        Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
    }
}

impl TypedCommand for SetCredential {
    const NAME: &'static str = "set_credential";
    type Input = SetCredentialInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let site_category =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(input.site_category.trim()).to_string();

        let cred_type =
            match crate::credential_store::CredentialType::from_str(&input.credential_type) {
                Some(ct) => ct,
                None => {
                    return Err(format!(
                        "Invalid credential_type: {}",
                        input.credential_type
                    ))
                }
            };

        if site_category == "rule34" {
            if cred_type != crate::credential_store::CredentialType::ApiKey {
                return Err(
                    "rule34.xxx requires `api_key` credentials (user-id + api-key)".to_string(),
                );
            }
            let user_id_ok = input
                .username
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty());
            let api_key_ok = input
                .password
                .as_deref()
                .map(str::trim)
                .is_some_and(|v| !v.is_empty());
            if !user_id_ok || !api_key_ok {
                return Err(
                    "rule34.xxx requires both `user-id` and `api-key` (use username=user-id, password=api-key)"
                        .to_string(),
                );
            }
        }

        let cred = crate::credential_store::SiteCredential {
            site_category: site_category.clone(),
            credential_type: cred_type,
            username: input.username,
            password: input.password,
            cookies: input.cookies,
            oauth_token: input.oauth_token,
        };

        crate::credential_store::set_credential(&cred)?;

        state
            .db
            .upsert_credential_domain(
                &site_category,
                &input.credential_type,
                input.display_name.as_deref(),
            )
            .await?;

        let _ = state
            .db
            .upsert_credential_health(&site_category, "unknown", None)
            .await;

        Ok(())
    }
}

impl TypedCommand for DeleteCredential {
    const NAME: &'static str = "delete_credential";
    type Input = DeleteCredentialInput;
    type Output = ();

    async fn execute(state: &AppState, input: Self::Input) -> Result<Self::Output, String> {
        let canonical =
            crate::subscriptions::gallery_dl_runner::canonical_site_id(input.site_category.trim()).to_string();
        let mut categories = vec![input.site_category.clone(), canonical.clone()];
        if canonical == "rule34" {
            categories.push("rule34xxx".to_string());
            categories.push("rule34.xxx".to_string());
        }
        categories.sort();
        categories.dedup();

        for category in categories {
            let _ = crate::credential_store::delete_credential(&category);
            let _ = state.db.delete_credential_domain(&category).await;
            let _ = state.db.delete_credential_health(&category).await;
        }
        Ok(())
    }
}

// ─── Dispatch router ───────────────────────────────────────────────────────

pub async fn dispatch_typed(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        GetFlows::NAME => Some(run_typed::<GetFlows>(state, args).await),
        CreateFlow::NAME => Some(run_typed::<CreateFlow>(state, args).await),
        DeleteFlow::NAME => Some(run_typed::<DeleteFlow>(state, args).await),
        RenameFlow::NAME => Some(run_typed::<RenameFlow>(state, args).await),
        SetFlowSchedule::NAME => Some(run_typed::<SetFlowSchedule>(state, args).await),
        RunFlow::NAME => Some(run_typed::<RunFlow>(state, args).await),
        StopFlow::NAME => Some(run_typed::<StopFlow>(state, args).await),
        GetSites::NAME => Some(run_typed::<GetSites>(state, args).await),
        GetSiteMetadataSchema::NAME => Some(run_typed::<GetSiteMetadataSchema>(state, args).await),
        ValidateSiteMetadata::NAME => Some(run_typed::<ValidateSiteMetadata>(state, args).await),
        GetSubscriptions::NAME => Some(run_typed::<GetSubscriptions>(state, args).await),
        CreateSubscription::NAME => Some(run_typed::<CreateSubscription>(state, args).await),
        DeleteSubscription::NAME => Some(run_typed::<DeleteSubscription>(state, args).await),
        PauseSubscription::NAME => Some(run_typed::<PauseSubscription>(state, args).await),
        AddSubscriptionQuery::NAME => Some(run_typed::<AddSubscriptionQuery>(state, args).await),
        DeleteSubscriptionQuery::NAME => {
            Some(run_typed::<DeleteSubscriptionQuery>(state, args).await)
        }
        PauseSubscriptionQuery::NAME => {
            Some(run_typed::<PauseSubscriptionQuery>(state, args).await)
        }
        RunSubscription::NAME => Some(run_typed::<RunSubscription>(state, args).await),
        StopSubscription::NAME => Some(run_typed::<StopSubscription>(state, args).await),
        ResetSubscription::NAME => Some(run_typed::<ResetSubscription>(state, args).await),
        GetRunningSubscriptions::NAME => {
            Some(run_typed::<GetRunningSubscriptions>(state, args).await)
        }
        GetRunningSubscriptionProgress::NAME => {
            Some(run_typed::<GetRunningSubscriptionProgress>(state, args).await)
        }
        RenameSubscription::NAME => Some(run_typed::<RenameSubscription>(state, args).await),
        RunSubscriptionQuery::NAME => Some(run_typed::<RunSubscriptionQuery>(state, args).await),
        ListCredentials::NAME => Some(run_typed::<ListCredentials>(state, args).await),
        ListCredentialHealth::NAME => Some(run_typed::<ListCredentialHealth>(state, args).await),
        SetCredential::NAME => Some(run_typed::<SetCredential>(state, args).await),
        DeleteCredential::NAME => Some(run_typed::<DeleteCredential>(state, args).await),
        _ => None,
    }
}
