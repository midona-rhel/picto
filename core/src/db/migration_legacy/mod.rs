//! Legacy schema migration — one-shot import from old tables to new.
//!
//! This is the ONLY module allowed to reference old table names.
//! No runtime code depends on this module after migration completes.

use rusqlite::Connection;

/// Old schema version that triggers migration.
const OLD_SCHEMA_MAX_VERSION: i64 = 99;
/// New schema version after migration.
const NEW_SCHEMA_VERSION: i64 = 100;

/// Check if the database needs migration from old schema.
pub fn needs_migration(conn: &Connection) -> bool {
    let version: i64 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
        .unwrap_or(0);
    version > 0 && version <= OLD_SCHEMA_MAX_VERSION
}

/// Check if the database is already on the new schema.
pub fn is_new_schema(conn: &Connection) -> bool {
    let version: i64 = conn
        .query_row("SELECT version FROM schema_version LIMIT 1", [], |row| row.get(0))
        .unwrap_or(0);
    version >= NEW_SCHEMA_VERSION
}

/// Run the full migration from old schema to new.
/// Creates new tables, copies data, drops old tables, sets new version.
pub fn migrate(conn: &Connection) -> Result<MigrationResult, String> {
    let mut result = MigrationResult::default();

    // Disable FK checks during migration to avoid intermediate constraint violations.
    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|e| format!("Failed to disable FK: {e}"))?;

    // Tables that exist in both old and new schemas with different column layouts.
    // Rename them out of the way before creating the new versions.
    let overlapping = [
        "folder", "tag", "smart_folder", "media_entity",
        "subscription_group", "subscription", "subscription_query",
        "subscription_entity", "subscription_post_collection",
        "download_queue", "download_queue_item",
        "credential_domain", "credential_health",
        "duplicate", "file_color", "sidebar_node",
        "view_pref", "kv_settings", "manifest",
        "deferred_work",
    ];
    for table in &overlapping {
        let _ = conn.execute(&format!("ALTER TABLE {table} RENAME TO _old_{table}"), []);
    }

    // Create all new tables from scratch.
    conn.execute_batch(crate::db::core::schema::LIBRARY_DDL)
        .map_err(|e| format!("Failed to create new schema: {e}"))?;

    // ── Step 1: media_file ← file ─────────────────────────────────
    let file_count = conn
        .execute(
            "INSERT OR IGNORE INTO media_file (file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms, frame_count, has_audio, perceptual_hash, dominant_color_hex, dominant_palette_blob, date_added)
             SELECT file_id, hash, mime, size, width, height, duration_ms, num_frames, COALESCE(has_audio, 0), phash, dominant_color_hex, dominant_palette_blob, COALESCE(imported_at, datetime('now'))
             FROM file
             WHERE hash IS NOT NULL",
            [],
        )
        .map_err(|e| format!("Failed to migrate files: {e}"))?;
    result.files_migrated = file_count;

    // ── Step 2: media_entity ← _old_media_entity ───────────────────
    // Singles
    let singles = conn
        .execute(
            "INSERT OR IGNORE INTO media_entity (entity_id, entity_hash, entity_kind, status, name, notes, rating, source_urls_json, date_created, date_added, date_modified, parent_collection_entity_id, collection_ordinal)
             SELECT me.entity_id, COALESCE(me.hash, f.hash, hex(randomblob(32))), 'single', COALESCE(me.status, 0),
                    COALESCE(me.name, f.name), me.description, COALESCE(me.rating, f.rating),
                    f.source_urls_json,
                    COALESCE(me.created_at, f.imported_at, datetime('now')),
                    COALESCE(f.imported_at, me.created_at, datetime('now')),
                    COALESCE(me.updated_at, datetime('now')),
                    me.parent_collection_id, me.collection_ordinal
             FROM _old_media_entity me
             LEFT JOIN entity_file ef ON ef.entity_id = me.entity_id
             LEFT JOIN file f ON f.file_id = ef.file_id
             WHERE me.kind = 'single'",
            [],
        )
        .map_err(|e| format!("Failed to migrate single entities: {e}"))?;
    result.singles_migrated = singles;

    // Collections — primary_member_entity_id is NULL here, recomputed in step 6.
    let collections = conn
        .execute(
            "INSERT OR IGNORE INTO media_entity (entity_id, entity_hash, entity_kind, status, name, notes, rating, date_created, date_added, date_modified, member_count, total_size_bytes)
             SELECT me.entity_id, COALESCE(me.hash, hex(randomblob(32))), 'collection', COALESCE(me.status, 0),
                    me.name, me.description, me.rating,
                    COALESCE(me.created_at, datetime('now')),
                    COALESCE(me.created_at, datetime('now')),
                    COALESCE(me.updated_at, datetime('now')),
                    me.cached_item_count, me.cached_total_size_bytes
             FROM _old_media_entity me
             WHERE me.kind = 'collection'",
            [],
        )
        .map_err(|e| format!("Failed to migrate collection entities: {e}"))?;
    result.collections_migrated = collections;

    // ── Step 3: single_media_entity ← entity_file ─────────────────
    let bridges = conn
        .execute(
            "INSERT OR IGNORE INTO single_media_entity (entity_id, file_id)
             SELECT ef.entity_id, ef.file_id
             FROM entity_file ef
             JOIN _old_media_entity me ON me.entity_id = ef.entity_id
             WHERE me.kind = 'single'",
            [],
        )
        .map_err(|e| format!("Failed to migrate single_media_entity: {e}"))?;
    result.bridges_migrated = bridges;

    // ── Step 4: tag ← _old_tag, entity_tag ← entity_tag_raw ────────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO tag (tag_id, namespace, subtag, file_count)
         SELECT tag_id, namespace, subtag, file_count FROM _old_tag",
        [],
    );
    let tags = conn
        .execute(
            "INSERT OR IGNORE INTO entity_tag (entity_id, tag_id, source)
             SELECT entity_id, tag_id, source FROM entity_tag_raw",
            [],
        )
        .map_err(|e| format!("Failed to migrate entity_tag: {e}"))?;
    result.tags_migrated = tags;

    // ── Step 5: folder ← _old_folder, folder_member ← folder_entity
    let _ = conn.execute(
        "INSERT OR IGNORE INTO folder (folder_id, name, parent_id, icon, color, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode, date_added, date_modified)
         SELECT folder_id, name, parent_id, icon, color, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode, COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
         FROM _old_folder",
        [],
    );
    let folder_members = conn
        .execute(
            "INSERT OR IGNORE INTO folder_member (folder_id, entity_id, position_rank)
             SELECT folder_id, entity_id, position_rank FROM folder_entity",
            [],
        )
        .map_err(|e| format!("Failed to migrate folder_member: {e}"))?;
    result.folder_members_migrated = folder_members;

    // ── Step 6: Fix collection primary_member_entity_id ───────────
    // The old schema used cover_file_id (a file_id). The new schema uses
    // primary_member_entity_id (an entity_id of the first member by ordinal).
    conn.execute(
        "UPDATE media_entity SET primary_member_entity_id = (
             SELECT child.entity_id
             FROM media_entity child
             WHERE child.parent_collection_entity_id = media_entity.entity_id
             ORDER BY child.collection_ordinal ASC
             LIMIT 1
         )
         WHERE entity_kind = 'collection'",
        [],
    )
    .map_err(|e| format!("Failed to fix primary_member_entity_id: {e}"))?;

    // ── Step 7: Recompute collection aggregates ───────────────────
    conn.execute(
        "UPDATE media_entity SET
             member_count = (
                 SELECT COUNT(*) FROM media_entity child
                 WHERE child.parent_collection_entity_id = media_entity.entity_id
             ),
             total_size_bytes = (
                 SELECT COALESCE(SUM(mf.size_bytes), 0)
                 FROM media_entity child
                 JOIN single_media_entity sme ON sme.entity_id = child.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE child.parent_collection_entity_id = media_entity.entity_id
             )
         WHERE entity_kind = 'collection'",
        [],
    )
    .map_err(|e| format!("Failed to recompute collection aggregates: {e}"))?;

    // ── Step 8: smart_folder ← _old_smart_folder ────────────────────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO smart_folder (smart_folder_id, name, parent_id, icon, color, predicate_json, sort_field, sort_order, display_order, date_added, date_modified)
         SELECT smart_folder_id, name, parent_id, icon, color, predicate_json, sort_field, sort_order, display_order, COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
         FROM _old_smart_folder",
        [],
    );

    // ── Step 9: subscription tables ← _old_subscription_* ─────────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO subscription_group (group_id, name, schedule, date_added)
         SELECT group_id, name, schedule, COALESCE(created_at, datetime('now'))
         FROM _old_subscription_group",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO subscription (subscription_id, name, site_id, paused, group_id, initial_post_limit, periodic_post_limit, auto_collections, date_added)
         SELECT subscription_id, name, site_id, paused, group_id, initial_post_limit, periodic_post_limit, COALESCE(auto_collections, 1), COALESCE(created_at, datetime('now'))
         FROM _old_subscription",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO subscription_query (query_id, subscription_id, query_text, display_name, paused, last_check_time, files_found, posts_found, completed_initial_run, resume_cursor, resume_strategy)
         SELECT query_id, subscription_id, query_text, display_name, paused, last_check_time, files_found, COALESCE(posts_found, files_found), completed_initial_run, resume_cursor, resume_strategy
         FROM _old_subscription_query",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id)
         SELECT subscription_id, entity_id FROM _old_subscription_entity",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO subscription_post_collection (subscription_id, site_id, post_id, collection_entity_id, date_added, date_modified)
         SELECT subscription_id, site_id, post_id, collection_entity_id, COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
         FROM _old_subscription_post_collection",
        [],
    );

    // ── Step 10: file_color ← _old_file_color ─────────────────────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO file_color (rowid, file_id, hex, l, a, b)
         SELECT rowid, file_id, hex, l, a, b FROM _old_file_color",
        [],
    );

    // ── Step 11: credentials ← _old_credential_* ─────────────────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO credential_domain (site_category, credential_type, display_name, date_added)
         SELECT site_category, credential_type, display_name, COALESCE(created_at, datetime('now'))
         FROM _old_credential_domain",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO credential_health (site_category, health_status, last_checked_at, last_error)
         SELECT site_category, health_status, last_checked_at, last_error FROM _old_credential_health",
        [],
    );

    // ── Step 12: duplicates ← _old_duplicate ──────────────────────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO duplicate (file_id_a, file_id_b, distance, status, decision_at, decision_source, decision_reason, winner_file_id, loser_file_id)
         SELECT file_id_a, file_id_b, distance, status, decision_at, decision_source, decision_reason, winner_file_id, loser_file_id FROM _old_duplicate",
        [],
    );

    // ── Step 13: settings ← _old_kv_settings, _old_view_pref ─────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO kv_settings (key, value) SELECT key, value FROM _old_kv_settings",
        [],
    );
    let _ = conn.execute(
        "INSERT OR IGNORE INTO view_pref (scope, sort_field, sort_dir, layout, tile_size, show_name, show_resolution, show_extension, show_label, thumbnail_fit)
         SELECT scope, sort_field, sort_dir, layout, tile_size, show_name, show_resolution, show_extension, show_label, thumbnail_fit FROM _old_view_pref",
        [],
    );

    // ── Step 14: deferred_work_item ← _old_deferred_work ──────────
    let _ = conn.execute(
        "INSERT OR IGNORE INTO deferred_work_item (work_id, entity_hash, work_type, status, attempt_count, available_at, last_error, queued_at)
         SELECT work_id, hash, work_type, status, attempt_count, available_at, last_error, COALESCE(created_at, datetime('now'))
         FROM _old_deferred_work",
        [],
    );

    // ── Step 15: Drop ALL old/renamed tables ──────────────────────
    let tables_to_drop = [
        // Renamed old tables
        "_old_folder", "_old_tag", "_old_smart_folder", "_old_media_entity",
        "_old_subscription_group", "_old_subscription", "_old_subscription_query",
        "_old_subscription_entity", "_old_subscription_post_collection",
        "_old_download_queue", "_old_download_queue_item",
        "_old_credential_domain", "_old_credential_health",
        "_old_duplicate", "_old_file_color", "_old_sidebar_node",
        "_old_view_pref", "_old_kv_settings", "_old_manifest",
        "_old_deferred_work",
        // Original old-only tables
        "file", "entity_file", "entity_tag_raw", "folder_entity",
        "entity_metadata_projection", "artifact_manifest_meta",
        "artifact_manifest_entry", "mutation_action",
        "collection_source_url", "file_fts",
        "entity_tag_implied", "tag_ancestor", "tag_alias",
        "tag_implication", "tag_display",
    ];
    for table in &tables_to_drop {
        let _ = conn.execute(&format!("DROP TABLE IF EXISTS {table}"), []);
    }

    // ── Step 16: Re-enable FK checks ──────────────────────────────
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| format!("Failed to re-enable FK: {e}"))?;

    // ── Step 17: Set new schema version ───────────────────────────
    conn.execute("DELETE FROM schema_version", [])
        .map_err(|e| format!("Failed to clear schema_version: {e}"))?;
    conn.execute(
        "INSERT INTO schema_version (version) VALUES (?1)",
        [NEW_SCHEMA_VERSION],
    )
    .map_err(|e| format!("Failed to set new schema version: {e}"))?;

    Ok(result)
}

