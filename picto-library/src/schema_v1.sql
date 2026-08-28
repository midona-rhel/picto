CREATE TABLE library_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_generation INTEGER NOT NULL,
    schema_fingerprint TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    next_local_id INTEGER NOT NULL CHECK (next_local_id BETWEEN 1 AND 4294967295)
) STRICT;

CREATE TABLE library_item (
    local_id INTEGER PRIMARY KEY CHECK (local_id BETWEEN 1 AND 4294967295),
    stable_key TEXT NOT NULL UNIQUE,
    item_kind INTEGER NOT NULL CHECK (item_kind IN (1, 2))
) STRICT;

CREATE TABLE media_file (
    file_id INTEGER PRIMARY KEY CHECK (file_id BETWEEN 1 AND 4294967295),
    content_hash TEXT NOT NULL UNIQUE,
    file_path TEXT NOT NULL,
    mime TEXT NOT NULL,
    size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    width INTEGER,
    height INTEGER,
    duration_ms INTEGER,
    frame_count INTEGER,
    perceptual_hash TEXT,
    palette_json TEXT NOT NULL DEFAULT '[]'
) STRICT;

CREATE TABLE media_item (
    media_id INTEGER PRIMARY KEY REFERENCES library_item(local_id) ON DELETE CASCADE,
    media_name TEXT NOT NULL CHECK (length(media_name) <= 1024),
    file_id INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE RESTRICT
) STRICT;

CREATE TABLE duplicate_pair (
    file_id_a INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
    file_id_b INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE CASCADE,
    distance INTEGER NOT NULL CHECK (distance >= 0),
    status INTEGER NOT NULL DEFAULT 1 CHECK (status IN (1, 2, 3)),
    detected_at_ms INTEGER NOT NULL,
    decided_at_ms INTEGER,
    winner_file_id INTEGER REFERENCES media_file(file_id) ON DELETE RESTRICT,
    PRIMARY KEY(file_id_a, file_id_b),
    CHECK (file_id_a < file_id_b),
    CHECK (
        (status = 1 AND decided_at_ms IS NULL AND winner_file_id IS NULL) OR
        (status = 2 AND decided_at_ms IS NOT NULL AND winner_file_id IS NULL) OR
        (status = 3 AND decided_at_ms IS NOT NULL AND
            winner_file_id IN (file_id_a, file_id_b))
    )
) WITHOUT ROWID, STRICT;

