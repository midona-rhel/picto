use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

use super::{publish, schema, BitmapStore, HashIndex, ReadModelEvent, SqliteDatabase};

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

            schema::reconcile_schema(&conn)
                .map_err(|e| format!("Failed to reconcile schema: {e}"))?;

            Ok(conn)
        })
        .await
        .map_err(|e| format!("Join error: {e}"))??;

        let manifest = publish::Manifest::load_from_db(&conn)
            .map_err(|e| format!("Failed to load manifest: {e}"))?;

        let bitmap_payload = publish::bitmap_payload_from_manifest(&manifest);

        let bitmaps = BitmapStore::open_with_active_file(&db_dir, bitmap_payload.as_deref());
        let startup_keep: Vec<String> = bitmap_payload.into_iter().collect();
        if let Err(e) = bitmaps.prune_artifacts(&startup_keep) {
            tracing::warn!(error = %e, "Bitmap artifact cleanup (startup) failed — non-fatal");
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

        Ok(Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
            read_pool,
            read_pool_idx: AtomicUsize::new(0),
            bitmaps: Arc::new(bitmaps),
            hash_index: Arc::new(HashIndex::new()),
            manifest: Arc::new(manifest),
            read_model_tx,
            read_model_rx: Arc::new(Mutex::new(Some(read_model_rx))),
            db_path,
            events_held: AtomicBool::new(false),
            held_events: std::sync::Mutex::new(Vec::new()),
        }))
    }

    /// Take the compiler receiver (can only be called once, for the compiler task).
    pub async fn take_read_model_rx(&self) -> Option<mpsc::UnboundedReceiver<ReadModelEvent>> {
        self.read_model_rx.lock().await.take()
    }

    pub fn emit_read_model_event(&self, event: ReadModelEvent) {
        if self.events_held.load(std::sync::atomic::Ordering::SeqCst) {
            self.held_events.lock().unwrap().push(event);
        } else {
            let _ = self.read_model_tx.send(event);
        }
    }

    /// Hold all read-model events in a buffer instead of sending them.
    /// Use this during batch imports where you don't want the compiler to
    /// fire until all files are inserted and organized (e.g., collections).
    pub fn hold_events(&self) {
        self.events_held
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Release all held events, sending them to the compiler channel at once.
    pub fn release_events(&self) {
        self.events_held
            .store(false, std::sync::atomic::Ordering::SeqCst);
        let events: Vec<ReadModelEvent> = self.held_events.lock().unwrap().drain(..).collect();
        for event in events {
            let _ = self.read_model_tx.send(event);
        }
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
            events_held: AtomicBool::new(false),
            held_events: std::sync::Mutex::new(Vec::new()),
        }
    }
}
