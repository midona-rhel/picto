# Database & Schema

> SQLite schema, connection model, hash index, blob store, and manifest system.

---

## Overview

Picto uses SQLite (WAL mode) as its single persistent store, accessed via `rusqlite` (bundled).
The database lives at `<library_root>/db/library.sqlite`. Current schema version: **26**.

---

## SqliteDatabase Struct

**File:** `core/src/sqlite/mod.rs:289-306`

```rust
pub struct SqliteDatabase {
    conn:           Arc<Mutex<Connection>>,          // single write connection
    read_pool:      Vec<Arc<Mutex<Connection>>>,     // round-robin read connections
    read_pool_idx:  AtomicUsize,
    pub bitmaps:    Arc<BitmapStore>,
    pub hash_index: Arc<HashIndex>,
    pub manifest:   Arc<Manifest>,
    pub compiler_tx: mpsc::UnboundedSender<CompilerEvent>,
    compiler_rx:    Arc<Mutex<Option<mpsc::UnboundedReceiver<CompilerEvent>>>>,
    db_path:        PathBuf,
    pub scope_cache: Arc<RwLock<HashMap<ScopeSnapshotKey, ScopeSnapshot>>>,
}
```

### Connection Model

| Aspect | Detail |
|--------|--------|
| Write | Single `Arc<Mutex<Connection>>` — serialized writes |
| Read pool | 2–8 connections (`num_cpus::get().min(8).max(2)`) round-robin |
| Blocking | All rusqlite calls wrapped in `spawn_blocking` |
| Timeouts | Warning at 100 ms (reads), 200 ms (writes) |

### Key Access Methods

| Method | Purpose |
|--------|---------|
| `with_conn(f)` | Run closure on write connection via `spawn_blocking` |
| `with_conn_mut(f)` | Write connection with mutable ref (for transactions) |
| `with_read_conn(f)` | Round-robin read pool via `spawn_blocking` |
| `resolve_hash(hash) → i64` | Hash → file_id (cache-first, then DB) |
| `resolve_id(file_id) → String` | file_id → hash (cache-first, then DB) |
| `resolve_ids_batch(ids)` | Batch file_id → hash |
| `resolve_hashes_batch(hashes)` | Batch hash → file_id |

### SQLite Pragmas

```sql
PRAGMA journal_mode   = WAL;
PRAGMA synchronous    = NORMAL;
PRAGMA foreign_keys   = ON;
PRAGMA cache_size     = -64000;   -- 64 MB
PRAGMA mmap_size      = 268435456; -- 256 MB
PRAGMA temp_store     = MEMORY;
```

Defined in `core/src/sqlite/schema.rs:907-914`.

---

## HashIndex

**File:** `core/src/sqlite/hash_index.rs:1-71`

Bidirectional LRU cache mapping hex SHA256 hashes ↔ dense integer file_ids.

```rust
struct HashIndexInner {
    forward: LruCache<String, i64>,   // hash → file_id
    reverse: LruCache<i64, String>,   // file_id → hash
}
```

- **Capacity:** 50,000 entries (DEFAULT_CAPACITY)
- **Thread-safe:** `RwLock<HashIndexInner>` (writes lock for LRU promotion)
- Methods: `insert`, `get_id`, `get_hash`, `remove_by_hash`, `clear`

---

## BlobStore

**File:** `core/src/blob_store.rs:1-244`

Content-addressed file storage with two-level hex sharding.

### Directory Structure

```
<library_root>/blobs/
├── f/<ab>/<cd>/<hash>.<ext>   # originals
└── t/<ab>/<cd>/<hash>.<ext>   # thumbnails
```

Shard prefix: `hash[0..2]` / `hash[2..4]`. Extensions derived from MIME type.

### Key Methods

| Method | Purpose |
|--------|---------|
| `write_original(hash, data, ext)` | Store original (idempotent) |
| `read_original(hash, ext)` | Read original bytes |
| `find_original(hash, ext_hint)` | Find original path + extension |
| `write_thumbnail(hash, data, ext)` | Store thumbnail |
| `read_thumbnail(hash)` | Read thumbnail bytes |
| `delete(hash)` | Delete original + thumbnail |
| `wipe()` | Delete everything, recreate dirs |

### Supported MIME Types