CREATE TABLE library_root (
    root_id INTEGER PRIMARY KEY REFERENCES library_item(local_id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK (length(name) <= 1024),
    notes TEXT,
    source_urls_json TEXT NOT NULL DEFAULT '[]',
    cover_media_id INTEGER NOT NULL REFERENCES media_item(media_id) ON DELETE RESTRICT,
    imported_at_ms INTEGER NOT NULL,
    captured_at_ms INTEGER,
    modified_at_ms INTEGER NOT NULL,
    media_count INTEGER NOT NULL CHECK (media_count >= 1),
    total_size_bytes INTEGER NOT NULL CHECK (total_size_bytes >= 0)
) STRICT;

CREATE TABLE tag_namespace (
    namespace_id INTEGER PRIMARY KEY CHECK (namespace_id BETWEEN 1 AND 4294967295),
    stable_key TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL UNIQUE
) STRICT;

CREATE TABLE tag_definition (
    tag_id INTEGER PRIMARY KEY CHECK (tag_id BETWEEN 1 AND 4294967295),
    stable_key TEXT NOT NULL UNIQUE,
    namespace_id INTEGER NOT NULL REFERENCES tag_namespace(namespace_id) ON DELETE RESTRICT,
    subname TEXT NOT NULL,
    UNIQUE(namespace_id, subname)
) STRICT;

CREATE TABLE folder_definition (
    folder_id INTEGER PRIMARY KEY CHECK (folder_id BETWEEN 1 AND 4294967295),
    stable_key TEXT NOT NULL UNIQUE,
    parent_id INTEGER REFERENCES folder_definition(folder_id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    icon TEXT,
    color TEXT,
    notes TEXT,
    auto_tag_ids BLOB NOT NULL DEFAULT X'',
    watch_path TEXT UNIQUE,
    watch_enabled INTEGER NOT NULL DEFAULT 0 CHECK (watch_enabled IN (0, 1)),
    watch_subfolders INTEGER NOT NULL DEFAULT 0 CHECK (watch_subfolders IN (0, 1)),
    display_order INTEGER NOT NULL,
    UNIQUE(parent_id, name)
) STRICT;

CREATE TABLE smart_folder_definition (
    smart_folder_id INTEGER PRIMARY KEY CHECK (smart_folder_id BETWEEN 1 AND 4294967295),
    stable_key TEXT NOT NULL UNIQUE,
    parent_id INTEGER REFERENCES smart_folder_definition(smart_folder_id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    icon TEXT,
    color TEXT,
    notes TEXT,
    view_query_json TEXT NOT NULL,
    display_order INTEGER NOT NULL,
    UNIQUE(parent_id, name)
) STRICT;

CREATE TABLE canonical_bitmap (
    domain INTEGER NOT NULL,
    key_id INTEGER NOT NULL,
    high_bits INTEGER NOT NULL CHECK (high_bits BETWEEN 0 AND 65535),
    revision INTEGER NOT NULL,
    cardinality INTEGER NOT NULL CHECK (cardinality >= 0),
    format_version INTEGER NOT NULL,
    checksum BLOB NOT NULL,
    payload BLOB NOT NULL,
    PRIMARY KEY(domain, key_id, high_bits)
) WITHOUT ROWID, STRICT;

CREATE TABLE ordered_membership (
    owner_kind INTEGER NOT NULL CHECK (owner_kind IN (1, 2)),
    owner_id INTEGER NOT NULL,
    revision INTEGER NOT NULL,
    cardinality INTEGER NOT NULL CHECK (cardinality >= 0),
    format_version INTEGER NOT NULL,
    checksum BLOB NOT NULL,
    payload BLOB NOT NULL,
    PRIMARY KEY(owner_kind, owner_id)
) WITHOUT ROWID, STRICT;

CREATE TABLE recent_view (
    root_id INTEGER PRIMARY KEY REFERENCES library_root(root_id) ON DELETE CASCADE,
    viewed_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE source_provenance (
    source_key TEXT NOT NULL,
    source_item_key TEXT NOT NULL,
    media_id INTEGER NOT NULL REFERENCES media_item(media_id) ON DELETE CASCADE,
    source_text TEXT,
    PRIMARY KEY(source_key, source_item_key, media_id)
) WITHOUT ROWID, STRICT;

CREATE TABLE subscription (
    subscription_id INTEGER PRIMARY KEY,
    subscription_key TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    schedule TEXT NOT NULL DEFAULT 'manual',
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
    initial_post_limit INTEGER,
    periodic_post_limit INTEGER,
    next_run_at TEXT,
    created_at TEXT NOT NULL
) STRICT;

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
    group_posts INTEGER NOT NULL DEFAULT 1 CHECK (group_posts IN (0, 1)),
    paused INTEGER NOT NULL DEFAULT 0 CHECK (paused IN (0, 1)),
    resume_cursor TEXT,
    initial_run_complete INTEGER NOT NULL DEFAULT 0 CHECK (initial_run_complete IN (0, 1)),
    last_success_at TEXT,
    last_failure_at TEXT,
    last_failure_kind TEXT,
    last_failure_message TEXT
) STRICT;

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
) STRICT;

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
    UNIQUE(run_id, query_id)
) STRICT;

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
    root_item_id INTEGER REFERENCES library_root(root_id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(site_id, post_key)
) STRICT;

CREATE TABLE subscription_source_post (
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_id INTEGER NOT NULL REFERENCES subscription_query(query_id) ON DELETE CASCADE,
    source_post_id INTEGER NOT NULL REFERENCES source_post(source_post_id) ON DELETE CASCADE,
    last_seen_run_id INTEGER REFERENCES subscription_run(run_id) ON DELETE SET NULL,
    PRIMARY KEY(subscription_id, query_id, source_post_id)
) WITHOUT ROWID, STRICT;

CREATE TABLE source_item (
    source_item_id INTEGER PRIMARY KEY,
    source_post_id INTEGER NOT NULL REFERENCES source_post(source_post_id) ON DELETE CASCADE,
    item_key TEXT NOT NULL,
    position INTEGER NOT NULL,
    media_url TEXT,
    canonical_url TEXT,
    media_item_id INTEGER REFERENCES media_item(media_id) ON DELETE SET NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'downloaded', 'ingested', 'failed', 'deleted')),
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(source_post_id, item_key)
) STRICT;

