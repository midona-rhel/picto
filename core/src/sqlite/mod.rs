//! Core SQLite database module.
//!
//! All queries go through `SqliteDatabase` methods.
//! rusqlite is synchronous — all DB calls wrapped in `spawn_blocking`.

pub mod bitmaps;
pub mod compilers;
pub mod files;
pub mod hash_index;
pub mod publish;
pub mod projections;
pub mod read_model;
pub mod schema;

use bitmaps::BitmapStore;
use hash_index::HashIndex;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex};

pub use read_model::{DerivedArtifact, PublishedArtifacts, ReadModelBatchResult, ReadModelEvent};

const SLOW_READ_WARN_MS: u64 = 100;
const SLOW_WRITE_WARN_MS: u64 = 100;
const SLOW_TX_WARN_MS: u64 = 200;
const SLOW_QUERY_LOG_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy)]
struct SlowQueryWindow {
    started_at: Instant,
    count: u64,
    max_elapsed_ms: u64,
    max_label: &'static str,
}

static SLOW_QUERY_WINDOWS: OnceLock<StdMutex<HashMap<&'static str, SlowQueryWindow>>> =
    OnceLock::new();

fn record_slow_query(kind: &'static str, label: &'static str, elapsed_ms: u64) {
    let windows = SLOW_QUERY_WINDOWS.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut guard = crate::poison::mutex_or_recover(windows, "sqlite::slow_query_windows");
    let now = Instant::now();

    match guard.get_mut(kind) {
        Some(window) if now.duration_since(window.started_at) <= SLOW_QUERY_LOG_WINDOW => {
            window.count += 1;
            if elapsed_ms >= window.max_elapsed_ms {
                window.max_elapsed_ms = elapsed_ms;
                window.max_label = label;
            }
        }
        Some(window) => {
            if window.count > 1 {
                tracing::warn!(
                    kind,
                    label = window.max_label,
                    suppressed = window.count - 1,
                    max_elapsed_ms = window.max_elapsed_ms,
                    window_ms = SLOW_QUERY_LOG_WINDOW.as_millis() as u64,
                    "suppressed repeated slow sqlite queries"
                );
            }
            *window = SlowQueryWindow {
                started_at: now,
                count: 1,
                max_elapsed_ms: elapsed_ms,
                max_label: label,
            };
            tracing::warn!(kind, label, elapsed_ms, "slow sqlite query");
        }
        None => {
            guard.insert(
                kind,
                SlowQueryWindow {
                    started_at: now,
                    count: 1,
                    max_elapsed_ms: elapsed_ms,
                    max_label: label,
                },
            );
            tracing::warn!(kind, label, elapsed_ms, "slow sqlite query");
        }
    }
}

/// Cached snapshot of a filtered scope — avoids rebuilding temp id-sets on
/// consecutive page fetches for the same scope+filter+sort combination.
#[derive(Debug, Clone)]
pub struct ScopeSnapshot {
    pub ids: Vec<i64>,
    pub total_count: i64,
    pub created_at: std::time::Instant,
}

/// Key for the scope snapshot cache.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ScopeSnapshotKey {
    pub scope: String,
    pub predicate_hash: u64,
    pub sort_field: String,
    pub sort_dir: String,
}

/// Main library database handle.
pub struct SqliteDatabase {
    conn: Arc<Mutex<Connection>>,
    /// Pool of read-only connections for concurrent SELECT queries.
    read_pool: Vec<Arc<Mutex<Connection>>>,
    /// Round-robin counter for read pool.
    read_pool_idx: AtomicUsize,
    pub bitmaps: Arc<BitmapStore>,
    pub hash_index: Arc<HashIndex>,
    pub manifest: Arc<publish::Manifest>,
    pub read_model_tx: mpsc::UnboundedSender<ReadModelEvent>,
    read_model_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ReadModelEvent>>>>,
    db_path: PathBuf,
    /// Scope snapshot cache for grid paging (avoids repeated temp-table rebuilds).
    /// Key: scope+predicate+sort. Value: stable ordered id list.
    /// Invalidated on relevant mutations.
    pub scope_cache:
        Arc<std::sync::RwLock<std::collections::HashMap<ScopeSnapshotKey, ScopeSnapshot>>>,
}

