//! Exact pre-1.0 schema for the replacement backend.

use rusqlite::{Connection, OptionalExtension, Transaction};

pub const CURRENT_SCHEMA_VERSION: i64 = 126;
pub const CURRENT_PHASH_ANALYSIS_VERSION: i64 = 3;
pub const PHASH_VERSION_SETTING: &str = "media.perceptual_hash_version";

pub const LIBRARY_DDL: &str = r#"
CREATE TABLE library_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE media_file (
    file_id INTEGER PRIMARY KEY,
    file_hash TEXT NOT NULL UNIQUE,
    mime_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    pixel_width INTEGER,
    pixel_height INTEGER,
    duration_ms INTEGER,
    frame_count INTEGER,
    has_audio INTEGER NOT NULL DEFAULT 0,
    perceptual_hash TEXT,
    dominant_color_hex TEXT,
    dominant_palette_blob BLOB,
    color_analysis_version INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL
);

CREATE TABLE library_item (
    item_id INTEGER PRIMARY KEY,
    item_key TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('media', 'collection')),
    label TEXT,
    cover_media_item_id INTEGER REFERENCES library_item(item_id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE library_root (
    item_id INTEGER PRIMARY KEY REFERENCES library_item(item_id) ON DELETE CASCADE,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('inbox', 'active', 'trash')),
    sort_rank INTEGER
);
CREATE INDEX idx_library_root_lifecycle ON library_root(lifecycle, item_id);