CREATE TABLE subscription_run_source_item (
    run_query_id INTEGER NOT NULL REFERENCES subscription_run_query(run_query_id) ON DELETE CASCADE,
    source_item_id INTEGER NOT NULL REFERENCES source_item(source_item_id) ON DELETE CASCADE,
    PRIMARY KEY(run_query_id, source_item_id)
) WITHOUT ROWID, STRICT;

CREATE TABLE ingest_job (
    ingest_job_id INTEGER PRIMARY KEY,
    job_key TEXT NOT NULL UNIQUE,
    source_kind TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_item_id INTEGER REFERENCES source_item(source_item_id) ON DELETE CASCADE,
    payload_json TEXT NOT NULL,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('inbox', 'active')),
    delete_after_ingest INTEGER NOT NULL DEFAULT 0 CHECK (delete_after_ingest IN (0, 1)),
    status TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed')),
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at TEXT NOT NULL,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
) STRICT;

CREATE TABLE work_item (
    work_id INTEGER PRIMARY KEY,
    media_item_id INTEGER REFERENCES media_item(media_id) ON DELETE CASCADE,
    file_id INTEGER REFERENCES media_file(file_id) ON DELETE CASCADE,
    file_hash TEXT,
    work_type TEXT NOT NULL CHECK (work_type IN ('thumbnail', 'dominant_colors', 'perceptual_hash', 'blob_delete', 'ai_tag')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'running')),
    priority INTEGER NOT NULL DEFAULT 0,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    available_at TEXT NOT NULL,
    last_error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (media_item_id IS NOT NULL OR file_id IS NOT NULL OR file_hash IS NOT NULL)
) STRICT;

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
) STRICT;

CREATE TABLE credential (
    site_id TEXT PRIMARY KEY,
    credential_type TEXT NOT NULL CHECK (credential_type IN ('api_key', 'cookies', 'oauth_token')),
    display_name TEXT,
    created_at TEXT NOT NULL
) WITHOUT ROWID, STRICT;

CREATE TABLE credential_health (
    site_id TEXT PRIMARY KEY REFERENCES credential(site_id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'unknown',
    checked_at TEXT,
    last_error TEXT
) WITHOUT ROWID, STRICT;

CREATE TABLE view_pref (
    scope TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
) WITHOUT ROWID, STRICT;

CREATE TABLE setting (
    key TEXT PRIMARY KEY,
    value_json TEXT NOT NULL
) WITHOUT ROWID, STRICT;

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
) STRICT;

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
) WITHOUT ROWID, STRICT;

CREATE TABLE cloud_applied_mutation (
    mutation_id TEXT PRIMARY KEY,
    device_id TEXT NOT NULL,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    applied_at TEXT NOT NULL
) WITHOUT ROWID, STRICT;

CREATE TABLE cloud_device_frontier (
    device_id TEXT PRIMARY KEY,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    updated_at TEXT NOT NULL
) WITHOUT ROWID, STRICT;

CREATE TABLE cloud_field_clock (
    object_kind TEXT NOT NULL,
    object_key TEXT NOT NULL,
    field_name TEXT NOT NULL,
    hlc_physical_ms INTEGER NOT NULL,
    hlc_logical INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    mutation_id TEXT NOT NULL,
    PRIMARY KEY(object_kind, object_key, field_name)
) WITHOUT ROWID, STRICT;

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
    PRIMARY KEY(relation_kind, owner_key, member_key)
) WITHOUT ROWID, STRICT;

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
    PRIMARY KEY(object_kind, object_key)
) WITHOUT ROWID, STRICT;

CREATE TABLE cloud_quarantine (
    quarantine_id INTEGER PRIMARY KEY,
    mutation_id TEXT NOT NULL UNIQUE,
    reason TEXT NOT NULL,
    envelope_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT
) STRICT;

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
) WITHOUT ROWID, STRICT;