- **Images:** JPEG, PNG, GIF, WebP, BMP, TIFF, SVG, AVIF, HEIF, JXL, ICO, PSD
- **Videos:** MP4, WebM, MKV, MOV, FLV, AVI
- **Audio:** FLAC, WAV
- **Documents:** PDF, EPUB
- **Fallback:** `.bin`

---

## Manifest System

**File:** `core/src/sqlite/mod.rs:35-268`

Tracks version epochs for derived artifacts (bitmaps, sidebar, metadata projections).

```rust
struct ManifestState {
    published_epoch:             u64,
    published_artifact_versions: HashMap<String, u64>,
    published_artifact_payloads: HashMap<String, String>,
    working_artifact_versions:   HashMap<String, u64>,
    working_artifact_payloads:   HashMap<String, String>,
    dirty:                       bool,
}
```

### Tracked Artifacts

`"global"`, `"files"`, `"tags"`, `"tag_graph"`, `"effective_tags"`,
`"metadata_projection"`, `"sidebar"`, `"smart_folders"`, `"bitmaps"`

### Persistence Tables

```sql
CREATE TABLE artifact_manifest_meta (
    id             INTEGER PRIMARY KEY CHECK (id = 1),
    manifest_epoch INTEGER NOT NULL DEFAULT 0,
    updated_at     TEXT
);
CREATE TABLE artifact_manifest_entry (
    manifest_epoch   INTEGER NOT NULL,
    artifact_name    TEXT    NOT NULL,
    artifact_version INTEGER NOT NULL,
    built_from_truth_seq INTEGER NOT NULL DEFAULT 0,
    payload_json     TEXT NOT NULL DEFAULT '{}',
    PRIMARY KEY (manifest_epoch, artifact_name)
);
```

---

## ScopeSnapshot Cache

**File:** `core/src/sqlite/mod.rs:270-286`

Caches stable ordered ID lists for grid pagination.

```rust
pub struct ScopeSnapshot {
    pub ids:         Vec<i64>,
    pub total_count: i64,
    pub created_at:  Instant,
}
pub struct ScopeSnapshotKey {
    pub scope:          String,
    pub predicate_hash: u64,
    pub sort_field:     String,
    pub sort_dir:       String,
}
```

- Max **64** entries, **30 s** TTL
- Invalidated on mutations (status, tag, folder changes)

---

## Complete Schema (V26)

### file

```sql
CREATE TABLE file (
    file_id              INTEGER PRIMARY KEY,
    hash                 TEXT    NOT NULL UNIQUE,
    name                 TEXT,
    size                 INTEGER NOT NULL,
    mime                 TEXT    NOT NULL,
    width                INTEGER,
    height               INTEGER,
    duration_ms          INTEGER,
    num_frames           INTEGER,
    has_audio            INTEGER NOT NULL DEFAULT 0,
    blurhash             TEXT,
    status               INTEGER NOT NULL DEFAULT 0,
    rating               INTEGER,
    view_count           INTEGER NOT NULL DEFAULT 0,
    last_viewed_at       TEXT,
    phash                TEXT,
    imported_at          TEXT    NOT NULL,
    notes                TEXT,
    source_urls_json     TEXT,
    dominant_color_hex   TEXT,
    dominant_palette_blob BLOB,
    name_source          TEXT NOT NULL DEFAULT 'unknown'
);
```

**Indexes:** `idx_file_status`, `idx_file_imported`, `idx_file_size`, `idx_file_rating`,
`idx_file_view_count`, `idx_file_phash`, `idx_file_last_viewed`, `idx_file_mime`,
plus composite pagination indexes on `(status, <sort_col> DESC, file_id DESC)`.

**FTS:** `file_fts USING fts5(name, notes, source_urls)` — content table backed by `file`.

### file_color + R-tree

```sql
CREATE TABLE file_color (
    rowid   INTEGER PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    hex     TEXT NOT NULL,
    l REAL NOT NULL, a REAL NOT NULL, b REAL NOT NULL
);
CREATE VIRTUAL TABLE file_color_rtree USING rtree(
    id, l_min, l_max, a_min, a_max, b_min, b_max
);
```

### media_entity

