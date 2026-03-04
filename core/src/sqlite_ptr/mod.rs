//! PTR (Public Tag Repository) SQLite database — separate file from library DB.
//!
//! Read-only from UI. Only sync + compilers write PTR DB.

pub mod bootstrap;
pub mod cache;
pub mod overlay;
pub mod sync;
pub mod tags;

use rusqlite::{Connection, InterruptHandle, OptionalExtension};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use self::tags::PtrResolvedTag;

/// Max entries in the positive overlay cache before eviction.
const OVERLAY_CACHE_MAX: usize = 10_000;

pub(crate) fn hash_to_blob(hex_str: &str) -> Vec<u8> {
    hex::decode(hex_str).unwrap_or_else(|_| hex_str.as_bytes().to_vec())
}

pub(crate) fn blob_to_hash(bytes: &[u8]) -> String {
    hex::encode(bytes)
}

pub struct PtrSqliteDatabase {
    /// Writer connection — used for sync INSERT/UPDATE operations.
    conn: Arc<Mutex<Connection>>,
    /// Interrupt handle for the writer connection, used for fast sync cancel.
    writer_interrupt: Arc<InterruptHandle>,
    /// Pool of read-only connections for concurrent SELECT queries.
    read_pool: Vec<Arc<Mutex<Connection>>>,
    /// Round-robin counter for read pool.
    read_pool_idx: AtomicUsize,
    negative_cache_mem: Arc<RwLock<std::collections::HashSet<String>>>,
    /// Monotonic epoch — bumped after each overlay rebuild.
    /// Negative cache entries from older epochs are stale.
    overlay_epoch: AtomicI64,
    /// In-memory positive overlay cache: hash → resolved tags.
    /// Cleared on epoch bump. Bounded to OVERLAY_CACHE_MAX entries.
    overlay_cache: Arc<RwLock<HashMap<String, Vec<PtrResolvedTag>>>>,
}

