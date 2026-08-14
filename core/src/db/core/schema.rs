//! Library database schema — authoritative table definitions.
//! No other module may define or assume table structure.

use rusqlite::{params, Connection, OptionalExtension};

/// The latest canonical schema. Version 100 is the legacy-to-canonical boundary.
pub const CURRENT_SCHEMA_VERSION: i64 = 114;

/// Full DDL for a new library database.
pub const LIBRARY_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS media_entity (
    entity_id                    INTEGER PRIMARY KEY,
    entity_hash                  TEXT    NOT NULL UNIQUE,
    entity_kind                  TEXT    NOT NULL CHECK (entity_kind IN ('single', 'collection')),
    status                       INTEGER NOT NULL DEFAULT 0,
    name                         TEXT,
    notes                        TEXT,
    rating                       INTEGER,
    source_urls_json             TEXT,
    date_created                 TEXT    NOT NULL,
    date_added                   TEXT    NOT NULL,
    date_modified                TEXT    NOT NULL,
    member_count                 INTEGER,
    total_size_bytes             INTEGER,
    primary_member_entity_id     INTEGER REFERENCES media_entity(entity_id) ON DELETE SET NULL,
    parent_collection_entity_id  INTEGER REFERENCES media_entity(entity_id) ON DELETE SET NULL,
    collection_ordinal           INTEGER
);

CREATE INDEX IF NOT EXISTS idx_me_status ON media_entity(status);
CREATE INDEX IF NOT EXISTS idx_me_kind ON media_entity(entity_kind);
CREATE INDEX IF NOT EXISTS idx_me_parent ON media_entity(parent_collection_entity_id);
CREATE INDEX IF NOT EXISTS idx_me_date_added ON media_entity(date_added);
CREATE INDEX IF NOT EXISTS idx_me_rating ON media_entity(rating);