```sql
CREATE TABLE media_entity (
    entity_id                INTEGER PRIMARY KEY,
    kind                     TEXT NOT NULL CHECK(kind IN ('single','collection')),
    parent_collection_id     INTEGER REFERENCES media_entity(entity_id) ON DELETE SET NULL,
    collection_ordinal       INTEGER,
    cover_file_id            INTEGER REFERENCES file(file_id) ON DELETE SET NULL,
    cached_item_count        INTEGER NOT NULL DEFAULT 0,
    cached_total_size_bytes  INTEGER NOT NULL DEFAULT 0,
    name                     TEXT,
    description              TEXT NOT NULL DEFAULT '',
    status                   INTEGER NOT NULL DEFAULT 1,
    rating                   INTEGER,
    created_at               TEXT,
    updated_at               TEXT
);
```

Triggers enforce: collections can't nest in collections; only singles belong to collections.

### entity_file

```sql
CREATE TABLE entity_file (
    entity_id INTEGER PRIMARY KEY REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    file_id   INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE
);
```

### tag

```sql
CREATE TABLE tag (
    tag_id     INTEGER PRIMARY KEY,
    namespace  TEXT NOT NULL,
    subtag     TEXT NOT NULL,
    file_count INTEGER NOT NULL DEFAULT 0,
    UNIQUE(namespace, subtag)
);
```

**FTS:** `tag_fts USING fts5(namespace, subtag, content='tag', content_rowid='tag_id')`.

### tag_alias / tag_implication / tag_ancestor

```sql
CREATE TABLE tag_alias (
    from_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    to_tag_id   INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source      TEXT NOT NULL,
    PRIMARY KEY (from_tag_id, source)
);
CREATE TABLE tag_implication (
    child_tag_id  INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    parent_tag_id INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source        TEXT NOT NULL,
    PRIMARY KEY (child_tag_id, parent_tag_id, source)
);
CREATE TABLE tag_ancestor (
    tag_id      INTEGER NOT NULL,
    ancestor_id INTEGER NOT NULL,
    depth       INTEGER NOT NULL,
    PRIMARY KEY (tag_id, ancestor_id)
);
```

### entity_tag_raw / entity_tag_implied

```sql
CREATE TABLE entity_tag_raw (
    entity_id INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    tag_id    INTEGER NOT NULL REFERENCES tag(tag_id) ON DELETE CASCADE,
    source    TEXT NOT NULL DEFAULT 'local',
    PRIMARY KEY (entity_id, tag_id)
);
CREATE TABLE entity_tag_implied (
    entity_id INTEGER NOT NULL,
    tag_id    INTEGER NOT NULL,
    PRIMARY KEY (entity_id, tag_id)
);
```

### folder / folder_entity

```sql
CREATE TABLE folder (
    folder_id  INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    parent_id  INTEGER REFERENCES folder(folder_id) ON DELETE SET NULL,
    icon       TEXT, color TEXT,
    auto_tags  TEXT NOT NULL DEFAULT '[]',
    sort_order INTEGER,
    created_at TEXT, updated_at TEXT
);
CREATE TABLE folder_entity (
    folder_id     INTEGER NOT NULL REFERENCES folder(folder_id) ON DELETE CASCADE,
    entity_id     INTEGER NOT NULL REFERENCES media_entity(entity_id) ON DELETE CASCADE,
    position_rank INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (folder_id, entity_id)
);
```

### smart_folder

```sql
CREATE TABLE smart_folder (
    smart_folder_id INTEGER PRIMARY KEY,
    name            TEXT NOT NULL,
    icon TEXT, color TEXT,
    predicate_json  TEXT NOT NULL,
    sort_field TEXT, sort_order TEXT,
    display_order   INTEGER,
    created_at TEXT, updated_at TEXT
);
```

### subscription_group / subscription / subscription_query

```sql
CREATE TABLE subscription_group (
    group_id   INTEGER PRIMARY KEY,
    name       TEXT NOT NULL,
    schedule   TEXT NOT NULL DEFAULT 'manual',
    created_at TEXT NOT NULL
);
CREATE TABLE subscription (
    subscription_id     INTEGER PRIMARY KEY,
    name                TEXT NOT NULL,
    site_id             TEXT NOT NULL,
    paused              INTEGER NOT NULL DEFAULT 0,
    group_id            INTEGER REFERENCES subscription_group(group_id) ON DELETE CASCADE,
    initial_file_limit  INTEGER NOT NULL DEFAULT 100,
    periodic_file_limit INTEGER NOT NULL DEFAULT 50,
    created_at          TEXT NOT NULL
);
CREATE TABLE subscription_query (
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
```

### credential_domain / credential_health

