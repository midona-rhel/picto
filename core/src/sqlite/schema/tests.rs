use super::support::{has_column, table_exists};
use super::*;
#[test]
fn schema_init_creates_all_tables() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();

    // All core tables should exist
    let expected_tables = [
        "file",
        "file_color",
        "media_entity",
        "entity_file",
        "tag",
        "entity_tag_raw",
        "tag_alias",
        "tag_implication",
        "tag_ancestor",
        "tag_display",
        "entity_tag_implied",
        "folder",
        "folder_entity",
        "smart_folder",
        "subscription_group",
        "subscription",
        "subscription_query",
        "subscription_entity",
        "subscription_post_collection",
        "credential_domain",
        "credential_health",
        "duplicate",
        "sidebar_node",
        "entity_metadata_projection",
        "view_pref",
        "mutation_action",
        "manifest",
        "kv_settings",
        "schema_version",
        "artifact_manifest_meta",
        "artifact_manifest_entry",
    ];

    for table in &expected_tables {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "Table '{table}' should exist after init_schema");
    }

    // FTS5 + R*Tree virtual tables
    for vt in &["file_fts", "tag_fts", "file_color_rtree"] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [vt],
                |row| row.get(0),
            )
            .unwrap();
        assert!(
            exists,
            "Virtual table '{vt}' should exist after init_schema"
        );
    }

    // Schema version should be current version
    let version = get_schema_version(&conn).unwrap();
    assert_eq!(version, Some(CURRENT_VERSION));

    // Manifest should be seeded
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM manifest", [], |row| row.get(0))
        .unwrap();
    assert!(
        count >= 7,
        "Manifest should have at least 7 seeded keys, got {count}"
    );

    // Global manifest snapshot metadata should be seeded
    let published_epoch: i64 = conn
        .query_row(
            "SELECT manifest_epoch FROM artifact_manifest_meta WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(published_epoch, 0);
}

#[test]
fn schema_init_is_idempotent() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();
    // Second init should not fail (IF NOT EXISTS)
    init_schema(&conn).unwrap();
    let version = get_schema_version(&conn).unwrap();
    assert_eq!(version, Some(CURRENT_VERSION));
}

/// Verify the full V1→CURRENT migration path is safe on an already-current schema.
/// This ensures every migration step uses IF NOT EXISTS / has_column guards so
/// re-running migrations never fails (dry-run validation).
#[test]
fn migrations_are_idempotent_from_v1() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();

    // Simulate a V1 database that needs all migrations applied.
    conn.execute("UPDATE schema_version SET version = 1", [])
        .unwrap();

    // Run full migration chain — should succeed because all steps are guarded.
    run_migrations(&conn, 1).unwrap();

    let version = get_schema_version(&conn).unwrap();
    assert_eq!(version, Some(CURRENT_VERSION));

    // Verify V4 migration artifacts (group table + group_id column, renamed in V26)
    assert!(has_column(&conn, "subscription", "group_id").unwrap());

    // Verify V5 migration artifacts
    assert!(has_column(&conn, "file", "last_viewed_at").unwrap());
    assert!(!has_column(&conn, "file", "blurhash").unwrap());

    // Verify V7 migration artifacts
    assert!(has_column(&conn, "duplicate", "decision_at").unwrap());
    assert!(has_column(&conn, "file", "name_source").unwrap());

    // Verify V8 composite indexes exist
    let index_names: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='file'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    for expected in &[
        "idx_file_status_rating",
        "idx_file_status_size",
        "idx_file_status_viewcount",
        "idx_file_status_name",
    ] {
        assert!(
            index_names.iter().any(|n| n == expected),
            "Index {expected} should exist after V8 migration"
        );
    }

    // Verify V10 collection/entity schema artifacts.
    for table in &["media_entity", "entity_file"] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "Table '{table}' should exist after V10 migration");
    }

    // Verify V12 entity projection/tag artifacts.
    for table in &[
        "entity_tag_raw",
        "entity_tag_implied",
        "entity_metadata_projection",
    ] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "Table '{table}' should exist after V12 migration");
    }

    // Verify V21 denormalized collection metadata columns.
    assert!(has_column(&conn, "media_entity", "cover_file_id").unwrap());
    assert!(has_column(&conn, "media_entity", "cached_item_count").unwrap());
    assert!(has_column(&conn, "media_entity", "cached_total_size_bytes").unwrap());

    // Verify V13 link tables for upgraded libraries.
    for table in &["folder_entity", "subscription_entity"] {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name=?1",
                [table],
                |row| row.get(0),
            )
            .unwrap();
        assert!(exists, "Table '{table}' should exist after V13 migration");
    }
}