impl PtrSqliteDatabase {
    pub async fn open(library_root: &Path) -> Result<Arc<Self>, String> {
        let db_dir = library_root.join("db");
        std::fs::create_dir_all(&db_dir).map_err(|e| format!("Failed to create db dir: {e}"))?;

        let db_path = db_dir.join("ptr.sqlite");
        let db_path_clone = db_path.clone();

        let conn = tokio::task::spawn_blocking(move || -> Result<Connection, String> {
            let conn = Connection::open(&db_path_clone)
                .map_err(|e| format!("Failed to open PTR SQLite: {e}"))?;

            conn.execute_batch(PTR_PRAGMA_SQL)
                .map_err(|e| format!("Failed to set PTR pragmas: {e}"))?;

            if let Ok(Some(max_vars_opt)) = conn
                .query_row(
                    "SELECT compile_options
                     FROM pragma_compile_options
                     WHERE compile_options LIKE 'MAX_VARIABLE_NUMBER=%'
                     LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()
            {
                tracing::info!(option = %max_vars_opt, "PTR SQLite compile option detected");
            }

            let has_schema: bool = conn
                .query_row(
                    "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='table' AND name='ptr_tag'",
                    [],
                    |row| row.get(0),
                )
                .map_err(|e| format!("Schema check failed: {e}"))?;

            if !has_schema {
                conn.execute_batch(PTR_DDL)
                    .map_err(|e| format!("Failed to init PTR schema: {e}"))?;
                tracing::info!("Initialized fresh PTR database");
            } else {
                // Startup path must remain fast/non-blocking: only apply
                // lightweight migrations here, and defer heavy table rebuilds
                // to the background PTR service after UI launch.
                match apply_ptr_startup_migrations(&conn) {
                    Ok(rebuild_required) => {
                        if rebuild_required {
                            tracing::warn!(
                                "PTR schema rebuild required; deferring heavy migration to background service"
                            );
                        }
                    }
                    Err(e) => {
                        // Keep startup alive even if startup migration fails.
                        tracing::warn!(error = %e, "PTR startup migration failed");
                    }
                }
            }

            Ok(conn)
        })
        .await
        .map_err(|e| format!("Join error: {e}"))??;
        let writer_interrupt = Arc::new(conn.get_interrupt_handle());

        let pool_size = num_cpus::get().min(8).max(2);
        let mut read_pool = Vec::with_capacity(pool_size);
        for _ in 0..pool_size {
            let reader_path = db_path.clone();
            let reader_conn = tokio::task::spawn_blocking(move || -> Result<Connection, String> {
                let c = Connection::open_with_flags(
                    &reader_path,
                    rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
                        | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
                )
                .map_err(|e| format!("Failed to open PTR read connection: {e}"))?;
                c.execute_batch(PTR_PRAGMA_SQL)
                    .map_err(|e| format!("Failed to apply PTR pragmas to reader: {e}"))?;
                Ok(c)
            })
            .await
            .map_err(|e| format!("Join error: {e}"))??;
            read_pool.push(Arc::new(Mutex::new(reader_conn)));
        }
        tracing::info!("Opened {pool_size} PTR read-only connections");

        Ok(Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
            writer_interrupt,
            read_pool,
            read_pool_idx: AtomicUsize::new(0),
            negative_cache_mem: Arc::new(RwLock::new(std::collections::HashSet::new())),
            overlay_epoch: AtomicI64::new(0),
            overlay_cache: Arc::new(RwLock::new(HashMap::new())),
        }))
    }

    pub async fn with_read_conn<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let idx = self.read_pool_idx.fetch_add(1, Ordering::Relaxed) % self.read_pool.len();
        let conn = self.read_pool[idx].clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            f(&conn).map_err(|e| format!("PTR SQLite error: {e}"))
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    pub async fn with_conn<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.blocking_lock();
            f(&conn).map_err(|e| format!("PTR SQLite error: {e}"))
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    pub async fn with_conn_mut<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.blocking_lock();
            f(&mut conn).map_err(|e| format!("PTR SQLite error: {e}"))
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    pub async fn set_synchronous_off(&self) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute_batch("PRAGMA synchronous = OFF")?;
            Ok(())
        })
        .await
    }

    pub async fn set_synchronous_normal(&self) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute_batch("PRAGMA synchronous = NORMAL")?;
            Ok(())
        })
        .await
    }

    pub async fn set_wal_autocheckpoint(&self, pages: i32) -> Result<(), String> {
        self.with_conn(move |conn| {
            conn.execute_batch(&format!("PRAGMA wal_autocheckpoint = {pages}"))?;
            Ok(())
        })
        .await
    }

    pub async fn checkpoint_passive(&self) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE)")?;
            Ok(())
        })
        .await
    }

    pub(crate) fn negative_cache_mem(&self) -> Arc<RwLock<std::collections::HashSet<String>>> {
        self.negative_cache_mem.clone()
    }

    pub fn overlay_epoch(&self) -> i64 {
        self.overlay_epoch.load(Ordering::SeqCst)
    }

    pub async fn bump_epoch(&self) -> i64 {
        let new_epoch = chrono::Utc::now().timestamp();
        self.overlay_epoch.store(new_epoch, Ordering::SeqCst);
        self.negative_cache_mem.write().await.clear();
        self.overlay_cache.write().await.clear();
        tracing::info!(
            epoch = new_epoch,
            "PTR overlay epoch bumped, caches invalidated"
        );
        new_epoch
    }

    pub(crate) fn overlay_cache(&self) -> Arc<RwLock<HashMap<String, Vec<PtrResolvedTag>>>> {
        self.overlay_cache.clone()
    }

    pub async fn needs_schema_rebuild(&self) -> Result<bool, String> {
        self.with_read_conn(get_ptr_rebuild_required).await
    }

    pub async fn run_schema_rebuild(&self) -> Result<(), String> {
        self.with_conn_mut(apply_ptr_full_rebuild).await
    }

    pub fn interrupt_writer(&self) {
        self.writer_interrupt.interrupt();
    }
}

const PTR_PRAGMA_SQL: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -128000;
PRAGMA mmap_size = 536870912;
PRAGMA temp_store = MEMORY;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA busy_timeout = 5000;
"#;

const PTR_DDL: &str = r#"
CREATE TABLE IF NOT EXISTS ptr_tag (
    tag_id     INTEGER PRIMARY KEY,
    namespace  TEXT NOT NULL,
    subtag     TEXT NOT NULL,
    UNIQUE(namespace, subtag)
);
CREATE INDEX IF NOT EXISTS idx_ptr_tag_ns_st ON ptr_tag(namespace, subtag);
CREATE INDEX IF NOT EXISTS idx_ptr_tag_st_id ON ptr_tag(subtag, tag_id);

