CREATE TABLE library_meta (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    schema_version INTEGER NOT NULL,
    revision INTEGER NOT NULL DEFAULT 0
, schema_fingerprint TEXT);

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
    cover_media_item_id INTEGER REFERENCES library_item(item_id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE library_root (
    item_id INTEGER PRIMARY KEY REFERENCES library_item(item_id) ON DELETE CASCADE,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('inbox', 'active', 'trash')),
    sort_rank INTEGER
);

CREATE TABLE media_asset (
    item_id INTEGER PRIMARY KEY REFERENCES library_item(item_id) ON DELETE CASCADE,
    file_id INTEGER NOT NULL REFERENCES media_file(file_id) ON DELETE RESTRICT,
    name TEXT,
    captured_at TEXT,
    imported_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE media_view (
    item_id INTEGER PRIMARY KEY REFERENCES library_root(item_id) ON DELETE CASCADE,
    viewed_at TEXT NOT NULL
);

CREATE TABLE tag (
    tag_id INTEGER PRIMARY KEY AUTOINCREMENT
        CHECK (tag_id BETWEEN 1 AND 4294967295),
    namespace TEXT NOT NULL DEFAULT 'general',
    subtag TEXT NOT NULL,
    UNIQUE (namespace, subtag)
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

CREATE TABLE subscription_run_source_item (
    run_query_id INTEGER NOT NULL REFERENCES subscription_run_query(run_query_id) ON DELETE CASCADE,
    source_item_id INTEGER NOT NULL REFERENCES source_item(source_item_id) ON DELETE CASCADE,
    PRIMARY KEY (run_query_id, source_item_id)
);

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
    updated_at TEXT NOT NULL, priority INTEGER NOT NULL DEFAULT 0,
    CHECK (media_item_id IS NOT NULL OR file_id IS NOT NULL OR file_hash IS NOT NULL)
);

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
    schema_generation INTEGER NOT NULL DEFAULT 2,
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

CREATE TABLE projection_write_control (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    suppress_folder_summary INTEGER NOT NULL DEFAULT 0 CHECK (suppress_folder_summary IN (0, 1)),
    suppress_tag_summary INTEGER NOT NULL DEFAULT 0 CHECK (suppress_tag_summary IN (0, 1)),
    suppress_smart_dirty INTEGER NOT NULL DEFAULT 0 CHECK (suppress_smart_dirty IN (0, 1))
, suppress_root_summary INTEGER NOT NULL DEFAULT 0
    CHECK (suppress_root_summary IN (0, 1))
, suppress_membership_capture INTEGER NOT NULL DEFAULT 0
    CHECK (suppress_membership_capture IN (0, 1)));

CREATE TABLE root_metadata (
    root_item_id INTEGER PRIMARY KEY
        REFERENCES library_root(item_id) ON DELETE CASCADE,
    name TEXT,
    rating INTEGER CHECK (rating IS NULL OR rating BETWEEN 0 AND 5),
    notes TEXT,
    source_urls_json TEXT NOT NULL DEFAULT '[]'
        CHECK (json_valid(source_urls_json)),
    updated_at TEXT NOT NULL
);

CREATE TABLE canonical_bitmap_key (
    domain INTEGER NOT NULL,
    key_id INTEGER NOT NULL CHECK (key_id BETWEEN 1 AND 4294967295),
    value TEXT NOT NULL,
    PRIMARY KEY (domain, key_id),
    UNIQUE (domain, value)
) WITHOUT ROWID;

CREATE TABLE canonical_bitmap_key_allocator (
    domain INTEGER PRIMARY KEY,
    next_key_id INTEGER NOT NULL CHECK (next_key_id BETWEEN 1 AND 4294967296)
) WITHOUT ROWID;

CREATE TABLE canonical_bitmap (
    domain INTEGER NOT NULL,
    key_id INTEGER NOT NULL,
    shard INTEGER NOT NULL CHECK (shard BETWEEN 0 AND 65535),
    revision INTEGER NOT NULL CHECK (revision >= 1),
    cardinality INTEGER NOT NULL CHECK (cardinality > 0),
    format_version INTEGER NOT NULL CHECK (format_version = 1),
    checksum TEXT NOT NULL CHECK (length(checksum) = 64),
    payload BLOB NOT NULL,
    PRIMARY KEY (domain, key_id, shard)
) WITHOUT ROWID;

CREATE TABLE canonical_order (
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('group', 'folder')),
    owner_id INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision >= 1),
    cardinality INTEGER NOT NULL CHECK (cardinality >= 0),
    format_version INTEGER NOT NULL CHECK (format_version = 1),
    checksum TEXT NOT NULL CHECK (length(checksum) = 64),
    payload BLOB NOT NULL,
    PRIMARY KEY (owner_kind, owner_id)
) WITHOUT ROWID;

