//! Subscription and flow domain handlers.

use crate::state::AppState;

use super::common::{de, de_opt, ok_null, to_json};

pub async fn handle(
    state: &AppState,
    command: &str,
    args: &serde_json::Value,
) -> Option<Result<String, String>> {
    match command {
        "get_flows" => {
            let result = crate::flow_controller::FlowController::get_flows(&state.db).await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "create_flow" => {
            let name: String = match de(args, "name") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let schedule: Option<String> = de_opt(args, "schedule");
            let result =
                crate::flow_controller::FlowController::create_flow(&state.db, name, schedule)
                    .await;
            match result {
                Ok(flow) => {
                    crate::events::emit_mutation(
                        "create_flow",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(to_json(&flow))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_flow" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let delete_files: Option<bool> = de_opt(args, "delete_files");
            let result = crate::flow_controller::FlowController::delete_flow(
                &state.db,
                &state.blob_store,
                id,
                delete_files,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "delete_flow",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        )
                        .domains(&[
                            crate::events::Domain::Subscriptions,
                            crate::events::Domain::Sidebar,
                            crate::events::Domain::Files,
                        ])
                        .selection_summary(),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "rename_flow" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let name: String = match de(args, "name") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::flow_controller::FlowController::rename_flow(&state.db, id, name).await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "rename_flow",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "set_flow_schedule" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let schedule: String = match de(args, "schedule") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::flow_controller::FlowController::set_flow_schedule(&state.db, id, schedule)
                    .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "set_flow_schedule",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "run_flow" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::flow_controller::FlowController::run_flow(
                &state.db,
                &state.blob_store,
                &state.rate_limiter,
                &state.running_subscriptions,
                &state.sub_terminal_statuses,
                id,
                &state.settings,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "run_flow",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "stop_flow" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::flow_controller::FlowController::stop_flow(
                &state.db,
                &state.running_subscriptions,
                id,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "stop_flow",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }

        "get_sites" => Some(to_json(&crate::gallery_dl_runner::SITES)),
        "get_site_metadata_schema" => {
            let site_id: String = match de(args, "site_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let schema = crate::gallery_dl_runner::get_site_metadata_schema(&site_id)
                .ok_or_else(|| format!("Unsupported site for metadata schema: {site_id}"));
            Some(schema.and_then(|s| to_json(&s)))
        }
        "validate_site_metadata" => {
            let site_id: String = match de(args, "site_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let sample_url: String = de_opt(args, "sample_url").unwrap_or_default();
            let sample_metadata_json: Option<serde_json::Value> =
                de_opt(args, "sample_metadata_json");
            let result = crate::gallery_dl_runner::validate_site_metadata(
                &site_id,
                &sample_url,
                sample_metadata_json.as_ref(),
            );
            Some(to_json(&result))
        }

        "get_subscriptions" => {
            let result = crate::subscription_controller::SubscriptionController::get_subscriptions(
                &state.db,
            )
            .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "create_subscription" => {
            let name: String = match de(args, "name") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let site_id: String = match de(args, "site_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let queries: Vec<String> = match de(args, "queries") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let flow_id: Option<i64> = de_opt(args, "flow_id");
            let initial_file_limit: Option<u32> = de_opt(args, "initial_file_limit");
            let periodic_file_limit: Option<u32> = de_opt(args, "periodic_file_limit");
            let result =
                crate::subscription_controller::SubscriptionController::create_subscription(
                    &state.db,
                    name,
                    site_id,
                    queries,
                    flow_id,
                    initial_file_limit,
                    periodic_file_limit,
                )
                .await;
            match result {
                Ok(sub) => {
                    crate::events::emit_mutation(
                        "create_subscription",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(to_json(&sub))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_subscription" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let delete_files: Option<bool> = de_opt(args, "delete_files");
            let result =
                crate::subscription_controller::SubscriptionController::delete_subscription(
                    &state.db,
                    &state.blob_store,
                    id,
                    delete_files,
                )
                .await;
            match result {
                Ok(count) => {
                    crate::events::emit_mutation(
                        "delete_subscription",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        )
                        .domains(&[
                            crate::events::Domain::Subscriptions,
                            crate::events::Domain::Sidebar,
                            crate::events::Domain::Files,
                        ])
                        .selection_summary(),
                    );
                    Some(to_json(&count))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "pause_subscription" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let paused: bool = match de(args, "paused") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::subscription_controller::SubscriptionController::pause_subscription(
                    &state.db, id, paused,
                )
                .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "pause_subscription",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "add_subscription_query" => {
            let subscription_id: String = match de(args, "subscription_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let query_text: String = match de(args, "query_text") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::subscription_controller::SubscriptionController::add_subscription_query(
                    &state.db,
                    subscription_id,
                    query_text,
                )
                .await;
            match result {
                Ok(query) => {
                    crate::events::emit_mutation(
                        "add_subscription_query",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Subscriptions]),
                    );
                    Some(to_json(&query))
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_subscription_query" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::subscription_controller::SubscriptionController::delete_subscription_query(
                    &state.db, id,
                )
                .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "delete_subscription_query",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Subscriptions]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "pause_subscription_query" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let paused: bool = match de(args, "paused") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::subscription_controller::SubscriptionController::pause_subscription_query(
                    &state.db, id, paused,
                )
                .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "pause_subscription_query",
                        crate::events::MutationImpact::new()
                            .domains(&[crate::events::Domain::Subscriptions]),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "run_subscription" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::subscription_controller::SubscriptionController::run_subscription(
                &state.db,
                &state.blob_store,
                &state.rate_limiter,
                &state.running_subscriptions,
                id,
                Some(state.sub_terminal_statuses.clone()),
                &state.settings,
            )
            .await;
            match result {
                // PBI-043: Fire-and-forget — return null, not fake results.
                Ok(()) => Some(ok_null()),
                Err(e) => Some(Err(e)),
            }
        }
        "stop_subscription" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::subscription_controller::SubscriptionController::stop_subscription(
                &state.db,
                &state.running_subscriptions,
                id,
            )
            .await;
            // The subscription task emits subscription-finished when it
            // actually exits; eventBridge handles sidebar/grid refresh from
            // that event. No premature state-changed needed here.
            match result {
                Ok(()) => Some(ok_null()),
                Err(e) => Some(Err(e)),
            }
        }
        "reset_subscription" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result = crate::subscription_controller::SubscriptionController::reset_subscription_checked(
                &state.db,
                &state.running_subscriptions,
                id,
            )
            .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "reset_subscription",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "get_running_subscriptions" => {
            let result =
                crate::subscription_controller::SubscriptionController::get_running_subscriptions(
                    &state.running_subscriptions,
                )
                .await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "get_running_subscription_progress" => Some(to_json(
            &crate::subscription_controller::SubscriptionController::get_running_subscription_progress(),
        )),
        "rename_subscription" => {
            let id: String = match de(args, "id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let name: String = match de(args, "name") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::subscription_controller::SubscriptionController::rename_subscription(
                    &state.db, id, name,
                )
                .await;
            match result {
                Ok(()) => {
                    crate::events::emit_mutation(
                        "rename_subscription",
                        crate::events::MutationImpact::sidebar(
                            crate::events::Domain::Subscriptions,
                        ),
                    );
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "run_subscription_query" => {
            let subscription_id: String = match de(args, "subscription_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let query_id: String = match de(args, "query_id") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let result =
                crate::subscription_controller::SubscriptionController::run_subscription_query(
                    &state.db,
                    &state.blob_store,
                    &state.rate_limiter,
                    &state.running_subscriptions,
                    subscription_id,
                    query_id,
                    &state.settings,
                )
                .await;
            match result {
                // PBI-043: Fire-and-forget — return null, not fake results.
                Ok(()) => Some(ok_null()),
                Err(e) => Some(Err(e)),
            }
        }
        "list_credentials" => {
            let result = state.db.list_credential_domains().await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "list_credential_health" => {
            let result = state.db.list_credential_health().await;
            Some(result.and_then(|r| to_json(&r)))
        }
        "set_credential" => {
            let raw_site_category: String = match de(args, "site_category") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let site_category =
                crate::gallery_dl_runner::canonical_site_id(raw_site_category.trim()).to_string();
            let credential_type: String = match de(args, "credential_type") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let display_name: Option<String> = de_opt(args, "display_name");
            let username: Option<String> = de_opt(args, "username");
            let password: Option<String> = de_opt(args, "password");
            let cookies: Option<std::collections::HashMap<String, String>> =
                de_opt(args, "cookies");
            let oauth_token: Option<String> = de_opt(args, "oauth_token");

            let cred_type =
                match crate::credential_store::CredentialType::from_str(&credential_type) {
                    Some(ct) => ct,
                    None => {
                        return Some(Err(format!("Invalid credential_type: {credential_type}")))
                    }
                };

            if site_category == "rule34" {
                if cred_type != crate::credential_store::CredentialType::ApiKey {
                    return Some(Err(
                        "rule34.xxx requires `api_key` credentials (user-id + api-key)".to_string(),
                    ));
                }
                let user_id_ok = username
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|v| !v.is_empty());
                let api_key_ok = password
                    .as_deref()
                    .map(str::trim)
                    .is_some_and(|v| !v.is_empty());
                if !user_id_ok || !api_key_ok {
                    return Some(Err(
                        "rule34.xxx requires both `user-id` and `api-key` (use username=user-id, password=api-key)"
                            .to_string(),
                    ));
                }
            }

            let cred = crate::credential_store::SiteCredential {
                site_category: site_category.clone(),
                credential_type: cred_type,
                username,
                password,
                cookies,
                oauth_token,
            };

            if let Err(e) = crate::credential_store::set_credential(&cred) {
                return Some(Err(e));
            }

            let result = state
                .db
                .upsert_credential_domain(&site_category, &credential_type, display_name.as_deref())
                .await;
            match result {
                Ok(()) => {
                    let _ = state
                        .db
                        .upsert_credential_health(&site_category, "unknown", None)
                        .await;
                    Some(ok_null())
                }
                Err(e) => Some(Err(e)),
            }
        }
        "delete_credential" => {
            let site_category: String = match de(args, "site_category") {
                Ok(v) => v,
                Err(e) => return Some(Err(e)),
            };
            let canonical =
                crate::gallery_dl_runner::canonical_site_id(site_category.trim()).to_string();
            let mut categories = vec![site_category.clone(), canonical.clone()];
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
            Some(ok_null())
        }
        _ => None,
    }
}