CREATE TABLE media_asset (
    item_id INTEGER PRIMARY KEY REFERENCES library_item(item_id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE RESTRICT,
    name TEXT,
    notes TEXT,
    rating INTEGER,
    source_urls_json TEXT,
    captured_at TEXT,
    imported_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_media_asset_file ON media_asset(file_id, item_id);
CREATE INDEX idx_media_asset_rating ON media_asset(rating, item_id);

CREATE TABLE collection_member (
    collection_id INTEGER NOT NULL REFERENCES library_item(item_id) ON DELETE CASCADE,
    media_item_id INTEGER NOT NULL UNIQUE REFERENCES media_asset(item_id) ON DELETE CASCADE,
    position_rank INTEGER NOT NULL,
    PRIMARY KEY (collection_id, media_item_id)
);
CREATE INDEX idx_collection_member_order
    ON collection_member(collection_id, position_rank, media_item_id);

CREATE TABLE media_view (
    item_id INTEGER PRIMARY KEY REFERENCES library_root(item_id) ON DELETE CASCADE,
    viewed_at TEXT NOT NULL
);
CREATE INDEX idx_media_view_recent ON media_view(viewed_at DESC, item_id);

CREATE TABLE tag (
    tag_id INTEGER PRIMARY KEY,
    namespace TEXT NOT NULL DEFAULT 'general',
    subtag TEXT NOT NULL,
    UNIQUE (namespace, subtag)
);

CREATE TABLE media_tag (
    media_item_id INTEGER NOT NULL REFERENCES media_asset(item_id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT 'local',
    provenance_mask INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (media_item_id, tag_id, source)
);
CREATE INDEX idx_media_tag_tag ON media_tag(tag_id, media_item_id);

CREATE TABLE tag_alias (
    from_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    to_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT 'local',
    PRIMARY KEY (from_tag_id, source)
);

CREATE TABLE tag_implication (
    child_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    parent_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source TEXT NOT NULL DEFAULT 'local',
    PRIMARY KEY (child_tag_id, parent_tag_id, source)
);

CREATE TABLE folder (
    folder_id INTEGER PRIMARY KEY,
    folder_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    parent_id INTEGER REFERENCES folder(folder_id) ON DELETE CASCADE,
    icon TEXT,
    color TEXT,
    notes TEXT,
    sort_rank INTEGER,
    watch_path TEXT UNIQUE,
    watch_enabled INTEGER NOT NULL DEFAULT 0,
    watch_subfolders INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE folder_item (
    folder_id INTEGER NOT NULL REFERENCES folder(folder_id) ON DELETE CASCADE,
    item_id INTEGER NOT NULL REFERENCES library_root(item_id) ON DELETE CASCADE,
    position_rank INTEGER,
    PRIMARY KEY (folder_id, item_id)
);
CREATE INDEX idx_folder_item_item ON folder_item(item_id, folder_id);

CREATE TABLE smart_folder (
    smart_folder_id INTEGER PRIMARY KEY,
    smart_folder_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    parent_id INTEGER REFERENCES smart_folder(smart_folder_id) ON DELETE CASCADE,
    icon TEXT,
    color TEXT,
    notes TEXT,
    predicate_json TEXT NOT NULL,
    sort_field TEXT,
    sort_order TEXT,
    display_order INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE subscription (
    subscription_id INTEGER PRIMARY KEY,
    subscription_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    schedule TEXT NOT NULL DEFAULT 'manual',
    paused INTEGER NOT NULL DEFAULT 0,
    initial_post_limit INTEGER,
    periodic_post_limit INTEGER,
    next_run_at TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE subscription_query (
    query_id INTEGER PRIMARY KEY,
    query_key TEXT NOT NULL UNIQUE,
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    site_id TEXT NOT NULL,
    domain_key TEXT NOT NULL,
    query_kind TEXT NOT NULL,
    query_text TEXT NOT NULL,
    display_name TEXT,
    notes TEXT,
    group_posts INTEGER NOT NULL DEFAULT 1,
    paused INTEGER NOT NULL DEFAULT 0,
    resume_cursor TEXT,
    initial_run_complete INTEGER NOT NULL DEFAULT 0,
    last_success_at TEXT,
    last_failure_at TEXT,
    last_failure_kind TEXT,
    last_failure_message TEXT
);

CREATE TABLE subscription_run (
    run_id INTEGER PRIMARY KEY,
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    requested_by TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    started_at TEXT,
    finished_at TEXT,
    failure_kind TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_subscription_one_active_run
    ON subscription_run(subscription_id)
    WHERE status IN ('pending', 'running');

CREATE TABLE subscription_run_query (
    run_query_id INTEGER PRIMARY KEY,
    run_id INTEGER NOT NULL REFERENCES subscription_run(run_id) ON DELETE CASCADE,
    query_id INTEGER NOT NULL REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'cancelled')),
    resume_cursor TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at TEXT NOT NULL,
    started_at TEXT,
    finished_at TEXT,
    failure_kind TEXT,
    error_message TEXT,
    UNIQUE (run_id, query_id)
);
CREATE INDEX idx_subscription_run_query_ready
    ON subscription_run_query(status, available_at, run_query_id);

CREATE TABLE source_post (
    source_post_id INTEGER PRIMARY KEY,
    site_id TEXT NOT NULL,
    post_key TEXT NOT NULL,
    canonical_url TEXT,
    creator_name TEXT,
    title TEXT,
    description TEXT,
    captured_at TEXT,
    metadata_json TEXT,
    root_item_id INTEGER REFERENCES library_root(item_id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (site_id, post_key)
);

CREATE TABLE subscription_source_post (
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id INTEGER NOT NULL REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    source_post_id INTEGER NOT NULL REFERENCES source_post(source_post_id) ON DELETE CASCADE,
    last_seen_run_id INTEGER REFERENCES subscription_run(run_id) ON DELETE SET NULL,
    PRIMARY KEY (subscription_id, query_id, source_post_id)
);

CREATE TABLE source_item (
    source_item_id INTEGER PRIMARY KEY,
    source_post_id INTEGER NOT NULL REFERENCES source_post(source_post_id) ON DELETE CASCADE,
    item_key TEXT NOT NULL,
    position INTEGER NOT NULL,
    media_url TEXT,
    canonical_url TEXT,
    media_item_id INTEGER REFERENCES media_asset(item_id) ON DELETE SET NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'downloaded', 'ingested', 'failed', 'deleted')),
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (source_post_id, item_key)
);
CREATE INDEX idx_source_item_state ON source_item(state, source_item_id);

CREATE TABLE subscription_run_source_item (
    run_query_id INTEGER NOT NULL REFERENCES subscription_run_query(run_query_id) ON DELETE CASCADE,
    source_item_id INTEGER NOT NULL REFERENCES source_item(source_item_id) ON DELETE CASCADE,
    PRIMARY KEY (run_query_id, source_item_id)
);
CREATE INDEX idx_subscription_run_source_item_source
    ON subscription_run_source_item(source_item_id, run_query_id);

CREATE TABLE ingest_job (
    ingest_job_id INTEGER PRIMARY KEY,
    job_key TEXT NOT NULL UNIQUE,
    source_kind TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_item_id INTEGER REFERENCES source_item(source_item_id) ON DELETE CASCADE,
    payload_json TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('inbox', 'active')),
    delete_after_ingest INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at TEXT NOT NULL,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX idx_ingest_source_item
    ON ingest_job(source_item_id) WHERE source_item_id IS NOT NULL;
CREATE INDEX idx_ingest_ready ON ingest_job(status, available_at, ingest_job_id);

CREATE TABLE work_item (
    work_id INTEGER PRIMARY KEY,
    media_item_id INTEGER REFERENCES media_asset(item_id) ON DELETE CASCADE,
    file_id INTEGER REFERENCES media_file(file_id) ON DELETE CASCADE,
    file_hash TEXT,
    work_type TEXT NOT NULL CHECK (work_type IN ('thumbnail', 'dominant_colors', 'perceptual_hash', 'blob_delete', 'ai_tag')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'running')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at TEXT NOT NULL,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (media_item_id IS NOT NULL OR file_id IS NOT NULL OR file_hash IS NOT NULL)
);
CREATE INDEX idx_work_ready ON work_item(status, available_at, work_id);
CREATE UNIQUE INDEX idx_work_media_target
    ON work_item(media_item_id, file_id, work_type)
    WHERE media_item_id IS NOT NULL;
CREATE UNIQUE INDEX idx_work_file_target
    ON work_item(file_id, work_type)
    WHERE media_item_id IS NULL AND file_id IS NOT NULL;
CREATE UNIQUE INDEX idx_work_hash_target
    ON work_item(file_hash, work_type) WHERE file_hash IS NOT NULL;

CREATE TABLE subscription_issue (
    issue_id INTEGER PRIMARY KEY,
    issue_key TEXT NOT NULL UNIQUE,
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    issue_kind TEXT NOT NULL,
    message TEXT NOT NULL,
    detail TEXT,
    status TEXT NOT NULL DEFAULT 'open',
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    resolved_at TEXT
);
CREATE INDEX idx_subscription_issue_page
    ON subscription_issue(subscription_id, status, last_seen_at DESC, issue_id DESC);

CREATE TABLE credential (
    site_id TEXT PRIMARY KEY,
    credential_type TEXT NOT NULL CHECK (credential_type IN ('api_key', 'cookies', 'oauth_token')),
    display_name TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE credential_health (
    site_id TEXT PRIMARY KEY REFERENCES credential(site_id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'unknown',
    checked_at TEXT,
    last_error TEXT
);

CREATE TABLE duplicate (
    file_id_a INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
    file_id_b INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
    distance INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'detected',
    decided_at TEXT,
    winner_file_id INTEGER REFERENCES media_file(file_id),
    PRIMARY KEY (file_id_a, file_id_b),
    CHECK (file_id_a < file_id_b)
);

CREATE TABLE file_color (
    color_id INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
    hex TEXT NOT NULL,
    l REAL NOT NULL,
    a REAL NOT NULL,
    b REAL NOT NULL
);

CREATE TABLE view_pref (
    scope TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);

CREATE TABLE setting (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
);

CREATE TABLE history_entry (
    entry_id INTEGER PRIMARY KEY,
    command TEXT NOT NULL,
    label TEXT NOT NULL,
    forward_changeset BLOB NOT NULL,
    resources_json TEXT NOT NULL,
    item_ids_json TEXT NOT NULL,
    reload_projections INTEGER NOT NULL DEFAULT 0 CHECK (reload_projections IN (0, 1)),
    applied INTEGER NOT NULL DEFAULT 1 CHECK (applied IN (0, 1)),
    byte_size INTEGER NOT NULL,
    created_at TEXT NOT NULL
);
CREATE INDEX idx_history_entry_applied ON history_entry(applied, entry_id);

CREATE VIRTUAL TABLE media_fts USING fts5(
    name,
    notes,
    source_urls,
    content='media_asset',
    content_rowid='item_id'
);
"#;

pub fn create(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(LIBRARY_DDL)
        .map_err(|error| format!("Failed to create schema: {error}"))?;
    transaction
        .execute(
            "INSERT INTO library_meta (singleton, schema_version, revision) VALUES (1, ?1, 0)",
            [CURRENT_SCHEMA_VERSION],
        )
        .map_err(|error| format!("Failed to record schema version: {error}"))?;
    transaction
        .execute(
            "INSERT INTO setting (key, value_json) VALUES (?1, ?2)",
            rusqlite::params![
                PHASH_VERSION_SETTING,
                CURRENT_PHASH_ANALYSIS_VERSION.to_string()
            ],
        )
        .map_err(|error| format!("Failed to record pHash version: {error}"))?;
    transaction.commit().map_err(|error| error.to_string())
}

pub fn validate(connection: &Connection) -> Result<(), String> {
    let version: Option<i64> = connection
        .query_row(
            "SELECT schema_version FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Invalid Picto library schema: {error}"))?;
    match version {
        Some(version) if version == CURRENT_SCHEMA_VERSION => Ok(()),
        Some(version) => Err(format!(
            "Picto schema {version} is incompatible with required schema {CURRENT_SCHEMA_VERSION}"
        )),
        None => Err("This is not a current Picto library".to_string()),
    }
}

pub fn revision(connection: &Connection) -> rusqlite::Result<u64> {
    connection.query_row(
        "SELECT revision FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
}

pub fn increment_revision(transaction: &Transaction<'_>) -> rusqlite::Result<u64> {
    transaction.execute(
        "UPDATE library_meta SET revision = revision + 1 WHERE singleton = 1",
        [],
    )?;
    transaction.query_row(
        "SELECT revision FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::{create, validate, CURRENT_SCHEMA_VERSION};
    use rusqlite::Connection;

    #[test]
    fn creates_only_the_replacement_schema() {
        let mut connection = Connection::open_in_memory().unwrap();
        create(&mut connection).unwrap();
        validate(&connection).unwrap();

        let names = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        for expected in [
            "library_item",
            "library_root",
            "media_asset",
            "media_file",
            "collection_member",
            "folder_item",
            "source_post",
            "source_item",
            "ingest_job",
            "work_item",
            "credential",
            "credential_health",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "missing {expected}"
            );
        }
        for removed in [
            "media_entity",
            "folder_member",
            "op_outbox",
            "sync_conflict_clock",
        ] {
            assert!(
                !names.iter().any(|name| name == removed),
                "retained {removed}"
            );
        }
        assert_eq!(CURRENT_SCHEMA_VERSION, 126);
    }

    #[test]
    fn rejects_other_versions_without_mutation() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_meta (
                    singleton INTEGER PRIMARY KEY,
                    schema_version INTEGER NOT NULL,
                    revision INTEGER NOT NULL
                );
                INSERT INTO library_meta VALUES (1, 117, 42);",
            )
            .unwrap();

        let error = validate(&connection).unwrap_err();
        assert!(error.contains("117"));
        let revision: i64 = connection
            .query_row("SELECT revision FROM library_meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(revision, 42);
    }
}
