//! Canonical SQLite DDL constants for fresh library initialization.

pub(super) const PRAGMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -64000;
PRAGMA mmap_size = 268435456;
PRAGMA temp_store = MEMORY;
"#;

pub(super) const LIBRARY_DDL: &str = r#"
-- ═══════════════════════════════════════════════════
-- FILES
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS file (
    file_id         INTEGER PRIMARY KEY,
    hash            TEXT    NOT NULL UNIQUE,
    name            TEXT,
    size            INTEGER NOT NULL,
    mime            TEXT    NOT NULL,
    width           INTEGER,
    height          INTEGER,
    duration_ms     INTEGER,
    num_frames      INTEGER,
    has_audio       INTEGER NOT NULL DEFAULT 0,
    status          INTEGER NOT NULL DEFAULT 0,
    rating          INTEGER,
    view_count      INTEGER NOT NULL DEFAULT 0,
    last_viewed_at  TEXT,
    phash           TEXT,
    imported_at     TEXT    NOT NULL,
    notes           TEXT,
    source_urls_json TEXT,
    dominant_color_hex TEXT,
    dominant_palette_blob BLOB,
    name_source TEXT NOT NULL DEFAULT 'unknown'
);
CREATE INDEX IF NOT EXISTS idx_file_status     ON file(status);
CREATE INDEX IF NOT EXISTS idx_file_imported   ON file(imported_at);
CREATE INDEX IF NOT EXISTS idx_file_size       ON file(size);
CREATE INDEX IF NOT EXISTS idx_file_rating     ON file(rating);
CREATE INDEX IF NOT EXISTS idx_file_view_count ON file(view_count);
CREATE INDEX IF NOT EXISTS idx_file_phash      ON file(phash) WHERE phash IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_file_last_viewed ON file(last_viewed_at) WHERE last_viewed_at IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_file_mime       ON file(mime);
-- Composite indexes for grid pagination (status + sort column + file_id tiebreaker)
CREATE INDEX IF NOT EXISTS idx_file_status_imported  ON file(status, imported_at DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_viewed    ON file(status, last_viewed_at DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_rating    ON file(status, rating DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_size      ON file(status, size DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_viewcount ON file(status, view_count DESC, file_id DESC);
CREATE INDEX IF NOT EXISTS idx_file_status_name      ON file(status, name COLLATE NOCASE, file_id);

CREATE TABLE IF NOT EXISTS file_color (
    rowid   INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    hex     TEXT    NOT NULL,
    l       REAL    NOT NULL,
    a       REAL    NOT NULL,
    b       REAL    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_fc_file ON file_color(file_id);
CREATE INDEX IF NOT EXISTS idx_fc_lab  ON file_color(l, a, b);

CREATE VIRTUAL TABLE IF NOT EXISTS file_color_rtree USING rtree(
    id,
    l_min, l_max,
    a_min, a_max,
    b_min, b_max
);

CREATE VIRTUAL TABLE IF NOT EXISTS file_fts USING fts5(
    name, notes, source_urls,
    content='file',
    content_rowid='file_id',
    tokenize='unicode61'
);

-- ═══════════════════════════════════════════════════
-- MEDIA ENTITIES (single + collection)
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS media_entity (
    entity_id    INTEGER PRIMARY KEY,
    kind         TEXT NOT NULL CHECK(kind IN ('single','collection')),
    parent_collection_id INTEGER REFERENCES media_entity(entity_id) ON DELETE SET NULL,
    collection_ordinal   INTEGER,
    cover_file_id        INTEGER REFERENCES file(file_id) ON DELETE SET NULL,
    cached_item_count    INTEGER NOT NULL DEFAULT 0,
    cached_total_size_bytes INTEGER NOT NULL DEFAULT 0,
    name         TEXT,
    description  TEXT NOT NULL DEFAULT '',
    status       INTEGER NOT NULL DEFAULT 1,
    rating       INTEGER,
    created_at   TEXT,
    updated_at   TEXT
);
CREATE INDEX IF NOT EXISTS idx_media_entity_kind    ON media_entity(kind);
CREATE INDEX IF NOT EXISTS idx_media_entity_updated ON media_entity(updated_at);
CREATE INDEX IF NOT EXISTS idx_media_entity_status_entity_id ON media_entity(status, entity_id DESC);
CREATE INDEX IF NOT EXISTS idx_media_entity_parent ON media_entity(parent_collection_id);
CREATE INDEX IF NOT EXISTS idx_media_entity_parent_ord ON media_entity(parent_collection_id, collection_ordinal, entity_id);
CREATE INDEX IF NOT EXISTS idx_media_entity_cover ON media_entity(cover_file_id) WHERE cover_file_id IS NOT NULL;

CREATE TRIGGER IF NOT EXISTS trg_media_entity_parent_validate_insert
BEFORE INSERT ON media_entity
BEGIN
    SELECT RAISE(ABORT, 'media_entity: collections cannot belong to a collection')
    WHERE NEW.kind = 'collection' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: only singles can belong to a collection')
    WHERE NEW.kind != 'single' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: parent must be kind=collection')
    WHERE NEW.parent_collection_id IS NOT NULL
      AND COALESCE(
          (SELECT kind FROM media_entity WHERE entity_id = NEW.parent_collection_id),
          ''
      ) != 'collection';

    SELECT RAISE(ABORT, 'media_entity: collection_ordinal requires parent_collection_id')
    WHERE NEW.collection_ordinal IS NOT NULL AND NEW.parent_collection_id IS NULL;
END;

CREATE TRIGGER IF NOT EXISTS trg_media_entity_parent_validate_update
BEFORE UPDATE OF kind, parent_collection_id, collection_ordinal ON media_entity
BEGIN
    SELECT RAISE(ABORT, 'media_entity: collections cannot belong to a collection')
    WHERE NEW.kind = 'collection' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: only singles can belong to a collection')
    WHERE NEW.kind != 'single' AND NEW.parent_collection_id IS NOT NULL;

    SELECT RAISE(ABORT, 'media_entity: parent must be kind=collection')
    WHERE NEW.parent_collection_id IS NOT NULL
      AND COALESCE(
          (SELECT kind FROM media_entity WHERE entity_id = NEW.parent_collection_id),
          ''
      ) != 'collection';

    SELECT RAISE(ABORT, 'media_entity: collection_ordinal requires parent_collection_id')
    WHERE NEW.collection_ordinal IS NOT NULL AND NEW.parent_collection_id IS NULL;
END;

CREATE TABLE IF NOT EXISTS entity_file (
    entity_id INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    file_id   INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_entity_file_file_id ON entity_file(file_id);
CREATE TRIGGER IF NOT EXISTS trg_entity_file_kind_check
BEFORE INSERT ON entity_file
BEGIN
    SELECT RAISE(ABORT, 'entity_file: entity must be kind=single')
    WHERE (SELECT kind FROM media_entity WHERE entity_id = NEW.entity_id) != 'single';
END;

-- collection_member table removed — membership is tracked via media_entity.parent_collection_id

CREATE TABLE IF NOT EXISTS collection_tag (
    collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag                  TEXT NOT NULL,
    PRIMARY KEY (collection_entity_id, tag)
);
CREATE INDEX IF NOT EXISTS idx_collection_tag_tag ON collection_tag(tag COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS collection_source_url (
    collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    url                  TEXT NOT NULL,
    PRIMARY KEY (collection_entity_id, url)
);
CREATE INDEX IF NOT EXISTS idx_collection_source_url ON collection_source_url(collection_entity_id);

-- ═══════════════════════════════════════════════════
-- TAGS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS tag (
    tag_id     INTEGER PRIMARY KEY,
    namespace  TEXT NOT NULL,
    subtag     TEXT NOT NULL,
    file_count INTEGER NOT NULL DEFAULT 0,
    UNIQUE(namespace, subtag)
);
CREATE INDEX IF NOT EXISTS idx_tag_subtag     ON tag(subtag COLLATE NOCASE);
CREATE INDEX IF NOT EXISTS idx_tag_file_count ON tag(file_count) WHERE file_count > 0;

CREATE TABLE IF NOT EXISTS entity_tag_raw (
    entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag_id    INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source    TEXT NOT NULL DEFAULT 'local',
    PRIMARY KEY (entity_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_etr_tag ON entity_tag_raw(tag_id, entity_id);

CREATE TABLE IF NOT EXISTS tag_alias (
    from_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    to_tag_id   INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source      TEXT NOT NULL,
    PRIMARY KEY (from_tag_id, source)
);

CREATE TABLE IF NOT EXISTS tag_implication (
    child_tag_id  INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    parent_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source        TEXT NOT NULL,
    PRIMARY KEY (child_tag_id, parent_tag_id, source)
);

CREATE TABLE IF NOT EXISTS tag_ancestor (
    tag_id      INTEGER NOT NULL,
    ancestor_id INTEGER NOT NULL,
    depth       INTEGER NOT NULL,
    PRIMARY KEY (tag_id, ancestor_id)
);
CREATE INDEX IF NOT EXISTS idx_ta_ancestor ON tag_ancestor(ancestor_id, tag_id);

CREATE TABLE IF NOT EXISTS tag_display (
    tag_id     INTEGER PRIMARY KEY,
    display_ns TEXT NOT NULL,
    display_st TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS entity_tag_implied (
    entity_id INTEGER NOT NULL,
    tag_id    INTEGER NOT NULL,
    PRIMARY KEY (entity_id, tag_id)
);
CREATE INDEX IF NOT EXISTS idx_eti_tag ON entity_tag_implied(tag_id, entity_id);

CREATE VIRTUAL TABLE IF NOT EXISTS tag_fts USING fts5(
    namespace, subtag,
    content='tag',
    content_rowid='tag_id',
    tokenize='unicode61'
);

-- ═══════════════════════════════════════════════════
-- FOLDERS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS folder (
    folder_id  INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    parent_id  INTEGER REFERENCES folder(folder_id) ON DELETE SET NULL,
    icon       TEXT,
    color      TEXT,
    auto_tags  TEXT NOT NULL DEFAULT '[]',
    watch_path TEXT,
    watch_enabled INTEGER NOT NULL DEFAULT 0,
    watch_subfolders INTEGER NOT NULL DEFAULT 0,
    watch_import_status_mode TEXT NOT NULL DEFAULT 'inherit',
    sort_order INTEGER,
    created_at TEXT,
    updated_at TEXT
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_folder_watch_path
    ON folder(watch_path)
    WHERE watch_path IS NOT NULL;

CREATE TABLE IF NOT EXISTS folder_entity (
    folder_id     INTEGER NOT NULL REFERENCES folder(folder_id) ON DELETE CASCADE,
    entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    position_rank INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (folder_id, entity_id)
);
CREATE INDEX IF NOT EXISTS idx_fe_rank ON folder_entity(folder_id, position_rank);

-- ═══════════════════════════════════════════════════
-- SMART FOLDERS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS smart_folder (
    smart_folder_id INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    parent_id       INTEGER REFERENCES smart_folder(smart_folder_id) ON DELETE SET NULL,
    icon            TEXT,
    color           TEXT,
    predicate_json  TEXT NOT NULL,
    sort_field      TEXT,
    sort_order      TEXT,
    display_order   INTEGER,
    created_at      TEXT,
    updated_at      TEXT
);
CREATE INDEX IF NOT EXISTS idx_smart_folder_parent_order
    ON smart_folder(parent_id, COALESCE(display_order, smart_folder_id), smart_folder_id);

-- ═══════════════════════════════════════════════════
-- SUBSCRIPTION GROUPS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS subscription_group (
    group_id   INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    schedule   TEXT NOT NULL DEFAULT 'manual',
    created_at TEXT NOT NULL
);

-- ═══════════════════════════════════════════════════
-- SUBSCRIPTIONS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS subscription (
    subscription_id         INTEGER PRIMARY KEY,
    name                    TEXT NOT NULL,
    site_id                 TEXT NOT NULL,
    paused                  INTEGER NOT NULL DEFAULT 0,
    group_id                INTEGER REFERENCES subscription_group(group_id) ON DELETE CASCADE,
    initial_file_limit      INTEGER NOT NULL DEFAULT 100,
    periodic_file_limit     INTEGER NOT NULL DEFAULT 50,
    auto_collections        INTEGER NOT NULL DEFAULT 1,
    created_at              TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS subscription_query (
    query_id              INTEGER PRIMARY KEY,
    subscription_id       INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    query_text            TEXT NOT NULL,
    display_name          TEXT,
    paused                INTEGER NOT NULL DEFAULT 0,
    last_check_time       TEXT,
    files_found           INTEGER NOT NULL DEFAULT 0,
    completed_initial_run INTEGER NOT NULL DEFAULT 0,
    resume_cursor         TEXT,
    resume_strategy       TEXT
);
CREATE INDEX IF NOT EXISTS idx_sq_sub ON subscription_query(subscription_id);

CREATE TABLE IF NOT EXISTS subscription_entity (
    subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    entity_id       INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    PRIMARY KEY (subscription_id, entity_id)
);

CREATE TABLE IF NOT EXISTS subscription_post_collection (
    subscription_id      INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
    site_id              TEXT NOT NULL,
    post_id              TEXT NOT NULL,
    collection_entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    PRIMARY KEY (subscription_id, site_id, post_id)
);
CREATE INDEX IF NOT EXISTS idx_spc_collection ON subscription_post_collection(collection_entity_id);

-- ═══════════════════════════════════════════════════
-- CREDENTIALS (domain list; actual secrets in OS keychain)
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS credential_domain (
    site_category   TEXT PRIMARY KEY,
    credential_type TEXT NOT NULL,
    display_name    TEXT,
    created_at      TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS credential_health (
    site_category   TEXT PRIMARY KEY,
    health_status   TEXT NOT NULL,
    last_checked_at TEXT NOT NULL,
    last_error      TEXT
);

-- ═══════════════════════════════════════════════════
-- DUPLICATES
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS duplicate (
    file_id_a      INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    file_id_b      INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    distance       REAL    NOT NULL,
    status         TEXT    NOT NULL DEFAULT 'detected',
    decision_at    TEXT,
    decision_source TEXT,
    decision_reason TEXT,
    winner_file_id INTEGER,
    loser_file_id  INTEGER,
    PRIMARY KEY (file_id_a, file_id_b),
    CHECK (file_id_a < file_id_b)
);
CREATE INDEX IF NOT EXISTS idx_dup_b      ON duplicate(file_id_b);
CREATE INDEX IF NOT EXISTS idx_dup_status ON duplicate(status);

-- ═══════════════════════════════════════════════════
-- SIDEBAR PROJECTION
-- ═══════════════════════════════════════════════════
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
    expanded_by_default INTEGER NOT NULL DEFAULT 0,
    meta_json           TEXT,
    updated_at          TEXT
);

-- ═══════════════════════════════════════════════════
-- METADATA PROJECTION
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS entity_metadata_projection (
    entity_id     INTEGER PRIMARY KEY,
    epoch         INTEGER NOT NULL,
    resolved_json TEXT NOT NULL,
    parents_json  TEXT NOT NULL
);

-- ═══════════════════════════════════════════════════
-- VIEW PREFERENCES
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS view_pref (
    scope           TEXT PRIMARY KEY,
    sort_field      TEXT,
    sort_dir        TEXT,
    layout          TEXT,
    tile_size       INTEGER,
    show_name       INTEGER,
    show_resolution INTEGER,
    show_extension  INTEGER,
    show_label      INTEGER,
    thumbnail_fit   TEXT
);

-- ═══════════════════════════════════════════════════
-- LARGE MUTATION TRACKING
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS mutation_action (
    action_id   INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL,
    status      TEXT    NOT NULL DEFAULT 'running',
    total       INTEGER NOT NULL DEFAULT 0,
    progress    INTEGER NOT NULL DEFAULT 0,
    description TEXT,
    created_at  TEXT NOT NULL,
    finished_at TEXT
);

-- ═══════════════════════════════════════════════════
-- MANIFEST (global epoch tracking)
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS manifest (
    key   TEXT PRIMARY KEY,
    epoch INTEGER NOT NULL DEFAULT 0
);

-- Global manifest snapshot metadata (V2)
CREATE TABLE IF NOT EXISTS artifact_manifest_meta (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    manifest_epoch INTEGER NOT NULL DEFAULT 0,
    updated_at     TEXT
);

CREATE TABLE IF NOT EXISTS artifact_manifest_entry (
    manifest_epoch      INTEGER NOT NULL,
    artifact_name       TEXT NOT NULL,
    artifact_version    INTEGER NOT NULL,
    built_from_truth_seq INTEGER NOT NULL DEFAULT 0,
    payload_json        TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (manifest_epoch, artifact_name)
);

-- ═══════════════════════════════════════════════════
-- KV SETTINGS
-- ═══════════════════════════════════════════════════
CREATE TABLE IF NOT EXISTS kv_settings (
    key   TEXT PRIMARY KEY,
    value TEXT
);

-- Schema version
CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
INSERT OR IGNORE INTO schema_version (version) VALUES (33);
"#;
