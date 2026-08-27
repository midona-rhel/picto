//! Exact pre-1.0 schema for the replacement backend.

use rusqlite::{Connection, OptionalExtension, Transaction};

pub const CURRENT_SCHEMA_VERSION: i64 = 1;
pub const CURRENT_SCHEMA_FINGERPRINT: &str = "picto-canonical-bitmap-v1";
pub const CURRENT_PHASH_ANALYSIS_VERSION: i64 = 5;
pub const PHASH_VERSION_SETTING: &str = "media.perceptual_hash_version";
const SCHEMA_V1_DDL: &str = include_str!("schema_v1.sql");

pub const SUBSCRIPTION_READ_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_subscription_source_post_query
    ON subscription_source_post(query_id, source_post_id);
CREATE INDEX IF NOT EXISTS idx_subscription_run_query_success
    ON subscription_run_query(query_id)
    WHERE status = 'succeeded';
"#;

pub const PERFORMANCE_INDEXES: &str = r#"
CREATE INDEX IF NOT EXISTS idx_cloud_blob_upload_priority
    ON cloud_blob_state(remote_present, state, priority DESC, updated_at);
CREATE INDEX IF NOT EXISTS idx_source_post_provisional
    ON source_post(source_post_id) WHERE root_item_id IS NULL;
CREATE INDEX IF NOT EXISTS idx_media_file_mime
    ON media_file(mime_type, file_id);
CREATE INDEX IF NOT EXISTS idx_media_file_size
    ON media_file(size_bytes, file_id);
CREATE INDEX IF NOT EXISTS idx_media_file_width
    ON media_file(pixel_width, file_id);
CREATE INDEX IF NOT EXISTS idx_media_file_height
    ON media_file(pixel_height, file_id);
CREATE INDEX IF NOT EXISTS idx_media_file_duration
    ON media_file(duration_ms, file_id);
CREATE INDEX IF NOT EXISTS idx_media_file_audio
    ON media_file(has_audio, file_id);
CREATE INDEX IF NOT EXISTS idx_media_asset_imported
    ON media_asset(imported_at, item_id);
CREATE INDEX IF NOT EXISTS idx_media_asset_captured
    ON media_asset(captured_at, item_id);
CREATE INDEX IF NOT EXISTS idx_root_summary_imported_asc
    ON root_summary(lifecycle, imported_at ASC, root_item_id ASC);
CREATE INDEX IF NOT EXISTS idx_file_color_lookup
    ON file_color(file_id, hex);
CREATE INDEX IF NOT EXISTS idx_source_item_media
    ON source_item(media_item_id, source_item_id)
    WHERE media_item_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_folder_parent_order
    ON folder(parent_id, sort_rank, folder_id);
CREATE INDEX IF NOT EXISTS idx_smart_folder_parent_order
    ON smart_folder(parent_id, display_order, smart_folder_id);
CREATE INDEX IF NOT EXISTS idx_source_post_site_key
    ON source_post(site_id, post_key, source_post_id);
"#;

