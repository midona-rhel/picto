//! Command dispatcher — routes command names to domain handler functions.
//!
//! The napi-rs addon calls `dispatch("command_name", "{...args}")` and
//! gets back a JSON string result.

pub mod common;
pub mod typed;

pub use common::{ok_null, to_json};

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
struct ResolveAssetInput {
    entity_hash: String,
    role: crate::engine::assets::AssetRole,
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

#[derive(serde::Deserialize)]
struct DeferredWorkItemsInput {
    filter: Option<crate::background_work::DeferredWorkFilter>,
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
    "import_files",
    "import_folder",
    "set_entity_status",
    "delete_entities",
    "wipe_image_data",
    "add_tags",
    "remove_tags",
    "manage_tag_alias",
    "manage_tag_implication",
    "merge_tags",
    "rename_tag",
    "delete_tag",
    "set_tag_site_mask",
    "scan_duplicates",
    "resolve_duplicate_pair",
    "update_duplicate_settings",
    "create_smart_folder",
    "update_smart_folder",
    "delete_smart_folder",
    "move_smart_folder",
    "reorder_smart_folders",
    "create_folder",
    "update_folder",
    "delete_folder",
    "move_folder",
    "update_folder_parent",
    "add_entities_to_folder",
    "remove_entities_from_folder",
    "reorder_folders",
    "reorder_folder_items",
    "reorder_folder_members",
    "set_folder_watch_config",
    "clear_folder_watch_config",
    "create_collection",
    "update_collection",
    "delete_collection",
    "add_collection_members",
    "remove_collection_members",
    "reorder_collection_members",
    "list_collection_member_hashes",
    "save_settings",
    "reorder_sidebar_nodes",
    "set_view_prefs",
    "set_zoom_factor",
    "create_group",
    "delete_group",
    "rename_group",
    "set_group_schedule",
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
    "set_credential",
    "delete_credential",
    "pixiv_oauth_start",
    "pixiv_oauth_exchange",
    "export_file",
    "export_media",
    "regenerate_thumbnail",
    "regenerate_thumbnails_batch",
    "reanalyze_file_colors",
    "ai_tag_apply",
    "ai_tagger_download_model",
    "close_library",
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
    // ─── Pre-state commands ──────────────────────────────────
    match command {
        "close_library" => {
            crate::state::close_library().await?;
            crate::events::emit_empty(crate::events::event_names::LIBRARY_CLOSED);
            return ok_null();
        }
        "get_runtime_snapshot" => {
            let snapshot = crate::runtime_state::get_runtime_snapshot();
            return to_json(&snapshot);
        }
        _ => {}
    }

    let state = crate::state::get_state()?;

    // ── Engine-routed commands (canonical names from PBI-568) ──
    // These go through ApplicationEngine. The old dispatch/typed handlers
    // below remain as fallback until the frontend migrates to new names.
    match command {
        "query_entity_view" => {
            let query: crate::db::types::EntityViewQuery =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.query_entity_view(query)?;
            return to_json(&result);
        }
        "reconcile_entity_view" => {
            let req: crate::db::types::EntityViewReconcileRequest =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.reconcile_entity_view(req)?;
            return to_json(&result);
        }
        "get_entity_details" => {
            let input: GetHashInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.get_entity_details(&input.entity_hash)?;
            return to_json(&result);
        }
        "get_entity_grid_items" => {
            let input: GetHashesInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.get_entity_grid_items(&input.entity_hashes)?;
            return to_json(&result);
        }
        "patch_media_entities" => {
            let input: PatchEntitiesInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state
                .engine
                .patch_media_entities(input.target, input.patch)?;
            return to_json(&result);
        }
        "apply_entity_tags" => {
            let input: ApplyTagsInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
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
            let input: SetTagSiteMaskInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let site_mask = crate::db::types::parse_mask_decimal(&input.site_mask)?;
            state.engine.set_tag_site_mask(input.tag_id, site_mask)?;
            return ok_null();
        }
        "update_folder_membership" => {
            let input: FolderMembershipInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.update_folder_membership(
                input.target,
                input.folder_id,
                input.operation,
            )?;
            return to_json(&result);
        }
        "resolve_entity_asset" => {
            let input: ResolveAssetInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state
                .engine
                .resolve_entity_asset(&input.entity_hash, input.role)?;
            return to_json(&result);
        }
        "get_deferred_work_summary" => {
            let result = state.engine.get_deferred_work_summary()?;
            return to_json(&result);
        }
        "list_deferred_work_items" => {
            let input: DeferredWorkItemsInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state
                .engine
                .list_deferred_work_items(input.filter.unwrap_or_default())?;
            return to_json(&result);
        }
        "retry_deferred_work" => {
            let input: GetHashInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            state.engine.retry_deferred_work(&input.entity_hash)?;
            return ok_null();
        }
        "set_entity_status" => {
            let input: SetStatusInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.set_entity_status(input.target, input.status)?;
            return to_json(&result);
        }
        "delete_entities" => {
            let input: DeleteEntitiesInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.delete_entities(input.target)?;
            return to_json(&result);
        }
        "get_selection_summary" => {
            let input: SelectionSummaryInput =
                serde_json::from_value(args).map_err(|e| format!("Invalid args: {e}"))?;
            let result = state.engine.get_selection_summary(input.target).await?;
            return to_json(&result);
        }
        _ => {}
    }