CREATE TABLE IF NOT EXISTS ptr_file_stub (
    file_stub_id INTEGER PRIMARY KEY,
    hash         BLOB NOT NULL UNIQUE
);

CREATE TABLE IF NOT EXISTS ptr_file_tag (
    file_stub_id INTEGER NOT NULL,
    tag_id       INTEGER NOT NULL,
    PRIMARY KEY (file_stub_id, tag_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_ptr_ft_tag ON ptr_file_tag(tag_id);

CREATE TABLE IF NOT EXISTS ptr_tag_sibling (
    from_tag_id INTEGER NOT NULL,
    to_tag_id   INTEGER NOT NULL,
    PRIMARY KEY (from_tag_id, to_tag_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_ptr_ts_to ON ptr_tag_sibling(to_tag_id);

CREATE TABLE IF NOT EXISTS ptr_tag_parent (
    child_tag_id  INTEGER NOT NULL,
    parent_tag_id INTEGER NOT NULL,
    PRIMARY KEY (child_tag_id, parent_tag_id)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS idx_ptr_tp_parent ON ptr_tag_parent(parent_tag_id);

CREATE TABLE IF NOT EXISTS ptr_tag_display (
    tag_id     INTEGER PRIMARY KEY,
    display_ns TEXT NOT NULL,
    display_st TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ptr_tag_count (
    tag_id     INTEGER PRIMARY KEY,
    file_count INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_ptr_tc ON ptr_tag_count(file_count);

CREATE TABLE IF NOT EXISTS ptr_overlay (
    hash          BLOB PRIMARY KEY,
    resolved_json TEXT NOT NULL,
    epoch         INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ptr_negative_cache (
    hash  BLOB PRIMARY KEY,
    epoch INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ptr_tag_def (
    def_id    INTEGER PRIMARY KEY,
    tag_string TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS ptr_hash_def (
    def_id   INTEGER PRIMARY KEY,
    hash_hex TEXT NOT NULL
);

-- Persistent def_id -> internal ID mapping caches to avoid repeatedly
-- resolving large def-id sets during sync content processing.
CREATE TABLE IF NOT EXISTS ptr_hash_def_resolved (
    def_id       INTEGER PRIMARY KEY,
    file_stub_id INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ptr_tag_def_resolved (
    def_id INTEGER PRIMARY KEY,
    tag_id INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ptr_compact_hash (
    hash            BLOB PRIMARY KEY,
    service_hash_id INTEGER NOT NULL
) WITHOUT ROWID;
CREATE UNIQUE INDEX IF NOT EXISTS idx_ptr_compact_hash_sid ON ptr_compact_hash(service_hash_id);

CREATE TABLE IF NOT EXISTS ptr_compact_tag (
    service_tag_id INTEGER PRIMARY KEY,
    namespace      TEXT NOT NULL,
    subtag         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ptr_compact_tag_ns_st ON ptr_compact_tag(namespace, subtag);

CREATE TABLE IF NOT EXISTS ptr_compact_posting (
    service_hash_id INTEGER PRIMARY KEY,
    tag_ids_blob    BLOB NOT NULL,
    tag_count       INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS ptr_compact_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    running INTEGER NOT NULL DEFAULT 0,
    stage TEXT NOT NULL DEFAULT 'idle',
    rows_done_stage INTEGER NOT NULL DEFAULT 0,
    rows_total_stage INTEGER NOT NULL DEFAULT 0,
    rows_per_sec REAL NOT NULL DEFAULT 0,
    snapshot_dir TEXT,
    service_id INTEGER,
    snapshot_max_index INTEGER,
    updated_at TEXT,
    checkpoint_phase TEXT NOT NULL DEFAULT 'idle',
    checkpoint_last_hash_id INTEGER NOT NULL DEFAULT 0,
    checkpoint_last_tag_id INTEGER NOT NULL DEFAULT 0,
    checkpoint_last_service_hash_id INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO ptr_compact_state (id) VALUES (1);

CREATE TABLE IF NOT EXISTS ptr_schema_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
INSERT OR IGNORE INTO ptr_schema_meta (key, value) VALUES ('schema_version', '4');
INSERT OR IGNORE INTO ptr_schema_meta (key, value) VALUES ('required_schema_version', '4');
INSERT OR IGNORE INTO ptr_schema_meta (key, value) VALUES ('rebuild_required', '0');

CREATE TABLE IF NOT EXISTS ptr_cursor (
    id         INTEGER PRIMARY KEY DEFAULT 1,
    last_index INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO ptr_cursor (id, last_index) VALUES (1, 0);

-- FTS5 contentless index for fast tag search (replaces LIKE '%query%' full-scan).
-- Rebuilt manually via rebuild_fts_index() after each sync.
CREATE VIRTUAL TABLE IF NOT EXISTS ptr_tag_fts USING fts5(
    combined_tag,
    content='',
    tokenize='unicode61 remove_diacritics 2'
);
"#;

const PTR_MIGRATION_TO_V2_WITHOUT_ROWID: &str = r#"
PRAGMA foreign_keys = OFF;

DROP INDEX IF EXISTS idx_ptr_ft_tag;
DROP TABLE IF EXISTS ptr_file_tag_new;
CREATE TABLE ptr_file_tag_new (
    file_stub_id INTEGER NOT NULL,
    tag_id       INTEGER NOT NULL,
    PRIMARY KEY (file_stub_id, tag_id)
) WITHOUT ROWID;
INSERT OR IGNORE INTO ptr_file_tag_new (file_stub_id, tag_id)
SELECT file_stub_id, tag_id FROM ptr_file_tag;
DROP TABLE ptr_file_tag;
ALTER TABLE ptr_file_tag_new RENAME TO ptr_file_tag;
CREATE INDEX IF NOT EXISTS idx_ptr_ft_tag ON ptr_file_tag(tag_id);

DROP INDEX IF EXISTS idx_ptr_ts_to;
DROP TABLE IF EXISTS ptr_tag_sibling_new;
CREATE TABLE ptr_tag_sibling_new (
    from_tag_id INTEGER NOT NULL,
    to_tag_id   INTEGER NOT NULL,
    PRIMARY KEY (from_tag_id, to_tag_id)
) WITHOUT ROWID;
INSERT OR IGNORE INTO ptr_tag_sibling_new (from_tag_id, to_tag_id)
SELECT from_tag_id, to_tag_id FROM ptr_tag_sibling;
DROP TABLE ptr_tag_sibling;
ALTER TABLE ptr_tag_sibling_new RENAME TO ptr_tag_sibling;
CREATE INDEX IF NOT EXISTS idx_ptr_ts_to ON ptr_tag_sibling(to_tag_id);

DROP INDEX IF EXISTS idx_ptr_tp_parent;
DROP TABLE IF EXISTS ptr_tag_parent_new;
CREATE TABLE ptr_tag_parent_new (
    child_tag_id  INTEGER NOT NULL,
    parent_tag_id INTEGER NOT NULL,
    PRIMARY KEY (child_tag_id, parent_tag_id)
) WITHOUT ROWID;
INSERT OR IGNORE INTO ptr_tag_parent_new (child_tag_id, parent_tag_id)
SELECT child_tag_id, parent_tag_id FROM ptr_tag_parent;
DROP TABLE ptr_tag_parent;
ALTER TABLE ptr_tag_parent_new RENAME TO ptr_tag_parent;
CREATE INDEX IF NOT EXISTS idx_ptr_tp_parent ON ptr_tag_parent(parent_tag_id);

PRAGMA foreign_keys = ON;
"#;

const PTR_MIGRATION_TO_V3_RESOLVED_MAPS: &str = r#"
CREATE TABLE IF NOT EXISTS ptr_hash_def_resolved (
    def_id       INTEGER PRIMARY KEY,
    file_stub_id INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS ptr_tag_def_resolved (
    def_id INTEGER PRIMARY KEY,
    tag_id INTEGER NOT NULL
);
"#;

const PTR_MIGRATION_TO_V4_COMPACT_INDEX: &str = r#"
CREATE TABLE IF NOT EXISTS ptr_compact_hash (
    hash            BLOB PRIMARY KEY,
    service_hash_id INTEGER NOT NULL
) WITHOUT ROWID;
CREATE UNIQUE INDEX IF NOT EXISTS idx_ptr_compact_hash_sid ON ptr_compact_hash(service_hash_id);

CREATE TABLE IF NOT EXISTS ptr_compact_tag (
    service_tag_id INTEGER PRIMARY KEY,
    namespace      TEXT NOT NULL,
    subtag         TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ptr_compact_tag_ns_st ON ptr_compact_tag(namespace, subtag);

CREATE TABLE IF NOT EXISTS ptr_compact_posting (
    service_hash_id INTEGER PRIMARY KEY,
    tag_ids_blob    BLOB NOT NULL,
    tag_count       INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE IF NOT EXISTS ptr_compact_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    running INTEGER NOT NULL DEFAULT 0,
    stage TEXT NOT NULL DEFAULT 'idle',
    rows_done_stage INTEGER NOT NULL DEFAULT 0,
    rows_total_stage INTEGER NOT NULL DEFAULT 0,
    rows_per_sec REAL NOT NULL DEFAULT 0,
    snapshot_dir TEXT,
    service_id INTEGER,
    snapshot_max_index INTEGER,
    updated_at TEXT,
    checkpoint_phase TEXT NOT NULL DEFAULT 'idle',
    checkpoint_last_hash_id INTEGER NOT NULL DEFAULT 0,
    checkpoint_last_tag_id INTEGER NOT NULL DEFAULT 0,
    checkpoint_last_service_hash_id INTEGER NOT NULL DEFAULT 0
);
INSERT OR IGNORE INTO ptr_compact_state (id) VALUES (1);
"#;

fn set_ptr_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO ptr_schema_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [key, value],
    )?;
    Ok(())
}

fn get_ptr_meta_i64(conn: &Connection, key: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        "SELECT CAST(value AS INTEGER) FROM ptr_schema_meta WHERE key = ?1",
        [key],
        |row| row.get(0),
    )
    .optional()
}

fn table_uses_without_rowid(conn: &Connection, table_name: &str) -> rusqlite::Result<bool> {
    let ddl: Option<String> = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name=?1",
            [table_name],
            |row| row.get(0),
        )
        .optional()?;
    Ok(ddl
        .as_deref()
        .map(|s| s.to_ascii_uppercase().contains("WITHOUT ROWID"))
        .unwrap_or(false))
}

fn get_ptr_rebuild_required(conn: &Connection) -> rusqlite::Result<bool> {
    Ok(get_ptr_meta_i64(conn, "rebuild_required")?.unwrap_or(0) != 0)
}

fn apply_ptr_startup_migrations(conn: &Connection) -> rusqlite::Result<bool> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ptr_schema_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )?;

    // Always apply lightweight table additions on startup.
    conn.execute_batch(PTR_MIGRATION_TO_V3_RESOLVED_MAPS)?;
    conn.execute_batch(PTR_MIGRATION_TO_V4_COMPACT_INDEX)?;

    let schema_version = get_ptr_meta_i64(conn, "schema_version")?.unwrap_or(1);
    let mut rebuild_required = get_ptr_meta_i64(conn, "rebuild_required")?.unwrap_or(0) != 0;

    if schema_version < 2 {
        rebuild_required = true;
    }
    if !table_uses_without_rowid(conn, "ptr_file_tag")?
        || !table_uses_without_rowid(conn, "ptr_tag_sibling")?
        || !table_uses_without_rowid(conn, "ptr_tag_parent")?
    {
        rebuild_required = true;
    }

    set_ptr_meta(conn, "required_schema_version", "4")?;
    if rebuild_required {
        set_ptr_meta(conn, "rebuild_required", "1")?;
        // Keep existing schema_version value; heavy rebuild will finalize to v4.
        set_ptr_meta(conn, "schema_version", &schema_version.to_string())?;
    } else {
        set_ptr_meta(conn, "rebuild_required", "0")?;
        set_ptr_meta(conn, "schema_version", "4")?;
    }

    Ok(rebuild_required)
}

fn apply_ptr_full_rebuild(conn: &mut Connection) -> rusqlite::Result<()> {
    conn.execute_batch(PTR_MIGRATION_TO_V2_WITHOUT_ROWID)?;
    conn.execute_batch(PTR_MIGRATION_TO_V3_RESOLVED_MAPS)?;
    conn.execute_batch(PTR_MIGRATION_TO_V4_COMPACT_INDEX)?;
    set_ptr_meta(conn, "schema_version", "4")?;
    set_ptr_meta(conn, "required_schema_version", "4")?;
    set_ptr_meta(conn, "rebuild_required", "0")?;

    Ok(())
}