const SMART_FOLDER_READ_MODEL_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS smart_folder_root (
    smart_folder_id INTEGER NOT NULL
        REFERENCES smart_folder(smart_folder_id) ON DELETE CASCADE,
    root_item_id INTEGER NOT NULL
        REFERENCES library_root(item_id) ON DELETE CASCADE,
    PRIMARY KEY (smart_folder_id, root_item_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_smart_folder_root_root
    ON smart_folder_root(root_item_id, smart_folder_id);
"#;

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
    item_id INTEGER PRIMARY KEY AUTOINCREMENT
        CHECK (item_id BETWEEN 1 AND 4294967295),
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

CREATE TABLE root_metadata (
    root_item_id INTEGER PRIMARY KEY REFERENCES library_root(item_id) ON DELETE CASCADE,
    name TEXT,
    rating INTEGER,
    notes TEXT,
    source_urls_json TEXT NOT NULL DEFAULT '[]',
    updated_at TEXT NOT NULL
);

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
    tag_id INTEGER PRIMARY KEY AUTOINCREMENT
        CHECK (tag_id BETWEEN 1 AND 4294967295),
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

CREATE TABLE root_tag (
    root_item_id INTEGER NOT NULL REFERENCES library_root(item_id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    provenance_mask INTEGER NOT NULL DEFAULT 0,
    source_mask INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (root_item_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX idx_root_tag_tag ON root_tag(tag_id, root_item_id);

CREATE TABLE tag_summary (
    tag_id INTEGER PRIMARY KEY REFERENCES tag(tag_id) ON DELETE CASCADE,
    visible_root_count INTEGER NOT NULL DEFAULT 0,
    assignment_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE folder (
    folder_id INTEGER PRIMARY KEY AUTOINCREMENT
        CHECK (folder_id BETWEEN 1 AND 4294967295),
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
    smart_folder_id INTEGER PRIMARY KEY AUTOINCREMENT
        CHECK (smart_folder_id BETWEEN 1 AND 4294967295),
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

CREATE TABLE smart_folder_dependency (
    smart_folder_id INTEGER NOT NULL
        REFERENCES smart_folder(smart_folder_id) ON DELETE CASCADE,
    dependency_kind TEXT NOT NULL,
    dependency_key TEXT NOT NULL,
    PRIMARY KEY (smart_folder_id, dependency_kind, dependency_key)
) WITHOUT ROWID;
CREATE INDEX idx_smart_folder_dependency_lookup
    ON smart_folder_dependency(dependency_kind, dependency_key, smart_folder_id);

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

CREATE TABLE cloud_state (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    library_id TEXT NOT NULL UNIQUE,
    device_id TEXT NOT NULL,
    provider TEXT CHECK (provider IN ('google_drive', 'dropbox')),
    account_label TEXT,
    remote_root TEXT,
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
    state TEXT NOT NULL DEFAULT 'disabled',
    phase TEXT NOT NULL DEFAULT 'idle',
    blocking INTEGER NOT NULL DEFAULT 0 CHECK (blocking IN (0, 1)),
    completed_units INTEGER NOT NULL DEFAULT 0,
    total_units INTEGER,
    message TEXT NOT NULL DEFAULT '',
    last_sync_at TEXT,
    last_snapshot_at TEXT,
    pending_blobs INTEGER NOT NULL DEFAULT 0,
    missing_blobs INTEGER NOT NULL DEFAULT 0,
    schema_generation INTEGER NOT NULL DEFAULT 1,
    hlc_physical_ms INTEGER NOT NULL DEFAULT 0,
    hlc_logical INTEGER NOT NULL DEFAULT 0,
    retention_json TEXT NOT NULL DEFAULT '{"daily":30,"weekly":26,"yearly":5,"epochs_days":30,"deleted_blobs_days":30,"full_media_history":false}'
);

CREATE TABLE cloud_outbox (
    mutation_id TEXT PRIMARY KEY,
    library_id TEXT NOT NULL,
    device_id TEXT NOT NULL,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    causal_frontier_json TEXT NOT NULL,
    operation TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    schema_generation INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    byte_size INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    published_at TEXT,
    current_epoch INTEGER NOT NULL DEFAULT 0 CHECK (current_epoch IN (0, 1))
);
CREATE INDEX idx_cloud_outbox_pending
    ON cloud_outbox(published_at, hlc_physical_ms, hlc_logical, mutation_id);
CREATE INDEX idx_cloud_outbox_current
    ON cloud_outbox(current_epoch, hlc_physical_ms, hlc_logical, mutation_id);

CREATE TABLE cloud_applied_mutation (
    mutation_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    applied_at TEXT NOT NULL
);

CREATE TABLE cloud_device_frontier (
    device_id TEXT PRIMARY KEY,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE cloud_field_clock (
    object_kind TEXT NOT NULL,
    object_key TEXT NOT NULL,
    field_name TEXT NOT NULL,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    mutation_id TEXT NOT NULL,
    PRIMARY KEY (object_kind, object_key, field_name)
);

CREATE TABLE cloud_membership_clock (
    relation_kind TEXT NOT NULL,
    owner_key TEXT NOT NULL,
    member_key TEXT NOT NULL,
    present INTEGER NOT NULL CHECK (present IN (0, 1)),
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    mutation_id TEXT NOT NULL,
    causal_frontier_json TEXT NOT NULL,
    PRIMARY KEY (relation_kind, owner_key, member_key)
);

CREATE TABLE cloud_tombstone (
    object_kind TEXT NOT NULL,
    object_key TEXT NOT NULL,
    mutation_id TEXT NOT NULL,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    causal_frontier_json TEXT NOT NULL,
    deleted_at TEXT NOT NULL,
    purge_after TEXT,
    PRIMARY KEY (object_kind, object_key)
);

CREATE TABLE cloud_quarantine (
    quarantine_id INTEGER PRIMARY KEY,
    mutation_id TEXT NOT NULL UNIQUE,
    reason TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT
);

CREATE TABLE cloud_snapshot (
    snapshot_id TEXT PRIMARY KEY,
    frontier_json TEXT NOT NULL,
    database_sha256 TEXT NOT NULL,
    artifact_sha256 TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    verified INTEGER NOT NULL DEFAULT 0 CHECK (verified IN (0, 1)),
    created_at TEXT NOT NULL,
    remote_path TEXT,
    published_at TEXT
);

CREATE TABLE cloud_blob_state (
    file_hash TEXT PRIMARY KEY REFERENCES media_file(file_hash) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('available', 'queued', 'downloading', 'missing_remote', 'corrupt')),
    priority INTEGER NOT NULL DEFAULT 0,
    remote_present INTEGER NOT NULL DEFAULT 0 CHECK (remote_present IN (0, 1)),
    remote_extension TEXT,
    last_error TEXT,
    uploaded_at TEXT,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_cloud_blob_queue ON cloud_blob_state(state, priority DESC, updated_at, file_hash);
CREATE INDEX idx_cloud_blob_upload ON cloud_blob_state(remote_present, state, priority DESC, updated_at, file_hash);

-- These tables are disposable projections of canonical roots and memberships.
-- They are rebuilt incrementally after each canonical transaction.
CREATE TABLE read_model_dirty_root (
    root_item_id INTEGER PRIMARY KEY
);

CREATE TABLE projection_write_control (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    suppress_folder_summary INTEGER NOT NULL DEFAULT 0
        CHECK (suppress_folder_summary IN (0, 1)),
    suppress_tag_summary INTEGER NOT NULL DEFAULT 0
        CHECK (suppress_tag_summary IN (0, 1)),
    suppress_smart_dirty INTEGER NOT NULL DEFAULT 0
        CHECK (suppress_smart_dirty IN (0, 1))
);
INSERT INTO projection_write_control(singleton) VALUES (1);

CREATE TABLE folder_summary (
    folder_id INTEGER PRIMARY KEY REFERENCES folder(folder_id) ON DELETE CASCADE,
    root_count INTEGER NOT NULL DEFAULT 0,
    media_count INTEGER NOT NULL DEFAULT 0,
    total_size_bytes INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE root_summary (
    root_item_id INTEGER PRIMARY KEY REFERENCES library_root(item_id) ON DELETE CASCADE,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('inbox', 'active', 'trash')),
    collection_member_count INTEGER NOT NULL,
    total_size_bytes INTEGER NOT NULL,
    imported_at TEXT,
    captured_at TEXT,
    sort_rating INTEGER,
    first_media_item_id INTEGER REFERENCES media_asset(item_id) ON DELETE SET NULL
);

CREATE TABLE lifecycle_summary (
    lifecycle TEXT PRIMARY KEY CHECK (lifecycle IN ('inbox', 'active', 'trash')),
    root_count INTEGER NOT NULL DEFAULT 0,
    media_count INTEGER NOT NULL DEFAULT 0,
    total_size_bytes INTEGER NOT NULL DEFAULT 0
);
INSERT INTO lifecycle_summary (lifecycle) VALUES ('inbox'), ('active'), ('trash');

CREATE TABLE projection_checkpoint (
    component TEXT PRIMARY KEY,
    schema_fingerprint TEXT NOT NULL,
    implementation_hash TEXT NOT NULL,
    database_revision INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    health TEXT NOT NULL CHECK (health IN ('healthy', 'rebuilding', 'unhealthy')),
    checkpoint_path TEXT,
    updated_at TEXT NOT NULL
);
CREATE INDEX idx_root_summary_imported_cover
    ON root_summary(imported_at, root_item_id, collection_member_count,
                    total_size_bytes, captured_at, sort_rating, first_media_item_id);
CREATE INDEX idx_root_summary_imported_asc
    ON root_summary(lifecycle, imported_at ASC, root_item_id ASC);
CREATE INDEX idx_root_summary_captured_cover
    ON root_summary(captured_at, root_item_id, collection_member_count,
                    total_size_bytes, imported_at, sort_rating, first_media_item_id);
CREATE INDEX idx_root_summary_rating_cover
    ON root_summary(sort_rating, root_item_id, collection_member_count,
                    total_size_bytes, imported_at, captured_at, first_media_item_id);
CREATE INDEX idx_root_summary_size_cover
    ON root_summary(total_size_bytes, root_item_id, collection_member_count,
                    imported_at, captured_at, sort_rating, first_media_item_id);

CREATE TABLE root_tag_count (
    root_item_id INTEGER NOT NULL REFERENCES library_root(item_id) ON DELETE CASCADE,
    tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    media_count INTEGER NOT NULL,
    PRIMARY KEY (root_item_id, tag_id)
);
CREATE INDEX idx_root_tag_count_tag_cover
    ON root_tag_count(tag_id, root_item_id, media_count);

-- Search text is indexed once per object. Tag and folder membership remains
-- relational so a rename never rewrites every matching library item.
CREATE TABLE search_dirty_item (
    item_id INTEGER PRIMARY KEY
);
CREATE TABLE search_dirty_media (
    media_item_id INTEGER PRIMARY KEY
);
CREATE TABLE search_dirty_tag (
    tag_id INTEGER PRIMARY KEY
);
CREATE TABLE search_dirty_folder (
    folder_id INTEGER PRIMARY KEY
);

CREATE VIRTUAL TABLE item_search_fts USING fts5(
    item_id UNINDEXED,
    searchable_text,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);
CREATE VIRTUAL TABLE media_search_fts USING fts5(
    media_item_id UNINDEXED,
    searchable_text,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);
CREATE VIRTUAL TABLE tag_search_fts USING fts5(
    tag_id UNINDEXED,
    searchable_text,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);
CREATE VIRTUAL TABLE folder_search_fts USING fts5(
    folder_id UNINDEXED,
    searchable_text,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);

CREATE TRIGGER search_library_item_insert AFTER INSERT ON library_item BEGIN
    INSERT INTO search_dirty_item(item_id) VALUES (NEW.item_id)
    ON CONFLICT(item_id) DO NOTHING;
END;
CREATE TRIGGER search_library_item_update AFTER UPDATE OF kind, label ON library_item BEGIN
    INSERT INTO search_dirty_item(item_id) VALUES (NEW.item_id)
    ON CONFLICT(item_id) DO NOTHING;
END;
CREATE TRIGGER search_library_item_delete AFTER DELETE ON library_item BEGIN
    INSERT INTO search_dirty_item(item_id) VALUES (OLD.item_id)
    ON CONFLICT(item_id) DO NOTHING;
END;
CREATE TRIGGER search_collection_member_insert AFTER INSERT ON collection_member BEGIN
    INSERT INTO search_dirty_item(item_id) VALUES (NEW.media_item_id)
    ON CONFLICT(item_id) DO NOTHING;
END;
CREATE TRIGGER search_collection_member_update
AFTER UPDATE OF media_item_id ON collection_member BEGIN
    INSERT INTO search_dirty_item(item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(item_id) DO NOTHING;
    INSERT INTO search_dirty_item(item_id) VALUES (NEW.media_item_id)
    ON CONFLICT(item_id) DO NOTHING;
END;
CREATE TRIGGER search_collection_member_delete AFTER DELETE ON collection_member BEGIN
    INSERT INTO search_dirty_item(item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(item_id) DO NOTHING;
END;

CREATE TRIGGER search_media_asset_insert AFTER INSERT ON media_asset BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (NEW.item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_media_asset_update
AFTER UPDATE OF name, notes, source_urls_json ON media_asset BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (NEW.item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_media_asset_delete AFTER DELETE ON media_asset BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (OLD.item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_media_file_update AFTER UPDATE OF mime_type ON media_file BEGIN
    INSERT INTO search_dirty_media(media_item_id)
    SELECT item_id FROM media_asset WHERE file_id = NEW.file_id
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_item_insert AFTER INSERT ON source_item WHEN NEW.media_item_id IS NOT NULL BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (NEW.media_item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_item_update
AFTER UPDATE OF source_post_id, media_url, canonical_url, media_item_id ON source_item BEGIN
    INSERT INTO search_dirty_media(media_item_id)
    SELECT OLD.media_item_id WHERE OLD.media_item_id IS NOT NULL
    ON CONFLICT(media_item_id) DO NOTHING;
    INSERT INTO search_dirty_media(media_item_id)
    SELECT NEW.media_item_id WHERE NEW.media_item_id IS NOT NULL
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_item_delete AFTER DELETE ON source_item WHEN OLD.media_item_id IS NOT NULL BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_post_update
AFTER UPDATE OF site_id, post_key, canonical_url, creator_name, title, description ON source_post BEGIN
    INSERT INTO search_dirty_media(media_item_id)
    SELECT media_item_id FROM source_item
    WHERE source_post_id = NEW.source_post_id AND media_item_id IS NOT NULL
    ON CONFLICT(media_item_id) DO NOTHING;
END;

CREATE TRIGGER search_tag_insert AFTER INSERT ON tag BEGIN
    INSERT INTO search_dirty_tag(tag_id) VALUES (NEW.tag_id)
    ON CONFLICT(tag_id) DO NOTHING;
END;
CREATE TRIGGER search_tag_update AFTER UPDATE OF namespace, subtag ON tag BEGIN
    INSERT INTO search_dirty_tag(tag_id) VALUES (NEW.tag_id)
    ON CONFLICT(tag_id) DO NOTHING;
END;
CREATE TRIGGER search_tag_delete AFTER DELETE ON tag BEGIN
    INSERT INTO search_dirty_tag(tag_id) VALUES (OLD.tag_id)
    ON CONFLICT(tag_id) DO NOTHING;
END;
CREATE TRIGGER search_folder_insert AFTER INSERT ON folder BEGIN
    INSERT INTO search_dirty_folder(folder_id) VALUES (NEW.folder_id)
    ON CONFLICT(folder_id) DO NOTHING;
END;
CREATE TRIGGER search_folder_update AFTER UPDATE OF name, notes ON folder BEGIN
    INSERT INTO search_dirty_folder(folder_id) VALUES (NEW.folder_id)
    ON CONFLICT(folder_id) DO NOTHING;
END;
CREATE TRIGGER search_folder_delete AFTER DELETE ON folder BEGIN
    INSERT INTO search_dirty_folder(folder_id) VALUES (OLD.folder_id)
    ON CONFLICT(folder_id) DO NOTHING;
END;
CREATE TRIGGER read_model_library_root_update AFTER UPDATE OF item_id ON library_root BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (NEW.item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
END;
CREATE TRIGGER read_model_library_root_delete AFTER DELETE ON library_root BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
END;
CREATE TRIGGER read_model_library_root_lifecycle
AFTER UPDATE OF lifecycle ON library_root
WHEN NOT EXISTS (
    SELECT 1 FROM collection_member WHERE media_item_id = NEW.item_id
) BEGIN
    UPDATE lifecycle_summary
    SET root_count = root_count - 1,
        media_count = media_count - COALESCE((
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = NEW.item_id
        ), 0),
        total_size_bytes = total_size_bytes - COALESCE((
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = NEW.item_id
        ), 0)
    WHERE lifecycle = OLD.lifecycle;
    UPDATE lifecycle_summary
    SET root_count = root_count + 1,
        media_count = media_count + COALESCE((
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = NEW.item_id
        ), 0),
        total_size_bytes = total_size_bytes + COALESCE((
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = NEW.item_id
        ), 0)
    WHERE lifecycle = NEW.lifecycle;
    UPDATE root_summary SET lifecycle = NEW.lifecycle
    WHERE root_item_id = NEW.item_id;
END;
CREATE TRIGGER read_model_library_root_before_delete
BEFORE DELETE ON library_root
WHEN NOT EXISTS (
    SELECT 1 FROM collection_member WHERE media_item_id = OLD.item_id
) BEGIN
    UPDATE lifecycle_summary
    SET root_count = root_count - CASE WHEN EXISTS (
            SELECT 1 FROM root_summary WHERE root_item_id = OLD.item_id
        ) THEN 1 ELSE 0 END,
        media_count = media_count - COALESCE((
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = OLD.item_id
        ), 0),
        total_size_bytes = total_size_bytes - COALESCE((
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = OLD.item_id
        ), 0)
    WHERE lifecycle = OLD.lifecycle;
END;
CREATE TRIGGER read_model_root_summary_insert AFTER INSERT ON root_summary BEGIN
    UPDATE lifecycle_summary
    SET root_count = root_count + 1,
        media_count = media_count + NEW.collection_member_count,
        total_size_bytes = total_size_bytes + NEW.total_size_bytes
    WHERE lifecycle = (
        SELECT lifecycle FROM library_root WHERE item_id = NEW.root_item_id
    ) AND NOT EXISTS (
        SELECT 1 FROM collection_member WHERE media_item_id = NEW.root_item_id
    );
END;
CREATE TRIGGER read_model_root_summary_update
AFTER UPDATE OF collection_member_count, total_size_bytes ON root_summary BEGIN
    UPDATE lifecycle_summary
    SET media_count = media_count
            + NEW.collection_member_count - OLD.collection_member_count,
        total_size_bytes = total_size_bytes
            + NEW.total_size_bytes - OLD.total_size_bytes
    WHERE lifecycle = (
        SELECT lifecycle FROM library_root WHERE item_id = NEW.root_item_id
    ) AND NOT EXISTS (
        SELECT 1 FROM collection_member WHERE media_item_id = NEW.root_item_id
    );
END;
CREATE TRIGGER read_model_root_summary_delete BEFORE DELETE ON root_summary
WHEN EXISTS (SELECT 1 FROM library_root WHERE item_id = OLD.root_item_id)
 AND NOT EXISTS (
     SELECT 1 FROM collection_member WHERE media_item_id = OLD.root_item_id
 ) BEGIN
    UPDATE lifecycle_summary
    SET root_count = root_count - 1,
        media_count = media_count - OLD.collection_member_count,
        total_size_bytes = total_size_bytes - OLD.total_size_bytes
    WHERE lifecycle = (
        SELECT lifecycle FROM library_root WHERE item_id = OLD.root_item_id
    );
END;
CREATE TRIGGER read_model_library_item_update AFTER UPDATE OF kind ON library_item BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (NEW.item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
END;

CREATE TRIGGER read_model_collection_member_insert AFTER INSERT ON collection_member BEGIN
    UPDATE lifecycle_summary
    SET root_count = root_count - CASE WHEN EXISTS (
            SELECT 1 FROM root_summary WHERE root_item_id = NEW.media_item_id
        ) THEN 1 ELSE 0 END,
        media_count = media_count - COALESCE((
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = NEW.media_item_id
        ), 0),
        total_size_bytes = total_size_bytes - COALESCE((
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = NEW.media_item_id
        ), 0)
    WHERE lifecycle = (
        SELECT lifecycle FROM library_root WHERE item_id = NEW.media_item_id
    );
    INSERT INTO root_summary (
        root_item_id, lifecycle, collection_member_count, total_size_bytes,
        imported_at, captured_at, sort_rating, first_media_item_id
    )
    SELECT NEW.collection_id, lr.lifecycle, 1, mf.size_bytes,
           ma.imported_at, ma.captured_at, ma.rating, NEW.media_item_id
    FROM library_root lr
    JOIN media_asset ma ON ma.item_id = NEW.media_item_id
    JOIN media_file mf ON mf.file_id = ma.file_id
    WHERE lr.item_id = NEW.collection_id
    ON CONFLICT(root_item_id) DO UPDATE SET
        lifecycle = excluded.lifecycle,
        collection_member_count = root_summary.collection_member_count + 1,
        total_size_bytes = root_summary.total_size_bytes + excluded.total_size_bytes,
        imported_at = CASE
            WHEN excluded.imported_at IS NULL THEN root_summary.imported_at
            WHEN root_summary.imported_at IS NULL
                OR excluded.imported_at > root_summary.imported_at
                THEN excluded.imported_at
            ELSE root_summary.imported_at
        END,
        captured_at = CASE
            WHEN excluded.captured_at IS NULL THEN root_summary.captured_at
            WHEN root_summary.captured_at IS NULL
                OR excluded.captured_at > root_summary.captured_at
                THEN excluded.captured_at
            ELSE root_summary.captured_at
        END,
        sort_rating = CASE
            WHEN excluded.sort_rating IS NULL THEN root_summary.sort_rating
            WHEN root_summary.sort_rating IS NULL
                OR excluded.sort_rating > root_summary.sort_rating
                THEN excluded.sort_rating
            ELSE root_summary.sort_rating
        END,
        first_media_item_id = MIN(
            root_summary.first_media_item_id, excluded.first_media_item_id
        );
    INSERT INTO root_tag_count (root_item_id, tag_id, media_count)
    SELECT NEW.collection_id, tags.tag_id, 1
    FROM (
        SELECT DISTINCT tag_id
        FROM media_tag
        WHERE media_item_id = NEW.media_item_id
    ) tags
    WHERE EXISTS (
        SELECT 1 FROM library_root WHERE item_id = NEW.collection_id
    )
    ON CONFLICT(root_item_id, tag_id) DO UPDATE SET
        media_count = root_tag_count.media_count + 1;
END;
CREATE TRIGGER read_model_collection_member_update
AFTER UPDATE OF collection_id, media_item_id ON collection_member BEGIN
    UPDATE lifecycle_summary
    SET root_count = root_count + CASE WHEN EXISTS (
            SELECT 1 FROM root_summary WHERE root_item_id = OLD.media_item_id
        ) THEN 1 ELSE 0 END,
        media_count = media_count + COALESCE((
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = OLD.media_item_id
        ), 0),
        total_size_bytes = total_size_bytes + COALESCE((
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = OLD.media_item_id
        ), 0)
    WHERE OLD.media_item_id <> NEW.media_item_id
      AND lifecycle = (
          SELECT lifecycle FROM library_root WHERE item_id = OLD.media_item_id
      )
      AND NOT EXISTS (
          SELECT 1 FROM collection_member WHERE media_item_id = OLD.media_item_id
      );
    UPDATE lifecycle_summary
    SET root_count = root_count - CASE WHEN EXISTS (
            SELECT 1 FROM root_summary WHERE root_item_id = NEW.media_item_id
        ) THEN 1 ELSE 0 END,
        media_count = media_count - COALESCE((
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = NEW.media_item_id
        ), 0),
        total_size_bytes = total_size_bytes - COALESCE((
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = NEW.media_item_id
        ), 0)
    WHERE OLD.media_item_id <> NEW.media_item_id
      AND lifecycle = (
          SELECT lifecycle FROM library_root WHERE item_id = NEW.media_item_id
      );
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.collection_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (NEW.collection_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (NEW.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
END;
CREATE TRIGGER read_model_collection_member_delete AFTER DELETE ON collection_member BEGIN
    UPDATE lifecycle_summary
    SET root_count = root_count + CASE WHEN EXISTS (
            SELECT 1 FROM root_summary WHERE root_item_id = OLD.media_item_id
        ) THEN 1 ELSE 0 END,
        media_count = media_count + COALESCE((
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = OLD.media_item_id
        ), 0),
        total_size_bytes = total_size_bytes + COALESCE((
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = OLD.media_item_id
        ), 0)
    WHERE lifecycle = (
        SELECT lifecycle FROM library_root WHERE item_id = OLD.media_item_id
    ) AND NOT EXISTS (
        SELECT 1 FROM collection_member WHERE media_item_id = OLD.media_item_id
    );
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.collection_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
END;

CREATE TRIGGER read_model_media_asset_update
AFTER UPDATE OF file_id, imported_at, captured_at, rating ON media_asset BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (NEW.item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id)
    SELECT collection_id FROM collection_member WHERE media_item_id = NEW.item_id
    ON CONFLICT(root_item_id) DO NOTHING;
END;
CREATE TRIGGER read_model_media_asset_delete AFTER DELETE ON media_asset BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id)
    SELECT collection_id FROM collection_member WHERE media_item_id = OLD.item_id
    ON CONFLICT(root_item_id) DO NOTHING;
END;
CREATE TRIGGER read_model_media_file_update AFTER UPDATE OF size_bytes ON media_file BEGIN
    INSERT INTO read_model_dirty_root(root_item_id)
    SELECT ma.item_id FROM media_asset ma WHERE ma.file_id = NEW.file_id
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id)
    SELECT cm.collection_id
    FROM collection_member cm
    JOIN media_asset ma ON ma.item_id = cm.media_item_id
    WHERE ma.file_id = NEW.file_id
    ON CONFLICT(root_item_id) DO NOTHING;
END;

CREATE TRIGGER read_model_media_tag_insert AFTER INSERT ON media_tag
WHEN (SELECT suppress_tag_summary FROM projection_write_control WHERE singleton = 1) = 0 BEGIN
    INSERT INTO root_tag_count (root_item_id, tag_id, media_count)
    SELECT roots.root_item_id, NEW.tag_id, 1
    FROM (
        SELECT item_id AS root_item_id
        FROM library_root
        WHERE item_id = NEW.media_item_id
        UNION
        SELECT collection_id AS root_item_id
        FROM collection_member
        WHERE media_item_id = NEW.media_item_id
    ) roots
    WHERE (SELECT COUNT(*) FROM media_tag
           WHERE media_item_id = NEW.media_item_id AND tag_id = NEW.tag_id) = 1
    ON CONFLICT(root_item_id, tag_id) DO UPDATE SET
        media_count = root_tag_count.media_count + 1;
END;
CREATE TRIGGER read_model_media_tag_update
AFTER UPDATE OF media_item_id, tag_id, source ON media_tag BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (NEW.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id)
    SELECT collection_id FROM collection_member
    WHERE media_item_id IN (OLD.media_item_id, NEW.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
END;
CREATE TRIGGER read_model_media_tag_delete AFTER DELETE ON media_tag
WHEN (SELECT suppress_tag_summary FROM projection_write_control WHERE singleton = 1) = 0 BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id)
    SELECT collection_id FROM collection_member WHERE media_item_id = OLD.media_item_id
    ON CONFLICT(root_item_id) DO NOTHING;
END;
"#;

// Standalone ingest maintains its exact read models in the canonical write.
// Keep these replaceable because schema-generation-1 development libraries
// may have been created while the trigger body was still being optimized.
const INCREMENTAL_ROOT_INSERT_TRIGGER_DDL: &str = r#"
DROP TRIGGER IF EXISTS read_model_library_root_insert;
DROP TRIGGER IF EXISTS read_model_media_asset_insert;

CREATE TRIGGER read_model_library_root_insert AFTER INSERT ON library_root BEGIN
    INSERT INTO root_summary (
        root_item_id, lifecycle, collection_member_count, total_size_bytes,
        imported_at, captured_at, sort_rating, first_media_item_id
    )
    SELECT NEW.item_id, NEW.lifecycle, 1, mf.size_bytes,
           ma.imported_at, ma.captured_at, ma.rating, ma.item_id
    FROM library_item li
    JOIN media_asset ma ON ma.item_id = li.item_id
    JOIN media_file mf ON mf.file_id = ma.file_id
    WHERE li.item_id = NEW.item_id AND li.kind = 'media'
      AND NOT EXISTS (
          SELECT 1 FROM collection_member WHERE media_item_id = NEW.item_id
      )
    ON CONFLICT(root_item_id) DO UPDATE SET
        lifecycle = excluded.lifecycle,
        collection_member_count = excluded.collection_member_count,
        total_size_bytes = excluded.total_size_bytes,
        imported_at = excluded.imported_at,
        captured_at = excluded.captured_at,
        sort_rating = excluded.sort_rating,
        first_media_item_id = excluded.first_media_item_id;
    INSERT INTO root_tag_count (root_item_id, tag_id, media_count)
    SELECT NEW.item_id, mt.tag_id, 1
    FROM media_tag mt
    JOIN library_item li ON li.item_id = mt.media_item_id AND li.kind = 'media'
    WHERE mt.media_item_id = NEW.item_id
      AND NOT EXISTS (
          SELECT 1 FROM collection_member WHERE media_item_id = NEW.item_id
      )
    GROUP BY mt.tag_id
    ON CONFLICT(root_item_id, tag_id) DO UPDATE SET media_count = 1;
END;

CREATE TRIGGER read_model_media_asset_insert AFTER INSERT ON media_asset BEGIN
    INSERT INTO root_summary (
        root_item_id, lifecycle, collection_member_count, total_size_bytes,
        imported_at, captured_at, sort_rating, first_media_item_id
    )
    SELECT lr.item_id, lr.lifecycle, 1, mf.size_bytes,
           NEW.imported_at, NEW.captured_at, NEW.rating, NEW.item_id
    FROM library_root lr
    JOIN library_item li ON li.item_id = lr.item_id AND li.kind = 'media'
    JOIN media_file mf ON mf.file_id = NEW.file_id
    WHERE lr.item_id = NEW.item_id
      AND NOT EXISTS (
          SELECT 1 FROM collection_member WHERE media_item_id = NEW.item_id
      )
    ON CONFLICT(root_item_id) DO UPDATE SET
        lifecycle = excluded.lifecycle,
        collection_member_count = excluded.collection_member_count,
        total_size_bytes = excluded.total_size_bytes,
        imported_at = excluded.imported_at,
        captured_at = excluded.captured_at,
        sort_rating = excluded.sort_rating,
        first_media_item_id = excluded.first_media_item_id;
    INSERT INTO root_tag_count (root_item_id, tag_id, media_count)
    SELECT NEW.item_id, mt.tag_id, 1
    FROM media_tag mt
    JOIN library_root lr ON lr.item_id = NEW.item_id
    WHERE mt.media_item_id = NEW.item_id
      AND NOT EXISTS (
          SELECT 1 FROM collection_member WHERE media_item_id = NEW.item_id
      )
    GROUP BY mt.tag_id
    ON CONFLICT(root_item_id, tag_id) DO UPDATE SET media_count = 1;
END;
"#;

pub fn ensure_incremental_read_model_triggers(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(INCREMENTAL_ROOT_INSERT_TRIGGER_DDL)
        .map_err(|error| format!("Failed to refresh incremental read-model triggers: {error}"))?;
    connection
        .execute_batch(FOLDER_SUMMARY_DDL)
        .map_err(|error| format!("Failed to refresh folder summary: {error}"))?;
    connection
        .execute_batch(BULK_TAG_SUMMARY_DDL)
        .map_err(|error| format!("Failed to refresh tag summary trigger: {error}"))
}

const BULK_TAG_SUMMARY_DDL: &str = r#"
DROP TRIGGER IF EXISTS read_model_media_tag_insert;
DROP TRIGGER IF EXISTS read_model_media_tag_delete;
CREATE TRIGGER read_model_media_tag_insert AFTER INSERT ON media_tag
WHEN (SELECT suppress_tag_summary FROM projection_write_control WHERE singleton = 1) = 0 BEGIN
    INSERT INTO root_tag_count (root_item_id, tag_id, media_count)
    SELECT roots.root_item_id, NEW.tag_id, 1
    FROM (
        SELECT item_id AS root_item_id
        FROM library_root
        WHERE item_id = NEW.media_item_id
        UNION
        SELECT collection_id AS root_item_id
        FROM collection_member
        WHERE media_item_id = NEW.media_item_id
    ) roots
    WHERE (SELECT COUNT(*) FROM media_tag
           WHERE media_item_id = NEW.media_item_id AND tag_id = NEW.tag_id) = 1
    ON CONFLICT(root_item_id, tag_id) DO UPDATE SET
        media_count = root_tag_count.media_count + 1;
END;
CREATE TRIGGER read_model_media_tag_delete AFTER DELETE ON media_tag
WHEN (SELECT suppress_tag_summary FROM projection_write_control WHERE singleton = 1) = 0 BEGIN
    INSERT INTO read_model_dirty_root(root_item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(root_item_id) DO NOTHING;
    INSERT INTO read_model_dirty_root(root_item_id)
    SELECT collection_id FROM collection_member WHERE media_item_id = OLD.media_item_id
    ON CONFLICT(root_item_id) DO NOTHING;
END;
"#;

// Folder totals are a fixed materialized view, so explicit O(1) deltas are
// simpler and cheaper than evaluating a general filtered aggregate. This is
// replaceable read-model state and is rebuilt from canonical rows on open.
const FOLDER_SUMMARY_DDL: &str = r#"
DROP TABLE IF EXISTS projection_write_control;
CREATE TABLE projection_write_control (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    suppress_folder_summary INTEGER NOT NULL DEFAULT 0 CHECK (suppress_folder_summary IN (0, 1)),
    suppress_tag_summary INTEGER NOT NULL DEFAULT 0 CHECK (suppress_tag_summary IN (0, 1)),
    suppress_smart_dirty INTEGER NOT NULL DEFAULT 0 CHECK (suppress_smart_dirty IN (0, 1))
);
INSERT INTO projection_write_control(singleton) VALUES (1)
ON CONFLICT(singleton) DO NOTHING;

CREATE TABLE IF NOT EXISTS folder_summary (
    folder_id INTEGER PRIMARY KEY REFERENCES folder(folder_id) ON DELETE CASCADE,
    root_count INTEGER NOT NULL DEFAULT 0,
    media_count INTEGER NOT NULL DEFAULT 0,
    total_size_bytes INTEGER NOT NULL DEFAULT 0
);

DROP TRIGGER IF EXISTS folder_summary_folder_insert;
DROP TRIGGER IF EXISTS folder_summary_folder_item_insert;
DROP TRIGGER IF EXISTS folder_summary_folder_item_delete;
DROP TRIGGER IF EXISTS folder_summary_folder_item_update;
DROP TRIGGER IF EXISTS folder_summary_root_insert;
DROP TRIGGER IF EXISTS folder_summary_root_delete;
DROP TRIGGER IF EXISTS folder_summary_root_values;
DROP TRIGGER IF EXISTS folder_summary_root_lifecycle;

INSERT INTO folder_summary (folder_id, root_count, media_count, total_size_bytes)
SELECT f.folder_id,
       COUNT(rs.root_item_id),
       COALESCE(SUM(rs.collection_member_count), 0),
       COALESCE(SUM(rs.total_size_bytes), 0)
FROM folder f
LEFT JOIN folder_item fi ON fi.folder_id = f.folder_id
LEFT JOIN root_summary rs
  ON rs.root_item_id = fi.item_id
 AND rs.lifecycle = 'active'
 AND NOT EXISTS (
     SELECT 1 FROM collection_member cm
     WHERE cm.media_item_id = rs.root_item_id
 )
GROUP BY f.folder_id
ON CONFLICT(folder_id) DO NOTHING;

CREATE TRIGGER folder_summary_folder_insert AFTER INSERT ON folder BEGIN
    INSERT INTO folder_summary(folder_id) VALUES (NEW.folder_id)
    ON CONFLICT(folder_id) DO NOTHING;
END;

CREATE TRIGGER folder_summary_folder_item_insert AFTER INSERT ON folder_item
WHEN (SELECT suppress_folder_summary FROM projection_write_control WHERE singleton = 1) = 0
 AND EXISTS (
    SELECT 1 FROM root_summary rs
    WHERE rs.root_item_id = NEW.item_id AND rs.lifecycle = 'active'
) AND NOT EXISTS (
    SELECT 1 FROM collection_member cm WHERE cm.media_item_id = NEW.item_id
) BEGIN
    INSERT INTO folder_summary(folder_id, root_count, media_count, total_size_bytes)
    SELECT NEW.folder_id, 1, rs.collection_member_count, rs.total_size_bytes
    FROM root_summary rs WHERE rs.root_item_id = NEW.item_id
    ON CONFLICT(folder_id) DO UPDATE SET
        root_count = folder_summary.root_count + 1,
        media_count = folder_summary.media_count + excluded.media_count,
        total_size_bytes = folder_summary.total_size_bytes + excluded.total_size_bytes;
END;

CREATE TRIGGER folder_summary_folder_item_delete BEFORE DELETE ON folder_item
WHEN (SELECT suppress_folder_summary FROM projection_write_control WHERE singleton = 1) = 0
 AND EXISTS (
    SELECT 1 FROM root_summary rs
    WHERE rs.root_item_id = OLD.item_id AND rs.lifecycle = 'active'
) AND NOT EXISTS (
    SELECT 1 FROM collection_member cm WHERE cm.media_item_id = OLD.item_id
) BEGIN
    UPDATE folder_summary
    SET root_count = root_count - 1,
        media_count = media_count - (
            SELECT collection_member_count FROM root_summary
            WHERE root_item_id = OLD.item_id
        ),
        total_size_bytes = total_size_bytes - (
            SELECT total_size_bytes FROM root_summary
            WHERE root_item_id = OLD.item_id
        )
    WHERE folder_id = OLD.folder_id;
END;

CREATE TRIGGER folder_summary_folder_item_update
AFTER UPDATE OF folder_id, item_id ON folder_item BEGIN
    UPDATE folder_summary
    SET root_count = root_count - CASE WHEN EXISTS (
            SELECT 1 FROM root_summary rs
            WHERE rs.root_item_id = OLD.item_id AND rs.lifecycle = 'active'
        ) AND NOT EXISTS (
            SELECT 1 FROM collection_member cm WHERE cm.media_item_id = OLD.item_id
        ) THEN 1 ELSE 0 END,
        media_count = media_count - COALESCE((
            SELECT collection_member_count FROM root_summary rs
            WHERE rs.root_item_id = OLD.item_id AND rs.lifecycle = 'active'
              AND NOT EXISTS (
                  SELECT 1 FROM collection_member cm
                  WHERE cm.media_item_id = OLD.item_id
              )
        ), 0),
        total_size_bytes = total_size_bytes - COALESCE((
            SELECT total_size_bytes FROM root_summary rs
            WHERE rs.root_item_id = OLD.item_id AND rs.lifecycle = 'active'
              AND NOT EXISTS (
                  SELECT 1 FROM collection_member cm
                  WHERE cm.media_item_id = OLD.item_id
              )
        ), 0)
    WHERE folder_id = OLD.folder_id;
    INSERT INTO folder_summary(folder_id, root_count, media_count, total_size_bytes)
    SELECT NEW.folder_id, 1, rs.collection_member_count, rs.total_size_bytes
    FROM root_summary rs
    WHERE rs.root_item_id = NEW.item_id AND rs.lifecycle = 'active'
      AND NOT EXISTS (
          SELECT 1 FROM collection_member cm WHERE cm.media_item_id = NEW.item_id
      )
    ON CONFLICT(folder_id) DO UPDATE SET
        root_count = folder_summary.root_count + 1,
        media_count = folder_summary.media_count + excluded.media_count,
        total_size_bytes = folder_summary.total_size_bytes + excluded.total_size_bytes;
END;

CREATE TRIGGER folder_summary_root_insert AFTER INSERT ON root_summary
WHEN NEW.lifecycle = 'active' AND NOT EXISTS (
    SELECT 1 FROM collection_member cm WHERE cm.media_item_id = NEW.root_item_id
) BEGIN
    INSERT INTO folder_summary(folder_id, root_count, media_count, total_size_bytes)
    SELECT fi.folder_id, 1, NEW.collection_member_count, NEW.total_size_bytes
    FROM folder_item fi WHERE fi.item_id = NEW.root_item_id
    ON CONFLICT(folder_id) DO UPDATE SET
        root_count = folder_summary.root_count + 1,
        media_count = folder_summary.media_count + excluded.media_count,
        total_size_bytes = folder_summary.total_size_bytes + excluded.total_size_bytes;
END;

CREATE TRIGGER folder_summary_root_delete BEFORE DELETE ON root_summary
WHEN OLD.lifecycle = 'active' AND NOT EXISTS (
    SELECT 1 FROM collection_member cm WHERE cm.media_item_id = OLD.root_item_id
) BEGIN
    UPDATE folder_summary
    SET root_count = root_count - 1,
        media_count = media_count - OLD.collection_member_count,
        total_size_bytes = total_size_bytes - OLD.total_size_bytes
    WHERE folder_id IN (
        SELECT folder_id FROM folder_item WHERE item_id = OLD.root_item_id
    );
END;

CREATE TRIGGER folder_summary_root_values
AFTER UPDATE OF collection_member_count, total_size_bytes ON root_summary
WHEN NEW.lifecycle = 'active' AND NOT EXISTS (
    SELECT 1 FROM collection_member cm WHERE cm.media_item_id = NEW.root_item_id
) BEGIN
    UPDATE folder_summary
    SET media_count = media_count
            + NEW.collection_member_count - OLD.collection_member_count,
        total_size_bytes = total_size_bytes
            + NEW.total_size_bytes - OLD.total_size_bytes
    WHERE folder_id IN (
        SELECT folder_id FROM folder_item WHERE item_id = NEW.root_item_id
    );
END;

CREATE TRIGGER folder_summary_root_lifecycle
AFTER UPDATE OF lifecycle ON root_summary
WHEN OLD.lifecycle <> NEW.lifecycle AND NOT EXISTS (
    SELECT 1 FROM collection_member cm WHERE cm.media_item_id = NEW.root_item_id
) BEGIN
    UPDATE folder_summary
    SET root_count = root_count - CASE WHEN OLD.lifecycle = 'active' THEN 1 ELSE 0 END,
        media_count = media_count - CASE WHEN OLD.lifecycle = 'active'
            THEN OLD.collection_member_count ELSE 0 END,
        total_size_bytes = total_size_bytes - CASE WHEN OLD.lifecycle = 'active'
            THEN OLD.total_size_bytes ELSE 0 END
    WHERE folder_id IN (
        SELECT folder_id FROM folder_item WHERE item_id = NEW.root_item_id
    );
    UPDATE folder_summary
    SET root_count = root_count + CASE WHEN NEW.lifecycle = 'active' THEN 1 ELSE 0 END,
        media_count = media_count + CASE WHEN NEW.lifecycle = 'active'
            THEN NEW.collection_member_count ELSE 0 END,
        total_size_bytes = total_size_bytes + CASE WHEN NEW.lifecycle = 'active'
            THEN NEW.total_size_bytes ELSE 0 END
    WHERE folder_id IN (
        SELECT folder_id FROM folder_item WHERE item_id = NEW.root_item_id
    );
END;
"#;

// Derived search triggers are safe to replace in place. During alpha schema
// development their conflict handling changed without changing canonical
// library data, so existing development libraries may still carry the older
// `INSERT OR IGNORE` form. An outer UPSERT overrides that conflict policy and
// can otherwise abort media ingestion.
const SEARCH_MEDIA_TRIGGER_DDL: &str = r#"
DROP TRIGGER IF EXISTS search_media_asset_insert;
DROP TRIGGER IF EXISTS search_media_asset_update;
DROP TRIGGER IF EXISTS search_media_asset_delete;
DROP TRIGGER IF EXISTS search_media_file_update;
DROP TRIGGER IF EXISTS search_source_item_insert;
DROP TRIGGER IF EXISTS search_source_item_update;
DROP TRIGGER IF EXISTS search_source_item_delete;
DROP TRIGGER IF EXISTS search_source_post_update;

CREATE TRIGGER search_media_asset_insert AFTER INSERT ON media_asset BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (NEW.item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_media_asset_update
AFTER UPDATE OF name, notes, source_urls_json ON media_asset BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (NEW.item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_media_asset_delete AFTER DELETE ON media_asset BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (OLD.item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_media_file_update AFTER UPDATE OF mime_type ON media_file BEGIN
    INSERT INTO search_dirty_media(media_item_id)
    SELECT item_id FROM media_asset WHERE file_id = NEW.file_id
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_item_insert AFTER INSERT ON source_item WHEN NEW.media_item_id IS NOT NULL BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (NEW.media_item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_item_update
AFTER UPDATE OF source_post_id, media_url, canonical_url, media_item_id ON source_item BEGIN
    INSERT INTO search_dirty_media(media_item_id)
    SELECT OLD.media_item_id WHERE OLD.media_item_id IS NOT NULL
    ON CONFLICT(media_item_id) DO NOTHING;
    INSERT INTO search_dirty_media(media_item_id)
    SELECT NEW.media_item_id WHERE NEW.media_item_id IS NOT NULL
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_item_delete AFTER DELETE ON source_item WHEN OLD.media_item_id IS NOT NULL BEGIN
    INSERT INTO search_dirty_media(media_item_id) VALUES (OLD.media_item_id)
    ON CONFLICT(media_item_id) DO NOTHING;
END;
CREATE TRIGGER search_source_post_update
AFTER UPDATE OF site_id, post_key, canonical_url, creator_name, title, description ON source_post BEGIN
    INSERT INTO search_dirty_media(media_item_id)
    SELECT media_item_id FROM source_item
    WHERE source_post_id = NEW.source_post_id AND media_item_id IS NOT NULL
    ON CONFLICT(media_item_id) DO NOTHING;
END;
"#;

pub fn ensure_search_media_triggers(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(SEARCH_MEDIA_TRIGGER_DDL)
        .map_err(|error| format!("Failed to refresh search triggers: {error}"))
}

/// Install the rebuildable smart-folder membership read model. Existing
/// schema-generation-1 development libraries may predate this derived table;
/// creating and rebuilding it changes no canonical library data.
pub fn ensure_smart_folder_read_model(connection: &mut Connection) -> Result<(), String> {
    let canonical_generation_model: bool = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'smart_folder_generation'
             ) AND EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'smart_folder_membership'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if canonical_generation_model {
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        crate::smart_v2::refresh_materialized(&transaction)
            .map_err(|error| format!("Failed to build smart-folder generations: {error}"))?;
        transaction.commit().map_err(|error| error.to_string())?;
        return Ok(());
    }

    let existed: bool = connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_master
                 WHERE type = 'table' AND name = 'smart_folder_root'
             )",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute_batch(SMART_FOLDER_READ_MODEL_DDL)
        .map_err(|error| format!("Failed to install smart-folder read model: {error}"))?;
    if !existed {
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        crate::smart_v2::refresh_materialized(&transaction)
            .map_err(|error| format!("Failed to build smart-folder read model: {error}"))?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Refresh exact root summaries dirtied by canonical writes. These read
/// models participate in foreground publication because lifecycle, folder,
/// tag, and sidebar results must be exact immediately.
pub fn refresh_read_models(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    let legacy: bool = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM sqlite_master
             WHERE type = 'table' AND name = 'read_model_dirty_root'
         )",
        [],
        |row| row.get(0),
    )?;
    if legacy {
        refresh_derived_state(transaction)?;
    }
    Ok(())
}

pub fn search_indexes_dirty(connection: &Connection) -> rusqlite::Result<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM search_dirty_name)
             OR EXISTS(SELECT 1 FROM search_dirty_notes)
             OR EXISTS(SELECT 1 FROM search_dirty_source)",
        [],
        |row| row.get(0),
    )
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SearchRefreshBatch {
    pub processed: usize,
    pub remaining: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchCategory {
    Name,
    Notes,
    Source,
}

impl SearchCategory {
    fn as_str(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Notes => "notes",
            Self::Source => "source",
        }
    }
}

/// Refresh every compact FTS row dirtied by canonical writes. Conversion and
/// focused tests use this path; runtime maintenance uses bounded batches.
pub fn refresh_search_indexes(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    while refresh_search_indexes_batch(transaction, 4_096)?.remaining {}
    Ok(())
}

/// Materialize at most `limit` dirty objects from one FTS category. Processing
/// one category keeps each maintenance transaction predictably small and lets
/// foreground writer admission interleave between batches.
pub fn refresh_search_indexes_batch(
    transaction: &Transaction<'_>,
    limit: usize,
) -> rusqlite::Result<SearchRefreshBatch> {
    let category: Option<String> = transaction.query_row(
        "SELECT CASE
             WHEN EXISTS(SELECT 1 FROM search_dirty_name) THEN 'name'
             WHEN EXISTS(SELECT 1 FROM search_dirty_notes) THEN 'notes'
             WHEN EXISTS(SELECT 1 FROM search_dirty_source) THEN 'source'
             ELSE NULL
         END",
        [],
        |row| row.get(0),
    )?;
    let Some(category) = category else {
        return Ok(SearchRefreshBatch::default());
    };
    refresh_search_indexes_named_batch(transaction, &category, limit)
}

/// Refresh one explicit category so a continuously dirty media queue cannot
/// starve the other rebuildable indexes.
pub fn refresh_search_indexes_category_batch(
    transaction: &Transaction<'_>,
    category: SearchCategory,
    limit: usize,
) -> rusqlite::Result<SearchRefreshBatch> {
    refresh_search_indexes_named_batch(transaction, category.as_str(), limit)
}

fn refresh_search_indexes_named_batch(
    transaction: &Transaction<'_>,
    category: &str,
    limit: usize,
) -> rusqlite::Result<SearchRefreshBatch> {
    let limit = i64::try_from(limit.max(1)).unwrap_or(i64::MAX);
    let processed = match category {
        "name" => refresh_name_search_batch(transaction, limit)?,
        "notes" => refresh_notes_search_batch(transaction, limit)?,
        "source" => refresh_source_search_batch(transaction, limit)?,
        _ => unreachable!("search category query returned an unknown value"),
    };

    Ok(SearchRefreshBatch {
        processed: usize::try_from(processed)
            .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, processed))?,
        remaining: search_indexes_dirty(transaction)?,
    })
}

fn refresh_name_search_batch(transaction: &Transaction<'_>, limit: i64) -> rusqlite::Result<i64> {
    let processed = transaction.query_row(
        "SELECT COUNT(*) FROM (
             SELECT root_item_id FROM search_dirty_name
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         )",
        [limit],
        |row| row.get(0),
    )?;
    transaction.execute(
        "DELETE FROM root_name_fts WHERE rowid IN (
             SELECT root_item_id FROM search_dirty_name
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         )",
        [limit],
    )?;
    transaction.execute(
        "INSERT INTO root_name_fts(rowid, root_item_id, name)
         SELECT item.item_id, item.item_id,
                trim(COALESCE(metadata.name, cover.name, '') || ' ' ||
                     CASE item.kind WHEN 'collection' THEN 'collection group'
                                    ELSE 'standalone media' END)
         FROM library_item item
         JOIN library_root root ON root.item_id = item.item_id
         LEFT JOIN root_metadata metadata ON metadata.root_item_id = item.item_id
         LEFT JOIN media_asset cover ON cover.item_id = COALESCE(
             item.cover_media_item_id,
             CASE WHEN item.kind = 'media' THEN item.item_id END
         )
         JOIN (
             SELECT root_item_id FROM search_dirty_name
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         ) dirty ON dirty.root_item_id = item.item_id",
        [limit],
    )?;
    transaction.execute(
        "DELETE FROM search_dirty_name WHERE root_item_id IN (
             SELECT root_item_id FROM search_dirty_name
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         )",
        [limit],
    )?;
    Ok(processed)
}

fn refresh_notes_search_batch(transaction: &Transaction<'_>, limit: i64) -> rusqlite::Result<i64> {
    let processed = transaction.query_row(
        "SELECT COUNT(*) FROM (
             SELECT root_item_id FROM search_dirty_notes
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         )",
        [limit],
        |row| row.get(0),
    )?;
    transaction.execute(
        "DELETE FROM root_notes_fts WHERE rowid IN (
             SELECT root_item_id FROM search_dirty_notes
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         )",
        [limit],
    )?;
    transaction.execute(
        "INSERT INTO root_notes_fts(rowid, root_item_id, notes)
         SELECT metadata.root_item_id, metadata.root_item_id,
                trim(COALESCE(metadata.notes, '') || ' ' ||
                     COALESCE(metadata.source_urls_json, ''))
         FROM root_metadata metadata
         JOIN library_root root ON root.item_id = metadata.root_item_id
         JOIN (
             SELECT root_item_id FROM search_dirty_notes
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         ) dirty ON dirty.root_item_id = metadata.root_item_id",
        [limit],
    )?;
    transaction.execute(
        "DELETE FROM search_dirty_notes WHERE root_item_id IN (
             SELECT root_item_id FROM search_dirty_notes
             ORDER BY queued_at_ms, root_item_id LIMIT ?1
         )",
        [limit],
    )?;
    Ok(processed)
}

fn refresh_source_search_batch(transaction: &Transaction<'_>, limit: i64) -> rusqlite::Result<i64> {
    let processed = transaction.query_row(
        "SELECT COUNT(*) FROM (
             SELECT source_post_id FROM search_dirty_source
             ORDER BY queued_at_ms, source_post_id LIMIT ?1
         )",
        [limit],
        |row| row.get(0),
    )?;
    transaction.execute(
        "DELETE FROM source_text_fts WHERE rowid IN (
             SELECT source_post_id FROM search_dirty_source
             ORDER BY queued_at_ms, source_post_id LIMIT ?1
         )",
        [limit],
    )?;
    transaction.execute(
        "INSERT INTO source_text_fts(rowid, source_post_id, searchable_text)
         SELECT post.source_post_id, post.source_post_id,
                trim(COALESCE(post.site_id, '') || ' ' ||
                     COALESCE(post.post_key, '') || ' ' ||
                     COALESCE(post.creator_name, '') || ' ' ||
                     COALESCE(post.title, '') || ' ' ||
                     COALESCE(post.description, '') || ' ' ||
                     COALESCE(post.canonical_url, '') || ' ' ||
                     COALESCE((
                         SELECT group_concat(
                             COALESCE(item.canonical_url, '') || ' ' ||
                             COALESCE(item.media_url, ''), ' '
                         )
                         FROM source_item item
                         WHERE item.source_post_id = post.source_post_id
                     ), ''))
         FROM source_post post
         JOIN (
             SELECT source_post_id FROM search_dirty_source
             ORDER BY queued_at_ms, source_post_id LIMIT ?1
         ) dirty ON dirty.source_post_id = post.source_post_id",
        [limit],
    )?;
    transaction.execute(
        "DELETE FROM search_dirty_source WHERE source_post_id IN (
             SELECT source_post_id FROM search_dirty_source
             ORDER BY queued_at_ms, source_post_id LIMIT ?1
         )",
        [limit],
    )?;
    Ok(processed)
}

fn refresh_derived_state(transaction: &Transaction<'_>) -> rusqlite::Result<()> {
    let dirty_roots: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM read_model_dirty_root)",
        [],
        |row| row.get(0),
    )?;
    if !dirty_roots {
        return Ok(());
    }

    transaction.execute(
        "DELETE FROM root_tag_count
             WHERE root_item_id IN (SELECT root_item_id FROM read_model_dirty_root)",
        [],
    )?;
    transaction.execute(
        "DELETE FROM root_summary
             WHERE root_item_id IN (SELECT root_item_id FROM read_model_dirty_root)",
        [],
    )?;
    transaction.execute(
        "INSERT INTO root_summary (
             root_item_id, lifecycle, collection_member_count, total_size_bytes,
             imported_at, captured_at, sort_rating, first_media_item_id
         )
         SELECT lr.item_id, lr.lifecycle, 1, mf.size_bytes,
                ma.imported_at, ma.captured_at, ma.rating, ma.item_id
         FROM read_model_dirty_root dirty
         JOIN library_root lr ON lr.item_id = dirty.root_item_id
         JOIN library_item li ON li.item_id = lr.item_id AND li.kind = 'media'
         JOIN media_asset ma ON ma.item_id = lr.item_id
         JOIN media_file mf ON mf.file_id = ma.file_id

         UNION ALL

         SELECT lr.item_id, lr.lifecycle, COUNT(*),
                COALESCE(SUM(mf.size_bytes), 0), MAX(ma.imported_at),
                MAX(ma.captured_at), MAX(ma.rating), MIN(cm.media_item_id)
         FROM read_model_dirty_root dirty
         JOIN library_root lr ON lr.item_id = dirty.root_item_id
         JOIN library_item li ON li.item_id = lr.item_id AND li.kind = 'collection'
         JOIN collection_member cm ON cm.collection_id = lr.item_id
         JOIN media_asset ma ON ma.item_id = cm.media_item_id
         JOIN media_file mf ON mf.file_id = ma.file_id
             GROUP BY lr.item_id, lr.lifecycle",
        [],
    )?;
    transaction.execute(
        "INSERT INTO root_tag_count (root_item_id, tag_id, media_count)
         WITH root_media(root_item_id, media_item_id) AS (
             SELECT dirty.root_item_id, dirty.root_item_id
             FROM read_model_dirty_root dirty
             JOIN library_root lr ON lr.item_id = dirty.root_item_id
             JOIN library_item li ON li.item_id = lr.item_id AND li.kind = 'media'
             JOIN media_asset ma ON ma.item_id = lr.item_id

             UNION ALL

             SELECT dirty.root_item_id, cm.media_item_id
             FROM read_model_dirty_root dirty
             JOIN library_root lr ON lr.item_id = dirty.root_item_id
             JOIN library_item li ON li.item_id = lr.item_id AND li.kind = 'collection'
             JOIN collection_member cm ON cm.collection_id = lr.item_id
         )
         SELECT rm.root_item_id, mt.tag_id, COUNT(DISTINCT rm.media_item_id)
         FROM root_media rm
         JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
             GROUP BY rm.root_item_id, mt.tag_id",
        [],
    )?;
    transaction.execute("DELETE FROM read_model_dirty_root", [])?;
    Ok(())
}

pub fn create(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(LIBRARY_DDL)
        .map_err(|error| format!("Failed to create schema: {error}"))?;
    transaction
        .execute_batch(SMART_FOLDER_READ_MODEL_DDL)
        .map_err(|error| format!("Failed to create smart-folder read model: {error}"))?;
    transaction
        .execute_batch(SUBSCRIPTION_READ_INDEXES)
        .map_err(|error| format!("Failed to create subscription read indexes: {error}"))?;
    transaction
        .execute_batch(PERFORMANCE_INDEXES)
        .map_err(|error| format!("Failed to create performance indexes: {error}"))?;
    transaction
        .execute_batch(INCREMENTAL_ROOT_INSERT_TRIGGER_DDL)
        .map_err(|error| format!("Failed to create incremental read-model triggers: {error}"))?;
    transaction
        .execute_batch(FOLDER_SUMMARY_DDL)
        .map_err(|error| format!("Failed to create folder summary: {error}"))?;
    transaction
        .execute(
            "INSERT INTO library_meta (singleton, schema_version, revision) VALUES (1, ?1, 1)",
            [CURRENT_SCHEMA_VERSION],
        )
        .map_err(|error| format!("Failed to record schema version: {error}"))?;
    transaction
        .execute(
            "INSERT INTO cloud_state (singleton, library_id, device_id)
             VALUES (1, lower(hex(randomblob(16))), lower(hex(randomblob(16))))",
            [],
        )
        .map_err(|error| format!("Failed to create cloud identity: {error}"))?;
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

/// Create backend schema generation 1 in a new, empty database.
///
/// This is the only runtime schema creation path. The temporary conversion
/// executable is external to this contract and is deleted after verification.
pub fn create_canonical_v1(connection: &mut Connection) -> Result<(), String> {
    let transaction = connection
        .transaction()
        .map_err(|error| format!("Failed to start schema generation 1 creation: {error}"))?;
    transaction
        .execute_batch(SCHEMA_V1_DDL)
        .map_err(|error| format!("Failed to create schema generation 1: {error}"))?;
    transaction
        .execute(
            "INSERT INTO library_meta (
                 singleton, schema_version, revision, schema_fingerprint
             ) VALUES (1, ?1, 1, ?2)",
            rusqlite::params![CURRENT_SCHEMA_VERSION, CURRENT_SCHEMA_FINGERPRINT],
        )
        .map_err(|error| format!("Failed to initialize schema generation 1: {error}"))?;
    transaction
        .execute(
            "INSERT INTO cloud_state (singleton, library_id, device_id)
             VALUES (1, lower(hex(randomblob(16))), lower(hex(randomblob(16))))",
            [],
        )
        .map_err(|error| format!("Failed to initialize library identity: {error}"))?;
    transaction
        .execute(
            "INSERT INTO setting (key, value_json) VALUES (?1, ?2)",
            rusqlite::params![
                PHASH_VERSION_SETTING,
                CURRENT_PHASH_ANALYSIS_VERSION.to_string()
            ],
        )
        .map_err(|error| format!("Failed to initialize media analysis version: {error}"))?;
    transaction
        .execute(
            "INSERT INTO projection_write_control(singleton) VALUES (1)",
            [],
        )
        .map_err(|error| format!("Failed to initialize projection controls: {error}"))?;
    transaction
        .execute_batch(
            "INSERT INTO lifecycle_summary(lifecycle)
             VALUES ('inbox'), ('active'), ('trash');",
        )
        .map_err(|error| format!("Failed to initialize lifecycle summaries: {error}"))?;
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
        Some(version) if version == CURRENT_SCHEMA_VERSION => validate_canonical_shape(connection),
        Some(version) => Err(format!(
            "Picto schema {version} is incompatible with required schema {CURRENT_SCHEMA_VERSION}"
        )),
        None => Err("This is not a current Picto library".to_string()),
    }
}

fn validate_canonical_shape(connection: &Connection) -> Result<(), String> {
    let fingerprint: Option<String> = connection
        .query_row(
            "SELECT schema_fingerprint FROM library_meta WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| format!("Invalid Picto library schema: {error}"))?;
    if fingerprint.as_deref() != Some(CURRENT_SCHEMA_FINGERPRINT) {
        return Err(
            "This library is incompatible with Picto backend schema generation 1".to_string(),
        );
    }

    for table in [
        "root_metadata",
        "canonical_bitmap_key",
        "canonical_bitmap_key_allocator",
        "canonical_bitmap",
        "canonical_order",
        "root_tag",
        "root_summary",
        "tag_summary",
        "smart_folder_dependency",
        "smart_folder_generation",
        "smart_folder_membership",
        "projection_checkpoint",
        "root_name_fts",
        "root_notes_fts",
        "source_text_fts",
        "search_dirty_name",
        "search_dirty_notes",
        "search_dirty_source",
    ] {
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                 )",
                [table],
                |row| row.get(0),
            )
            .map_err(|error| format!("Invalid Picto library schema: {error}"))?;
        if !exists {
            return Err(format!(
                "Picto backend schema generation 1 is incomplete (missing {table})"
            ));
        }
    }

    for (table, forbidden_column) in [
        ("library_item", "label"),
        ("media_asset", "notes"),
        ("media_asset", "rating"),
        ("media_asset", "source_urls_json"),
    ] {
        let present: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2)",
                rusqlite::params![table, forbidden_column],
                |row| row.get(0),
            )
            .map_err(|error| format!("Invalid Picto library schema: {error}"))?;
        if present {
            return Err(format!(
                "Picto backend schema generation 1 contains incompatible {table}.{forbidden_column}"
            ));
        }
    }
    Ok(())
}

