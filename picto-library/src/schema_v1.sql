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
