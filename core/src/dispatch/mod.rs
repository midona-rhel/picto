//! Command dispatcher — routes command names to domain handler functions.
//!
//! The napi-rs addon calls `dispatch("command_name", "{...args}")` and
//! gets back a JSON string result.

pub mod common;
pub mod typed;

pub use common::{from_args, ok_null, to_json};

// Transport-layer input types for engine-routed commands.
// These are pure deserialization targets — no behavior.
#[derive(serde::Deserialize)]
struct GetHashInput {
    entity_hash: String,
}

#[derive(serde::Deserialize)]
struct GetHashesInput {
    entity_hashes: Vec<String>,
}

#[derive(serde::Deserialize)]
struct PatchEntitiesInput {
    target: crate::db::types::EntityTarget,
    patch: crate::db::types::MediaEntityPatch,
}

#[derive(serde::Deserialize)]
struct ApplyTagsInput {
    target: crate::db::types::EntityTarget,
    operation: crate::engine::tags::TagOperation,
    tags: Vec<String>,
    provenance_mask: Option<String>,
}

#[derive(serde::Deserialize)]
struct SetTagSiteMaskInput {
    tag_id: i64,
    site_mask: String,
}

#[derive(serde::Deserialize)]
struct FolderMembershipInput {
    target: crate::db::types::EntityTarget,
    folder_id: i64,
    operation: crate::engine::folders::MembershipOperation,
}

#[derive(serde::Deserialize)]
struct SetStatusInput {
    target: crate::db::types::EntityTarget,
    status: i64,
}

#[derive(serde::Deserialize)]
struct DeleteEntitiesInput {
    target: crate::db::types::EntityTarget,
}

#[derive(serde::Deserialize)]
struct SelectionSummaryInput {
    target: crate::db::types::EntityTarget,
}

/// Deserialize args, call a handler function, serialize its output.
macro_rules! call {
    ($func:path, $state:expr, $args:expr) => {{
        let input = serde_json::from_value($args).map_err(|e| format!("Invalid args: {e}"))?;
        let output = $func($state, input).await?;
        to_json(&output)
    }};
}

/// Commands that mutate state. Logged at `info!` level; everything else is `debug!`.
const WRITE_COMMANDS: &[&str] = &[
    "add_media",
    "record_media_view",
    "set_entity_status",
    "delete_entities",
    "manage_tag_alias",
    "manage_tag_implication",
    "merge_tags",
    "rename_tag",
    "delete_tag",
    "set_tag_site_mask",
    "scan_duplicates",
    "resolve_duplicate_pair",
    "create_smart_folder",
    "update_smart_folder",
    "delete_smart_folder",
    "move_smart_folder",
    "create_folder",
    "update_folder",
    "delete_folder",
    "move_folder",
    "remove_entities_from_folder",
    "reorder_folder_items",
    "reorder_folder_members",
    "set_folder_watch_config",
    "clear_folder_watch_config",
    "create_collection",
    "split_collection",
    "add_collection_members",
    "remove_collection_members",
    "reorder_collection_members",
    "save_settings",
    "sync_create_remote_library",
    "sync_connect_remote_library",
    "sync_disconnect",
    "sync_now",
    "reorder_sidebar_nodes",
    "pin_sidebar_item",
    "unpin_sidebar_item",
    "reorder_pinned_items",
    "set_view_prefs",
    "set_zoom_factor",
    "create_group",
    "delete_group",
    "rename_group",
    "set_subscription_schedule",
    "run_group",
    "stop_group",
    "create_subscription",
    "delete_subscription",
    "pause_subscription",
    "add_subscription_query",
    "delete_subscription_query",
    "edit_subscription_query",
    "pause_subscription_query",
    "set_subscription_auto_collections",
    "run_subscription",
    "stop_subscription",
    "reset_subscription",
    "reset_subscription_query",
    "rename_subscription",
    "run_subscription_query",
    "stop_subscription_query",
    "retry_subscription_failed_post",
    "retry_subscription_failed_posts",
    "set_credential",
    "delete_credential",
    "pixiv_oauth_start",
    "pixiv_oauth_exchange",
    "export_media",
    "regenerate_thumbnails_batch",
    "ai_tag_apply",
    "ai_tagger_download_model",
];