impl SqliteDatabase {
    /// Open (or create) the library database at the given directory.
    pub async fn open(library_root: &Path) -> Result<Arc<Self>, String> {
        let db_dir = library_root.join("db");
        std::fs::create_dir_all(&db_dir).map_err(|e| format!("Failed to create db dir: {e}"))?;

        let db_path = db_dir.join("library.sqlite");
        let db_path_clone = db_path.clone();

        let conn = tokio::task::spawn_blocking(move || -> Result<Connection, String> {
            let conn = Connection::open(&db_path_clone)
                .map_err(|e| format!("Failed to open SQLite: {e}"))?;

            schema::apply_pragmas(&conn).map_err(|e| format!("Failed to apply pragmas: {e}"))?;

            let version = schema::get_schema_version(&conn)
                .map_err(|e| format!("Failed to check schema version: {e}"))?;

            match version {
                None => {
                    schema::init_schema(&conn)
                        .map_err(|e| format!("Failed to init schema: {e}"))?;
                    tracing::info!("Initialized fresh library database");
                }
                Some(v) => {
                    if v < schema::CURRENT_VERSION {
                        schema::run_migrations(&conn, v)
                            .map_err(|e| format!("Failed to run migrations: {e}"))?;
                        tracing::info!(
                            "Migrated library database from v{v} to v{}",
                            schema::CURRENT_VERSION
                        );
                    } else {
                        tracing::info!("Library database at schema v{v}");
                    }
                }
            }

            // Heal known schema drift cases even when schema_version already
            // reports current.
            schema::reconcile_schema(&conn)
                .map_err(|e| format!("Failed to reconcile schema: {e}"))?;

            Ok(conn)
        })
        .await
        .map_err(|e| format!("Join error: {e}"))??;

        let manifest = publish::Manifest::load_from_db(&conn)
            .map_err(|e| format!("Failed to load manifest: {e}"))?;

        let active_bitmap_file = publish::active_bitmap_file_from_manifest(&manifest);

        let bitmaps = BitmapStore::open_with_active_file(&db_dir, active_bitmap_file.as_deref());
        let startup_keep = vec![
            active_bitmap_file
                .clone()
                .unwrap_or_else(|| "bitmaps.bin".to_string()),
        ];
        if let Err(e) = bitmaps.prune_artifacts(&startup_keep) {
            tracing::warn!(error = %e, "Bitmap artifact cleanup (startup) failed");
        }

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
                .map_err(|e| format!("Failed to open read connection: {e}"))?;
                schema::apply_pragmas(&c)
                    .map_err(|e| format!("Failed to apply pragmas to reader: {e}"))?;
                Ok(c)
            })
            .await
            .map_err(|e| format!("Join error: {e}"))??;
            read_pool.push(Arc::new(Mutex::new(reader_conn)));
        }
        tracing::info!("Opened {pool_size} read-only connections");

        let (read_model_tx, read_model_rx) = mpsc::unbounded_channel();

        let db = Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
            read_pool,
            read_pool_idx: AtomicUsize::new(0),
            bitmaps: Arc::new(bitmaps),
            hash_index: Arc::new(HashIndex::new()),
            manifest: Arc::new(manifest),
            read_model_tx,
            read_model_rx: Arc::new(Mutex::new(Some(read_model_rx))),
            db_path,
            scope_cache: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        });

        Ok(db)
    }

    /// Take the compiler receiver (can only be called once, for the compiler task).
    pub async fn take_read_model_rx(&self) -> Option<mpsc::UnboundedReceiver<ReadModelEvent>> {
        self.read_model_rx.lock().await.take()
    }

    /// Run a read-only closure on a pooled reader connection.
    /// Uses round-robin to spread reads across the pool.
    /// Reader connections are opened with SQLITE_OPEN_READ_ONLY — writes will fail.
    pub async fn with_read_conn<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.with_read_conn_labeled("unlabeled_read", f).await
    }

    /// Run a read-only closure on a pooled reader connection with a diagnostic label.
    pub async fn with_read_conn_labeled<F, R>(
        &self,
        label: &'static str,
        f: F,
    ) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let idx = self.read_pool_idx.fetch_add(1, Ordering::Relaxed) % self.read_pool.len();
        let conn = self.read_pool[idx].clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let conn = conn.blocking_lock();
            let result = f(&conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > SLOW_READ_WARN_MS {
                record_slow_query("read", label, elapsed_ms);
            }
            result
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    /// Run a synchronous closure with the database connection.
    /// All rusqlite operations must go through this method.
    pub async fn with_conn<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.with_conn_labeled("unlabeled_write", f).await
    }

    /// Run a synchronous closure with the database connection and a diagnostic label.
    pub async fn with_conn_labeled<F, R>(&self, label: &'static str, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let conn = conn.blocking_lock();
            let result = f(&conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > SLOW_WRITE_WARN_MS {
                record_slow_query("write", label, elapsed_ms);
            }
            result
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    /// Run a synchronous closure with a mutable reference (for transactions).
    pub async fn with_conn_mut<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        self.with_conn_mut_labeled("unlabeled_transaction", f).await
    }

    /// Run a synchronous transactional closure with a diagnostic label.
    pub async fn with_conn_mut_labeled<F, R>(
        &self,
        label: &'static str,
        f: F,
    ) -> Result<R, String>
    where
        F: FnOnce(&mut Connection) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let mut conn = conn.blocking_lock();
            let result = f(&mut conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > SLOW_TX_WARN_MS {
                record_slow_query("transaction", label, elapsed_ms);
            }
            result
        })
        .await
        .map_err(|e| format!("Join error: {e}"))?
    }

    /// Resolve a hex hash to file_id, checking cache first, then DB.
    pub async fn resolve_hash(&self, hash: &str) -> Result<i64, String> {
        if let Some(id) = self.hash_index.get_id(hash) {
            return Ok(id);
        }
        let hash_owned = hash.to_string();
        let id = self
            .with_read_conn_labeled("hash_index/resolve_hash", move |conn| {
                conn.query_row(
                    "SELECT file_id FROM file WHERE hash = ?1",
                    [&hash_owned],
                    |row| row.get::<_, i64>(0),
                )
            })
            .await?;
        self.hash_index.insert(hash.to_string(), id);
        Ok(id)
    }

    /// Resolve a file_id to hex hash, checking cache first, then DB.
    pub async fn resolve_id(&self, file_id: i64) -> Result<String, String> {
        if let Some(hash) = self.hash_index.get_hash(file_id) {
            return Ok(hash);
        }
        let hash = self
            .with_read_conn_labeled("hash_index/resolve_id", move |conn| {
                conn.query_row(
                    "SELECT hash FROM file WHERE file_id = ?1",
                    [file_id],
                    |row| row.get::<_, String>(0),
                )
            })
            .await?;
        self.hash_index.insert(hash.clone(), file_id);
        Ok(hash)
    }

    /// Batch resolve file_ids → hashes. Checks cache first, then DB for misses.
    /// Returns results in arbitrary order; missing IDs are silently skipped.
    pub async fn resolve_ids_batch(&self, file_ids: &[i64]) -> Result<Vec<(i64, String)>, String> {
        let mut results = Vec::with_capacity(file_ids.len());
        let mut misses = Vec::new();

        for &fid in file_ids {
            if let Some(hash) = self.hash_index.get_hash(fid) {
                results.push((fid, hash));
            } else {
                misses.push(fid);
            }
        }

        if !misses.is_empty() {
            let hash_index = self.hash_index.clone();
            let db_results = self
                .with_read_conn_labeled("hash_index/resolve_ids_batch", move |conn| {
                    let placeholders = std::iter::repeat_n("?", misses.len())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!(
                        "SELECT file_id, hash FROM file WHERE file_id IN ({})",
                        placeholders
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt
                        .query_map(rusqlite::params_from_iter(misses.iter()), |row| {
                            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                        })?;
                    let mut batch = Vec::new();
                    for row in rows {
                        let (fid, hash) = row?;
                        hash_index.insert(hash.clone(), fid);
                        batch.push((fid, hash));
                    }
                    Ok(batch)
                })
                .await?;
            results.extend(db_results);
        }

        Ok(results)
    }

    /// Batch resolve hashes → file_ids. Checks cache first, then DB for misses.
    /// Returns results in arbitrary order; missing hashes are silently skipped.
    pub async fn resolve_hashes_batch(
        &self,
        hashes: &[String],
    ) -> Result<Vec<(String, i64)>, String> {
        let mut results = Vec::with_capacity(hashes.len());
        let mut misses = Vec::new();

        for hash in hashes {
            if let Some(id) = self.hash_index.get_id(hash) {
                results.push((hash.clone(), id));
            } else {
                misses.push(hash.clone());
            }
        }

        if !misses.is_empty() {
            let hash_index = self.hash_index.clone();
            let db_results = self
                .with_read_conn_labeled("hash_index/resolve_hashes_batch", move |conn| {
                    let placeholders = std::iter::repeat_n("?", misses.len())
                        .collect::<Vec<_>>()
                        .join(", ");
                    let sql = format!(
                        "SELECT hash, file_id FROM file WHERE hash IN ({})",
                        placeholders
                    );
                    let mut stmt = conn.prepare(&sql)?;
                    let rows = stmt
                        .query_map(rusqlite::params_from_iter(misses.iter()), |row| {
                            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                        })?;
                    let mut batch = Vec::new();
                    for row in rows {
                        let (hash, fid) = row?;
                        hash_index.insert(hash.clone(), fid);
                        batch.push((hash, fid));
                    }
                    Ok(batch)
                })
                .await?;
            results.extend(db_results);
        }

        Ok(results)
    }

    pub fn emit_read_model_event(&self, event: ReadModelEvent) {
        let _ = self.read_model_tx.send(event);
    }

    pub async fn flush(&self) -> Result<(), String> {
        publish::publish_pending(&Arc::new(self.clone_for_publish()), &[])
            .await
            .map(|_| ())
    }

    pub fn db_dir(&self) -> PathBuf {
        self.db_path
            .parent()
            .unwrap_or(Path::new("."))
            .to_path_buf()
    }

    const SCOPE_CACHE_MAX_ENTRIES: usize = 64;
    const SCOPE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

    pub fn scope_cache_get(&self, key: &ScopeSnapshotKey) -> Option<ScopeSnapshot> {
        let cache = crate::poison::read_or_recover(&self.scope_cache, "scope_cache::get");
        cache.get(key).and_then(|snap| {
            if snap.created_at.elapsed() < Self::SCOPE_CACHE_TTL {
                Some(snap.clone())
            } else {
                None
            }
        })
    }

    pub fn scope_cache_put(&self, key: ScopeSnapshotKey, snapshot: ScopeSnapshot) {
        let mut cache = crate::poison::write_or_recover(&self.scope_cache, "scope_cache::put");
        if cache.len() >= Self::SCOPE_CACHE_MAX_ENTRIES {
            cache.retain(|_, v| v.created_at.elapsed() < Self::SCOPE_CACHE_TTL);
        }
        if cache.len() >= Self::SCOPE_CACHE_MAX_ENTRIES {
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _): (&ScopeSnapshotKey, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }
        cache.insert(key, snapshot);
    }

    pub fn scope_cache_invalidate_all(&self) {
        let mut cache =
            crate::poison::write_or_recover(&self.scope_cache, "scope_cache::invalidate_all");
        cache.clear();
    }

    pub fn scope_cache_invalidate_scope(&self, scope_prefix: &str) {
        let mut cache =
            crate::poison::write_or_recover(&self.scope_cache, "scope_cache::invalidate_scope");
        cache.retain(|k, _| !k.scope.starts_with(scope_prefix));
    }
}