CREATE TABLE cloud_blob_state (
    file_hash TEXT PRIMARY KEY REFERENCES media_file(content_hash) ON DELETE CASCADE,
    state TEXT NOT NULL CHECK (state IN ('available', 'queued', 'downloading', 'missing_remote', 'corrupt')),
    priority INTEGER NOT NULL DEFAULT 0,
    remote_present INTEGER NOT NULL DEFAULT 0 CHECK (remote_present IN (0, 1)),
    remote_extension TEXT,
    last_error TEXT,
    uploaded_at TEXT,
    updated_at TEXT NOT NULL
) WITHOUT ROWID, STRICT;

CREATE TABLE fts_dirty (
    root_id INTEGER NOT NULL REFERENCES library_root(root_id) ON DELETE CASCADE,
    category INTEGER NOT NULL,
    queued_at_ms INTEGER NOT NULL,
    PRIMARY KEY(root_id, category)
) WITHOUT ROWID, STRICT;

CREATE VIRTUAL TABLE root_fts USING fts5(
    root_id UNINDEXED,
    name,
    notes,
    urls,
    source_text,
    tokenize = 'unicode61'
);

CREATE TABLE cloud_journal (
    journal_id INTEGER PRIMARY KEY,
    revision INTEGER NOT NULL,
    operation_kind TEXT NOT NULL,
    target_bitmap BLOB,
    payload_json TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    expanded_at_ms INTEGER
) STRICT;

CREATE TABLE deletion_tombstone (
    stable_key TEXT PRIMARY KEY,
    revision INTEGER NOT NULL,
    deleted_at_ms INTEGER NOT NULL
) WITHOUT ROWID, STRICT;

CREATE TABLE blob_cleanup_queue (
    file_id INTEGER PRIMARY KEY REFERENCES media_file(file_id) ON DELETE CASCADE,
    file_path TEXT NOT NULL,
    enqueued_revision INTEGER NOT NULL
) STRICT;

CREATE TABLE projection_checkpoint (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_fingerprint TEXT NOT NULL,
    implementation_hash TEXT NOT NULL,
    database_revision INTEGER NOT NULL,
    checksum BLOB NOT NULL,
    payload BLOB NOT NULL
) STRICT;

CREATE INDEX idx_library_item_kind ON library_item(item_kind, local_id);
CREATE INDEX idx_media_item_file ON media_item(file_id, media_id);
CREATE INDEX idx_duplicate_pair_status ON duplicate_pair(status, distance, file_id_a, file_id_b);
CREATE INDEX idx_media_file_mime ON media_file(mime, file_id);
CREATE INDEX idx_root_imported ON library_root(imported_at_ms, root_id);
CREATE INDEX idx_root_captured ON library_root(captured_at_ms, root_id);
CREATE INDEX idx_root_modified ON library_root(modified_at_ms, root_id);
CREATE INDEX idx_root_name ON library_root(name COLLATE NOCASE, root_id);
CREATE INDEX idx_root_size ON library_root(total_size_bytes, root_id);
CREATE INDEX idx_root_cover ON library_root(cover_media_id, root_id);
CREATE INDEX idx_recent_viewed ON recent_view(viewed_at_ms DESC, root_id);
CREATE INDEX idx_source_media ON source_provenance(media_id, source_key, source_item_key);
CREATE INDEX idx_fts_dirty_order ON fts_dirty(category, queued_at_ms, root_id);
CREATE INDEX idx_cloud_pending ON cloud_journal(expanded_at_ms, journal_id);
CREATE INDEX idx_subscription_due ON subscription(paused, next_run_at, subscription_id);
CREATE INDEX idx_subscription_query_subscription ON subscription_query(subscription_id, query_id);
CREATE INDEX idx_subscription_run_subscription ON subscription_run(subscription_id, created_at, run_id);
CREATE INDEX idx_source_post_root ON source_post(root_item_id, source_post_id);
CREATE INDEX idx_source_item_media ON source_item(media_item_id, source_item_id);
CREATE INDEX idx_ingest_job_ready ON ingest_job(status, available_at, ingest_job_id);
CREATE INDEX idx_work_item_ready ON work_item(status, priority, available_at, work_id);
CREATE INDEX idx_subscription_issue_open ON subscription_issue(status, last_seen_at, issue_id);
CREATE INDEX idx_cloud_outbox_pending ON cloud_outbox(published_at, created_at, mutation_id);