pub fn revision(connection: &Connection) -> rusqlite::Result<u64> {
    let revision: i64 = connection.query_row(
        "SELECT revision FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    u64::try_from(revision).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, revision))
}

pub fn increment_revision(transaction: &Transaction<'_>) -> rusqlite::Result<u64> {
    transaction.execute(
        "UPDATE library_meta SET revision = revision + 1 WHERE singleton = 1",
        [],
    )?;
    let revision: i64 = transaction.query_row(
        "SELECT revision FROM library_meta WHERE singleton = 1",
        [],
        |row| row.get(0),
    )?;
    u64::try_from(revision).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, revision))
}

#[cfg(test)]
mod tests {
    use super::{
        create, create_canonical_v1, ensure_search_media_triggers, ensure_smart_folder_read_model,
        validate, CURRENT_SCHEMA_VERSION,
    };

    #[test]
    fn bundled_sqlite_contains_the_wal_reset_fix() {
        assert!(
            rusqlite::version_number() >= 3_051_003,
            "SQLite {} predates the WAL-reset corruption fix",
            rusqlite::version()
        );
    }
    use rusqlite::Connection;

    #[test]
    fn creates_only_the_replacement_schema() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_canonical_v1(&mut connection).unwrap();
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
            "cloud_state",
            "cloud_outbox",
            "cloud_applied_mutation",
            "cloud_device_frontier",
            "cloud_field_clock",
            "cloud_membership_clock",
            "cloud_tombstone",
            "cloud_quarantine",
            "cloud_snapshot",
            "cloud_blob_state",
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
            "smart_projection_dirty_root",
            "smart_projection_dirty_all",
        ] {
            assert!(
                !names.iter().any(|name| name == removed),
                "retained {removed}"
            );
        }
        for expected in [
            "root_summary",
            "folder_summary",
            "root_metadata",
            "root_tag",
            "tag_summary",
            "smart_folder_generation",
            "smart_folder_membership",
            "root_name_fts",
            "root_notes_fts",
            "source_text_fts",
        ] {
            assert!(
                names.iter().any(|name| name == expected),
                "missing {expected}"
            );
        }
        assert_eq!(CURRENT_SCHEMA_VERSION, 1);
        assert_eq!(
            connection
                .query_row("SELECT revision FROM library_meta", [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap(),
            1
        );
    }

    #[test]
    fn canonical_v1_has_only_root_owned_organization_and_text_fts() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_canonical_v1(&mut connection).unwrap();
        ensure_smart_folder_read_model(&mut connection).unwrap();

        for required in [
            "root_metadata",
            "root_tag",
            "root_summary",
            "tag_summary",
            "smart_folder_dependency",
            "smart_folder_generation",
            "smart_folder_membership",
            "projection_write_control",
            "projection_checkpoint",
            "root_name_fts",
            "root_notes_fts",
            "source_text_fts",
            "search_dirty_name",
            "search_dirty_notes",
            "search_dirty_source",
        ] {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                     )",
                    [required],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(exists, "missing canonical table {required}");
        }
        for removed in [
            "media_tag",
            "root_tag_count",
            "tag_alias",
            "tag_implication",
            "smart_folder_root",
            "smart_projection_dirty_root",
            "smart_projection_dirty_all",
            "tag_search_fts",
            "folder_search_fts",
            "search_dirty_tag",
            "search_dirty_folder",
        ] {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1
                     )",
                    [removed],
                    |row| row.get(0),
                )
                .unwrap();
            assert!(
                !exists,
                "retained legacy search/organization table {removed}"
            );
        }
        let legacy_search_trigger_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master
                 WHERE type = 'trigger'
                   AND name LIKE 'search_%'
                   AND (LOWER(sql) LIKE '%tag%' OR LOWER(sql) LIKE '%folder%')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(legacy_search_trigger_count, 0);
        let media_columns = connection
            .prepare("SELECT name FROM pragma_table_info('media_asset')")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for removed in ["notes", "rating", "source_urls_json"] {
            assert!(!media_columns.iter().any(|column| column == removed));
        }
        let item_columns = connection
            .prepare("SELECT name FROM pragma_table_info('library_item')")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(!item_columns.iter().any(|column| column == "label"));
        let root_tag_columns = connection
            .prepare("SELECT name FROM pragma_table_info('root_tag')")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for required in ["root_item_id", "tag_id"] {
            assert!(
                root_tag_columns.iter().any(|column| column == required),
                "missing canonical root_tag column {required}"
            );
        }
        let write_control_columns = connection
            .prepare("SELECT name FROM pragma_table_info('projection_write_control')")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        for required in [
            "singleton",
            "suppress_folder_summary",
            "suppress_tag_summary",
            "suppress_smart_dirty",
        ] {
            assert!(
                write_control_columns
                    .iter()
                    .any(|column| column == required),
                "missing canonical projection_write_control column {required}"
            );
        }
    }

    #[test]
    fn canonical_summaries_store_hidden_organization_but_count_only_active_roots() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_canonical_v1(&mut connection).unwrap();
        connection
            .execute_batch(
                "INSERT INTO media_file
                     (file_id, file_hash, mime_type, size_bytes, created_at)
                 VALUES (1, 'active-file', 'image/jpeg', 10, 'now'),
                        (2, 'inbox-file', 'image/jpeg', 20, 'now');
                 INSERT INTO library_item
                     (item_id, item_key, kind, created_at, updated_at)
                 VALUES (1, 'active-item', 'media', 'now', 'now'),
                        (2, 'inbox-item', 'media', 'now', 'now');
                 INSERT INTO library_root(item_id, lifecycle)
                 VALUES (1, 'active'), (2, 'inbox');
                 INSERT INTO media_asset
                     (item_id, file_id, name, imported_at, updated_at)
                 VALUES (1, 1, 'active.jpg', 'now', 'now'),
                        (2, 2, 'inbox.jpg', 'now', 'now');
                 INSERT INTO root_metadata(root_item_id, name, updated_at)
                 VALUES (1, 'Active', 'now'), (2, 'Inbox', 'now');
                 INSERT INTO folder(folder_id, folder_key, name, created_at, updated_at)
                 VALUES (1, 'folder', 'Folder', 'now', 'now');
                 INSERT INTO folder_item(folder_id, item_id) VALUES (1, 1), (1, 2);
                 INSERT INTO tag(tag_id, namespace, subtag)
                 VALUES (1, 'creator', 'artist');
                 INSERT INTO root_tag(root_item_id, tag_id) VALUES (1, 1), (2, 1);",
            )
            .unwrap();

        let folder: (i64, i64, i64) = connection
            .query_row(
                "SELECT visible_root_count, media_count, total_size_bytes
                 FROM folder_summary WHERE folder_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(folder, (1, 1, 10));
        let tag: (i64, i64) = connection
            .query_row(
                "SELECT visible_root_count, assignment_count
                 FROM tag_summary WHERE tag_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(tag, (1, 2));

        connection
            .execute_batch(
                "UPDATE library_root SET lifecycle = 'trash' WHERE item_id = 1;
                 UPDATE library_root SET lifecycle = 'active' WHERE item_id = 2;",
            )
            .unwrap();
        let folder: (i64, i64, i64) = connection
            .query_row(
                "SELECT visible_root_count, media_count, total_size_bytes
                 FROM folder_summary WHERE folder_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(folder, (1, 1, 20));
        let tag_visible: i64 = connection
            .query_row(
                "SELECT visible_root_count FROM tag_summary WHERE tag_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tag_visible, 1);
        let active: (i64, i64, i64) = connection
            .query_row(
                "SELECT root_count, media_count, total_size_bytes
                 FROM lifecycle_summary WHERE lifecycle = 'active'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(active, (1, 1, 20));
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
                INSERT INTO library_meta VALUES (1, 128, 42);",
            )
            .unwrap();

        let error = validate(&connection).unwrap_err();
        assert!(error.contains("128"));
        let revision: i64 = connection
            .query_row("SELECT revision FROM library_meta", [], |row| row.get(0))
            .unwrap();
        assert_eq!(revision, 42);
    }

    #[test]
    fn search_dirty_triggers_remain_idempotent_under_outer_conflict_policy() {
        let mut connection = Connection::open_in_memory().unwrap();
        create(&mut connection).unwrap();

        // Reproduce the trigger form present in development libraries created
        // before explicit trigger-local conflict handling was introduced.
        connection
            .execute_batch(
                "DROP TRIGGER search_media_asset_update;
                 CREATE TRIGGER search_media_asset_update
                 AFTER UPDATE OF name, notes, source_urls_json ON media_asset BEGIN
                     INSERT OR IGNORE INTO search_dirty_media(media_item_id)
                     VALUES (NEW.item_id);
                 END;",
            )
            .unwrap();
        ensure_search_media_triggers(&connection).unwrap();

        connection
            .execute_batch(
                "INSERT INTO media_file (
                    file_hash, mime_type, size_bytes, created_at
                 ) VALUES ('hash', 'image/jpeg', 1, 'now');
                 INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                 VALUES (1, 'item', 'media', 'now', 'now');
                 INSERT INTO media_asset (
                    item_id, file_id, name, source_urls_json, imported_at, updated_at
                 ) VALUES (1, 1, 'first', '[]', 'now', 'now');
                 INSERT OR REPLACE INTO media_asset (
                    item_id, file_id, name, source_urls_json, imported_at, updated_at
                 ) VALUES (1, 1, 'second', '[]', 'now', 'now');",
            )
            .unwrap();

        let dirty_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM search_dirty_media", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(dirty_count, 1);
    }

    #[test]
    fn standalone_ingest_publishes_exact_read_models_without_dirty_refresh() {
        let mut connection = Connection::open_in_memory().unwrap();
        create(&mut connection).unwrap();

        connection
            .execute_batch(
                "INSERT INTO media_file
                     (file_id, file_hash, mime_type, size_bytes, created_at)
                 VALUES (1, 'one', 'image/jpeg', 42, 'now');
                 INSERT INTO library_item
                     (item_id, item_key, kind, created_at, updated_at)
                 VALUES (1, 'one', 'media', 'now', 'now');
                 INSERT INTO media_asset
                     (item_id, file_id, name, rating, imported_at, updated_at)
                 VALUES (1, 1, 'one.jpg', 3, '2026-01-01', 'now');
                 INSERT INTO tag (tag_id, namespace, subtag)
                 VALUES (1, 'general', 'first');
                 INSERT INTO media_tag (media_item_id, tag_id)
                 VALUES (1, 1);
                 INSERT INTO library_root (item_id, lifecycle)
                 VALUES (1, 'inbox');",
            )
            .unwrap();

        let dirty: i64 = connection
            .query_row("SELECT COUNT(*) FROM read_model_dirty_root", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(dirty, 0);
        let summary: (String, i64, i64, Option<i64>, i64) = connection
            .query_row(
                "SELECT lifecycle, collection_member_count, total_size_bytes,
                        sort_rating, first_media_item_id
                 FROM root_summary WHERE root_item_id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(summary, ("inbox".to_string(), 1, 42, Some(3), 1));
        let tag_count: i64 = connection
            .query_row(
                "SELECT media_count FROM root_tag_count
                 WHERE root_item_id = 1 AND tag_id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tag_count, 1);
        let lifecycle: (i64, i64, i64) = connection
            .query_row(
                "SELECT root_count, media_count, total_size_bytes
                 FROM lifecycle_summary WHERE lifecycle = 'inbox'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(lifecycle, (1, 1, 42));
    }

    #[test]
    fn standalone_read_model_is_exact_when_root_precedes_asset() {
        let mut connection = Connection::open_in_memory().unwrap();
        create(&mut connection).unwrap();

        connection
            .execute_batch(
                "INSERT INTO media_file
                     (file_id, file_hash, mime_type, size_bytes, created_at)
                 VALUES (1, 'one', 'image/jpeg', 23, 'now');
                 INSERT INTO library_item
                     (item_id, item_key, kind, created_at, updated_at)
                 VALUES (1, 'one', 'media', 'now', 'now');
                 INSERT INTO library_root (item_id, lifecycle)
                 VALUES (1, 'active');
                 INSERT INTO media_asset
                     (item_id, file_id, name, imported_at, updated_at)
                 VALUES (1, 1, 'one.jpg', '2026-01-01', 'now');",
            )
            .unwrap();

        let dirty: i64 = connection
            .query_row("SELECT COUNT(*) FROM read_model_dirty_root", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(dirty, 0);
        let summary: (i64, i64) = connection
            .query_row(
                "SELECT collection_member_count, total_size_bytes
                 FROM root_summary WHERE root_item_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(summary, (1, 23));
        let lifecycle: (i64, i64, i64) = connection
            .query_row(
                "SELECT root_count, media_count, total_size_bytes
                 FROM lifecycle_summary WHERE lifecycle = 'active'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(lifecycle, (1, 1, 23));
    }

    #[test]
    fn root_read_models_refresh_only_affected_roots() {
        let mut connection = Connection::open_in_memory().unwrap();
        create(&mut connection).unwrap();

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute_batch(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (1, 'one', 'image/jpeg', 10, 'now'),
                            (2, 'two', 'image/jpeg', 20, 'now');
                     INSERT INTO library_item
                         (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'one', 'media', 'now', 'now'),
                            (2, 'two', 'media', 'now', 'now'),
                            (10, 'album', 'collection', 'now', 'now');
                     INSERT INTO library_root (item_id, lifecycle)
                     VALUES (1, 'active'), (2, 'active'), (10, 'active');
                     INSERT INTO media_asset
                         (item_id, file_id, name, rating, imported_at, updated_at)
                     VALUES (1, 1, 'one.jpg', 2, '2026-01-01', 'now'),
                            (2, 2, 'two.jpg', 3, '2026-01-02', 'now');
                     INSERT INTO collection_member
                         (collection_id, media_item_id, position_rank)
                     VALUES (10, 1, 0);
                     INSERT INTO tag (tag_id, namespace, subtag)
                     VALUES (1, 'general', 'first'), (2, 'general', 'second');
                     INSERT INTO media_tag (media_item_id, tag_id)
                     VALUES (1, 1);",
                )
                .unwrap();
            super::refresh_read_models(&transaction).unwrap();
            transaction.commit().unwrap();
        }

        let first_summary: (i64, i64, Option<i64>) = connection
            .query_row(
                "SELECT collection_member_count, total_size_bytes, sort_rating
                 FROM root_summary WHERE root_item_id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(first_summary, (1, 10, Some(2)));
        let collection_summary: (i64, i64) = connection
            .query_row(
                "SELECT collection_member_count, total_size_bytes
                 FROM root_summary WHERE root_item_id = 10",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(collection_summary, (1, 10));
        assert_eq!(
            connection
                .query_row(
                    "SELECT media_count FROM root_tag_count
                     WHERE root_item_id = 10 AND tag_id = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "INSERT INTO collection_member
                         (collection_id, media_item_id, position_rank)
                     VALUES (10, 2, 1)",
                    [],
                )
                .unwrap();
            assert_eq!(
                transaction
                    .query_row("SELECT COUNT(*) FROM read_model_dirty_root", [], |row| {
                        row.get::<_, i64>(0)
                    })
                    .unwrap(),
                0
            );
            super::refresh_read_models(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        assert_eq!(
            connection
                .query_row(
                    "SELECT collection_member_count, total_size_bytes
                     FROM root_summary WHERE root_item_id = 10",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap(),
            (2, 30)
        );

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute_batch(
                    "UPDATE media_file SET size_bytes = 99 WHERE file_id = 2;
                     UPDATE media_asset SET rating = 5 WHERE item_id = 2;
                     INSERT INTO media_tag (media_item_id, tag_id) VALUES (2, 2);",
                )
                .unwrap();
            super::refresh_read_models(&transaction).unwrap();
            transaction.commit().unwrap();
        }

        assert_eq!(
            connection
                .query_row(
                    "SELECT collection_member_count, total_size_bytes, sort_rating
                     FROM root_summary WHERE root_item_id = 2",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap(),
            (1, 99, Some(5))
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT collection_member_count, total_size_bytes, sort_rating
                     FROM root_summary WHERE root_item_id = 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .unwrap(),
            first_summary
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT media_count FROM root_tag_count
                     WHERE root_item_id = 10 AND tag_id = 2",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            1
        );

        assert_eq!(
            connection
                .query_row(
                    "SELECT collection_member_count, total_size_bytes
                     FROM root_summary WHERE root_item_id = 10",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap(),
            (2, 109)
        );

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute_batch(
                    "UPDATE media_file SET size_bytes = 11 WHERE file_id = 1;
                     INSERT INTO media_tag (media_item_id, tag_id, source)
                     VALUES (1, 2, 'remote');
                     DELETE FROM collection_member
                     WHERE collection_id = 10 AND media_item_id = 1;",
                )
                .unwrap();
            super::refresh_read_models(&transaction).unwrap();
            transaction.commit().unwrap();
        }

        assert_eq!(
            connection
                .query_row(
                    "SELECT collection_member_count, total_size_bytes
                     FROM root_summary WHERE root_item_id = 10",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap(),
            (1, 99)
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT total_size_bytes FROM root_summary WHERE root_item_id = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap(),
            11
        );

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "DELETE FROM collection_member
                     WHERE collection_id = 10 AND media_item_id = 2",
                    [],
                )
                .unwrap();
            super::refresh_read_models(&transaction).unwrap();
            transaction.commit().unwrap();
        }
        assert_eq!(
            connection
                .query_row::<i64, _, _>(
                    "SELECT COUNT(*) FROM root_summary WHERE root_item_id = 10",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
    }

    #[test]
    fn smart_folder_memberships_follow_incremental_root_changes() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_canonical_v1(&mut connection).unwrap();

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (
                         1, 'matching', 'Matching',
                         '{\"groups\":[{\"match_mode\":\"all\",\"negate\":false,\"rules\":[{\"field\":\"name\",\"op\":\"contains\",\"value\":\"match\"}]}]}',
                         'now', 'now'
                     )",
                    [],
                )
                .unwrap();
            crate::smart_v2::refresh_materialized(&transaction).unwrap();
            transaction.commit().unwrap();
        }

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute_batch(
                    "INSERT INTO media_file
                         (file_id, file_hash, mime_type, size_bytes, created_at)
                     VALUES (1, 'one', 'image/jpeg', 10, 'now'),
                            (2, 'two', 'image/jpeg', 20, 'now');
                     INSERT INTO library_item
                         (item_id, item_key, kind, created_at, updated_at)
                     VALUES (1, 'one', 'media', 'now', 'now'),
                            (2, 'two', 'media', 'now', 'now');
                     INSERT INTO library_root (item_id, lifecycle)
                     VALUES (1, 'active'), (2, 'active');
                     INSERT INTO media_asset
                         (item_id, file_id, name, imported_at, updated_at)
                     VALUES (1, 1, 'first-match.jpg', 'now', 'now'),
                            (2, 2, 'second.jpg', 'now', 'now');
                     INSERT INTO root_metadata (
                         root_item_id, name, notes, source_urls_json, updated_at
                     ) VALUES (1, 'first-match.jpg', NULL, '[]', 'now'),
                              (2, 'second.jpg', NULL, '[]', 'now');",
                )
                .unwrap();
            crate::smart_v2::refresh_impacted_roots(
                &transaction,
                &roaring::RoaringBitmap::from_iter([1, 2]),
                &["name", "lifecycle"],
                &[],
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        assert_eq!(smart_members(&connection), vec![1]);

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "UPDATE root_metadata
                     SET name = 'second-match.jpg' WHERE root_item_id = 2",
                    [],
                )
                .unwrap();
            crate::smart_v2::refresh_impacted_roots(
                &transaction,
                &roaring::RoaringBitmap::from_iter([2]),
                &["name"],
                &[],
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        assert_eq!(smart_members(&connection), vec![1, 2]);

        {
            let transaction = connection.transaction().unwrap();
            transaction
                .execute(
                    "UPDATE root_metadata SET name = 'first.jpg' WHERE root_item_id = 1",
                    [],
                )
                .unwrap();
            crate::smart_v2::refresh_impacted_roots(
                &transaction,
                &roaring::RoaringBitmap::from_iter([1]),
                &["name"],
                &[],
            )
            .unwrap();
            transaction.commit().unwrap();
        }
        assert_eq!(smart_members(&connection), vec![2]);
    }

    #[test]
    fn canonical_bitmap_ids_are_u32_and_never_reused() {
        let mut connection = Connection::open_in_memory().unwrap();
        create_canonical_v1(&mut connection).unwrap();
        connection
            .execute(
                "INSERT INTO library_item
                     (item_key, kind, created_at, updated_at)
                 VALUES ('first', 'media', 'now', 'now')",
                [],
            )
            .unwrap();
        let first = connection.last_insert_rowid();
        connection
            .execute("DELETE FROM library_item WHERE item_id = ?1", [first])
            .unwrap();
        connection
            .execute(
                "INSERT INTO library_item
                     (item_key, kind, created_at, updated_at)
                 VALUES ('second', 'media', 'now', 'now')",
                [],
            )
            .unwrap();
        assert_eq!(connection.last_insert_rowid(), first + 1);

        assert!(connection
            .execute(
                "INSERT INTO library_item
                     (item_id, item_key, kind, created_at, updated_at)
                 VALUES (4294967296, 'overflow', 'media', 'now', 'now')",
                [],
            )
            .is_err());
    }

    fn smart_members(connection: &Connection) -> Vec<i64> {
        connection
            .prepare(
                "SELECT membership.root_item_id
                 FROM smart_folder_generation generation
                 JOIN smart_folder_membership membership
                   ON membership.generation_id = generation.generation_id
                 WHERE generation.smart_folder_id = 1
                   AND generation.state = 'active'
                 ORDER BY membership.root_item_id",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap()
    }
}