impl SqliteDatabase {
    fn clone_for_publish(&self) -> Self {
        Self {
            conn: self.conn.clone(),
            read_pool: self.read_pool.clone(),
            read_pool_idx: AtomicUsize::new(self.read_pool_idx.load(Ordering::Relaxed)),
            bitmaps: self.bitmaps.clone(),
            hash_index: self.hash_index.clone(),
            manifest: self.manifest.clone(),
            read_model_tx: self.read_model_tx.clone(),
            read_model_rx: self.read_model_rx.clone(),
            db_path: self.db_path.clone(),
            scope_cache: self.scope_cache.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::publish::Manifest;
    use super::DerivedArtifact;
    use rusqlite::Connection;

    fn init_manifest_tables(conn: &Connection) {
        conn.execute_batch(
            "
            CREATE TABLE manifest (
                key TEXT PRIMARY KEY,
                epoch INTEGER NOT NULL
            );
            CREATE TABLE artifact_manifest_meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                manifest_epoch INTEGER NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            CREATE TABLE artifact_manifest_entry (
                manifest_epoch INTEGER NOT NULL,
                artifact_name TEXT NOT NULL,
                artifact_version INTEGER NOT NULL,
                built_from_truth_seq INTEGER NOT NULL DEFAULT 0,
                payload_json TEXT NOT NULL DEFAULT '{}',
                PRIMARY KEY (manifest_epoch, artifact_name)
            );
            ",
        )
        .unwrap();
    }

    #[test]
    fn manifest_readers_do_not_see_unflushed_bumps() {
        let mut conn = Connection::open_in_memory().unwrap();
        init_manifest_tables(&conn);

        let manifest = Manifest::new();
        let initial_bitmap_payload = manifest.published_artifact_payload_json("bitmaps").unwrap();
        assert_eq!(manifest.published_artifact_version("files"), 0);
        assert_eq!(manifest.published_epoch(), 0);

        let new_files_version = manifest.mark_artifact_dirty(DerivedArtifact::Files);
        assert_eq!(new_files_version, 1);
        manifest.set_working_artifact_payload_json(
            "bitmaps",
            "{\"active_file\":\"bitmaps.v1.bin\"}".to_string(),
        );

        // Readers should continue to see the last published snapshot until flush_to_db() publishes.
        assert_eq!(manifest.published_artifact_version("files"), 0);
        assert_eq!(
            manifest.published_artifact_payload_json("bitmaps").unwrap(),
            initial_bitmap_payload
        );
        assert_eq!(manifest.published_epoch(), 0);

        manifest.flush_to_db(&mut conn).unwrap();

        assert_eq!(manifest.published_epoch(), 1);
        assert_eq!(manifest.published_artifact_version("files"), 1);
        assert_eq!(
            manifest.published_artifact_payload_json("bitmaps").unwrap(),
            "{\"active_file\":\"bitmaps.v1.bin\"}"
        );

        // Reloaded manifest should expose the same published snapshot.
        let loaded = Manifest::load_from_db(&conn).unwrap();
        assert_eq!(loaded.published_epoch(), 1);
        assert_eq!(loaded.published_artifact_version("files"), 1);
        assert_eq!(
            loaded.published_artifact_payload_json("bitmaps").unwrap(),
            "{\"active_file\":\"bitmaps.v1.bin\"}"
        );
    }
}