CREATE TABLE root_summary (
    root_item_id INTEGER PRIMARY KEY
        REFERENCES library_root(item_id) ON DELETE CASCADE,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('inbox', 'active', 'trash')),
    kind TEXT NOT NULL CHECK (kind IN ('media', 'collection')),
    cover_media_item_id INTEGER
        REFERENCES media_asset(item_id) ON DELETE SET NULL,
    media_count INTEGER NOT NULL CHECK (media_count >= 0),
    total_size_bytes INTEGER NOT NULL CHECK (total_size_bytes >= 0),
    imported_at TEXT,
    captured_at TEXT,
    sort_rating INTEGER,
    sort_name TEXT,
    updated_at TEXT NOT NULL
);

CREATE TABLE lifecycle_summary (
    lifecycle TEXT PRIMARY KEY CHECK (lifecycle IN ('inbox', 'active', 'trash')),
    root_count INTEGER NOT NULL DEFAULT 0,
    media_count INTEGER NOT NULL DEFAULT 0,
    total_size_bytes INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

CREATE TABLE folder_summary (
    folder_id INTEGER PRIMARY KEY REFERENCES folder(folder_id) ON DELETE CASCADE,
    visible_root_count INTEGER NOT NULL DEFAULT 0,
    media_count INTEGER NOT NULL DEFAULT 0,
    total_size_bytes INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE tag_summary (
    tag_id INTEGER PRIMARY KEY REFERENCES tag(tag_id) ON DELETE CASCADE,
    visible_root_count INTEGER NOT NULL DEFAULT 0,
    assignment_count INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE smart_folder_dependency (
    smart_folder_id INTEGER NOT NULL
        REFERENCES smart_folder(smart_folder_id) ON DELETE CASCADE,
    dependency_kind TEXT NOT NULL,
    dependency_key TEXT NOT NULL,
    PRIMARY KEY (smart_folder_id, dependency_kind, dependency_key)
) WITHOUT ROWID;

CREATE TABLE smart_folder_generation (
    generation_id INTEGER PRIMARY KEY,
    smart_folder_id INTEGER NOT NULL
        REFERENCES smart_folder(smart_folder_id) ON DELETE CASCADE,
    database_revision INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('building', 'active', 'retired')),
    member_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    activated_at TEXT
);

CREATE TABLE smart_folder_membership (
    generation_id INTEGER NOT NULL
        REFERENCES smart_folder_generation(generation_id) ON DELETE CASCADE,
    root_item_id INTEGER NOT NULL
        REFERENCES library_root(item_id) ON DELETE CASCADE,
    PRIMARY KEY (generation_id, root_item_id)
) WITHOUT ROWID;

CREATE TABLE projection_checkpoint (
    component TEXT PRIMARY KEY,
    schema_fingerprint TEXT NOT NULL,
    implementation_hash TEXT NOT NULL,
    database_revision INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    health TEXT NOT NULL CHECK (health IN ('healthy', 'rebuilding', 'unhealthy')),
    checkpoint_path TEXT,
    updated_at TEXT NOT NULL
) WITHOUT ROWID;

CREATE TABLE search_dirty_name (
    root_item_id INTEGER PRIMARY KEY,
    queued_at_ms INTEGER NOT NULL
);

CREATE TABLE search_dirty_notes (
    root_item_id INTEGER PRIMARY KEY,
    queued_at_ms INTEGER NOT NULL
);

CREATE TABLE search_dirty_source (
    source_post_id INTEGER PRIMARY KEY,
    queued_at_ms INTEGER NOT NULL
);

CREATE VIRTUAL TABLE root_name_fts USING fts5(
    root_item_id UNINDEXED,
    name,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);

CREATE VIRTUAL TABLE root_notes_fts USING fts5(
    root_item_id UNINDEXED,
    notes,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);

CREATE VIRTUAL TABLE source_text_fts USING fts5(
    source_post_id UNINDEXED,
    searchable_text,
    tokenize='unicode61 remove_diacritics 2',
    prefix='2 3'
);

CREATE INDEX idx_library_root_lifecycle ON library_root(lifecycle, item_id);

CREATE INDEX idx_media_asset_file ON media_asset(file_id, item_id);

CREATE INDEX idx_media_view_recent ON media_view(viewed_at DESC, item_id);

CREATE UNIQUE INDEX idx_subscription_one_active_run
    ON subscription_run(subscription_id)
    WHERE status IN ('pending', 'running');

CREATE INDEX idx_subscription_run_query_ready
    ON subscription_run_query(status, available_at, run_query_id);

CREATE INDEX idx_source_item_state ON source_item(state, source_item_id);

CREATE INDEX idx_subscription_run_source_item_source
    ON subscription_run_source_item(source_item_id, run_query_id);

CREATE UNIQUE INDEX idx_ingest_source_item
    ON ingest_job(source_item_id) WHERE source_item_id IS NOT NULL;

CREATE INDEX idx_ingest_ready ON ingest_job(status, available_at, ingest_job_id);

CREATE UNIQUE INDEX idx_work_media_target
    ON work_item(media_item_id, file_id, work_type)
    WHERE media_item_id IS NOT NULL;

CREATE UNIQUE INDEX idx_work_file_target
    ON work_item(file_id, work_type)
    WHERE media_item_id IS NULL AND file_id IS NOT NULL;

CREATE UNIQUE INDEX idx_work_hash_target
    ON work_item(file_hash, work_type) WHERE file_hash IS NOT NULL;

CREATE INDEX idx_subscription_issue_page
    ON subscription_issue(subscription_id, status, last_seen_at DESC, issue_id DESC);

CREATE INDEX idx_cloud_outbox_pending
    ON cloud_outbox(published_at, hlc_physical_ms, hlc_logical, mutation_id);

CREATE INDEX idx_cloud_outbox_current
    ON cloud_outbox(current_epoch, hlc_physical_ms, hlc_logical, mutation_id);

CREATE INDEX idx_cloud_blob_queue ON cloud_blob_state(state, priority DESC, updated_at, file_hash);

CREATE INDEX idx_cloud_blob_upload ON cloud_blob_state(remote_present, state, priority DESC, updated_at, file_hash);

CREATE INDEX idx_subscription_source_post_query
    ON subscription_source_post(query_id, source_post_id);

CREATE INDEX idx_subscription_run_query_success
    ON subscription_run_query(query_id)
    WHERE status = 'succeeded';

CREATE INDEX idx_cloud_blob_upload_priority
    ON cloud_blob_state(remote_present, state, priority DESC, updated_at);

CREATE INDEX idx_source_post_provisional
    ON source_post(source_post_id) WHERE root_item_id IS NULL;

CREATE INDEX idx_media_file_mime
    ON media_file(mime_type, file_id);

CREATE INDEX idx_media_file_size
    ON media_file(size_bytes, file_id);

CREATE INDEX idx_media_file_width
    ON media_file(pixel_width, file_id);

CREATE INDEX idx_media_file_height
    ON media_file(pixel_height, file_id);

CREATE INDEX idx_media_file_duration
    ON media_file(duration_ms, file_id);

CREATE INDEX idx_media_file_audio
    ON media_file(has_audio, file_id);

CREATE INDEX idx_media_asset_imported
    ON media_asset(imported_at, item_id);

CREATE INDEX idx_media_asset_captured
    ON media_asset(captured_at, item_id);

CREATE INDEX idx_file_color_lookup
    ON file_color(file_id, hex);

CREATE INDEX idx_source_item_media
    ON source_item(media_item_id, source_item_id)
    WHERE media_item_id IS NOT NULL;

CREATE INDEX idx_folder_parent_order
    ON folder(parent_id, sort_rank, folder_id);

CREATE INDEX idx_smart_folder_parent_order
    ON smart_folder(parent_id, display_order, smart_folder_id);

CREATE INDEX idx_source_post_site_key
    ON source_post(site_id, post_key, source_post_id);

CREATE INDEX idx_root_metadata_rating
    ON root_metadata(rating, root_item_id);

CREATE INDEX idx_root_metadata_name
    ON root_metadata(name COLLATE NOCASE, root_item_id);

CREATE INDEX idx_root_metadata_notes_present
    ON root_metadata(root_item_id)
    WHERE notes IS NOT NULL AND TRIM(notes) <> '';

CREATE INDEX idx_root_metadata_sources_present
    ON root_metadata(root_item_id)
    WHERE json_array_length(source_urls_json) > 0;

CREATE INDEX idx_canonical_bitmap_revision
    ON canonical_bitmap(revision, domain, key_id);

CREATE INDEX idx_root_summary_imported_asc
    ON root_summary(lifecycle, imported_at ASC, root_item_id ASC);

CREATE INDEX idx_root_summary_captured_desc
    ON root_summary(captured_at DESC, root_item_id ASC);

CREATE INDEX idx_root_summary_captured_asc
    ON root_summary(captured_at ASC, root_item_id ASC);

CREATE INDEX idx_root_summary_rating_desc
    ON root_summary(sort_rating DESC, root_item_id ASC);

CREATE INDEX idx_root_summary_rating_asc
    ON root_summary(sort_rating ASC, root_item_id ASC);

CREATE INDEX idx_root_summary_size_desc
    ON root_summary(total_size_bytes DESC, root_item_id ASC);

CREATE INDEX idx_root_summary_size_asc
    ON root_summary(total_size_bytes ASC, root_item_id ASC);

CREATE INDEX idx_root_summary_name_asc
    ON root_summary(sort_name COLLATE NOCASE ASC, root_item_id ASC);

CREATE INDEX idx_root_summary_name_desc
    ON root_summary(sort_name COLLATE NOCASE DESC, root_item_id ASC);

CREATE INDEX idx_root_summary_kind
    ON root_summary(kind, root_item_id);

CREATE INDEX idx_smart_folder_dependency_lookup
    ON smart_folder_dependency(dependency_kind, dependency_key, smart_folder_id);

CREATE UNIQUE INDEX idx_smart_folder_generation_active
    ON smart_folder_generation(smart_folder_id)
    WHERE state = 'active';

CREATE UNIQUE INDEX idx_smart_folder_generation_building
    ON smart_folder_generation(smart_folder_id)
    WHERE state = 'building';

CREATE INDEX idx_smart_folder_generation_state
    ON smart_folder_generation(state, smart_folder_id, generation_id);

CREATE INDEX idx_smart_folder_membership_root
    ON smart_folder_membership(root_item_id, generation_id);

CREATE INDEX idx_work_ready_priority
    ON work_item(status, priority DESC, available_at, work_id);

CREATE INDEX idx_media_view_root_recent
    ON media_view(item_id, viewed_at DESC);

CREATE INDEX idx_duplicate_status_files
    ON duplicate(status, file_id_a, file_id_b);

CREATE INDEX idx_source_item_media_source
    ON source_item(media_item_id, source_post_id, position)
    WHERE media_item_id IS NOT NULL;

CREATE INDEX idx_source_item_post_order
    ON source_item(source_post_id, position, source_item_id);

CREATE INDEX idx_source_post_root
    ON source_post(root_item_id, source_post_id)
    WHERE root_item_id IS NOT NULL;

CREATE TRIGGER smart_generation_membership_insert
AFTER INSERT ON smart_folder_membership
WHEN (SELECT suppress_smart_dirty FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE smart_folder_generation
    SET member_count = member_count + 1
    WHERE generation_id = NEW.generation_id;
END;

CREATE TRIGGER smart_generation_membership_delete
AFTER DELETE ON smart_folder_membership
WHEN (SELECT suppress_smart_dirty FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE smart_folder_generation
    SET member_count = member_count - 1
    WHERE generation_id = OLD.generation_id;
END;

CREATE TRIGGER smart_generation_definition_insert
AFTER INSERT ON smart_folder BEGIN
    INSERT OR IGNORE INTO smart_folder_generation (
        smart_folder_id, database_revision, state, created_at
    ) VALUES (
        NEW.smart_folder_id,
        (SELECT revision + 1 FROM library_meta WHERE singleton = 1),
        'building',
        strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    );
END;

CREATE TRIGGER smart_generation_definition_update
AFTER UPDATE OF parent_id, predicate_json ON smart_folder BEGIN
    INSERT OR IGNORE INTO smart_folder_generation (
        smart_folder_id, database_revision, state, created_at
    )
    WITH RECURSIVE affected(smart_folder_id) AS (
        SELECT NEW.smart_folder_id
        UNION
        SELECT child.smart_folder_id
        FROM smart_folder child
        JOIN affected parent ON child.parent_id = parent.smart_folder_id
    )
    SELECT smart_folder_id,
           (SELECT revision + 1 FROM library_meta WHERE singleton = 1),
           'building',
           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM affected;
END;

CREATE TRIGGER smart_generation_tag_insert AFTER INSERT ON tag BEGIN
    INSERT OR IGNORE INTO smart_folder_generation (
        smart_folder_id, database_revision, state, created_at
    )
    SELECT dependency.smart_folder_id,
           (SELECT revision + 1 FROM library_meta WHERE singleton = 1),
           'building',
           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM smart_folder_dependency dependency
    WHERE dependency.dependency_kind = 'tag'
      AND dependency.dependency_key = CASE
            WHEN NEW.namespace = 'general' THEN NEW.subtag
            ELSE NEW.namespace || ':' || NEW.subtag
          END;
END;

CREATE TRIGGER smart_generation_tag_identity_update
AFTER UPDATE OF namespace, subtag ON tag BEGIN
    INSERT OR IGNORE INTO smart_folder_generation (
        smart_folder_id, database_revision, state, created_at
    )
    SELECT dependency.smart_folder_id,
           (SELECT revision + 1 FROM library_meta WHERE singleton = 1),
           'building',
           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM smart_folder_dependency dependency
    WHERE dependency.dependency_kind = 'tag'
      AND dependency.dependency_key IN (
          CASE WHEN OLD.namespace = 'general' THEN OLD.subtag
               ELSE OLD.namespace || ':' || OLD.subtag END,
          CASE WHEN NEW.namespace = 'general' THEN NEW.subtag
               ELSE NEW.namespace || ':' || NEW.subtag END
      );
END;

CREATE TRIGGER smart_generation_tag_delete AFTER DELETE ON tag BEGIN
    INSERT OR IGNORE INTO smart_folder_generation (
        smart_folder_id, database_revision, state, created_at
    )
    SELECT dependency.smart_folder_id,
           (SELECT revision + 1 FROM library_meta WHERE singleton = 1),
           'building',
           strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    FROM smart_folder_dependency dependency
    WHERE dependency.dependency_kind = 'tag'
      AND dependency.dependency_key = CASE
            WHEN OLD.namespace = 'general' THEN OLD.subtag
            ELSE OLD.namespace || ':' || OLD.subtag
          END;
END;

CREATE TRIGGER search_root_metadata_insert AFTER INSERT ON root_metadata BEGIN
    INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
    VALUES (NEW.root_item_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
    INSERT INTO search_dirty_notes(root_item_id, queued_at_ms)
    VALUES (NEW.root_item_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_root_metadata_update
AFTER UPDATE OF name, notes ON root_metadata BEGIN
    INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
    SELECT NEW.root_item_id, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.name IS NOT NEW.name
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
    INSERT INTO search_dirty_notes(root_item_id, queued_at_ms)
    SELECT NEW.root_item_id, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.notes IS NOT NEW.notes
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_root_metadata_delete AFTER DELETE ON root_metadata BEGIN
    INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
    VALUES (OLD.root_item_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
    INSERT INTO search_dirty_notes(root_item_id, queued_at_ms)
    VALUES (OLD.root_item_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_media_name_update
AFTER UPDATE OF name ON media_asset BEGIN
    INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
    SELECT item_id, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM library_root WHERE item_id = NEW.item_id
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_group_order_insert
AFTER INSERT ON canonical_order WHEN NEW.owner_kind = 'group' BEGIN
    INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
    VALUES (NEW.owner_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_group_order_update
AFTER UPDATE OF payload, checksum ON canonical_order
WHEN NEW.owner_kind = 'group' BEGIN
    INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
    VALUES (NEW.owner_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_group_order_delete
AFTER DELETE ON canonical_order WHEN OLD.owner_kind = 'group' BEGIN
    INSERT INTO search_dirty_name(root_item_id, queued_at_ms)
    VALUES (OLD.owner_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(root_item_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_source_post_insert AFTER INSERT ON source_post BEGIN
    INSERT INTO search_dirty_source(source_post_id, queued_at_ms)
    VALUES (NEW.source_post_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(source_post_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_source_post_update
AFTER UPDATE OF site_id, post_key, canonical_url, creator_name, title, description
ON source_post BEGIN
    INSERT INTO search_dirty_source(source_post_id, queued_at_ms)
    VALUES (NEW.source_post_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(source_post_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_source_post_delete AFTER DELETE ON source_post BEGIN
    INSERT INTO search_dirty_source(source_post_id, queued_at_ms)
    VALUES (OLD.source_post_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(source_post_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_source_item_insert AFTER INSERT ON source_item BEGIN
    INSERT INTO search_dirty_source(source_post_id, queued_at_ms)
    VALUES (NEW.source_post_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(source_post_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_source_item_update
AFTER UPDATE OF source_post_id, media_url, canonical_url ON source_item BEGIN
    INSERT INTO search_dirty_source(source_post_id, queued_at_ms)
    VALUES (OLD.source_post_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(source_post_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
    INSERT INTO search_dirty_source(source_post_id, queued_at_ms)
    VALUES (NEW.source_post_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(source_post_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER search_source_item_delete AFTER DELETE ON source_item BEGIN
    INSERT INTO search_dirty_source(source_post_id, queued_at_ms)
    VALUES (OLD.source_post_id, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT(source_post_id) DO UPDATE SET queued_at_ms = excluded.queued_at_ms;
END;

CREATE TRIGGER canonical_root_insert AFTER INSERT ON library_root
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    INSERT INTO root_summary (
        root_item_id, lifecycle, kind, cover_media_item_id, media_count,
        total_size_bytes, imported_at, captured_at, sort_rating, sort_name, updated_at
    )
    SELECT item.item_id, NEW.lifecycle, item.kind,
           COALESCE(item.cover_media_item_id,
                    CASE WHEN item.kind = 'media' THEN asset.item_id END),
           CASE WHEN item.kind = 'media' AND asset.item_id IS NOT NULL THEN 1
                ELSE 0 END,
           CASE WHEN item.kind = 'media'
                THEN COALESCE(file.size_bytes, 0)
                ELSE 0 END,
           CASE WHEN item.kind = 'media' THEN asset.imported_at END,
           CASE WHEN item.kind = 'media' THEN asset.captured_at END,
           metadata.rating,
           COALESCE(metadata.name, asset.name),
           COALESCE(metadata.updated_at, item.updated_at)
    FROM library_item item
    LEFT JOIN media_asset asset ON asset.item_id = COALESCE(
        item.cover_media_item_id, CASE WHEN item.kind = 'media' THEN item.item_id END
    )
    LEFT JOIN media_file file ON file.file_id = asset.file_id
    LEFT JOIN root_metadata metadata ON metadata.root_item_id = item.item_id
    WHERE item.item_id = NEW.item_id;
END;

CREATE TRIGGER canonical_root_lifecycle_update
AFTER UPDATE OF lifecycle ON library_root
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE root_summary SET lifecycle = NEW.lifecycle,
        updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
    WHERE root_item_id = NEW.item_id;
END;

CREATE TRIGGER canonical_metadata_insert AFTER INSERT ON root_metadata
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE root_summary SET sort_rating = NEW.rating,
        sort_name = COALESCE(NEW.name, sort_name), updated_at = NEW.updated_at
    WHERE root_item_id = NEW.root_item_id;
END;

CREATE TRIGGER canonical_metadata_update
AFTER UPDATE OF name, rating, updated_at ON root_metadata
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE root_summary SET sort_rating = NEW.rating,
        sort_name = COALESCE(NEW.name, sort_name), updated_at = NEW.updated_at
    WHERE root_item_id = NEW.root_item_id;
END;

CREATE TRIGGER canonical_cover_update AFTER UPDATE OF cover_media_item_id ON library_item
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE root_summary SET cover_media_item_id = COALESCE(
        NEW.cover_media_item_id, CASE WHEN NEW.kind = 'media' THEN NEW.item_id END
    ), updated_at = NEW.updated_at WHERE root_item_id = NEW.item_id;
END;

CREATE TRIGGER canonical_media_update
AFTER UPDATE OF imported_at, captured_at, name, file_id ON media_asset
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE root_summary SET
        imported_at = CASE WHEN kind = 'media' THEN NEW.imported_at ELSE imported_at END,
        captured_at = CASE WHEN kind = 'media' THEN NEW.captured_at ELSE captured_at END,
        sort_name = CASE WHEN kind = 'media' AND NOT EXISTS (
            SELECT 1 FROM root_metadata WHERE root_item_id = root_summary.root_item_id
              AND name IS NOT NULL
        ) THEN NEW.name ELSE sort_name END,
        total_size_bytes = CASE WHEN kind = 'media' THEN COALESCE((
            SELECT size_bytes FROM media_file WHERE file_id = NEW.file_id
        ), 0) ELSE total_size_bytes END,
        updated_at = NEW.updated_at
    WHERE root_item_id = NEW.item_id;
END;

CREATE TRIGGER canonical_media_insert AFTER INSERT ON media_asset
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE root_summary SET
        cover_media_item_id = CASE WHEN kind = 'media' THEN NEW.item_id ELSE cover_media_item_id END,
        media_count = CASE WHEN kind = 'media' THEN 1 ELSE media_count END,
        imported_at = CASE WHEN kind = 'media' THEN NEW.imported_at ELSE imported_at END,
        captured_at = CASE WHEN kind = 'media' THEN NEW.captured_at ELSE captured_at END,
        sort_name = CASE WHEN kind = 'media' AND NOT EXISTS (
            SELECT 1 FROM root_metadata WHERE root_item_id = root_summary.root_item_id
              AND name IS NOT NULL
        ) THEN NEW.name ELSE sort_name END,
        total_size_bytes = CASE WHEN kind = 'media' THEN COALESCE((
            SELECT size_bytes FROM media_file WHERE file_id = NEW.file_id
        ), 0) ELSE total_size_bytes END,
        updated_at = NEW.updated_at
    WHERE root_item_id = NEW.item_id;
END;

CREATE TRIGGER canonical_file_size_update AFTER UPDATE OF size_bytes ON media_file
WHEN OLD.size_bytes <> NEW.size_bytes
 AND (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE root_summary SET total_size_bytes = total_size_bytes + NEW.size_bytes - OLD.size_bytes
    WHERE root_item_id IN (
        SELECT asset.item_id FROM media_asset asset JOIN library_root root ON root.item_id = asset.item_id
        WHERE asset.file_id = NEW.file_id
    );
END;

CREATE TRIGGER canonical_lifecycle_summary_insert AFTER INSERT ON root_summary
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE lifecycle_summary SET root_count = root_count + 1,
        media_count = media_count + NEW.media_count,
        total_size_bytes = total_size_bytes + NEW.total_size_bytes
    WHERE lifecycle = NEW.lifecycle;
END;

CREATE TRIGGER canonical_lifecycle_summary_delete BEFORE DELETE ON root_summary
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE lifecycle_summary SET root_count = root_count - 1,
        media_count = media_count - OLD.media_count,
        total_size_bytes = total_size_bytes - OLD.total_size_bytes
    WHERE lifecycle = OLD.lifecycle;
END;

CREATE TRIGGER canonical_lifecycle_summary_update
AFTER UPDATE OF lifecycle, media_count, total_size_bytes ON root_summary
WHEN (SELECT suppress_root_summary FROM projection_write_control WHERE singleton = 1) = 0
BEGIN
    UPDATE lifecycle_summary SET root_count = root_count - 1,
        media_count = media_count - OLD.media_count,
        total_size_bytes = total_size_bytes - OLD.total_size_bytes
    WHERE lifecycle = OLD.lifecycle;
    UPDATE lifecycle_summary SET root_count = root_count + 1,
        media_count = media_count + NEW.media_count,
        total_size_bytes = total_size_bytes + NEW.total_size_bytes
    WHERE lifecycle = NEW.lifecycle;
END;

CREATE TRIGGER canonical_folder_insert AFTER INSERT ON folder BEGIN
    INSERT INTO folder_summary(folder_id) VALUES (NEW.folder_id)
    ON CONFLICT(folder_id) DO NOTHING;
END;

CREATE TRIGGER canonical_tag_insert AFTER INSERT ON tag BEGIN
    INSERT INTO tag_summary(tag_id) VALUES (NEW.tag_id) ON CONFLICT(tag_id) DO NOTHING;
END;

