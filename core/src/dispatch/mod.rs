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

/// Dispatch a command by name with JSON arguments. Returns JSON result.
pub async fn dispatch(command: &str, args_json: &str) -> Result<String, String> {
    let args: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("Invalid JSON args: {}", e))?;

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
        "get_file" => call!(typed::grid::get_file, &state, args),
        "get_files_metadata_batch" => call!(typed::grid::get_files_metadata_batch, &state, args),

        // ── Tags ──────────────────────────────────────────────
        "search_tags" => call!(typed::tags::search_tags, &state, args),
        "get_all_tags_with_counts" => call!(typed::tags::get_all_tags_with_counts, &state, args),
        "get_file_tags" => call!(typed::tags::get_file_tags, &state, args),
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
        "reorder_smart_folders" => call!(typed::smart_folders::reorder_smart_folders, &state, args),

        // ── Media Metadata ────────────────────────────────────
        "get_file_all_metadata" => call!(typed::media_metadata::get_file_all_metadata, &state, args),
        "update_file_metadata" => call!(typed::media_metadata::update_file_metadata, &state, args),
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
        "get_file_folders" => call!(typed::folders::get_file_folders, &state, args),
        "get_entity_folders" => call!(typed::folders::get_entity_folders, &state, args),
        "move_folder" => call!(typed::folders::move_folder, &state, args),
        "create_folder" => call!(typed::folders::create_folder, &state, args),
        "update_folder" => call!(typed::folders::update_folder, &state, args),
        "delete_folder" => call!(typed::folders::delete_folder, &state, args),
        "update_folder_parent" => call!(typed::folders::update_folder_parent, &state, args),
        "add_files_to_folder" => call!(typed::folders::add_files_to_folder, &state, args),
        "remove_files_from_folder" => call!(typed::folders::remove_files_from_folder, &state, args),
        "reorder_folders" => call!(typed::folders::reorder_folders, &state, args),
        "reorder_folder_items" => call!(typed::folders::reorder_folder_items, &state, args),
        "get_collections" => call!(typed::folders::get_collections, &state, args),
        "get_collection_summary" => call!(typed::folders::get_collection_summary, &state, args),
        "create_collection" => call!(typed::folders::create_collection, &state, args),
        "update_collection" => call!(typed::folders::update_collection, &state, args),
        "set_collection_rating" => call!(typed::folders::set_collection_rating, &state, args),
        "set_collection_source_urls" => call!(typed::folders::set_collection_source_urls, &state, args),
        "reorder_collection_members" => call!(typed::folders::reorder_collection_members, &state, args),
        "add_collection_members" => call!(typed::folders::add_collection_members, &state, args),
        "remove_collection_members" => call!(typed::folders::remove_collection_members, &state, args),
        "delete_collection" => call!(typed::folders::delete_collection, &state, args),

        // ── Media I/O ─────────────────────────────────────────
        "resolve_file_path" => call!(typed::media_io::resolve_file_path, &state, args),
        "open_file_default" => call!(typed::media_io::open_file_default, &state, args),
        "reveal_in_folder" => call!(typed::media_io::reveal_in_folder, &state, args),
        "open_in_new_window" => call!(typed::media_io::open_in_new_window, &state, args),
        "resolve_thumbnail_path" => call!(typed::media_io::resolve_thumbnail_path, &state, args),
        "ensure_thumbnail" => call!(typed::media_io::ensure_thumbnail, &state, args),
        "regenerate_thumbnail" => call!(typed::media_io::regenerate_thumbnail, &state, args),
        "regenerate_thumbnails_batch" => call!(typed::media_io::regenerate_thumbnails_batch, &state, args),
        "reanalyze_file_colors" => call!(typed::media_io::reanalyze_file_colors, &state, args),
        "backfill_missing_blurhashes" => call!(typed::media_io::backfill_missing_blurhashes, &state, args),

        // ── Media Lifecycle ───────────────────────────────────
        "import_files" => call!(typed::media_lifecycle::import_files, &state, args),
        "update_file_status" => call!(typed::media_lifecycle::update_file_status, &state, args),
        "delete_files" => call!(typed::media_lifecycle::delete_files, &state, args),
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
        "pause_subscription_query" => call!(typed::subscriptions::pause_subscription_query, &state, args),
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

        _ => Err(format!("Unknown command: {}", command)),
    }
}
