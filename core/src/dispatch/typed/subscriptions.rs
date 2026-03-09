//! Handler functions for subscription and group operations.

use serde::Deserialize;
use ts_rs::TS;

use crate::state::AppState;

// ─── Input structs ─────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct CreateGroupInput {
    pub name: String,
    pub schedule: Option<String>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteGroupInput {
    pub id: String,
    pub delete_files: Option<bool>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct RenameGroupInput {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct SetGroupScheduleInput {
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
    pub site_id: String,
    pub queries: Vec<String>,
    #[ts(type = "number | null")]
    pub group_id: Option<i64>,
    #[ts(type = "number | null")]
    pub initial_file_limit: Option<u32>,
    #[ts(type = "number | null")]
    pub periodic_file_limit: Option<u32>,
}

#[derive(Debug, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct DeleteSubscriptionInput {
    pub id: String,
    pub delete_files: Option<bool>,
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
    pub query_text: String,
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

pub async fn get_groups(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = crate::subscriptions::subscription_group_controller::SubscriptionGroupController::get_groups(&state.db).await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn create_group(state: &AppState, input: CreateGroupInput) -> Result<serde_json::Value, String> {
    let group = crate::subscriptions::subscription_group_controller::SubscriptionGroupController::create_group(
        &state.db, input.name, input.schedule,
    ).await?;
    crate::events::emit_mutation(
        "create_group",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(serde_json::to_value(&group).map_err(|e| e.to_string())?)
}

pub async fn delete_group(state: &AppState, input: DeleteGroupInput) -> Result<(), String> {
    let delete_files = input.delete_files.unwrap_or(false);
    crate::subscriptions::subscription_group_controller::SubscriptionGroupController::delete_group(
        &state.db, &state.blob_store, input.id, input.delete_files,
    ).await?;
    crate::events::emit_mutation(
        "delete_group",
        if delete_files {
            crate::events::MutationImpact::subscriptions_file_status_change(&state.db)
        } else {
            crate::events::MutationImpact::subscriptions_sidebar()
        },
    );
    Ok(())
}

pub async fn rename_group(state: &AppState, input: RenameGroupInput) -> Result<(), String> {
    crate::subscriptions::subscription_group_controller::SubscriptionGroupController::rename_group(&state.db, input.id, input.name).await?;
    crate::events::emit_mutation(
        "rename_group",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn set_group_schedule(state: &AppState, input: SetGroupScheduleInput) -> Result<(), String> {
    crate::subscriptions::subscription_group_controller::SubscriptionGroupController::set_group_schedule(
        &state.db, input.id, input.schedule,
    ).await?;
    crate::events::emit_mutation(
        "set_group_schedule",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn run_group(state: &AppState, input: RunGroupInput) -> Result<(), String> {
    crate::subscriptions::subscription_group_controller::SubscriptionGroupController::run_group(
        &state.db,
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        &state.sub_terminal_statuses,
        input.id,
        &state.settings,
    ).await?;
    crate::events::emit_mutation(
        "run_group",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn stop_group(state: &AppState, input: StopGroupInput) -> Result<(), String> {
    crate::subscriptions::subscription_group_controller::SubscriptionGroupController::stop_group(
        &state.db, &state.running_subscriptions, input.id,
    ).await?;
    crate::events::emit_mutation(
        "stop_group",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn get_sites(_state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    Ok(serde_json::to_value(&crate::subscriptions::gallery_dl_runner::SITES).map_err(|e| e.to_string())?)
}

pub async fn get_site_metadata_schema(_state: &AppState, input: GetSiteMetadataSchemaInput) -> Result<serde_json::Value, String> {
    let schema = crate::subscriptions::gallery_dl_runner::get_site_metadata_schema(&input.site_id)
        .ok_or_else(|| format!("Unsupported site for metadata schema: {}", input.site_id))?;
    Ok(serde_json::to_value(&schema).map_err(|e| e.to_string())?)
}

pub async fn validate_site_metadata(_state: &AppState, input: ValidateSiteMetadataInput) -> Result<serde_json::Value, String> {
    let sample_url = input.sample_url.unwrap_or_default();
    let result = crate::subscriptions::gallery_dl_runner::validate_site_metadata(
        &input.site_id, &sample_url, input.sample_metadata_json.as_ref(),
    );
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn get_subscriptions(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = crate::subscriptions::controller::SubscriptionController::get_subscriptions(&state.db).await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn create_subscription(state: &AppState, input: CreateSubscriptionInput) -> Result<serde_json::Value, String> {
    let sub = crate::subscriptions::controller::SubscriptionController::create_subscription(
        &state.db,
        input.name,
        input.site_id,
        input.queries,
        input.group_id,
        input.initial_file_limit,
        input.periodic_file_limit,
    ).await?;
    crate::events::emit_mutation(
        "create_subscription",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(serde_json::to_value(&sub).map_err(|e| e.to_string())?)
}

pub async fn delete_subscription(state: &AppState, input: DeleteSubscriptionInput) -> Result<serde_json::Value, String> {
    let delete_files = input.delete_files.unwrap_or(false);
    let count = crate::subscriptions::controller::SubscriptionController::delete_subscription(
        &state.db, &state.blob_store, input.id, input.delete_files,
    ).await?;
    crate::events::emit_mutation(
        "delete_subscription",
        if delete_files {
            crate::events::MutationImpact::subscriptions_file_status_change(&state.db)
        } else {
            crate::events::MutationImpact::subscriptions_sidebar()
        },
    );
    Ok(serde_json::to_value(&count).map_err(|e| e.to_string())?)
}

pub async fn pause_subscription(state: &AppState, input: PauseSubscriptionInput) -> Result<(), String> {
    crate::subscriptions::controller::SubscriptionController::pause_subscription(
        &state.db, input.id, input.paused,
    ).await?;
    crate::events::emit_mutation(
        "pause_subscription",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn add_subscription_query(state: &AppState, input: AddSubscriptionQueryInput) -> Result<serde_json::Value, String> {
    let query = crate::subscriptions::controller::SubscriptionController::add_subscription_query(
        &state.db, input.subscription_id, input.query_text,
    ).await?;
    crate::events::emit_mutation(
        "add_subscription_query",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(serde_json::to_value(&query).map_err(|e| e.to_string())?)
}

pub async fn delete_subscription_query(state: &AppState, input: DeleteSubscriptionQueryInput) -> Result<(), String> {
    crate::subscriptions::controller::SubscriptionController::delete_subscription_query(
        &state.db, input.id,
    ).await?;
    crate::events::emit_mutation(
        "delete_subscription_query",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn pause_subscription_query(state: &AppState, input: PauseSubscriptionQueryInput) -> Result<(), String> {
    crate::subscriptions::controller::SubscriptionController::pause_subscription_query(
        &state.db, input.id, input.paused,
    ).await?;
    crate::events::emit_mutation(
        "pause_subscription_query",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn run_subscription(state: &AppState, input: RunSubscriptionInput) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::run_subscription(
        &state.db,
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        input.id,
        Some(state.sub_terminal_statuses.clone()),
        &state.settings,
    ).await?;
    Ok(())
}

pub async fn stop_subscription(state: &AppState, input: StopSubscriptionInput) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::stop_subscription(
        &state.db, &state.running_subscriptions, input.id,
    ).await?;
    Ok(())
}

pub async fn reset_subscription(state: &AppState, input: ResetSubscriptionInput) -> Result<(), String> {
    crate::subscriptions::controller::SubscriptionController::reset_subscription_checked(
        &state.db, &state.running_subscriptions, input.id,
    ).await?;
    crate::events::emit_mutation(
        "reset_subscription",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn get_running_subscriptions(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::get_running_subscriptions(
        &state.running_subscriptions,
    ).await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn get_running_subscription_progress(_state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::get_running_subscription_progress();
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn rename_subscription(state: &AppState, input: RenameSubscriptionInput) -> Result<(), String> {
    crate::subscriptions::controller::SubscriptionController::rename_subscription(
        &state.db, input.id, input.name,
    ).await?;
    crate::events::emit_mutation(
        "rename_subscription",
        crate::events::MutationImpact::subscriptions_sidebar(),
    );
    Ok(())
}

pub async fn run_subscription_query(state: &AppState, input: RunSubscriptionQueryInput) -> Result<(), String> {
    crate::subscriptions::run_orchestrator::SubscriptionRunOrchestrator::run_subscription_query(
        &state.db,
        &state.blob_store,
        &state.rate_limiter,
        &state.running_subscriptions,
        input.subscription_id,
        input.query_id,
        &state.settings,
    ).await?;
    Ok(())
}

pub async fn list_credentials(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = state.db.list_credential_domains().await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn list_credential_health(state: &AppState, _input: serde_json::Value) -> Result<serde_json::Value, String> {
    let result = state.db.list_credential_health().await?;
    Ok(serde_json::to_value(&result).map_err(|e| e.to_string())?)
}

pub async fn set_credential(state: &AppState, input: SetCredentialInput) -> Result<(), String> {
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

pub async fn delete_credential(state: &AppState, input: DeleteCredentialInput) -> Result<(), String> {
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