    // ── Legacy dispatch (isolated compatibility surface) ──
    match command {
        // ── Tags ──────────────────────────────────────────────
        "search_tags" => call!(typed::tags::search_tags, &state, args),
        "get_all_tags_with_counts" => call!(typed::tags::get_all_tags_with_counts, &state, args),
        "get_entity_tags" => call!(typed::tags::get_file_tags, &state, args),
        "add_tags" => call!(typed::tags::add_tags, &state, args),
        "remove_tags" => call!(typed::tags::remove_tags, &state, args),
        "find_files_by_tags" => call!(typed::tags::find_files_by_tags, &state, args),
        "manage_tag_alias" => call!(typed::tags::manage_tag_alias, &state, args),
        "get_tag_relations" => call!(typed::tags::get_tag_relations, &state, args),
        "manage_tag_implication" => call!(typed::tags::manage_tag_implication, &state, args),
        "merge_tags" => call!(typed::tags::merge_tags, &state, args),
        "get_tags_paginated" => call!(typed::tags::get_tags_paginated, &state, args),
        "get_namespace_summary" => call!(typed::tags::get_namespace_summary, &state, args),
        "rename_tag" => call!(typed::tags::rename_tag, &state, args),
        "delete_tag" => call!(typed::tags::delete_tag, &state, args),
        "companion_get_namespace_values" => {
            call!(typed::tags::companion_get_namespace_values, &state, args)
        }
        "companion_get_files_by_tag" => {
            call!(typed::tags::companion_get_files_by_tag, &state, args)
        }

        // ── Duplicates ────────────────────────────────────────
        "find_similar" => call!(typed::duplicates::find_similar, &state, args),
        "scan_duplicates" => call!(typed::duplicates::scan_duplicates, &state, args),
        "get_duplicate_pairs" => call!(typed::duplicates::get_duplicate_pairs, &state, args),
        "resolve_duplicate_pair" => call!(typed::duplicates::resolve_duplicate_pair, &state, args),
        "get_duplicate_count" => call!(typed::duplicates::get_duplicate_count, &state, args),
        "get_duplicate_settings" => call!(typed::duplicates::get_duplicate_settings, &state, args),
        "update_duplicate_settings" => {
            call!(typed::duplicates::update_duplicate_settings, &state, args)
        }

        // ── Smart Folders ─────────────────────────────────────
        "list_smart_folders" => call!(typed::smart_folders::list_smart_folders, &state, args),
        "create_smart_folder" => call!(typed::smart_folders::create_smart_folder, &state, args),
        "update_smart_folder" => call!(typed::smart_folders::update_smart_folder, &state, args),
        "delete_smart_folder" => call!(typed::smart_folders::delete_smart_folder, &state, args),
        "count_smart_folder" => call!(typed::smart_folders::count_smart_folder, &state, args),
        "move_smart_folder" => call!(typed::smart_folders::move_smart_folder, &state, args),
        "reorder_smart_folders" => call!(typed::smart_folders::reorder_smart_folders, &state, args),

        // ── Media Metadata ────────────────────────────────────
        "get_media_entity_metadata" => call!(
            typed::media_metadata::get_media_entity_metadata,
            &state,
            args
        ),
        "get_storage_stats" => call!(typed::media_metadata::get_storage_stats, &state, args),

        // ── System ────────────────────────────────────────────
        "get_settings" => call!(typed::system::get_settings, &state, args),
        "save_settings" => call!(typed::system::save_settings, &state, args),
        "get_library_info" => call!(typed::system::get_library_info, &state, args),
        "get_perf_snapshot" => call!(typed::system::get_perf_snapshot, &state, args),
        "check_perf_slo" => call!(typed::system::check_perf_slo, &state, args),
        "open_external_url" => call!(typed::system::open_external_url, &state, args),
        "get_sidebar_tree" => call!(typed::system::get_sidebar_tree, &state, args),
        "reorder_sidebar_nodes" => call!(typed::system::reorder_sidebar_nodes, &state, args),
        "get_view_prefs" => call!(typed::system::get_view_prefs, &state, args),
        "set_view_prefs" => call!(typed::system::set_view_prefs, &state, args),
        "set_zoom_factor" => call!(typed::system::set_zoom_factor, &state, args),
        "get_zoom_factor" => call!(typed::system::get_zoom_factor, &state, args),
        // ── Folders & Collections ─────────────────────────────
        "list_folders" => call!(typed::folders::list_folders, &state, args),
        "get_folder_files" => call!(typed::folders::get_folder_files, &state, args),
        "get_folder_cover_hash" => call!(typed::folders::get_folder_cover_hash, &state, args),
        "get_entity_folders" => call!(typed::folders::get_entity_folders, &state, args),
        "get_entity_folders_by_hash" => call!(typed::folders::get_file_folders, &state, args),
        "move_folder" => call!(typed::folders::move_folder, &state, args),
        "create_folder" => call!(typed::folders::create_folder, &state, args),
        "update_folder" => call!(typed::folders::update_folder, &state, args),
        "set_folder_watch_config" => call!(typed::folders::set_folder_watch_config, &state, args),
        "clear_folder_watch_config" => {
            call!(typed::folders::clear_folder_watch_config, &state, args)
        }
        "delete_folder" => call!(typed::folders::delete_folder, &state, args),
        "update_folder_parent" => call!(typed::folders::update_folder_parent, &state, args),
        "add_entities_to_folder" => call!(typed::folders::add_files_to_folder, &state, args),
        "remove_entities_from_folder" => {
            call!(typed::folders::remove_files_from_folder, &state, args)
        }
        "reorder_folders" => call!(typed::folders::reorder_folders, &state, args),
        "reorder_folder_items" => call!(typed::folders::reorder_folder_items, &state, args),
        "reorder_folder_members" => call!(typed::folders::reorder_folder_members, &state, args),
        "get_collections" => call!(typed::folders::get_collections, &state, args),
        "get_collection_summary" => call!(typed::folders::get_collection_summary, &state, args),
        "create_collection" => call!(typed::folders::create_collection, &state, args),
        "update_collection" => call!(typed::folders::update_collection, &state, args),
        "reorder_collection_members" => {
            call!(typed::folders::reorder_collection_members, &state, args)
        }
        "add_collection_members" => call!(typed::folders::add_collection_members, &state, args),
        "remove_collection_members" => {
            call!(typed::folders::remove_collection_members, &state, args)
        }
        "delete_collection" => call!(typed::folders::delete_collection, &state, args),
        "list_collection_member_hashes" => {
            call!(typed::folders::list_collection_member_hashes, &state, args)
        }

        // ── Media I/O ─────────────────────────────────────────
        "resolve_file_path" => call!(typed::media_io::resolve_file_path, &state, args),
        "resolve_file_paths_batch" => {
            call!(typed::media_io::resolve_file_paths_batch, &state, args)
        }
        "open_file_default" => call!(typed::media_io::open_file_default, &state, args),
        "reveal_in_folder" => call!(typed::media_io::reveal_in_folder, &state, args),
        "export_file" => call!(typed::media_io::export_file, &state, args),
        "export_media" => call!(typed::media_io::export_media, &state, args),
        "open_in_new_window" => call!(typed::media_io::open_in_new_window, &state, args),
        "resolve_thumbnail_path" => call!(typed::media_io::resolve_thumbnail_path, &state, args),
        "ensure_thumbnail" => call!(typed::media_io::ensure_thumbnail, &state, args),
        "regenerate_thumbnail" => call!(typed::media_io::regenerate_thumbnail, &state, args),
        "regenerate_thumbnails_batch" => {
            call!(typed::media_io::regenerate_thumbnails_batch, &state, args)
        }
        "reanalyze_file_colors" => call!(typed::media_io::reanalyze_file_colors, &state, args),
        // ── Media Lifecycle ───────────────────────────────────
        "import_files" => call!(typed::media_lifecycle::import_files, &state, args),
        "import_folder" => call!(typed::media_lifecycle::import_folder, &state, args),
        "wipe_image_data" => call!(typed::media_lifecycle::wipe_image_data, &state, args),

        // ── Subscriptions ─────────────────────────────────────
        "get_groups" => call!(typed::subscriptions::get_groups, &state, args),
        "create_group" => call!(typed::subscriptions::create_group, &state, args),
        "delete_group" => call!(typed::subscriptions::delete_group, &state, args),
        "rename_group" => call!(typed::subscriptions::rename_group, &state, args),
        "set_group_schedule" => call!(typed::subscriptions::set_group_schedule, &state, args),
        "run_group" => call!(typed::subscriptions::run_group, &state, args),
        "stop_group" => call!(typed::subscriptions::stop_group, &state, args),
        "get_sites" => call!(typed::subscriptions::get_sites, &state, args),
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
        "ai_tag_apply" => call!(typed::ai_tagger::ai_tag_apply, &state, args),

        _ => Err(format!("Unknown command: {}", command)),
    }
}
