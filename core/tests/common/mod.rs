//! Shared test harness for integration tests.
//!
//! Provides a reusable `TestHarness` with seeded DB + event collector.

#![allow(dead_code)] // Each test binary uses a different subset of helpers.

use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use picto_core::events;
use picto_core::sqlite::SqliteDatabase;
use picto_core::sqlite::bitmaps::BitmapKey;
use picto_core::sqlite::files::NewFile;

// ---------------------------------------------------------------------------
// Test Harness
// ---------------------------------------------------------------------------

/// Reusable test fixture with a temporary library directory, seeded DB,
/// and an event collector that captures all emitted events.
pub struct TestHarness {
    _tmp: TempDir,
    pub db: Arc<SqliteDatabase>,
    pub events: Arc<Mutex<Vec<(String, String)>>>,
    // The native event callback is a global singleton; keep orchestration tests
    // serialized so callback ownership is deterministic across this test binary.
    _event_callback_guard: std::sync::MutexGuard<'static, ()>,
}

static EVENT_CALLBACK_TEST_LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

fn event_callback_test_lock() -> &'static Mutex<()> {
    EVENT_CALLBACK_TEST_LOCK.get_or_init(|| Mutex::new(()))
}

impl TestHarness {
    /// Create a new harness with a fresh, empty library database.
    pub async fn new() -> Self {
        let event_callback_guard = event_callback_test_lock()
            .lock()
            .expect("lock event callback test mutex");

        let tmp = TempDir::new().expect("create temp dir");
        let library_root = tmp.path().to_path_buf();

        let db = SqliteDatabase::open(&library_root)
            .await
            .expect("open library db");
        let collected = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
        let collected_clone = collected.clone();
        events::set_event_callback(move |name: &str, payload: &str| {
            collected_clone
                .lock()
                .unwrap()
                .push((name.to_string(), payload.to_string()));
        });

        Self {
            _tmp: tmp,
            db,
            events: collected,
            _event_callback_guard: event_callback_guard,
        }
    }

    /// Insert a test file into the library database. Returns the file_id.
    pub async fn insert_test_file(&self, hash: &str, name: &str, status: i64) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        self.db
            .insert_file(NewFile {
                hash: hash.to_string(),
                name: Some(name.to_string()),
                size: 1024,
                mime: "image/png".to_string(),
                width: Some(100),
                height: Some(100),
                duration_ms: None,
                num_frames: None,
                has_audio: false,
                status,
                imported_at: now,
                entity_created_at: None,
                notes: None,
                source_urls_json: None,
                dominant_color_hex: None,
                dominant_palette_blob: None,
            })
            .await
            .expect("insert test file")
    }

    /// Create a collection media entity and return collection ID.
    pub async fn create_collection(&self, name: &str) -> i64 {
        self.db
            .create_collection(name)
            .await
            .expect("create collection")
    }

    /// Add members (by hash) to a collection.
    pub async fn add_collection_members_by_hashes(
        &self,
        collection_id: i64,
        hashes: &[&str],
    ) -> usize {
        let hs = hashes.iter().map(|h| h.to_string()).collect::<Vec<_>>();
        self.db
            .add_collection_members_by_hashes(collection_id, &hs)
            .await
            .expect("add collection members")
    }

    /// Insert a tag and return the tag_id.
    pub async fn insert_test_tag(&self, namespace: &str, subtag: &str) -> i64 {
        let ns = namespace.to_string();
        let st = subtag.to_string();
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)",
                    rusqlite::params![ns, st],
                )?;
                Ok(conn.last_insert_rowid())
            })
            .await
            .expect("insert test tag")
    }

    /// Tag an entity.
    pub async fn tag_entity(&self, entity_id: i64, tag_id: i64) {
        self.db
            .with_conn(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO entity_tag_raw (entity_id, tag_id, source) VALUES (?1, ?2, 'local')",
                    rusqlite::params![entity_id, tag_id],
                )?;
                Ok(())
            })
            .await
            .expect("tag file");
    }

    /// Seed an EffectiveTag bitmap entry directly for deterministic bitmap-path tests.
    pub fn bitmaps_insert_effective_tag(&self, tag_id: i64, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::EffectiveTag(tag_id), entity_id as u32);
    }

    /// Seed active status bitmap (Status(1) only).
    pub fn bitmaps_mark_active(&self, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::Status(1), entity_id as u32);
    }

    /// Seed inbox status bitmap (Status(0) only).
    pub fn bitmaps_mark_inbox(&self, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::Status(0), entity_id as u32);
    }

    /// Seed trash status bitmap (Status(2) only).
    pub fn bitmaps_mark_trash(&self, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::Status(2), entity_id as u32);
    }

    /// Seed the Tagged bitmap for an entity.
    pub fn bitmaps_mark_tagged(&self, entity_id: i64) {
        self.db.bitmaps.insert(&BitmapKey::Tagged, entity_id as u32);
    }

    /// Drain collected events.
    pub fn drain_events(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    /// Find events by name.
    pub fn find_events(&self, name: &str) -> Vec<(String, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .cloned()
            .collect()
    }
}