#[test]
fn v14_backfills_entity_links_from_legacy_tables() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();

    // Seed minimal graph: one folder, one group+subscription, one file with single-entity mapping.
    conn.execute("INSERT INTO folder (folder_id, name) VALUES (1, 'f')", [])
        .unwrap();
    conn.execute(
            "INSERT INTO subscription_group (group_id, name, schedule, created_at) VALUES (1, 'grp', 'manual', CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
    conn.execute(
            "INSERT INTO subscription (subscription_id, name, site_id, paused, group_id, initial_post_limit, periodic_post_limit, created_at)
             VALUES (1, 'sub', 'x', 0, 1, 100, 50, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
    conn.execute(
            "INSERT INTO file (file_id, hash, name, size, mime, has_audio, status, view_count, imported_at)
             VALUES (100, 'h100', 'n100', 1, 'image/png', 0, 1, 0, CURRENT_TIMESTAMP)",
            [],
        )
        .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO media_entity
                (entity_id, kind, name, description, status, rating, created_at, updated_at)
             VALUES (100, 'single', 'n100', '', 1, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO entity_file (entity_id, file_id) VALUES (100, 100)",
        [],
    )
    .unwrap();

    // Legacy membership tables that older DBs still contain.
    conn.execute_batch(
        "CREATE TABLE folder_file (
                folder_id INTEGER NOT NULL,
                file_id INTEGER NOT NULL,
                position_rank INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (folder_id, file_id)
            );
            CREATE TABLE subscription_file (
                subscription_id INTEGER NOT NULL,
                file_id INTEGER NOT NULL,
                PRIMARY KEY (subscription_id, file_id)
            );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO folder_file (folder_id, file_id, position_rank) VALUES (1, 100, 7)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO subscription_file (subscription_id, file_id) VALUES (1, 100)",
        [],
    )
    .unwrap();

    // Simulate pre-v14 schema and run upgrade.
    conn.execute("UPDATE schema_version SET version = 13", [])
        .unwrap();
    run_migrations(&conn, 13).unwrap();

    let folder_links: i64 = conn
            .query_row("SELECT COUNT(*) FROM folder_entity WHERE folder_id = 1 AND entity_id = 100 AND position_rank = 7", [], |row| row.get(0))
            .unwrap();
    assert_eq!(folder_links, 1);

    let sub_links: i64 = conn
            .query_row("SELECT COUNT(*) FROM subscription_entity WHERE subscription_id = 1 AND entity_id = 100", [], |row| row.get(0))
            .unwrap();
    assert_eq!(sub_links, 1);
}

#[test]
fn v27_removes_blurhash_column_from_file_table() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();

    conn.execute("UPDATE schema_version SET version = 26", [])
        .unwrap();
    conn.execute_batch("ALTER TABLE file ADD COLUMN blurhash TEXT;")
        .unwrap();
    conn.execute(
            "INSERT INTO file (
                file_id, hash, name, size, mime, width, height, duration_ms, num_frames,
                has_audio, blurhash, status, rating, view_count, last_viewed_at, phash,
                imported_at, notes, source_urls_json, dominant_color_hex, dominant_palette_blob, name_source
            ) VALUES (
                1, 'hash_v27', 'name', 1, 'image/png', 100, 100, NULL, NULL,
                0, 'legacy-blurhash', 1, NULL, 0, NULL, NULL,
                CURRENT_TIMESTAMP, NULL, NULL, '#000000', NULL, 'unknown'
            )",
            [],
        )
        .unwrap();

    run_migrations(&conn, 26).unwrap();

    assert_eq!(get_schema_version(&conn).unwrap(), Some(CURRENT_VERSION));
    assert!(!has_column(&conn, "file", "blurhash").unwrap());

    let row: (String, Option<String>) = conn
        .query_row(
            "SELECT hash, dominant_color_hex FROM file WHERE file_id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(row.0, "hash_v27");
    assert_eq!(row.1.as_deref(), Some("#000000"));
}

