//! Core SQLite database module.
//!
//! All queries go through `SqliteDatabase` methods.
//! rusqlite is synchronous — all DB calls wrapped in `spawn_blocking`.

pub mod bitmaps;
pub mod collections;
pub mod compilers;
pub mod duplicates;
pub mod files;
pub mod flows;
pub mod folders;
pub mod hash_index;
pub mod import;
pub mod projections;
pub mod schema;
pub mod sidebar;
pub mod smart_folders;
pub mod subscriptions;
pub mod tags;
pub mod view_prefs;

use bitmaps::BitmapStore;
use hash_index::HashIndex;
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub use compilers::CompilerEvent;

fn parse_active_bitmap_file(payload_json: Option<&str>) -> Option<String> {
    payload_json
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|v| {
            v.get("active_file")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
}

struct ManifestState {
    published_epoch: u64,
    published_artifact_versions: HashMap<String, u64>,
    published_artifact_payloads: HashMap<String, String>,
    working_artifact_versions: HashMap<String, u64>,
    working_artifact_payloads: HashMap<String, String>,
    dirty: bool,
}

/// Global manifest snapshot tracker for derived artifact publication.
///
/// Compatibility note:
/// Current compiler/projection code still calls `get(key)` / `bump(key)` as if this were a key->epoch
/// map. Internally, these now mutate artifact versions and `flush_to_db()` publishes a new manifest
/// snapshot (`manifest_epoch`) with a full set of artifact entries.
pub struct Manifest {
    state: std::sync::RwLock<ManifestState>,
}

impl Manifest {
    pub fn new() -> Self {
        let mut artifact_versions = HashMap::new();
        let mut artifact_payloads = HashMap::new();
        for key in [
            "global",
            "files",
            "tags",
            "tag_graph",
            "effective_tags",
            "metadata_projection",
            "sidebar",
            "smart_folders",
            "bitmaps",
            "ptr_overlay",
        ] {
            artifact_versions.insert(key.to_string(), 0);
        }
        artifact_payloads.insert(
            "bitmaps".to_string(),
            json!({"active_file":"bitmaps.bin"}).to_string(),
        );
        Self {
            state: std::sync::RwLock::new(ManifestState {
                published_epoch: 0,
                published_artifact_versions: artifact_versions.clone(),
                published_artifact_payloads: artifact_payloads.clone(),
                working_artifact_versions: artifact_versions,
                working_artifact_payloads: artifact_payloads,
                dirty: false,
            }),
        }
    }

    /// Load the latest published manifest snapshot (artifact versions) from the database.
    pub fn load_from_db(conn: &Connection) -> rusqlite::Result<Self> {
        let m = Self::new();

        let has_new_manifest_tables: bool = conn.query_row(
            "SELECT COUNT(*) > 0 FROM sqlite_master
             WHERE type='table' AND name='artifact_manifest_meta'",
            [],
            |row| row.get(0),
        )?;

        if has_new_manifest_tables {
            let published_epoch: u64 = conn
                .query_row(
                    "SELECT manifest_epoch FROM artifact_manifest_meta WHERE id = 1",
                    [],
                    |row| row.get::<_, u64>(0),
                )
                .unwrap_or(0);

            let mut stmt = conn.prepare_cached(
                "SELECT artifact_name, artifact_version, payload_json
                 FROM artifact_manifest_entry
                 WHERE manifest_epoch = ?1",
            )?;
            let rows = stmt.query_map([published_epoch], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;

            let mut loaded_any = false;
            {
                let mut state = crate::poison::write_or_recover(&m.state, "manifest::init");
                state.published_epoch = published_epoch;
                for row in rows {
                    let (name, version, payload_json) = row?;
                    state
                        .published_artifact_versions
                        .insert(name.clone(), version);
                    state
                        .published_artifact_payloads
                        .insert(name.clone(), payload_json.clone());
                    state
                        .working_artifact_versions
                        .insert(name.clone(), version);
                    state.working_artifact_payloads.insert(name, payload_json);
                    loaded_any = true;
                }
                state.dirty = false;
            }

            if loaded_any {
                return Ok(m);
            }
        }

        // No artifact snapshot entries found — return defaults (all versions at 0).
        // A RebuildAll compiler event on startup will recompute everything.
        Ok(m)
    }

    /// Get the current published manifest epoch.
    pub fn published_epoch(&self) -> u64 {
        crate::poison::read_or_recover(&self.state, "manifest::published_epoch").published_epoch
    }

    /// Get the artifact version for a given key in the current published snapshot.
    pub fn published_artifact_version(&self, key: &str) -> u64 {
        crate::poison::read_or_recover(&self.state, "manifest::published_artifact_version")
            .published_artifact_versions
            .get(key)
            .copied()
            .unwrap_or(0)
    }

    /// Bump the artifact version for a key in the working snapshot and return the new value.
    pub fn bump_working_artifact_version(&self, key: &str) -> u64 {
        let mut state = crate::poison::write_or_recover(&self.state, "manifest::bump_version");
        let new_version = {
            let version = state
                .working_artifact_versions
                .entry(key.to_string())
                .or_insert(0);
            *version += 1;
            *version
        };
        state.dirty = true;
        new_version
    }

    /// Get the raw manifest payload JSON for an artifact (if present in the current published snapshot).
    pub fn published_artifact_payload_json(&self, key: &str) -> Option<String> {
        crate::poison::read_or_recover(&self.state, "manifest::published_payload")
            .published_artifact_payloads
            .get(key)
            .cloned()
    }

    /// Set the raw manifest payload JSON for an artifact in the working snapshot.
    pub fn set_working_artifact_payload_json(&self, key: &str, payload_json: String) {
        let mut state = crate::poison::write_or_recover(&self.state, "manifest::set_payload");
        let changed = state
            .working_artifact_payloads
            .get(key)
            .map(|existing| existing != &payload_json)
            .unwrap_or(true);
        if changed {
            state
                .working_artifact_payloads
                .insert(key.to_string(), payload_json);
            state.dirty = true;
        }
    }

    /// Flush a new manifest snapshot to the artifact manifest tables.
    pub fn flush_to_db(&self, conn: &mut Connection) -> rusqlite::Result<()> {
        let mut state = crate::poison::write_or_recover(&self.state, "manifest::flush");
        if !state.dirty {
            return Ok(());
        }

        let next_manifest_epoch = state.published_epoch + 1;
        let artifact_versions = state.working_artifact_versions.clone();
        let artifact_payloads = state.working_artifact_payloads.clone();

        let tx = conn.transaction()?;

        tx.execute(
            "INSERT OR IGNORE INTO artifact_manifest_meta (id, manifest_epoch, updated_at)
             VALUES (1, 0, CURRENT_TIMESTAMP)",
            [],
        )?;

        {
            let mut entry_stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO artifact_manifest_entry
                    (manifest_epoch, artifact_name, artifact_version, built_from_truth_seq, payload_json)
                 VALUES (?1, ?2, ?3, 0, ?4)",
            )?;
            for (artifact_name, artifact_version) in artifact_versions.iter() {
                let payload_json = artifact_payloads
                    .get(artifact_name)
                    .cloned()
                    .unwrap_or_else(|| "{}".to_string());
                entry_stmt.execute(rusqlite::params![
                    next_manifest_epoch,
                    artifact_name,
                    artifact_version,
                    payload_json
                ])?;
            }
        }

        tx.execute(
            "UPDATE artifact_manifest_meta
             SET manifest_epoch = ?1, updated_at = CURRENT_TIMESTAMP
             WHERE id = 1",
            [next_manifest_epoch],
        )?;

        tx.commit()?;

        // Keep only last 2 epochs to prevent unbounded accumulation
        if next_manifest_epoch > 2 {
            conn.execute(
                "DELETE FROM artifact_manifest_entry WHERE manifest_epoch < ?1",
                [next_manifest_epoch - 1],
            )?;
        }

        state.published_epoch = next_manifest_epoch;
        state.published_artifact_versions = artifact_versions.clone();
        state.published_artifact_payloads = artifact_payloads.clone();
        state.working_artifact_versions = artifact_versions;
        state.working_artifact_payloads = artifact_payloads;
        state.dirty = false;
        Ok(())
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
    pub manifest: Arc<Manifest>,
    pub compiler_tx: mpsc::UnboundedSender<CompilerEvent>,
    compiler_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<CompilerEvent>>>>,
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

        let manifest =
            Manifest::load_from_db(&conn).map_err(|e| format!("Failed to load manifest: {e}"))?;

        let active_bitmap_file =
            parse_active_bitmap_file(manifest.published_artifact_payload_json("bitmaps").as_deref());

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

        let (compiler_tx, compiler_rx) = mpsc::unbounded_channel();

        let db = Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
            read_pool,
            read_pool_idx: AtomicUsize::new(0),
            bitmaps: Arc::new(bitmaps),
            hash_index: Arc::new(HashIndex::new()),
            manifest: Arc::new(manifest),
            compiler_tx,
            compiler_rx: Arc::new(Mutex::new(Some(compiler_rx))),
            db_path,
            scope_cache: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
        });

        Ok(db)
    }

    /// Take the compiler receiver (can only be called once, for the compiler task).
    pub async fn take_compiler_rx(&self) -> Option<mpsc::UnboundedReceiver<CompilerEvent>> {
        self.compiler_rx.lock().await.take()
    }

    /// Run a read-only closure on a pooled reader connection.
    /// Uses round-robin to spread reads across the pool.
    /// Reader connections are opened with SQLITE_OPEN_READ_ONLY — writes will fail.
    pub async fn with_read_conn<F, R>(&self, f: F) -> Result<R, String>
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
            if elapsed_ms > 100 {
                tracing::warn!(elapsed_ms, "slow read query");
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
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let conn = conn.blocking_lock();
            let result = f(&conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > 100 {
                tracing::warn!(elapsed_ms, "slow write query");
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
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let start = std::time::Instant::now();
            let mut conn = conn.blocking_lock();
            let result = f(&mut conn).map_err(|e| format!("SQLite error: {e}"));
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if elapsed_ms > 200 {
                tracing::warn!(elapsed_ms, "slow transaction");
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
            .with_read_conn(move |conn| {
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
            .with_read_conn(move |conn| {
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
                .with_read_conn(move |conn| {
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
                .with_read_conn(move |conn| {
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

    pub fn emit_compiler_event(&self, event: CompilerEvent) {
        let _ = self.compiler_tx.send(event);
    }

    pub async fn flush(&self) -> Result<(), String> {
        let previous_active = parse_active_bitmap_file(
            self.manifest
                .published_artifact_payload_json("bitmaps")
                .as_deref(),
        );
        let mut new_active_for_cleanup: Option<String> = None;
        if self.bitmaps.is_dirty() {
            let bitmap_version = self.manifest.bump_working_artifact_version("bitmaps");
            let active_file = self
                .bitmaps
                .flush_versioned(bitmap_version)
                .map_err(|e| format!("Bitmap flush error: {e}"))?;
            new_active_for_cleanup = Some(active_file.clone());
            self.manifest.set_working_artifact_payload_json(
                "bitmaps",
                json!({ "active_file": active_file }).to_string(),
            );
        }

        let manifest = self.manifest.clone();
        self.with_conn_mut(move |conn| manifest.flush_to_db(conn))
            .await?;

        if let Some(active_file) = new_active_for_cleanup {
            let mut keep = vec![active_file.clone()];
            if let Some(prev) = previous_active.filter(|p| p != &active_file) {
                keep.push(prev);
            }
            match self.bitmaps.prune_artifacts(&keep) {
                Ok(deleted) => {
                    if deleted > 0 {
                        tracing::info!(deleted, "Pruned stale bitmap artifact files");
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Bitmap artifact cleanup (post-flush) failed");
                }
            }
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::Manifest;
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

        let new_files_version = manifest.bump_working_artifact_version("files");
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
