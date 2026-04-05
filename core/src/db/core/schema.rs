//! Library database schema — authoritative table definitions.
//! No other module may define or assume table structure.

/// Full DDL for a new library database.
pub const LIBRARY_DDL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

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

CREATE TABLE IF NOT EXISTS single_media_entity (
    entity_id  INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    file_id    INTEGER NOT NULL UNIQUE REFERENCES media_file(file_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS tag (
    tag_id     INTEGER PRIMARY KEY,
    namespace  TEXT    NOT NULL DEFAULT '',
    subtag     TEXT    NOT NULL,
    site_mask  INTEGER NOT NULL DEFAULT 0,
    file_count INTEGER NOT NULL DEFAULT 0,
    UNIQUE (namespace, subtag)
);

CREATE TABLE IF NOT EXISTS entity_tag (
    entity_id  INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag_id     INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    provenance_mask INTEGER NOT NULL DEFAULT 0,
    source     TEXT    NOT NULL DEFAULT 'local',
    PRIMARY KEY (entity_id, tag_id, source)
);

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
    entity_id INTEGER NOT NULL,
    tag_id    INTEGER NOT NULL,
    PRIMARY KEY (entity_id, tag_id)
);

CREATE TABLE IF NOT EXISTS tag_display (
    tag_id     INTEGER PRIMARY KEY REFERENCES tag(tag_id) ON DELETE CASCADE,
    display_ns TEXT,
    display_st TEXT
);

CREATE VIRTUAL TABLE IF NOT EXISTS tag_fts USING fts5(namespace, subtag, content=tag, content_rowid=tag_id);

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
    date_added                 TEXT    NOT NULL,
    date_modified              TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS folder_member (
    folder_id     INTEGER NOT NULL REFERENCES folder(folder_id) ON DELETE CASCADE,
    entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    position_rank INTEGER,
    PRIMARY KEY (folder_id, entity_id)
);

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
    date_added       TEXT    NOT NULL,
    date_modified    TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS subscription_group (
    group_id   INTEGER PRIMARY KEY,
    name       TEXT    NOT NULL,
    schedule   TEXT    NOT NULL DEFAULT 'manual',
    date_added TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS subscription (
    subscription_id    INTEGER PRIMARY KEY,
    name               TEXT    NOT NULL,
    site_id            TEXT    NOT NULL,
    paused             INTEGER NOT NULL DEFAULT 0,
    group_id           INTEGER REFERENCES subscription_group(group_id) ON DELETE CASCADE,
    initial_post_limit INTEGER DEFAULT 100,
    periodic_post_limit INTEGER DEFAULT 100,
    auto_collections   INTEGER NOT NULL DEFAULT 1,
    date_added         TEXT    NOT NULL
);

CREATE TABLE IF NOT EXISTS subscription_query (
    query_id            INTEGER PRIMARY KEY,
    subscription_id     INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    site_id             TEXT    NOT NULL,
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
    files_skipped        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS subscription_issue (
    issue_id             INTEGER PRIMARY KEY,
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id             INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    issue_kind           TEXT    NOT NULL,
    status               TEXT    NOT NULL DEFAULT 'open',
    message              TEXT    NOT NULL,
    detail               TEXT,
    first_seen_at        TEXT    NOT NULL,
    last_seen_at         TEXT    NOT NULL,
    resolved_at          TEXT,
    UNIQUE (subscription_id, query_id, issue_kind, message)
);

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
    work_type     TEXT    NOT NULL CHECK (work_type IN ('thumbnail', 'dominant_colors', 'perceptual_hash')),
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
    date_added      TEXT NOT NULL
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

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);

INSERT OR IGNORE INTO schema_version (version) VALUES (100);
"#;