#[test]
fn v15_syncs_single_entity_status_from_file() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();

    conn.execute(
        "INSERT INTO file (file_id, hash, size, mime, has_audio, status, view_count, imported_at)
             VALUES (200, 'h200', 1, 'image/png', 0, 2, 0, CURRENT_TIMESTAMP)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO media_entity
                (entity_id, kind, name, description, status, rating, created_at, updated_at)
             VALUES (200, 'single', '', '', 1, NULL, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT OR IGNORE INTO entity_file (entity_id, file_id) VALUES (200, 200)",
        [],
    )
    .unwrap();

    conn.execute("UPDATE schema_version SET version = 14", [])
        .unwrap();
    run_migrations(&conn, 14).unwrap();

    let status: i64 = conn
        .query_row(
            "SELECT status FROM media_entity WHERE entity_id = 200",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(status, 2);
}

// v16_backfills_parent_collection_membership removed: the legacy
// `collection_member` table no longer exists in fresh schemas, so the
// migration path it tested is unreachable from init_schema().

#[test]
fn reconcile_schema_restores_missing_current_read_model_tables() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();

    conn.execute_batch(
        "DROP TABLE IF EXISTS tag_ancestor;
             DROP TABLE IF EXISTS tag_display;
             DROP TABLE IF EXISTS entity_tag_implied;
             DROP TABLE IF EXISTS sidebar_node;
             DROP TABLE IF EXISTS entity_metadata_projection;
             DROP TABLE IF EXISTS artifact_manifest_entry;
             DROP TABLE IF EXISTS artifact_manifest_meta;
             DROP TABLE IF EXISTS manifest;
             DROP TABLE IF EXISTS kv_settings;",
    )
    .unwrap();

    reconcile_schema(&conn).unwrap();

    for table in &[
        "tag_ancestor",
        "tag_display",
        "entity_tag_implied",
        "sidebar_node",
        "entity_metadata_projection",
        "manifest",
        "artifact_manifest_meta",
        "artifact_manifest_entry",
        "kv_settings",
    ] {
        assert!(
            table_exists(&conn, table).unwrap(),
            "Table '{table}' should be recreated"
        );
    }

    let manifest_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM manifest", [], |row| row.get(0))
        .unwrap();
    assert!(manifest_rows > 0, "Manifest seed rows should be restored");

    let meta_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM artifact_manifest_meta WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        meta_exists, 1,
        "Artifact manifest meta row should be restored"
    );
}

#[test]
fn reconcile_schema_restores_missing_credential_columns() {
    let conn = Connection::open_in_memory().unwrap();
    apply_pragmas(&conn).unwrap();
    init_schema(&conn).unwrap();

    conn.execute_batch(
        "DROP TABLE credential_domain;
             DROP TABLE credential_health;
             CREATE TABLE credential_domain (
                 site_category   TEXT PRIMARY KEY,
                 credential_type TEXT NOT NULL
             );
             CREATE TABLE credential_health (
                 site_category TEXT PRIMARY KEY,
                 health_status TEXT NOT NULL
             );",
    )
    .unwrap();

    reconcile_schema(&conn).unwrap();

    assert!(has_column(&conn, "credential_domain", "display_name").unwrap());
    assert!(has_column(&conn, "credential_domain", "created_at").unwrap());
    assert!(has_column(&conn, "credential_health", "last_checked_at").unwrap());
    assert!(has_column(&conn, "credential_health", "last_error").unwrap());
}