/// Dispatch a command by name with JSON arguments. Returns JSON result.
pub async fn dispatch(command: &str, args_json: &str) -> Result<String, String> {
    let start = std::time::Instant::now();
    let args: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("Invalid JSON args: {}", e))?;

    let result = dispatch_inner(command, args).await;
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    let is_write = WRITE_COMMANDS.contains(&command);

    match &result {
        Ok(_) if is_write => tracing::info!(command, elapsed_ms, "dispatch ok"),
        Ok(_) => tracing::debug!(command, elapsed_ms, "dispatch ok"),
        Err(e) => tracing::warn!(command, elapsed_ms, error = %e, "dispatch error"),
    }

    result
}

async fn dispatch_inner(command: &str, args: serde_json::Value) -> Result<String, String> {
    let state = crate::state::get_state()?;

    // Commands whose transport shape is already the domain request type.
    match command {
        "query_entity_view" => {
            let query: crate::db::types::EntityViewQuery = from_args(args)?;
            let result = state.engine.query_entity_view(query)?;
            return to_json(&result);
        }
        "reconcile_entity_view" => {
            let req: crate::db::types::EntityViewReconcileRequest = from_args(args)?;
            let result = state.engine.reconcile_entity_view(req)?;
            return to_json(&result);
        }
        "get_entity_details" => {
            let input: GetHashInput = from_args(args)?;
            let result = state.engine.get_entity_details(&input.entity_hash)?;
            return to_json(&result);
        }
        "record_media_view" => {
            let input: GetHashInput = from_args(args)?;
            state.engine.record_media_view(&input.entity_hash)?;
            return ok_null();
        }
        "get_entity_grid_items" => {
            let input: GetHashesInput = from_args(args)?;
            let result = state.engine.get_entity_grid_items(&input.entity_hashes)?;
            return to_json(&result);
        }
        "patch_media_entities" => {
            let input: PatchEntitiesInput = from_args(args)?;
            let result = state
                .engine
                .patch_media_entities(input.target, input.patch)?;
            return to_json(&result);
        }
        "apply_entity_tags" => {
            let input: ApplyTagsInput = from_args(args)?;
            let provenance_mask = input
                .provenance_mask
                .as_deref()
                .map(crate::db::types::parse_mask_decimal)
                .transpose()?;
            let result = state.engine.apply_entity_tags(
                input.target,
                input.operation,
                &input.tags,
                provenance_mask,
            )?;
            return to_json(&result);
        }
        "set_tag_site_mask" => {
            let input: SetTagSiteMaskInput = from_args(args)?;
            let site_mask = crate::db::types::parse_mask_decimal(&input.site_mask)?;
            state.engine.set_tag_site_mask(input.tag_id, site_mask)?;
            return ok_null();
        }
        "update_folder_membership" => {
            let input: FolderMembershipInput = from_args(args)?;
            let result = state.engine.update_folder_membership(
                input.target,
                input.folder_id,
                input.operation,
            )?;
            return to_json(&result);
        }
        "set_entity_status" => {
            let input: SetStatusInput = from_args(args)?;
            let result = state.engine.set_entity_status(input.target, input.status)?;
            return to_json(&result);
        }
        "delete_entities" => {
            let input: DeleteEntitiesInput = from_args(args)?;
            let result = state.engine.delete_entities(input.target)?;
            // Transaction is committed — reclaim blobs whose last reference died.
            for hash in &result.freed_file_hashes {
                let _ = state.blob_store.delete(hash);
            }
            return to_json(&result);
        }
        "get_selection_summary" => {
            let input: SelectionSummaryInput = from_args(args)?;
            let result = state.engine.get_selection_summary(input.target).await?;
            return to_json(&result);
        }
        _ => {}
    }

    // Typed domain commands with transport-specific input DTOs.
    match command {
        // ── Tags ──────────────────────────────────────────────
        "search_tags" => call!(typed::tags::search_tags, &state, args),
        "manage_tag_alias" => call!(typed::tags::manage_tag_alias, &state, args),
        "get_tag_relations" => call!(typed::tags::get_tag_relations, &state, args),
        "manage_tag_implication" => call!(typed::tags::manage_tag_implication, &state, args),
        "merge_tags" => call!(typed::tags::merge_tags, &state, args),
        "get_tags_paginated" => call!(typed::tags::get_tags_paginated, &state, args),
        "get_namespace_summary" => call!(typed::tags::get_namespace_summary, &state, args),
        "rename_tag" => call!(typed::tags::rename_tag, &state, args),
        "delete_tag" => call!(typed::tags::delete_tag, &state, args),

        // ── Duplicates ────────────────────────────────────────
        "find_similar" => call!(typed::duplicates::find_similar, &state, args),
        "scan_duplicates" => call!(typed::duplicates::scan_duplicates, &state, args),
        "get_duplicate_pairs" => call!(typed::duplicates::get_duplicate_pairs, &state, args),
        "resolve_duplicate_pair" => call!(typed::duplicates::resolve_duplicate_pair, &state, args),

        // ── Smart Folders ─────────────────────────────────────
        "create_smart_folder" => call!(typed::smart_folders::create_smart_folder, &state, args),
        "update_smart_folder" => call!(typed::smart_folders::update_smart_folder, &state, args),
        "delete_smart_folder" => call!(typed::smart_folders::delete_smart_folder, &state, args),
        "move_smart_folder" => call!(typed::smart_folders::move_smart_folder, &state, args),

        // ── System ────────────────────────────────────────────
        "get_settings" => call!(typed::system::get_settings, &state, args),
        "save_settings" => call!(typed::system::save_settings, &state, args),
        "sync_get_status" => call!(typed::sync::sync_get_status, &state, args),
        "sync_detect_share_roots" => call!(typed::sync::sync_detect_share_roots, &state, args),
        "sync_list_remote_libraries" => {
            call!(typed::sync::sync_list_remote_libraries, &state, args)
        }
        "sync_create_remote_library" => {
            call!(typed::sync::sync_create_remote_library, &state, args)
        }
        "sync_connect_remote_library" => {
            call!(typed::sync::sync_connect_remote_library, &state, args)
        }
        "sync_disconnect" => call!(typed::sync::sync_disconnect, &state, args),
        "sync_now" => call!(typed::sync::sync_now, &state, args),
        "open_external_url" => call!(typed::system::open_external_url, &state, args),
        "get_sidebar_tree" => call!(typed::system::get_sidebar_tree, &state, args),
        "reorder_sidebar_nodes" => call!(typed::system::reorder_sidebar_nodes, &state, args),
        "pin_sidebar_item" => call!(typed::system::pin_sidebar_item, &state, args),
        "unpin_sidebar_item" => call!(typed::system::unpin_sidebar_item, &state, args),
        "reorder_pinned_items" => call!(typed::system::reorder_pinned_items, &state, args),
        "get_view_prefs" => call!(typed::system::get_view_prefs, &state, args),
        "set_view_prefs" => call!(typed::system::set_view_prefs, &state, args),
        "set_zoom_factor" => call!(typed::system::set_zoom_factor, &state, args),
        // ── Folders ──────────────────────────────────────────
        "get_folder_cover_hash" => call!(typed::folders::get_folder_cover_hash, &state, args),
        "move_folder" => call!(typed::folders::move_folder, &state, args),
        "create_folder" => call!(typed::folders::create_folder, &state, args),
        "update_folder" => call!(typed::folders::update_folder, &state, args),
        "set_folder_watch_config" => call!(typed::folders::set_folder_watch_config, &state, args),
        "clear_folder_watch_config" => {
            call!(typed::folders::clear_folder_watch_config, &state, args)
        }
        "delete_folder" => call!(typed::folders::delete_folder, &state, args),
        "remove_entities_from_folder" => {
            call!(typed::folders::remove_files_from_folder, &state, args)
        }
        "reorder_folder_items" => call!(typed::folders::reorder_folder_items, &state, args),
        "reorder_folder_members" => call!(typed::folders::reorder_folder_members, &state, args),

        // ── Collections ──────────────────────────────────────
        "get_collection_summary" => {
            call!(typed::collections::get_collection_summary, &state, args)
        }
        "create_collection" => call!(typed::collections::create_collection, &state, args),
        "reorder_collection_members" => {
            call!(typed::collections::reorder_collection_members, &state, args)
        }
        "add_collection_members" => {
            call!(typed::collections::add_collection_members, &state, args)
        }
        "remove_collection_members" => {
            call!(typed::collections::remove_collection_members, &state, args)
        }
        "split_collection" => call!(typed::collections::split_collection, &state, args),

        // ── Media I/O ─────────────────────────────────────────
        "resolve_file_path" => call!(typed::media_io::resolve_file_path, &state, args),
        "resolve_file_paths_batch" => {
            call!(typed::media_io::resolve_file_paths_batch, &state, args)
        }
        "export_media" => call!(typed::media_io::export_media, &state, args),
        "open_in_new_window" => call!(typed::media_io::open_in_new_window, &state, args),
        "ensure_thumbnail" => call!(typed::media_io::ensure_thumbnail, &state, args),
        "regenerate_thumbnails_batch" => {
            call!(typed::media_io::regenerate_thumbnails_batch, &state, args)
        }
        // ── Media Lifecycle ───────────────────────────────────
        "add_media" => call!(typed::media_lifecycle::add_media, &state, args),

        // ── Subscriptions ─────────────────────────────────────
        "get_groups" => call!(typed::subscriptions::get_groups, &state, args),
        "create_group" => call!(typed::subscriptions::create_group, &state, args),
        "delete_group" => call!(typed::subscriptions::delete_group, &state, args),
        "rename_group" => call!(typed::subscriptions::rename_group, &state, args),
        "set_subscription_schedule" => {
            call!(
                typed::subscriptions::set_subscription_schedule,
                &state,
                args
            )
        }
        "run_group" => call!(typed::subscriptions::run_group, &state, args),
        "stop_group" => call!(typed::subscriptions::stop_group, &state, args),
        "get_sites" => call!(typed::subscriptions::get_sites, &state, args),
        "verify_subscription_site" => {
            call!(typed::subscriptions::verify_subscription_site, &state, args)
        }
        "suggest_site_tags" => call!(typed::subscriptions::suggest_site_tags, &state, args),
        "set_subscription_group" => {
            call!(typed::subscriptions::set_subscription_group, &state, args)
        }
        "get_subscription_covers" => {
            call!(typed::subscriptions::get_subscription_covers, &state, args)
        }
        "sweep_orphaned_blobs" => {
            call!(typed::media_lifecycle::sweep_orphaned_blobs, &state, args)
        }
        "list_subscription_collections" => {
            call!(
                typed::subscriptions::list_subscription_collections,
                &state,
                args
            )
        }
        "get_site_metadata_schema" => {
            call!(typed::subscriptions::get_site_metadata_schema, &state, args)
        }
        "validate_site_metadata" => {
            call!(typed::subscriptions::validate_site_metadata, &state, args)
        }
        "get_subscriptions" => call!(typed::subscriptions::get_subscriptions, &state, args),
        "create_subscription" => call!(typed::subscriptions::create_subscription, &state, args),
        "delete_subscription" => call!(typed::subscriptions::delete_subscription, &state, args),
        "pause_subscription" => call!(typed::subscriptions::pause_subscription, &state, args),
        "add_subscription_query" => {
            call!(typed::subscriptions::add_subscription_query, &state, args)
        }
        "delete_subscription_query" => call!(
            typed::subscriptions::delete_subscription_query,
            &state,
            args
        ),
        "edit_subscription_query" => {
            call!(typed::subscriptions::edit_subscription_query, &state, args)
        }
        "pause_subscription_query" => {
            call!(typed::subscriptions::pause_subscription_query, &state, args)
        }
        "set_subscription_auto_collections" => call!(
            typed::subscriptions::set_subscription_auto_collections,
            &state,
            args
        ),
        "run_subscription" => call!(typed::subscriptions::run_subscription, &state, args),
        "stop_subscription" => call!(typed::subscriptions::stop_subscription, &state, args),
        "reset_subscription" => call!(typed::subscriptions::reset_subscription, &state, args),
        "reset_subscription_query" => {
            call!(typed::subscriptions::reset_subscription_query, &state, args)
        }
        "get_running_subscriptions" => call!(
            typed::subscriptions::get_running_subscriptions,
            &state,
            args
        ),
        "get_running_subscription_progress" => call!(
            typed::subscriptions::get_running_subscription_progress,
            &state,
            args
        ),
        "rename_subscription" => call!(typed::subscriptions::rename_subscription, &state, args),
        "run_subscription_query" => {
            call!(typed::subscriptions::run_subscription_query, &state, args)
        }
        "stop_subscription_query" => {
            call!(typed::subscriptions::stop_subscription_query, &state, args)
        }
        "retry_subscription_failed_post" => {
            call!(
                typed::subscriptions::retry_subscription_failed_post,
                &state,
                args
            )
        }
        "retry_subscription_failed_posts" => {
            call!(
                typed::subscriptions::retry_subscription_failed_posts,
                &state,
                args
            )
        }
        "list_subscription_runs" => {
            call!(typed::subscriptions::list_subscription_runs, &state, args)
        }
        "list_subscription_query_runs" => {
            call!(
                typed::subscriptions::list_subscription_query_runs,
                &state,
                args
            )
        }
        "list_subscription_issues" => {
            call!(typed::subscriptions::list_subscription_issues, &state, args)
        }
        "list_subscription_download_attempts" => {
            call!(
                typed::subscriptions::list_subscription_download_attempts,
                &state,
                args
            )
        }
        "list_credentials" => call!(typed::subscriptions::list_credentials, &state, args),
        "list_credential_health" => {
            call!(typed::subscriptions::list_credential_health, &state, args)
        }
        "set_credential" => call!(typed::subscriptions::set_credential, &state, args),
        "delete_credential" => call!(typed::subscriptions::delete_credential, &state, args),
        "pixiv_oauth_start" => call!(typed::subscriptions::pixiv_oauth_start, &state, args),
        "pixiv_oauth_exchange" => call!(typed::subscriptions::pixiv_oauth_exchange, &state, args),

        // ── AI Tagger ──────────────────────────────────────
        "ai_tagger_status" => call!(typed::ai_tagger::ai_tagger_status, &state, args),
        "ai_tagger_download_model" => {
            call!(typed::ai_tagger::ai_tagger_download_model, &state, args)
        }
        "ai_tagger_delete_model" => call!(typed::ai_tagger::ai_tagger_delete_model, &state, args),
        "ai_tag_predict" => call!(typed::ai_tagger::ai_tag_predict, &state, args),
        "ai_tag_cancel" => call!(typed::ai_tagger::ai_tag_cancel, &state, args),
        "ai_tag_apply" => call!(typed::ai_tagger::ai_tag_apply, &state, args),

        _ => Err(format!("Unknown command: {}", command)),
    }
}