#[derive(Debug, Default)]
pub struct MigrationResult {
    pub files_migrated: usize,
    pub singles_migrated: usize,
    pub collections_migrated: usize,
    pub bridges_migrated: usize,
    pub tags_migrated: usize,
    pub folder_members_migrated: usize,
}

/// Import data from old SqliteDatabase (attached as `old_db`) into the fresh new schema.
/// The new schema tables already exist in the main database. This copies core data.
pub fn migrate_from_attached(conn: &Connection) -> Result<MigrationResult, String> {
    let mut result = MigrationResult::default();

    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|e| format!("Failed to disable FK: {e}"))?;

    // Step 1: media_file ← old_db.file
    result.files_migrated = conn.execute(
        "INSERT OR IGNORE INTO media_file (file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms, frame_count, has_audio, perceptual_hash, dominant_color_hex, dominant_palette_blob, date_added)
         SELECT file_id, hash, mime, size, width, height, duration_ms, num_frames, COALESCE(has_audio, 0), phash, dominant_color_hex, dominant_palette_blob, COALESCE(imported_at, datetime('now'))
         FROM old_db.file WHERE hash IS NOT NULL",
        [],
    ).map_err(|e| format!("Failed to import files: {e}"))?;

    // Step 2: media_entity ← old_db.media_entity (singles)
    result.singles_migrated = conn.execute(
        "INSERT OR IGNORE INTO media_entity (entity_id, entity_hash, entity_kind, status, name, notes, rating, source_urls_json, date_created, date_added, date_modified, parent_collection_entity_id, collection_ordinal)
         SELECT me.entity_id, COALESCE(me.hash, f.hash, hex(randomblob(32))), 'single', COALESCE(me.status, 0),
                COALESCE(me.name, f.name), me.description, COALESCE(me.rating, f.rating),
                f.source_urls_json,
                COALESCE(me.created_at, f.imported_at, datetime('now')),
                COALESCE(f.imported_at, me.created_at, datetime('now')),
                COALESCE(me.updated_at, datetime('now')),
                me.parent_collection_id, me.collection_ordinal
         FROM old_db.media_entity me
         LEFT JOIN old_db.entity_file ef ON ef.entity_id = me.entity_id
         LEFT JOIN old_db.file f ON f.file_id = ef.file_id
         WHERE me.kind = 'single'",
        [],
    ).map_err(|e| format!("Failed to import singles: {e}"))?;

    // Collections
    result.collections_migrated = conn.execute(
        "INSERT OR IGNORE INTO media_entity (entity_id, entity_hash, entity_kind, status, name, notes, rating, date_created, date_added, date_modified, member_count, total_size_bytes)
         SELECT me.entity_id, COALESCE(me.hash, hex(randomblob(32))), 'collection', COALESCE(me.status, 0),
                me.name, me.description, me.rating,
                COALESCE(me.created_at, datetime('now')),
                COALESCE(me.created_at, datetime('now')),
                COALESCE(me.updated_at, datetime('now')),
                me.cached_item_count, me.cached_total_size_bytes
         FROM old_db.media_entity me WHERE me.kind = 'collection'",
        [],
    ).map_err(|e| format!("Failed to import collections: {e}"))?;

    // Step 3: single_media_entity ← old_db.entity_file
    result.bridges_migrated = conn.execute(
        "INSERT OR IGNORE INTO single_media_entity (entity_id, file_id)
         SELECT ef.entity_id, ef.file_id
         FROM old_db.entity_file ef
         JOIN old_db.media_entity me ON me.entity_id = ef.entity_id
         WHERE me.kind = 'single'",
        [],
    ).map_err(|e| format!("Failed to import single_media_entity: {e}"))?;

    // Step 4: tags
    let _ = conn.execute(
        "INSERT OR IGNORE INTO tag (tag_id, namespace, subtag, file_count)
         SELECT tag_id, namespace, subtag, file_count FROM old_db.tag", []);
    result.tags_migrated = conn.execute(
        "INSERT OR IGNORE INTO entity_tag (entity_id, tag_id, source)
         SELECT entity_id, tag_id, source FROM old_db.entity_tag_raw", [],
    ).map_err(|e| format!("Failed to import tags: {e}"))?;

    // Step 5: folders
    let _ = conn.execute(
        "INSERT OR IGNORE INTO folder (folder_id, name, parent_id, icon, color, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode, date_added, date_modified)
         SELECT folder_id, name, parent_id, icon, color, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode, COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
         FROM old_db.folder", []);
    result.folder_members_migrated = conn.execute(
        "INSERT OR IGNORE INTO folder_member (folder_id, entity_id, position_rank)
         SELECT folder_id, entity_id, position_rank FROM old_db.folder_entity", [],
    ).map_err(|e| format!("Failed to import folder_member: {e}"))?;

    // Step 6: Fix collection primary_member_entity_id
    let _ = conn.execute(
        "UPDATE media_entity SET primary_member_entity_id = (
             SELECT child.entity_id FROM media_entity child
             WHERE child.parent_collection_entity_id = media_entity.entity_id
             ORDER BY child.collection_ordinal ASC LIMIT 1
         ) WHERE entity_kind = 'collection'", []);

    // Step 7: Recompute collection aggregates
    let _ = conn.execute(
        "UPDATE media_entity SET
             member_count = (SELECT COUNT(*) FROM media_entity child WHERE child.parent_collection_entity_id = media_entity.entity_id),
             total_size_bytes = (SELECT COALESCE(SUM(mf.size_bytes), 0) FROM media_entity child JOIN single_media_entity sme ON sme.entity_id = child.entity_id JOIN media_file mf ON mf.file_id = sme.file_id WHERE child.parent_collection_entity_id = media_entity.entity_id)
         WHERE entity_kind = 'collection'", []);

    // Step 8: smart_folder
    let _ = conn.execute(
        "INSERT OR IGNORE INTO smart_folder (smart_folder_id, name, parent_id, icon, color, predicate_json, sort_field, sort_order, display_order, date_added, date_modified)
         SELECT smart_folder_id, name, parent_id, icon, color, predicate_json, sort_field, sort_order, display_order, COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now'))
         FROM old_db.smart_folder", []);

    // Step 9: subscriptions
    let _ = conn.execute("INSERT OR IGNORE INTO subscription_group (group_id, name, schedule, date_added) SELECT group_id, name, schedule, COALESCE(created_at, datetime('now')) FROM old_db.subscription_group", []);
    let _ = conn.execute("INSERT OR IGNORE INTO subscription (subscription_id, name, site_id, paused, group_id, initial_post_limit, periodic_post_limit, auto_collections, date_added) SELECT subscription_id, name, site_id, paused, group_id, initial_post_limit, periodic_post_limit, COALESCE(auto_collections, 1), COALESCE(created_at, datetime('now')) FROM old_db.subscription", []);
    let _ = conn.execute("INSERT OR IGNORE INTO subscription_query (query_id, subscription_id, query_text, display_name, paused, last_check_time, files_found, posts_found, completed_initial_run, resume_cursor, resume_strategy) SELECT query_id, subscription_id, query_text, display_name, paused, last_check_time, files_found, COALESCE(posts_found, files_found), completed_initial_run, resume_cursor, resume_strategy FROM old_db.subscription_query", []);
    let _ = conn.execute("INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id) SELECT subscription_id, entity_id FROM old_db.subscription_entity", []);
    let _ = conn.execute("INSERT OR IGNORE INTO subscription_post_collection (subscription_id, site_id, post_id, collection_entity_id, date_added, date_modified) SELECT subscription_id, site_id, post_id, collection_entity_id, COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now')) FROM old_db.subscription_post_collection", []);

    // Step 10: file_color, credentials, duplicates, settings
    let _ = conn.execute("INSERT OR IGNORE INTO file_color (rowid, file_id, hex, l, a, b) SELECT rowid, file_id, hex, l, a, b FROM old_db.file_color", []);
    let _ = conn.execute("INSERT OR IGNORE INTO credential_domain (site_category, credential_type, display_name, date_added) SELECT site_category, credential_type, display_name, COALESCE(created_at, datetime('now')) FROM old_db.credential_domain", []);
    let _ = conn.execute("INSERT OR IGNORE INTO credential_health (site_category, health_status, last_checked_at, last_error) SELECT site_category, health_status, last_checked_at, last_error FROM old_db.credential_health", []);
    let _ = conn.execute("INSERT OR IGNORE INTO duplicate (file_id_a, file_id_b, distance, status, decision_at, decision_source, decision_reason, winner_file_id, loser_file_id) SELECT file_id_a, file_id_b, distance, status, decision_at, decision_source, decision_reason, winner_file_id, loser_file_id FROM old_db.duplicate", []);
    let _ = conn.execute("INSERT OR IGNORE INTO kv_settings (key, value) SELECT key, value FROM old_db.kv_settings", []);
    let _ = conn.execute("INSERT OR IGNORE INTO view_pref (scope, sort_field, sort_dir, layout, tile_size, show_name, show_resolution, show_extension, show_label, thumbnail_fit) SELECT scope, sort_field, sort_dir, layout, tile_size, show_name, show_resolution, show_extension, show_label, thumbnail_fit FROM old_db.view_pref", []);

    // Step 11: deferred_work
    let _ = conn.execute("INSERT OR IGNORE INTO deferred_work_item (work_id, entity_hash, work_type, status, attempt_count, available_at, last_error, queued_at) SELECT work_id, hash, work_type, status, attempt_count, available_at, last_error, COALESCE(created_at, datetime('now')) FROM old_db.deferred_work", []);

    // Set schema version
    conn.execute("DELETE FROM schema_version", [])
        .map_err(|e| format!("Failed to clear schema_version: {e}"))?;
    conn.execute("INSERT INTO schema_version (version) VALUES (?1)", [NEW_SCHEMA_VERSION])
        .map_err(|e| format!("Failed to set schema version: {e}"))?;

    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|e| format!("Failed to re-enable FK: {e}"))?;

    Ok(result)
}

impl std::fmt::Display for MigrationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Migration complete: {} files, {} singles, {} collections, {} bridges, {} tags, {} folder members",
            self.files_migrated,
            self.singles_migrated,
            self.collections_migrated,
            self.bridges_migrated,
            self.tags_migrated,
            self.folder_members_migrated,
        )
    }
}