CREATE TABLE IF NOT EXISTS media_view (
    entity_id INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    viewed_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_media_view_viewed_at ON media_view(viewed_at DESC);

CREATE TABLE IF NOT EXISTS media_file (
    file_id              INTEGER PRIMARY KEY,
    file_hash            TEXT    NOT NULL UNIQUE,
    mime_type            TEXT    NOT NULL,
    size_bytes           INTEGER NOT NULL,
    pixel_width          INTEGER,
    pixel_height         INTEGER,
    duration_ms          INTEGER,
    frame_count          INTEGER,
    has_audio            INTEGER NOT NULL DEFAULT 0,
    perceptual_hash      TEXT,
    dominant_color_hex   TEXT,
    dominant_palette_blob BLOB,
    color_analysis_version INTEGER NOT NULL DEFAULT 0,
    date_added           TEXT    NOT NULL
);

-- Exact candidate index for the normal duplicate threshold (distance <= 7).
-- Candidate rows are always verified against the full pHash before use.
CREATE TABLE IF NOT EXISTS media_file_phash_index (
    file_id     INTEGER PRIMARY KEY REFERENCES media_file(file_id) ON DELETE CASCADE,
    partition_0 INTEGER NOT NULL,
    partition_1 INTEGER NOT NULL,
    partition_2 INTEGER NOT NULL,
    partition_3 INTEGER NOT NULL,
    partition_4 INTEGER NOT NULL,
    partition_5 INTEGER NOT NULL,
    partition_6 INTEGER NOT NULL,
    partition_7 INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p0 ON media_file_phash_index(partition_0);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p1 ON media_file_phash_index(partition_1);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p2 ON media_file_phash_index(partition_2);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p3 ON media_file_phash_index(partition_3);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p4 ON media_file_phash_index(partition_4);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p5 ON media_file_phash_index(partition_5);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p6 ON media_file_phash_index(partition_6);
CREATE INDEX IF NOT EXISTS idx_mf_phash_p7 ON media_file_phash_index(partition_7);

CREATE TABLE IF NOT EXISTS single_media_entity (
    entity_id  INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    file_id    INTEGER NOT NULL UNIQUE REFERENCES media_file(file_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tag (
    tag_id     INTEGER PRIMARY KEY,
    namespace  TEXT    NOT NULL DEFAULT 'general',
    subtag     TEXT    NOT NULL,
    UNIQUE (namespace, subtag)
);

CREATE TABLE IF NOT EXISTS entity_tag (
    entity_id  INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag_id     INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    provenance_mask INTEGER NOT NULL DEFAULT 0,
    source     TEXT    NOT NULL DEFAULT 'local',
    PRIMARY KEY (entity_id, tag_id, source)
);
CREATE INDEX IF NOT EXISTS idx_entity_tag_tag_id ON entity_tag(tag_id, entity_id);

CREATE TABLE IF NOT EXISTS tag_alias (
    from_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    to_tag_id   INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source       TEXT    NOT NULL DEFAULT 'local',
    PRIMARY KEY (from_tag_id, source)
);

CREATE TABLE IF NOT EXISTS tag_implication (
    child_tag_id  INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    parent_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source         TEXT    NOT NULL DEFAULT 'local',
    PRIMARY KEY (child_tag_id, parent_tag_id, source)
);

CREATE TABLE IF NOT EXISTS tag_ancestor (
    tag_id      INTEGER NOT NULL,
    ancestor_id INTEGER NOT NULL,
    depth       INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (tag_id, ancestor_id)
);

CREATE TABLE IF NOT EXISTS entity_tag_implied (
    entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag_id    INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    PRIMARY KEY (entity_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_entity_tag_implied_tag_id ON entity_tag_implied(tag_id, entity_id);

CREATE TABLE IF NOT EXISTS tag_display (
    tag_id     INTEGER PRIMARY KEY REFERENCES tag(tag_id) ON DELETE CASCADE,
    display_ns TEXT,
    display_st TEXT
);

CREATE TABLE IF NOT EXISTS folder (
    folder_id                  INTEGER PRIMARY KEY,
    name                       TEXT    NOT NULL,
    parent_id                  INTEGER REFERENCES folder(folder_id) ON DELETE CASCADE,
    icon                       TEXT,
    color                      TEXT,
    notes                      TEXT,
    sort_order                 INTEGER,
    auto_tags                  TEXT,
    watch_path                 TEXT    UNIQUE,
    watch_enabled              INTEGER DEFAULT 0,
    watch_subfolders           INTEGER DEFAULT 0,
    watch_import_status_mode   TEXT    DEFAULT 'inherit',
    total_size_bytes           INTEGER NOT NULL DEFAULT 0,
    pinned                     INTEGER NOT NULL DEFAULT 0,
    pin_order                  INTEGER NOT NULL DEFAULT 0,
    uuid                       TEXT,
    date_added                 TEXT    NOT NULL,
    date_modified              TEXT    NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_folder_uuid ON folder(uuid) WHERE uuid IS NOT NULL;

CREATE TABLE IF NOT EXISTS folder_member (
    folder_id     INTEGER NOT NULL REFERENCES folder(folder_id) ON DELETE CASCADE,
    entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    position_rank INTEGER,
    PRIMARY KEY (folder_id, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_folder_member_entity_id ON folder_member(entity_id, folder_id);

CREATE TABLE IF NOT EXISTS smart_folder (
    smart_folder_id  INTEGER PRIMARY KEY,
    name             TEXT    NOT NULL,
    parent_id        INTEGER REFERENCES smart_folder(smart_folder_id) ON DELETE SET NULL,
    icon             TEXT,
    color            TEXT,
    notes            TEXT,
    predicate_json   TEXT    NOT NULL,
    sort_field        TEXT,
    sort_order        TEXT,
    display_order    INTEGER,
    total_size_bytes INTEGER NOT NULL DEFAULT 0,
    pinned           INTEGER NOT NULL DEFAULT 0,
    pin_order        INTEGER NOT NULL DEFAULT 0,
    uuid             TEXT,
    date_added       TEXT    NOT NULL,
    date_modified    TEXT    NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_smart_folder_uuid ON smart_folder(uuid) WHERE uuid IS NOT NULL;

CREATE TABLE IF NOT EXISTS subscription_group (
    group_id   INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    uuid       TEXT,
    date_added TEXT    NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_subscription_group_uuid ON subscription_group(uuid) WHERE uuid IS NOT NULL;

CREATE TABLE IF NOT EXISTS subscription (
    subscription_id    INTEGER PRIMARY KEY,
    name               TEXT    NOT NULL,
    site_id            TEXT    NOT NULL,
    schedule           TEXT    NOT NULL DEFAULT 'manual',
    paused             INTEGER NOT NULL DEFAULT 0,
    group_id           INTEGER REFERENCES subscription_group(group_id) ON DELETE CASCADE,
    initial_post_limit INTEGER DEFAULT 100,
    periodic_post_limit INTEGER DEFAULT 100,
    auto_collections   INTEGER NOT NULL DEFAULT 1,
    uuid               TEXT,
    date_added         TEXT    NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_subscription_uuid ON subscription(uuid) WHERE uuid IS NOT NULL;

CREATE TABLE IF NOT EXISTS subscription_query (
    query_id            INTEGER PRIMARY KEY,
    subscription_id     INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    site_id             TEXT    NOT NULL,
    query_kind          TEXT    NOT NULL DEFAULT '',
    query_text          TEXT    NOT NULL,
    display_name        TEXT,
    notes               TEXT,
    paused              INTEGER NOT NULL DEFAULT 0,
    last_check_time     TEXT,
    files_found         INTEGER NOT NULL DEFAULT 0,
    posts_found         INTEGER NOT NULL DEFAULT 0,
    completed_initial_run INTEGER NOT NULL DEFAULT 0,
    resume_cursor       TEXT,
    resume_strategy     TEXT,
    last_success_at     TEXT,
    last_failure_at     TEXT,
    last_failure_kind   TEXT,
    last_failure_message TEXT
);

CREATE TABLE IF NOT EXISTS subscription_entity (
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    entity_id       INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    PRIMARY KEY (subscription_id, entity_id)
);

CREATE TABLE IF NOT EXISTS subscription_post_collection (
    subscription_id       INTEGER NOT NULL,
    site_id               TEXT    NOT NULL,
    post_id               TEXT    NOT NULL,
    collection_entity_id  INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    date_added            TEXT    NOT NULL,
    date_modified         TEXT    NOT NULL,
    PRIMARY KEY (subscription_id, site_id, post_id)
);

CREATE TABLE IF NOT EXISTS ingest_queue (
    queue_id        INTEGER PRIMARY KEY,
    queue_kind      TEXT    NOT NULL,
    source_kind     TEXT    NOT NULL,
    subscription_id INTEGER REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id        INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    query_run_id    INTEGER,
    cleanup_root    TEXT,
    post_id         TEXT,
    category        TEXT,
    preferred_name  TEXT,
    expected_count  INTEGER,
    status          TEXT    NOT NULL DEFAULT 'pending',
    last_error      TEXT,
    created_at      TEXT    NOT NULL,
    updated_at      TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS ingest_queue_item (
    item_id              INTEGER PRIMARY KEY,
    queue_id             INTEGER NOT NULL REFERENCES ingest_queue(queue_id) ON DELETE CASCADE,
    source_path          TEXT    NOT NULL,
    page_num             INTEGER NOT NULL DEFAULT 0,
    payload_json         TEXT    NOT NULL,
    delete_after_ingest  INTEGER NOT NULL DEFAULT 0,
    status               TEXT    NOT NULL DEFAULT 'pending',
    result_kind          TEXT,
    resolved_entity_hash TEXT,
    resolved_file_hash   TEXT,
    last_error           TEXT,
    created_at           TEXT    NOT NULL,
    updated_at           TEXT    NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ingest_queue_ready
    ON ingest_queue(status, created_at, queue_id);

CREATE INDEX IF NOT EXISTS idx_ingest_queue_subscription
    ON ingest_queue(subscription_id, status, queue_id);

CREATE INDEX IF NOT EXISTS idx_ingest_queue_item_queue
    ON ingest_queue_item(queue_id, status, page_num, item_id);

CREATE TABLE IF NOT EXISTS subscription_run (
    run_id               INTEGER PRIMARY KEY,
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    started_at           TEXT    NOT NULL,
    finished_at          TEXT,
    status               TEXT    NOT NULL DEFAULT 'running',
    failure_kind         TEXT,
    error_message        TEXT,
    files_downloaded     INTEGER NOT NULL DEFAULT 0,
    files_skipped        INTEGER NOT NULL DEFAULT 0,
    metadata_validated   INTEGER NOT NULL DEFAULT 0,
    metadata_invalid     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS subscription_query_run (
    query_run_id         INTEGER PRIMARY KEY,
    run_id               INTEGER REFERENCES subscription_run(run_id) ON DELETE SET NULL,
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id             INTEGER NOT NULL REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    started_at           TEXT    NOT NULL,
    finished_at          TEXT,
    status               TEXT    NOT NULL DEFAULT 'running',
    failure_kind         TEXT,
    error_message        TEXT,
    posts_processed      INTEGER NOT NULL DEFAULT 0,
    files_downloaded     INTEGER NOT NULL DEFAULT 0,
    files_skipped        INTEGER NOT NULL DEFAULT 0,
    metadata_validated   INTEGER NOT NULL DEFAULT 0,
    metadata_invalid     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS subscription_query_job (
    job_id               INTEGER PRIMARY KEY,
    run_id               INTEGER REFERENCES subscription_run(run_id) ON DELETE SET NULL,
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id             INTEGER NOT NULL REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    site_id              TEXT    NOT NULL,
    status               TEXT    NOT NULL DEFAULT 'queued',
    job_kind             TEXT    NOT NULL DEFAULT 'query_sync',
    requested_by         TEXT    NOT NULL DEFAULT 'subscription',
    post_id              TEXT,
    attempt_count        INTEGER NOT NULL DEFAULT 0,
    available_at         TEXT    NOT NULL,
    queued_at            TEXT    NOT NULL,
    started_at           TEXT,
    finished_at          TEXT,
    failure_kind         TEXT,
    error_message        TEXT
);

CREATE INDEX IF NOT EXISTS idx_subscription_query_job_ready
    ON subscription_query_job(status, available_at, job_id);

CREATE INDEX IF NOT EXISTS idx_subscription_query_job_subscription
    ON subscription_query_job(subscription_id, status, queued_at, job_id);

CREATE TABLE IF NOT EXISTS subscription_issue (
    issue_id             INTEGER PRIMARY KEY,
    issue_key            TEXT    NOT NULL,
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id             INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    issue_kind           TEXT    NOT NULL,
    status               TEXT    NOT NULL DEFAULT 'open',
    message              TEXT    NOT NULL,
    detail               TEXT,
    first_seen_at        TEXT    NOT NULL,
    last_seen_at         TEXT    NOT NULL,
    resolved_at          TEXT,
    recovery_action      TEXT    NOT NULL DEFAULT 'none',
    next_retry_at        TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_subscription_issue_key
    ON subscription_issue(issue_key);

CREATE TABLE IF NOT EXISTS subscription_download_attempt (
    attempt_id           INTEGER PRIMARY KEY,
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id             INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    query_run_id         INTEGER REFERENCES subscription_query_run(query_run_id) ON DELETE SET NULL,
    item_key             TEXT    NOT NULL,
    site_category        TEXT,
    post_id              TEXT,
    page_num             INTEGER,
    canonical_post_url   TEXT,
    media_url            TEXT,
    retry_url            TEXT,
    retry_count          INTEGER NOT NULL DEFAULT 0,
    status               TEXT    NOT NULL DEFAULT 'pending',
    failure_kind         TEXT,
    last_error           TEXT,
    next_retry_at        TEXT,
    created_at           TEXT    NOT NULL,
    updated_at           TEXT    NOT NULL,
    resolved_at          TEXT,
    UNIQUE (subscription_id, query_id, item_key)
);
CREATE INDEX IF NOT EXISTS idx_subscription_download_attempt_retry
    ON subscription_download_attempt(subscription_id, query_id, site_category, post_id, status);

CREATE TABLE IF NOT EXISTS subscription_post_member (
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    site_id              TEXT    NOT NULL,
    post_id              TEXT    NOT NULL,
    item_key             TEXT    NOT NULL,
    page_num             INTEGER,
    canonical_post_url   TEXT,
    media_url            TEXT,
    entity_hash          TEXT,
    status               TEXT    NOT NULL,
    created_at           TEXT    NOT NULL,
    updated_at           TEXT    NOT NULL,
    PRIMARY KEY (subscription_id, site_id, post_id, item_key)
);

CREATE TABLE IF NOT EXISTS deferred_work_item (
    work_id       INTEGER PRIMARY KEY,
    entity_hash   TEXT    NOT NULL,
    work_type     TEXT    NOT NULL CHECK (work_type IN ('thumbnail', 'dominant_colors', 'perceptual_hash', 'blob_delete', 'ai_tag')),
    status        TEXT    NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'running')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at  TEXT    NOT NULL,
    last_error    TEXT,
    queued_at     TEXT    NOT NULL,
    started_at    TEXT,
    finished_at   TEXT,
    last_error_at TEXT,
    UNIQUE (entity_hash, work_type)
);

CREATE TABLE IF NOT EXISTS credential_domain (
    site_category   TEXT PRIMARY KEY,
    credential_type TEXT NOT NULL,
    display_name    TEXT,
    date_added      TEXT NOT NULL,
    expires_at      TEXT
);

CREATE TABLE IF NOT EXISTS credential_health (
    site_category   TEXT PRIMARY KEY,
    health_status   TEXT NOT NULL DEFAULT 'unknown',
    last_checked_at TEXT,
    last_error      TEXT
);

CREATE TABLE IF NOT EXISTS duplicate (
    file_id_a       INTEGER NOT NULL REFERENCES media_file(file_id),
    file_id_b       INTEGER NOT NULL REFERENCES media_file(file_id),
    distance        INTEGER NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'detected',
    decision_at     TEXT,
    decision_source TEXT,
    decision_reason TEXT,
    winner_file_id  INTEGER,
    loser_file_id   INTEGER,
    PRIMARY KEY (file_id_a, file_id_b),
    CHECK (file_id_a < file_id_b)
);

CREATE TABLE IF NOT EXISTS file_color (
    rowid   INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
    hex     TEXT    NOT NULL,
    l       REAL    NOT NULL,
    a       REAL    NOT NULL,
    b       REAL    NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS file_color_rtree USING rtree(id, min_l, max_l, min_a, max_a, min_b, max_b);
CREATE VIRTUAL TABLE IF NOT EXISTS entity_fts USING fts5(name, notes, source_urls, content=media_entity, content_rowid=entity_id);

-- Projection tables (derived, deletable)

CREATE TABLE IF NOT EXISTS sidebar_node (
    node_id             TEXT PRIMARY KEY,
    kind                TEXT NOT NULL,
    parent_id           TEXT,
    name                TEXT NOT NULL,
    icon                TEXT,
    color               TEXT,
    sort_order          INTEGER,
    count               INTEGER,
    freshness           TEXT NOT NULL DEFAULT 'stale',
    epoch               INTEGER NOT NULL DEFAULT 0,
    selectable          INTEGER NOT NULL DEFAULT 1,
    expanded_by_default INTEGER DEFAULT 0,
    meta_json           TEXT,
    date_modified       TEXT
);

CREATE TABLE IF NOT EXISTS view_pref (
    scope       TEXT PRIMARY KEY,
    sort_field  TEXT,
    sort_dir    TEXT,
    layout      TEXT,
    tile_size   INTEGER,
    show_name   INTEGER DEFAULT 1,
    show_resolution INTEGER DEFAULT 0,
    show_extension  INTEGER DEFAULT 0,
    show_label      INTEGER DEFAULT 0,
    thumbnail_fit   TEXT DEFAULT 'cover'
);

CREATE TABLE IF NOT EXISTS kv_settings (
    key   TEXT PRIMARY KEY,
    value TEXT
);

CREATE TABLE IF NOT EXISTS manifest (
    key   TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL DEFAULT 0
);

-- Durable sync op outbox: one row per truth mutation, written in the same
-- transaction as the mutation. Device-local; drained into remote segments
-- by the sync engine (uploaded_seq marks drained rows).
CREATE TABLE IF NOT EXISTS op_outbox (
    op_id        INTEGER PRIMARY KEY,
    op_version   INTEGER NOT NULL,
    op_type      TEXT    NOT NULL,
    entity_key   TEXT    NOT NULL,
    payload_json TEXT    NOT NULL,
    hlc          TEXT    NOT NULL CHECK (
        length(hlc) = 18
        AND substr(hlc, 14, 1) = '-'
        AND substr(hlc, 1, 13) NOT GLOB '*[^0-9a-f]*'
        AND substr(hlc, 15, 4) NOT GLOB '*[^0-9a-f]*'
    ),
    device_id    TEXT    NOT NULL,
    created_at   TEXT    NOT NULL,
    uploaded_seq INTEGER
);
CREATE INDEX IF NOT EXISTS idx_op_outbox_pending ON op_outbox(op_id) WHERE uploaded_seq IS NULL;

-- Durable last-writer clocks for independently mergeable truth fields.
CREATE TABLE IF NOT EXISTS sync_conflict_clock (
    target_kind TEXT NOT NULL,
    target_key  TEXT NOT NULL,
    field_key   TEXT NOT NULL,
    hlc         TEXT NOT NULL CHECK (
        length(hlc) = 18
        AND substr(hlc, 14, 1) = '-'
        AND substr(hlc, 1, 13) NOT GLOB '*[^0-9a-f]*'
        AND substr(hlc, 15, 4) NOT GLOB '*[^0-9a-f]*'
    ),
    device_id   TEXT NOT NULL,
    PRIMARY KEY (target_kind, target_key, field_key)
);

-- Per-peer ingestion progress: highest contiguous remote segment applied.
CREATE TABLE IF NOT EXISTS sync_ingest_cursor (
    device_id    TEXT PRIMARY KEY,
    consumed_seq INTEGER NOT NULL DEFAULT 0
);

-- Remote originals that metadata is waiting for. The ingest cursor does not
-- advance until these blobs arrive, and this state survives process restart.
CREATE TABLE IF NOT EXISTS sync_missing_blob (
    entity_hash   TEXT PRIMARY KEY,
    object_key    TEXT NOT NULL,
    extension     TEXT NOT NULL,
    status        TEXT NOT NULL CHECK (status IN ('pending', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at  TEXT NOT NULL,
    last_error    TEXT,
    first_seen_at TEXT NOT NULL,
    updated_at    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

INSERT INTO schema_version (version)
SELECT 114
WHERE NOT EXISTS (SELECT 1 FROM schema_version);
"#;

/// Create a fresh pre-1.0 schema or validate an exact current-schema match.
/// Schema conversion starts only after the 1.0 format is locked.
pub fn initialize_schema(conn: &Connection) -> Result<(), String> {
    let user_table_count = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master
             WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| format!("Failed to inspect library schema: {error}"))?;

    if user_table_count == 0 {
        conn.execute_batch(LIBRARY_DDL)
            .map_err(|error| format!("Failed to create library schema: {error}"))?;
    } else if !has_schema_version_table(conn)? {
        return Err(
            "Unsupported pre-1.0 library schema: schema_version is missing. Create a new library."
                .to_owned(),
        );
    }

    let version = read_schema_version(conn)?;
    if version != CURRENT_SCHEMA_VERSION {
        return Err(format!(
            "Unsupported pre-1.0 library schema version {version}; this build requires exactly {CURRENT_SCHEMA_VERSION}. Create a new library."
        ));
    }
    validate_current_schema(conn)
}

pub(crate) fn has_schema_version_table(conn: &Connection) -> Result<bool, String> {
    table_exists(conn, "schema_version")
}

pub(crate) fn read_schema_version(conn: &Connection) -> Result<i64, String> {
    let mut statement = conn
        .prepare("SELECT version FROM schema_version")
        .map_err(|error| format!("Failed to read canonical schema version: {error}"))?;
    let versions = statement
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(|error| format!("Failed to read canonical schema version: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("Failed to read canonical schema version: {error}"))?;
    match versions.as_slice() {
        [version] => Ok(*version),
        _ => Err(format!(
            "Canonical schema_version must contain exactly one row; found {}",
            versions.len()
        )),
    }
}

fn validate_current_schema(conn: &Connection) -> Result<(), String> {
    const TABLE_PROBES: &[(&str, &str)] = &[
        ("media_entity", "SELECT entity_id FROM media_entity WHERE 0"),
        (
            "media_view",
            "SELECT entity_id, viewed_at FROM media_view WHERE 0",
        ),
        (
            "media_file_phash_index",
            "SELECT file_id, partition_0, partition_1, partition_2, partition_3, partition_4, partition_5, partition_6, partition_7 FROM media_file_phash_index WHERE 0",
        ),
        (
            "media_file",
            "SELECT file_id, color_analysis_version FROM media_file WHERE 0",
        ),
        (
            "single_media_entity",
            "SELECT entity_id FROM single_media_entity WHERE 0",
        ),
        ("tag", "SELECT tag_id FROM tag WHERE 0"),
        (
            "entity_tag",
            "SELECT entity_id, provenance_mask FROM entity_tag WHERE 0",
        ),
        ("tag_alias", "SELECT from_tag_id FROM tag_alias WHERE 0"),
        (
            "tag_implication",
            "SELECT child_tag_id FROM tag_implication WHERE 0",
        ),
        ("tag_ancestor", "SELECT tag_id FROM tag_ancestor WHERE 0"),
        (
            "entity_tag_implied",
            "SELECT entity_id FROM entity_tag_implied WHERE 0",
        ),
        ("tag_display", "SELECT tag_id FROM tag_display WHERE 0"),
        (
            "folder",
            "SELECT folder_id, notes, total_size_bytes, pinned, pin_order, uuid FROM folder WHERE 0",
        ),
        (
            "folder_member",
            "SELECT folder_id FROM folder_member WHERE 0",
        ),
        (
            "smart_folder",
            "SELECT smart_folder_id, notes, total_size_bytes, pinned, pin_order, uuid FROM smart_folder WHERE 0",
        ),
        (
            "subscription_group",
            "SELECT group_id, uuid FROM subscription_group WHERE 0",
        ),
        (
            "subscription",
            "SELECT subscription_id, schedule, uuid FROM subscription WHERE 0",
        ),
        (
            "subscription_query",
            "SELECT query_id, site_id, query_kind, notes, last_success_at, last_failure_at, last_failure_kind, last_failure_message FROM subscription_query WHERE 0",
        ),
        (
            "subscription_entity",
            "SELECT subscription_id FROM subscription_entity WHERE 0",
        ),
        (
            "subscription_post_collection",
            "SELECT subscription_id FROM subscription_post_collection WHERE 0",
        ),
        (
            "ingest_queue",
            "SELECT queue_id, queue_kind, source_kind, subscription_id, query_id, query_run_id, cleanup_root, post_id, category, preferred_name, expected_count, status, last_error, created_at, updated_at FROM ingest_queue WHERE 0",
        ),
        (
            "ingest_queue_item",
            "SELECT item_id, queue_id, source_path, page_num, payload_json, delete_after_ingest, status, result_kind, resolved_entity_hash, resolved_file_hash, last_error, created_at, updated_at FROM ingest_queue_item WHERE 0",
        ),
        (
            "subscription_run",
            "SELECT run_id, subscription_id, started_at, finished_at, status, failure_kind, error_message, files_downloaded, files_skipped, metadata_validated, metadata_invalid FROM subscription_run WHERE 0",
        ),
        (
            "subscription_query_run",
            "SELECT query_run_id, run_id, subscription_id, query_id, started_at, finished_at, status, failure_kind, error_message, posts_processed, files_downloaded, files_skipped, metadata_validated, metadata_invalid FROM subscription_query_run WHERE 0",
        ),
        (
            "subscription_query_job",
            "SELECT job_id, run_id, subscription_id, query_id, site_id, status, job_kind, requested_by, post_id, attempt_count, available_at, queued_at, started_at, finished_at, failure_kind, error_message FROM subscription_query_job WHERE 0",
        ),
        (
            "subscription_issue",
            "SELECT issue_id, issue_key, subscription_id, query_id, issue_kind, status, message, detail, first_seen_at, last_seen_at, resolved_at, recovery_action, next_retry_at FROM subscription_issue WHERE 0",
        ),
        (
            "subscription_download_attempt",
            "SELECT attempt_id, subscription_id, query_id, query_run_id, item_key, site_category, post_id, page_num, canonical_post_url, media_url, retry_url, retry_count, status, failure_kind, last_error, next_retry_at, created_at, updated_at, resolved_at FROM subscription_download_attempt WHERE 0",
        ),
        (
            "subscription_post_member",
            "SELECT subscription_id, site_id, post_id, item_key, page_num, canonical_post_url, media_url, entity_hash, status, created_at, updated_at FROM subscription_post_member WHERE 0",
        ),
        (
            "deferred_work_item",
            "SELECT work_id FROM deferred_work_item WHERE 0",
        ),
        (
            "credential_domain",
            "SELECT site_category, expires_at FROM credential_domain WHERE 0",
        ),
        (
            "credential_health",
            "SELECT site_category FROM credential_health WHERE 0",
        ),
        ("duplicate", "SELECT file_id_a FROM duplicate WHERE 0"),
        ("file_color", "SELECT file_id FROM file_color WHERE 0"),
        (
            "file_color_rtree",
            "SELECT id, min_l, max_l, min_a, max_a, min_b, max_b FROM file_color_rtree WHERE 0",
        ),
        ("entity_fts", "SELECT rowid FROM entity_fts WHERE 0"),
        ("sidebar_node", "SELECT node_id FROM sidebar_node WHERE 0"),
        ("view_pref", "SELECT scope FROM view_pref WHERE 0"),
        ("kv_settings", "SELECT key FROM kv_settings WHERE 0"),
        ("manifest", "SELECT key FROM manifest WHERE 0"),
        (
            "op_outbox",
            "SELECT op_id, op_version, op_type, entity_key, payload_json, hlc, device_id, created_at, uploaded_seq FROM op_outbox WHERE 0",
        ),
        (
            "sync_ingest_cursor",
            "SELECT device_id, consumed_seq FROM sync_ingest_cursor WHERE 0",
        ),
        (
            "sync_conflict_clock",
            "SELECT target_kind, target_key, field_key, hlc, device_id FROM sync_conflict_clock WHERE 0",
        ),
        (
            "sync_missing_blob",
            "SELECT entity_hash, object_key, extension, status, attempt_count, available_at, last_error, first_seen_at, updated_at FROM sync_missing_blob WHERE 0",
        ),
    ];
    const REQUIRED_INDEXES: &[(&str, &str, &[(&str, bool)])] = &[
        ("idx_me_status", "media_entity", &[("status", false)]),
        ("idx_me_kind", "media_entity", &[("entity_kind", false)]),
        (
            "idx_me_parent",
            "media_entity",
            &[("parent_collection_entity_id", false)],
        ),
        (
            "idx_me_date_added",
            "media_entity",
            &[("date_added", false)],
        ),
        ("idx_me_rating", "media_entity", &[("rating", false)]),
        (
            "idx_media_view_viewed_at",
            "media_view",
            &[("viewed_at", true)],
        ),
        (
            "idx_mf_phash_p0",
            "media_file_phash_index",
            &[("partition_0", false)],
        ),
        (
            "idx_mf_phash_p1",
            "media_file_phash_index",
            &[("partition_1", false)],
        ),
        (
            "idx_mf_phash_p2",
            "media_file_phash_index",
            &[("partition_2", false)],
        ),
        (
            "idx_mf_phash_p3",
            "media_file_phash_index",
            &[("partition_3", false)],
        ),
        (
            "idx_mf_phash_p4",
            "media_file_phash_index",
            &[("partition_4", false)],
        ),
        (
            "idx_mf_phash_p5",
            "media_file_phash_index",
            &[("partition_5", false)],
        ),
        (
            "idx_mf_phash_p6",
            "media_file_phash_index",
            &[("partition_6", false)],
        ),
        (
            "idx_mf_phash_p7",
            "media_file_phash_index",
            &[("partition_7", false)],
        ),
        (
            "idx_entity_tag_tag_id",
            "entity_tag",
            &[("tag_id", false), ("entity_id", false)],
        ),
        (
            "idx_entity_tag_implied_tag_id",
            "entity_tag_implied",
            &[("tag_id", false), ("entity_id", false)],
        ),
        (
            "idx_folder_member_entity_id",
            "folder_member",
            &[("entity_id", false), ("folder_id", false)],
        ),
        ("idx_folder_uuid", "folder", &[("uuid", false)]),
        ("idx_smart_folder_uuid", "smart_folder", &[("uuid", false)]),
        (
            "idx_subscription_group_uuid",
            "subscription_group",
            &[("uuid", false)],
        ),
        ("idx_subscription_uuid", "subscription", &[("uuid", false)]),
        (
            "idx_ingest_queue_ready",
            "ingest_queue",
            &[
                ("status", false),
                ("created_at", false),
                ("queue_id", false),
            ],
        ),
        (
            "idx_ingest_queue_subscription",
            "ingest_queue",
            &[
                ("subscription_id", false),
                ("status", false),
                ("queue_id", false),
            ],
        ),
        (
            "idx_ingest_queue_item_queue",
            "ingest_queue_item",
            &[
                ("queue_id", false),
                ("status", false),
                ("page_num", false),
                ("item_id", false),
            ],
        ),
        (
            "idx_subscription_query_job_ready",
            "subscription_query_job",
            &[
                ("status", false),
                ("available_at", false),
                ("job_id", false),
            ],
        ),
        (
            "idx_subscription_query_job_subscription",
            "subscription_query_job",
            &[
                ("subscription_id", false),
                ("status", false),
                ("queued_at", false),
                ("job_id", false),
            ],
        ),
        (
            "idx_subscription_download_attempt_retry",
            "subscription_download_attempt",
            &[
                ("subscription_id", false),
                ("query_id", false),
                ("site_category", false),
                ("post_id", false),
                ("status", false),
            ],
        ),
        (
            "idx_subscription_issue_key",
            "subscription_issue",
            &[("issue_key", false)],
        ),
        ("idx_op_outbox_pending", "op_outbox", &[("op_id", false)]),
    ];
    const UNIQUE_INDEXES: &[&str] = &[
        "idx_folder_uuid",
        "idx_smart_folder_uuid",
        "idx_subscription_group_uuid",
        "idx_subscription_uuid",
        "idx_subscription_issue_key",
    ];
    const PARTIAL_INDEXES: &[(&str, &str)] = &[
        ("idx_folder_uuid", "uuid is not null"),
        ("idx_smart_folder_uuid", "uuid is not null"),
        ("idx_subscription_group_uuid", "uuid is not null"),
        ("idx_subscription_uuid", "uuid is not null"),
        ("idx_op_outbox_pending", "uploaded_seq is null"),
    ];

    for (table, probe) in TABLE_PROBES {
        conn.prepare(probe).map_err(|error| {
            format!(
                "Canonical schema version {CURRENT_SCHEMA_VERSION} is missing or incompatible at {table}: {error}"
            )
        })?;
    }
    for (index, expected_table, expected_columns) in REQUIRED_INDEXES {
        let actual_index: Option<(String, String)> = conn
            .query_row(
                "SELECT tbl_name, sql FROM sqlite_master WHERE type = 'index' AND name = ?1",
                [index],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("Failed to validate canonical index {index}: {error}"))?;
        let Some((actual_table, actual_sql)) = actual_index else {
            return Err(format!(
                "Canonical schema version {CURRENT_SCHEMA_VERSION} is missing required index {index}"
            ));
        };
        if actual_table != *expected_table {
            return Err(format!(
                "Canonical schema version {CURRENT_SCHEMA_VERSION} has incompatible index {index}: expected table {expected_table}, found {actual_table}"
            ));
        }

        let (actual_unique, actual_partial): (bool, bool) = conn
            .query_row(
                "SELECT \"unique\", partial FROM pragma_index_list(?1) WHERE name = ?2",
                params![expected_table, index],
                |row| Ok((row.get::<_, i64>(0)? != 0, row.get::<_, i64>(1)? != 0)),
            )
            .map_err(|error| format!("Failed to inspect canonical index {index}: {error}"))?;
        let expected_unique = UNIQUE_INDEXES.contains(index);
        let expected_predicate = PARTIAL_INDEXES
            .iter()
            .find_map(|(name, predicate)| (*name == *index).then_some(*predicate));
        let actual_predicate = actual_sql
            .to_ascii_lowercase()
            .split_once(" where ")
            .map(|(_, predicate)| predicate.split_whitespace().collect::<Vec<_>>().join(" "));
        if actual_unique != expected_unique
            || actual_partial != expected_predicate.is_some()
            || actual_predicate.as_deref() != expected_predicate
        {
            return Err(format!(
                "Canonical schema version {CURRENT_SCHEMA_VERSION} has incompatible index {index}: expected unique={expected_unique}, predicate={expected_predicate:?}; found unique={actual_unique}, predicate={actual_predicate:?}"
            ));
        }

        let mut stmt = conn
            .prepare(
                "SELECT name, \"desc\" FROM pragma_index_xinfo(?1) WHERE key = 1 ORDER BY seqno",
            )
            .map_err(|error| format!("Failed to inspect canonical index {index}: {error}"))?;
        let actual_columns: Vec<(String, bool)> = stmt
            .query_map([index], |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)))
            .map_err(|error| format!("Failed to inspect canonical index {index}: {error}"))?
            .collect::<Result<_, _>>()
            .map_err(|error| format!("Failed to inspect canonical index {index}: {error}"))?;
        let expected_columns: Vec<(String, bool)> = expected_columns
            .iter()
            .map(|(name, descending)| ((*name).to_string(), *descending))
            .collect();
        if actual_columns != expected_columns {
            return Err(format!(
                "Canonical schema version {CURRENT_SCHEMA_VERSION} has incompatible index {index}: expected columns {expected_columns:?}, found {actual_columns:?}"
            ));
        }
    }
    let missing_blob_sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'sync_missing_blob'",
            [],
            |row| row.get(0),
        )
        .map_err(|error| format!("Failed to validate sync_missing_blob: {error}"))?;
    let normalized_sql = missing_blob_sql
        .to_ascii_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !normalized_sql.contains("check (status in ('pending', 'failed'))") {
        return Err(format!(
            "Canonical schema version {CURRENT_SCHEMA_VERSION} has incompatible sync_missing_blob status constraints"
        ));
    }
    Ok(())
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
    .map_err(|error| error.to_string())
}
