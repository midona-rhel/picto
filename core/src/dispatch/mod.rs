//! Command dispatcher — routes command names to domain handler functions.
//!
//! The napi-rs addon calls `dispatch("command_name", "{...args}")` and
//! gets back a JSON string result.

pub mod common;
pub mod typed;

pub use common::{ok_null, to_json};

/// Deserialize args, call a handler function, serialize its output.
macro_rules! call {
    ($func:path, $state:expr, $args:expr) => {{
        let input = serde_json::from_value($args.clone())
            .map_err(|e| format!("Invalid args: {e}"))?;
        let output = $func($state, input).await?;
        to_json(&output)
    }};
}

/// Commands that mutate state. Logged at `info!` level; everything else is `debug!`.
const WRITE_COMMANDS: &[&str] = &[
    "import_files", "import_folder", "set_entity_status", "delete_entities", "wipe_image_data",
    "add_tags", "remove_tags", "manage_tag_alias", "manage_tag_implication", "merge_tags",
    "rename_tag", "delete_tag",
    "add_tags_selection", "remove_tags_selection", "update_selection_metadata",
    "scan_duplicates", "resolve_duplicate_pair", "update_duplicate_settings",
    "create_smart_folder", "update_smart_folder", "delete_smart_folder", "move_smart_folder",
    "reorder_smart_folders",
    "create_folder", "update_folder", "delete_folder", "move_folder", "update_folder_parent",
    "add_entities_to_folder", "remove_entities_from_folder", "reorder_folders", "reorder_folder_items",
    "set_folder_watch_config", "clear_folder_watch_config",
    "create_collection", "update_collection", "delete_collection",
    "add_collection_members", "remove_collection_members", "reorder_collection_members",
    "list_collection_member_hashes",
    "save_settings", "reorder_sidebar_nodes", "set_view_prefs", "set_zoom_factor",
    "update_media_entity_metadata",
    "create_group", "delete_group", "rename_group", "set_group_schedule", "run_group", "stop_group",
    "create_subscription", "delete_subscription", "pause_subscription",
    "add_subscription_query", "delete_subscription_query", "edit_subscription_query", "pause_subscription_query", "set_subscription_auto_collections",
    "run_subscription", "stop_subscription", "reset_subscription", "rename_subscription",
    "run_subscription_query",
    "set_credential", "delete_credential",
    "pixiv_oauth_start", "pixiv_oauth_exchange",
    "export_file", "export_media", "regenerate_thumbnail", "regenerate_thumbnails_batch",
    "reanalyze_file_colors",
    "ai_tag_apply", "ai_tagger_download_model",
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

    match command {
        // ── Grid ──────────────────────────────────────────────
        "get_grid_page_slim" => call!(typed::grid::get_grid_page_slim, &state, args),
        "get_grid_outline" => call!(typed::grid::get_grid_outline, &state, args),
        "get_entity" => call!(typed::grid::get_entity, &state, args),
        "get_entities_metadata_batch" => call!(typed::grid::get_entities_metadata_batch, &state, args),

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
        "companion_get_namespace_values" => call!(typed::tags::companion_get_namespace_values, &state, args),
        "companion_get_files_by_tag" => call!(typed::tags::companion_get_files_by_tag, &state, args),

        // ── Selection ─────────────────────────────────────────
        "add_tags_selection" => call!(typed::selection::add_tags_selection, &state, args),
        "remove_tags_selection" => call!(typed::selection::remove_tags_selection, &state, args),
        "get_selection_summary" => call!(typed::selection::get_selection_summary, &state, args),
        "resolve_selection_hashes" => call!(typed::selection::resolve_selection_hashes, &state, args),
        "update_selection_metadata" => call!(typed::selection::update_selection_metadata, &state, args),

        // ── Duplicates ────────────────────────────────────────
        "scan_duplicates" => call!(typed::duplicates::scan_duplicates, &state, args),
        "get_duplicate_pairs" => call!(typed::duplicates::get_duplicate_pairs, &state, args),
        "resolve_duplicate_pair" => call!(typed::duplicates::resolve_duplicate_pair, &state, args),
        "get_duplicate_count" => call!(typed::duplicates::get_duplicate_count, &state, args),
        "get_duplicate_settings" => call!(typed::duplicates::get_duplicate_settings, &state, args),
        "update_duplicate_settings" => call!(typed::duplicates::update_duplicate_settings, &state, args),

        // ── Smart Folders ─────────────────────────────────────
        "list_smart_folders" => call!(typed::smart_folders::list_smart_folders, &state, args),
        "create_smart_folder" => call!(typed::smart_folders::create_smart_folder, &state, args),
        "update_smart_folder" => call!(typed::smart_folders::update_smart_folder, &state, args),
        "delete_smart_folder" => call!(typed::smart_folders::delete_smart_folder, &state, args),
        "count_smart_folder" => call!(typed::smart_folders::count_smart_folder, &state, args),
        "move_smart_folder" => call!(typed::smart_folders::move_smart_folder, &state, args),
        "reorder_smart_folders" => call!(typed::smart_folders::reorder_smart_folders, &state, args),

        // ── Media Metadata ────────────────────────────────────
        "get_media_entity_metadata" => call!(typed::media_metadata::get_media_entity_metadata, &state, args),
        "update_media_entity_metadata" => call!(typed::media_metadata::update_media_entity_metadata, &state, args),
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
        "clear_folder_watch_config" => call!(typed::folders::clear_folder_watch_config, &state, args),
        "delete_folder" => call!(typed::folders::delete_folder, &state, args),
        "update_folder_parent" => call!(typed::folders::update_folder_parent, &state, args),
        "add_entities_to_folder" => call!(typed::folders::add_files_to_folder, &state, args),
        "remove_entities_from_folder" => call!(typed::folders::remove_files_from_folder, &state, args),
        "reorder_folders" => call!(typed::folders::reorder_folders, &state, args),
        "reorder_folder_items" => call!(typed::folders::reorder_folder_items, &state, args),
        "get_collections" => call!(typed::folders::get_collections, &state, args),
        "get_collection_summary" => call!(typed::folders::get_collection_summary, &state, args),
        "create_collection" => call!(typed::folders::create_collection, &state, args),
        "update_collection" => call!(typed::folders::update_collection, &state, args),
        "add_collection_tags" => call!(typed::folders::add_collection_tags, &state, args),
        "remove_collection_tags" => call!(typed::folders::remove_collection_tags, &state, args),
        "reorder_collection_members" => call!(typed::folders::reorder_collection_members, &state, args),
        "add_collection_members" => call!(typed::folders::add_collection_members, &state, args),
        "remove_collection_members" => call!(typed::folders::remove_collection_members, &state, args),
        "delete_collection" => call!(typed::folders::delete_collection, &state, args),
        "list_collection_member_hashes" => call!(typed::folders::list_collection_member_hashes, &state, args),

        // ── Media I/O ─────────────────────────────────────────
        "resolve_file_path" => call!(typed::media_io::resolve_file_path, &state, args),
        "resolve_file_paths_batch" => call!(typed::media_io::resolve_file_paths_batch, &state, args),
        "open_file_default" => call!(typed::media_io::open_file_default, &state, args),
        "reveal_in_folder" => call!(typed::media_io::reveal_in_folder, &state, args),
        "export_file" => call!(typed::media_io::export_file, &state, args),
        "export_media" => call!(typed::media_io::export_media, &state, args),
        "open_in_new_window" => call!(typed::media_io::open_in_new_window, &state, args),
        "resolve_thumbnail_path" => call!(typed::media_io::resolve_thumbnail_path, &state, args),
        "ensure_thumbnail" => call!(typed::media_io::ensure_thumbnail, &state, args),
        "regenerate_thumbnail" => call!(typed::media_io::regenerate_thumbnail, &state, args),
        "regenerate_thumbnails_batch" => call!(typed::media_io::regenerate_thumbnails_batch, &state, args),
        "reanalyze_file_colors" => call!(typed::media_io::reanalyze_file_colors, &state, args),
        // ── Media Lifecycle ───────────────────────────────────
        "import_files" => call!(typed::media_lifecycle::import_files, &state, args),
        "import_folder" => call!(typed::media_lifecycle::import_folder, &state, args),
        "set_entity_status" => call!(typed::media_lifecycle::set_entity_status, &state, args),
        "delete_entities" => call!(typed::media_lifecycle::delete_entities, &state, args),
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
        "get_site_metadata_schema" => call!(typed::subscriptions::get_site_metadata_schema, &state, args),
        "validate_site_metadata" => call!(typed::subscriptions::validate_site_metadata, &state, args),
        "get_subscriptions" => call!(typed::subscriptions::get_subscriptions, &state, args),
        "create_subscription" => call!(typed::subscriptions::create_subscription, &state, args),
        "delete_subscription" => call!(typed::subscriptions::delete_subscription, &state, args),
        "pause_subscription" => call!(typed::subscriptions::pause_subscription, &state, args),
        "add_subscription_query" => call!(typed::subscriptions::add_subscription_query, &state, args),
        "delete_subscription_query" => call!(typed::subscriptions::delete_subscription_query, &state, args),
        "edit_subscription_query" => call!(typed::subscriptions::edit_subscription_query, &state, args),
        "pause_subscription_query" => call!(typed::subscriptions::pause_subscription_query, &state, args),
        "set_subscription_auto_collections" => call!(typed::subscriptions::set_subscription_auto_collections, &state, args),
        "run_subscription" => call!(typed::subscriptions::run_subscription, &state, args),
        "stop_subscription" => call!(typed::subscriptions::stop_subscription, &state, args),
        "reset_subscription" => call!(typed::subscriptions::reset_subscription, &state, args),
        "get_running_subscriptions" => call!(typed::subscriptions::get_running_subscriptions, &state, args),
        "get_running_subscription_progress" => call!(typed::subscriptions::get_running_subscription_progress, &state, args),
        "rename_subscription" => call!(typed::subscriptions::rename_subscription, &state, args),
        "run_subscription_query" => call!(typed::subscriptions::run_subscription_query, &state, args),
        "list_credentials" => call!(typed::subscriptions::list_credentials, &state, args),
        "list_credential_health" => call!(typed::subscriptions::list_credential_health, &state, args),
        "set_credential" => call!(typed::subscriptions::set_credential, &state, args),
        "delete_credential" => call!(typed::subscriptions::delete_credential, &state, args),
        "pixiv_oauth_start" => call!(typed::subscriptions::pixiv_oauth_start, &state, args),
        "pixiv_oauth_exchange" => call!(typed::subscriptions::pixiv_oauth_exchange, &state, args),

        // ── AI Tagger ──────────────────────────────────────
        "ai_tagger_status" => call!(typed::ai_tagger::ai_tagger_status, &state, args),
        "ai_tagger_download_model" => call!(typed::ai_tagger::ai_tagger_download_model, &state, args),
        "ai_tagger_delete_model" => call!(typed::ai_tagger::ai_tagger_delete_model, &state, args),
        "ai_tag_predict" => call!(typed::ai_tagger::ai_tag_predict, &state, args),
        "ai_tag_apply" => call!(typed::ai_tagger::ai_tag_apply, &state, args),

        _ => Err(format!("Unknown command: {}", command)),
    }
}