```sql
CREATE TABLE credential_domain (
    site_category   TEXT PRIMARY KEY,
    credential_type TEXT NOT NULL,
    display_name    TEXT,
    created_at      TEXT NOT NULL
);
CREATE TABLE credential_health (
    site_category   TEXT PRIMARY KEY,
    health_status   TEXT NOT NULL,
    last_checked_at TEXT NOT NULL,
    last_error      TEXT
);
```

### duplicate

```sql
CREATE TABLE duplicate (
    file_id_a       INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    file_id_b       INTEGER NOT NULL REFERENCES file(file_id) ON DELETE CASCADE,
    distance        REAL    NOT NULL,
    status          TEXT    NOT NULL DEFAULT 'detected',
    decision_at     TEXT,
    decision_source TEXT, decision_reason TEXT,
    winner_file_id  INTEGER, loser_file_id INTEGER,
    PRIMARY KEY (file_id_a, file_id_b),
    CHECK (file_id_a < file_id_b)
);
```

### sidebar_node (projection)

```sql
CREATE TABLE sidebar_node (
    node_id             TEXT PRIMARY KEY,
    kind                TEXT NOT NULL,
    parent_id           TEXT,
    name                TEXT NOT NULL,
    icon TEXT, color TEXT,
    sort_order          INTEGER,
    count               INTEGER,
    freshness           TEXT NOT NULL DEFAULT 'stale',
    epoch               INTEGER NOT NULL DEFAULT 0,
    selectable          INTEGER NOT NULL DEFAULT 1,
    expanded_by_default INTEGER NOT NULL DEFAULT 0,
    meta_json           TEXT,
    updated_at          TEXT
);
```

### entity_metadata_projection

```sql
CREATE TABLE entity_metadata_projection (
    entity_id     INTEGER PRIMARY KEY,
    epoch         INTEGER NOT NULL,
    resolved_json TEXT NOT NULL,
    parents_json  TEXT NOT NULL
);
```

### view_pref

```sql
CREATE TABLE view_pref (
    scope           TEXT PRIMARY KEY,
    sort_field      TEXT, sort_dir TEXT,
    layout          TEXT,
    tile_size       INTEGER,
    show_name       INTEGER, show_resolution INTEGER,
    show_extension  INTEGER, show_label INTEGER,
    thumbnail_fit   TEXT
);
```

### mutation_action

```sql
CREATE TABLE mutation_action (
    action_id   INTEGER PRIMARY KEY,
    kind        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'running',
    total       INTEGER NOT NULL DEFAULT 0,
    progress    INTEGER NOT NULL DEFAULT 0,
    description TEXT,
    created_at  TEXT NOT NULL,
    finished_at TEXT
);
```

### kv_settings / schema_version

```sql
CREATE TABLE kv_settings (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE schema_version (version INTEGER NOT NULL);
```

---

## AppState (Library Lifecycle)

**File:** `core/src/state.rs:20-32`

```rust
pub struct AppState {
    pub db:                     Arc<SqliteDatabase>,
    pub blob_store:             Arc<BlobStore>,
    pub settings:               SettingsStore,
    pub rate_limiter:            RateLimiter,
    pub running_subscriptions:   RunningSubscriptions,
    pub sub_terminal_statuses:   SubTerminalStatuses,
    pub library_root:            PathBuf,
    pub cancel:                  CancellationToken,
    pub worker_handles:          Mutex<Vec<(&'static str, JoinHandle<()>)>>,
}
```

### Lifecycle

| Operation | Description |
|-----------|-------------|
| `open_library(path)` | Create dirs, open SQLite, load BlobStore, start background workers |
| `close_library()` | Cancel workers, flush DB + bitmaps, join handles |

---

## Key Files

| File | Purpose |
|------|---------|
| `core/src/sqlite/mod.rs` | SqliteDatabase struct, connection pool, hash resolution |
| `core/src/sqlite/schema.rs` | DDL, migrations (V1–V26), pragma setup |
| `core/src/sqlite/hash_index.rs` | Bidirectional hash ↔ ID LRU cache |
| `core/src/sqlite/bitmaps.rs` | BitmapStore + BitmapKey enum |
| `core/src/sqlite/files.rs` | File CRUD, FileMetadataSlim DTO |
| `core/src/sqlite/compilers.rs` | Compiler event system |
| `core/src/sqlite/projections.rs` | Metadata projections |
| `core/src/blob_store.rs` | Content-addressed file + thumbnail storage |
| `core/src/state.rs` | AppState, library lifecycle |
