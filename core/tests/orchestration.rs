//! PBI-006: Core orchestration and controller test harness.
//!
//! Provides a reusable `TestHarness` with seeded DB + event collector,
//! and scenario tests for dispatch, grid paging, projections, events,
//! and selection.

use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use picto_core::events;
use picto_core::sqlite::files::NewFile;
use picto_core::sqlite::folders::NewFolder;
use picto_core::sqlite::SqliteDatabase;
use picto_core::sqlite_ptr::PtrSqliteDatabase;

// ---------------------------------------------------------------------------
// Test Harness
// ---------------------------------------------------------------------------

/// Reusable test fixture with a temporary library directory, seeded DB,
/// PTR DB, and an event collector that captures all emitted events.
struct TestHarness {
    _tmp: TempDir,
    db: Arc<SqliteDatabase>,
    _ptr_db: Arc<PtrSqliteDatabase>,
    events: Arc<Mutex<Vec<(String, String)>>>,
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
    async fn new() -> Self {
        let event_callback_guard = event_callback_test_lock()
            .lock()
            .expect("lock event callback test mutex");

        let tmp = TempDir::new().expect("create temp dir");
        let library_root = tmp.path().to_path_buf();

        let db = SqliteDatabase::open(&library_root)
            .await
            .expect("open library db");
        let ptr_db = PtrSqliteDatabase::open(&library_root)
            .await
            .expect("open ptr db");

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
            _ptr_db: ptr_db,
            events: collected,
            _event_callback_guard: event_callback_guard,
        }
    }

    /// Insert a test file into the library database. Returns the file_id.
    async fn insert_test_file(&self, hash: &str, name: &str, status: i64) -> i64 {
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
                blurhash: None,
                status,
                imported_at: now,
                notes: None,
                source_urls_json: None,
                dominant_color_hex: None,
                dominant_palette_blob: None,
            })
            .await
            .expect("insert test file")
    }

    /// Create a collection media entity and return collection ID.
    async fn create_collection(&self, name: &str) -> i64 {
        self.db
            .create_collection(name, None, &[])
            .await
            .expect("create collection")
    }

    /// Add members (by hash) to a collection.
    async fn add_collection_members_by_hashes(&self, collection_id: i64, hashes: &[&str]) -> usize {
        let hs = hashes.iter().map(|h| h.to_string()).collect::<Vec<_>>();
        self.db
            .add_collection_members_by_hashes(collection_id, &hs)
            .await
            .expect("add collection members")
    }

    /// Insert a tag and return the tag_id.
    async fn insert_test_tag(&self, namespace: &str, subtag: &str) -> i64 {
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
    async fn tag_entity(&self, entity_id: i64, tag_id: i64) {
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
    fn bitmaps_insert_effective_tag(&self, tag_id: i64, entity_id: i64) {
        self.db.bitmaps.insert(
            &picto_core::sqlite::bitmaps::BitmapKey::EffectiveTag(tag_id),
            entity_id as u32,
        );
    }

    /// Seed status/all-active bitmaps for an active file in bitmap-path tests.
    fn bitmaps_mark_active(&self, entity_id: i64) {
        self.db.bitmaps.insert(
            &picto_core::sqlite::bitmaps::BitmapKey::Status(1),
            entity_id as u32,
        );
        self.db.bitmaps.insert(
            &picto_core::sqlite::bitmaps::BitmapKey::AllActive,
            entity_id as u32,
        );
    }

    /// Drain collected events.
    fn drain_events(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }

    /// Find events by name.
    fn find_events(&self, name: &str) -> Vec<(String, String)> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|(n, _)| n == name)
            .cloned()
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 1. Dispatch argument validation and error typing
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dispatch_de_opt_returns_none_for_missing_field() {
    let args: serde_json::Value = serde_json::json!({ "other": 123 });
    let result: Option<String> = picto_core::dispatch::de_opt(&args, "nonexistent");
    assert!(result.is_none());
}

#[tokio::test]
async fn dispatch_de_returns_error_for_missing_field() {
    let args: serde_json::Value = serde_json::json!({ "other": 123 });
    let result: Result<String, String> = picto_core::dispatch::de(&args, "required_field");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("required_field"));
}

#[tokio::test]
async fn dispatch_de_returns_error_for_wrong_type() {
    let args: serde_json::Value = serde_json::json!({ "num": "not_a_number" });
    let result: Result<i64, String> = picto_core::dispatch::de(&args, "num");
    assert!(result.is_err());
}

#[tokio::test]
async fn dispatch_to_json_serializes_correctly() {
    let value = serde_json::json!({"key": "value"});
    let result = picto_core::dispatch::to_json(&value);
    assert!(result.is_ok());
    let json: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
    assert_eq!(json["key"], "value");
}

#[tokio::test]
async fn dispatch_ok_null_returns_null() {
    let result = picto_core::dispatch::ok_null();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "null");
}

// ---------------------------------------------------------------------------
// 2. Event system — emit and collect
// ---------------------------------------------------------------------------

#[tokio::test]
async fn event_emit_and_collect() {
    let harness = TestHarness::new().await;
    events::emit_event("test-event", r#"{"foo":"bar"}"#);
    let evts = harness.find_events("test-event");
    assert_eq!(evts.len(), 1);
    assert_eq!(evts[0].1, r#"{"foo":"bar"}"#);
}

#[tokio::test]
async fn event_emit_empty_sends_null_payload() {
    let harness = TestHarness::new().await;
    events::emit_empty("empty-event");
    let evts = harness.find_events("empty-event");
    assert_eq!(evts.len(), 1);
    assert_eq!(evts[0].1, "null");
}

#[tokio::test]
async fn state_changed_emits_sequence_numbers() {
    let harness = TestHarness::new().await;
    harness.drain_events();

    events::emit_state_changed(
        "test_origin",
        events::MutationImpact {
            domains: vec![events::Domain::Files],
            ..Default::default()
        },
    );
    events::emit_state_changed(
        "test_origin_2",
        events::MutationImpact {
            domains: vec![events::Domain::Tags],
            ..Default::default()
        },
    );

    let state_evts = harness.find_events("state-changed");
    assert!(state_evts.len() >= 2);

    let first: serde_json::Value = serde_json::from_str(&state_evts[0].1).unwrap();
    let second: serde_json::Value = serde_json::from_str(&state_evts[1].1).unwrap();
    let seq1 = first["seq"].as_u64().unwrap();
    let seq2 = second["seq"].as_u64().unwrap();
    assert!(
        seq2 > seq1,
        "seq numbers should be monotonically increasing"
    );
}

#[tokio::test]
async fn state_changed_emits_sidebar_invalidated_when_flagged() {
    let harness = TestHarness::new().await;
    harness.drain_events();

    events::emit_state_changed(
        "test_sidebar",
        events::MutationImpact {
            domains: vec![events::Domain::Sidebar],
            invalidate: events::Invalidate {
                sidebar_tree: Some(true),
                ..Default::default()
            },
            ..Default::default()
        },
    );

    let sidebar_evts = harness.find_events("sidebar-invalidated");
    assert_eq!(sidebar_evts.len(), 1);
    let payload: serde_json::Value = serde_json::from_str(&sidebar_evts[0].1).unwrap();
    assert_eq!(payload["reason"], "test_sidebar");
}

#[tokio::test]
async fn state_changed_emits_grid_snapshot_invalidated_per_scope() {
    let harness = TestHarness::new().await;
    harness.drain_events();

    events::emit_state_changed(
        "test_grid",
        events::MutationImpact {
            domains: vec![events::Domain::Files],
            invalidate: events::Invalidate {
                grid_scopes: Some(vec!["scope:a".to_string(), "scope:b".to_string()]),
                ..Default::default()
            },
            ..Default::default()
        },
    );

    let grid_evts = harness.find_events("grid-snapshot-invalidated");
    assert_eq!(grid_evts.len(), 2);
    let p1: serde_json::Value = serde_json::from_str(&grid_evts[0].1).unwrap();
    let p2: serde_json::Value = serde_json::from_str(&grid_evts[1].1).unwrap();
    let scopes: Vec<&str> = vec![
        p1["scope_key"].as_str().unwrap(),
        p2["scope_key"].as_str().unwrap(),
    ];
    assert!(scopes.contains(&"scope:a"));
    assert!(scopes.contains(&"scope:b"));
}

// ---------------------------------------------------------------------------
// 3. Grid paging — basic pagination and scope cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grid_page_slim_returns_empty_for_empty_db() {
    let harness = TestHarness::new().await;
    let query = picto_core::types::GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        status: None,
        sort_field: None,
        sort_order: None,
        smart_folder_predicate: None,
        search_tags: None,
        search_excluded_tags: None,
        tag_match_mode: None,
        folder_ids: None,
        excluded_folder_ids: None,
        folder_match_mode: None,
        collection_entity_id: None,
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let result =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, query)
            .await
            .expect("grid page");
    assert!(result.items.is_empty());
    assert!(!result.has_more);
}

#[tokio::test]
async fn grid_page_slim_returns_inserted_files() {
    let harness = TestHarness::new().await;

    // Insert files with status=1 (active)
    harness.insert_test_file("aaa111", "file1.png", 1).await;
    harness.insert_test_file("bbb222", "file2.png", 1).await;
    harness.insert_test_file("ccc333", "file3.png", 1).await;

    // Use status=active which goes through SQL directly (no bitmaps needed)
    let query = picto_core::types::GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        status: Some("active".to_string()),
        sort_field: None,
        sort_order: None,
        smart_folder_predicate: None,
        search_tags: None,
        search_excluded_tags: None,
        tag_match_mode: None,
        folder_ids: None,
        excluded_folder_ids: None,
        folder_match_mode: None,
        collection_entity_id: None,
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let result =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, query)
            .await
            .expect("grid page");
    assert_eq!(result.items.len(), 3);
    assert!(!result.has_more);
}

#[tokio::test]
async fn grid_page_slim_pagination_has_more() {
    let harness = TestHarness::new().await;

    for i in 0..5 {
        let hash = format!("hash_{:03}", i);
        let name = format!("file_{}.png", i);
        harness.insert_test_file(&hash, &name, 1).await;
    }

    let query = picto_core::types::GridPageSlimQuery {
        limit: Some(2),
        cursor: None,
        status: Some("active".to_string()),
        sort_field: None,
        sort_order: None,
        smart_folder_predicate: None,
        search_tags: None,
        search_excluded_tags: None,
        tag_match_mode: None,
        folder_ids: None,
        excluded_folder_ids: None,
        folder_match_mode: None,
        collection_entity_id: None,
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let result =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, query)
            .await
            .expect("grid page");
    assert_eq!(result.items.len(), 2);
    assert!(result.has_more);
    assert!(result.next_cursor.is_some());
}

#[tokio::test]
async fn grid_page_slim_collection_scope_returns_only_collection_members() {
    let harness = TestHarness::new().await;

    harness.insert_test_file("c111", "c1.png", 1).await;
    harness.insert_test_file("c222", "c2.png", 1).await;
    harness.insert_test_file("c333", "c3.png", 1).await;

    let collection_id = harness.create_collection("Collection A").await;
    let added = harness
        .add_collection_members_by_hashes(collection_id, &["c111", "c333"])
        .await;
    assert_eq!(added, 2);

    let query = picto_core::types::GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        status: Some("active".to_string()),
        sort_field: None,
        sort_order: None,
        smart_folder_predicate: None,
        search_tags: None,
        search_excluded_tags: None,
        tag_match_mode: None,
        folder_ids: None,
        excluded_folder_ids: None,
        folder_match_mode: None,
        collection_entity_id: Some(collection_id),
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let result =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, query)
            .await
            .expect("grid page");

    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].hash, "c111");
    assert_eq!(result.items[1].hash, "c333");
    assert!(!result.has_more);
    assert_eq!(result.total_count, Some(2));

    harness
        .db
        .reorder_collection_members_by_hashes(
            collection_id,
            &vec!["c333".to_string(), "c111".to_string()],
        )
        .await
        .expect("reorder collection members");

    let reordered_query = picto_core::types::GridPageSlimQuery {
        limit: Some(10),
        cursor: None,
        status: Some("active".to_string()),
        sort_field: Some("imported_at".to_string()),
        sort_order: Some("desc".to_string()),
        smart_folder_predicate: None,
        search_tags: None,
        search_excluded_tags: None,
        tag_match_mode: None,
        folder_ids: None,
        excluded_folder_ids: None,
        folder_match_mode: None,
        collection_entity_id: Some(collection_id),
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let reordered = picto_core::grid_controller::GridController::get_grid_page_slim(
        &harness.db,
        reordered_query,
    )
    .await
    .expect("grid page after reorder");

    assert_eq!(reordered.items.len(), 2);
    assert_eq!(reordered.items[0].hash, "c333");
    assert_eq!(reordered.items[1].hash, "c111");
}

#[tokio::test]
async fn grid_page_slim_tag_filters_support_any_all_and_reject() {
    let harness = TestHarness::new().await;

    let f1 = harness.insert_test_file("t_any_1", "t1.png", 1).await;
    let f2 = harness.insert_test_file("t_any_2", "t2.png", 1).await;
    let f3 = harness.insert_test_file("t_any_3", "t3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let red = harness.insert_test_tag("", "red").await;
    let blue = harness.insert_test_tag("", "blue").await;
    harness.tag_entity(f1, red).await;
    harness.tag_entity(f2, blue).await;
    harness.tag_entity(f3, red).await;
    harness.tag_entity(f3, blue).await;

    // Seed effective-tag bitmaps directly for deterministic filter tests.
    harness.bitmaps_insert_effective_tag(red, f1);
    harness.bitmaps_insert_effective_tag(red, f3);
    harness.bitmaps_insert_effective_tag(blue, f2);
    harness.bitmaps_insert_effective_tag(blue, f3);

    let any_query = picto_core::types::GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        status: None,
        sort_field: Some("name".to_string()),
        sort_order: Some("asc".to_string()),
        smart_folder_predicate: None,
        search_tags: Some(vec!["red".to_string(), "blue".to_string()]),
        search_excluded_tags: None,
        tag_match_mode: Some("any".to_string()),
        folder_ids: None,
        excluded_folder_ids: None,
        folder_match_mode: None,
        collection_entity_id: None,
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let any_res =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, any_query)
            .await
            .expect("any filter");
    assert_eq!(any_res.items.len(), 3);

    let all_query = picto_core::types::GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        status: None,
        sort_field: Some("name".to_string()),
        sort_order: Some("asc".to_string()),
        smart_folder_predicate: None,
        search_tags: Some(vec!["red".to_string(), "blue".to_string()]),
        search_excluded_tags: Some(vec!["blue".to_string()]),
        tag_match_mode: Some("all".to_string()),
        folder_ids: None,
        excluded_folder_ids: None,
        folder_match_mode: None,
        collection_entity_id: None,
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let all_res =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, all_query)
            .await
            .expect("all + reject filter");
    assert_eq!(all_res.items.len(), 0);
}

#[tokio::test]
async fn grid_page_slim_folder_filters_support_any_all_and_reject() {
    let harness = TestHarness::new().await;

    let f1 = harness.insert_test_file("f_any_1", "f1.png", 1).await;
    let f2 = harness.insert_test_file("f_any_2", "f2.png", 1).await;
    let f3 = harness.insert_test_file("f_any_3", "f3.png", 1).await;
    harness.bitmaps_mark_active(f1);
    harness.bitmaps_mark_active(f2);
    harness.bitmaps_mark_active(f3);
    let fa = harness
        .db
        .create_folder(NewFolder {
            name: "A".to_string(),
            parent_id: None,
            icon: None,
            color: None,
        })
        .await
        .expect("create folder A");
    let fb = harness
        .db
        .create_folder(NewFolder {
            name: "B".to_string(),
            parent_id: None,
            icon: None,
            color: None,
        })
        .await
        .expect("create folder B");

    harness
        .db
        .add_entity_to_folder(fa.folder_id, "f_any_1")
        .await
        .expect("add f1->A");
    harness
        .db
        .add_entity_to_folder(fa.folder_id, "f_any_3")
        .await
        .expect("add f3->A");
    harness
        .db
        .add_entity_to_folder(fb.folder_id, "f_any_2")
        .await
        .expect("add f2->B");
    harness
        .db
        .add_entity_to_folder(fb.folder_id, "f_any_3")
        .await
        .expect("add f3->B");

    let any_query = picto_core::types::GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        status: None,
        sort_field: Some("name".to_string()),
        sort_order: Some("asc".to_string()),
        smart_folder_predicate: None,
        search_tags: None,
        search_excluded_tags: None,
        tag_match_mode: None,
        folder_ids: Some(vec![fa.folder_id, fb.folder_id]),
        excluded_folder_ids: Some(vec![fb.folder_id]),
        folder_match_mode: Some("any".to_string()),
        collection_entity_id: None,
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let any_res =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, any_query)
            .await
            .expect("folder any + reject");
    let any_hashes: Vec<String> = any_res.items.iter().map(|i| i.hash.clone()).collect();
    assert_eq!(any_hashes, vec!["f_any_1".to_string()]);

    let all_query = picto_core::types::GridPageSlimQuery {
        limit: Some(20),
        cursor: None,
        status: None,
        sort_field: Some("name".to_string()),
        sort_order: Some("asc".to_string()),
        smart_folder_predicate: None,
        search_tags: None,
        search_excluded_tags: None,
        tag_match_mode: None,
        folder_ids: Some(vec![fa.folder_id, fb.folder_id]),
        excluded_folder_ids: None,
        folder_match_mode: Some("all".to_string()),
        collection_entity_id: None,
        rating_min: None,
        mime_prefixes: None,
        color_hex: None,
        color_accuracy: None,
        search_text: None,
        random_seed: None,
    };
    let all_res =
        picto_core::grid_controller::GridController::get_grid_page_slim(&harness.db, all_query)
            .await
            .expect("folder all");
    let all_hashes: Vec<String> = all_res.items.iter().map(|i| i.hash.clone()).collect();
    assert_eq!(all_hashes, vec!["f_any_3".to_string()]);
}

// ---------------------------------------------------------------------------
// 4. Scope cache — cached vs uncached consistency
// ---------------------------------------------------------------------------

#[tokio::test]
async fn scope_cache_put_and_get() {
    let harness = TestHarness::new().await;
    let key = picto_core::sqlite::ScopeSnapshotKey {
        scope: "test_scope".to_string(),
        predicate_hash: 42,
        sort_field: "imported_at".to_string(),
        sort_dir: "desc".to_string(),
    };
    let snapshot = picto_core::sqlite::ScopeSnapshot {
        ids: vec![1, 2, 3],
        total_count: 3,
        created_at: std::time::Instant::now(),
    };

    harness.db.scope_cache_put(key.clone(), snapshot);
    let cached = harness.db.scope_cache_get(&key);
    assert!(cached.is_some());
    let cached = cached.unwrap();
    assert_eq!(cached.ids, vec![1, 2, 3]);
    assert_eq!(cached.total_count, 3);
}

#[tokio::test]
async fn scope_cache_invalidate_all_clears() {
    let harness = TestHarness::new().await;
    let key = picto_core::sqlite::ScopeSnapshotKey {
        scope: "test".to_string(),
        predicate_hash: 1,
        sort_field: "imported_at".to_string(),
        sort_dir: "desc".to_string(),
    };
    harness.db.scope_cache_put(
        key.clone(),
        picto_core::sqlite::ScopeSnapshot {
            ids: vec![1],
            total_count: 1,
            created_at: std::time::Instant::now(),
        },
    );
    harness.db.scope_cache_invalidate_all();
    assert!(harness.db.scope_cache_get(&key).is_none());
}

#[tokio::test]
async fn scope_cache_invalidate_scope_prefix() {
    let harness = TestHarness::new().await;
    let key_a = picto_core::sqlite::ScopeSnapshotKey {
        scope: "folder".to_string(),
        predicate_hash: 1,
        sort_field: "imported_at".to_string(),
        sort_dir: "desc".to_string(),
    };
    let key_b = picto_core::sqlite::ScopeSnapshotKey {
        scope: "smart_folder".to_string(),
        predicate_hash: 2,
        sort_field: "imported_at".to_string(),
        sort_dir: "desc".to_string(),
    };
    let snap = || picto_core::sqlite::ScopeSnapshot {
        ids: vec![1],
        total_count: 1,
        created_at: std::time::Instant::now(),
    };
    harness.db.scope_cache_put(key_a.clone(), snap());
    harness.db.scope_cache_put(key_b.clone(), snap());

    harness.db.scope_cache_invalidate_scope("folder");
    assert!(harness.db.scope_cache_get(&key_a).is_none());
    assert!(harness.db.scope_cache_get(&key_b).is_some());
}

// ---------------------------------------------------------------------------
// 5. Projection corruption detection (PBI-012)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn projection_corruption_is_tracked() {
    let harness = TestHarness::new().await;

    // Insert a file and a corrupt projection row
    let file_id = harness
        .insert_test_file("corrupt_hash", "corrupt.png", 1)
        .await;
    let epoch = harness
        .db
        .manifest
        .published_artifact_version("metadata_projection") as i64;

    let fid = file_id;
    harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO entity_metadata_projection (entity_id, epoch, resolved_json, parents_json)
                 VALUES (?1, ?2, 'THIS_IS_NOT_JSON{{{', '[]')",
                rusqlite::params![fid, epoch],
            )?;
            Ok(())
        })
        .await
        .expect("insert corrupt projection");

    // Request metadata batch — should detect corruption and fallback
    let result = harness
        .db
        .get_files_metadata_batch(vec!["corrupt_hash".to_string()])
        .await
        .expect("batch should succeed even with corrupt projection");

    assert!(!result.is_empty(), "should still return data via fallback");

    // Corruption counter should have been incremented
    let count = picto_core::perf::get_projection_corruption_count();
    assert!(count > 0, "corruption count should be incremented");
}

// ---------------------------------------------------------------------------
// 6. Hash resolution (PBI-011)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn resolve_hashes_batch_returns_file_ids() {
    let harness = TestHarness::new().await;

    let fid1 = harness.insert_test_file("hash_a", "a.png", 1).await;
    let fid2 = harness.insert_test_file("hash_b", "b.png", 1).await;
    harness.insert_test_file("hash_c", "c.png", 1).await;

    let resolved = harness
        .db
        .resolve_hashes_batch(&[
            "hash_a".to_string(),
            "hash_b".to_string(),
            "nonexistent".to_string(),
        ])
        .await
        .expect("resolve batch");

    // Should resolve the existing hashes, skip nonexistent
    assert_eq!(resolved.len(), 2);
    let ids: Vec<i64> = resolved.iter().map(|(_, id)| *id).collect();
    assert!(ids.contains(&fid1));
    assert!(ids.contains(&fid2));
}

#[tokio::test]
async fn resolve_ids_batch_returns_hashes() {
    let harness = TestHarness::new().await;

    let fid1 = harness.insert_test_file("hash_x", "x.png", 1).await;
    let fid2 = harness.insert_test_file("hash_y", "y.png", 1).await;

    let resolved = harness
        .db
        .resolve_ids_batch(&[fid1, fid2, 99999])
        .await
        .expect("resolve ids batch");

    assert_eq!(resolved.len(), 2);
    let hashes: Vec<&str> = resolved.iter().map(|(_, h)| h.as_str()).collect();
    assert!(hashes.contains(&"hash_x"));
    assert!(hashes.contains(&"hash_y"));
}

// ---------------------------------------------------------------------------
// 7. Tag table regression (PBI-001)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tag_table_query_returns_seeded_tags() {
    let harness = TestHarness::new().await;

    // Seed a tag and a file with that tag
    let fid = harness
        .insert_test_file("tag_hash_1", "tagged.png", 1)
        .await;
    let tid = harness.insert_test_tag("character", "alice").await;
    harness.tag_entity(fid, tid).await;

    // Update file_count so the tag appears in results
    harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "UPDATE tag SET file_count = 1 WHERE tag_id = ?1",
                rusqlite::params![tid],
            )?;
            Ok(())
        })
        .await
        .expect("update file_count");

    let tags = harness
        .db
        .get_all_tags_with_counts()
        .await
        .expect("get_all_tags_with_counts");
    assert!(!tags.is_empty(), "should return seeded tag");
    assert!(tags
        .iter()
        .any(|t| t.subtag == "alice" && t.namespace == "character"));
}

#[tokio::test]
async fn tag_table_query_returns_empty_for_empty_db() {
    let harness = TestHarness::new().await;
    let tags = harness
        .db
        .get_all_tags_with_counts()
        .await
        .expect("get_all_tags_with_counts on empty db");
    assert!(
        tags.is_empty(),
        "empty tag table should return empty list, not error"
    );
}

// ---------------------------------------------------------------------------
// 8. Perf snapshot includes corruption counter
// ---------------------------------------------------------------------------

#[tokio::test]
async fn perf_snapshot_includes_projection_corruption() {
    let snap = picto_core::perf::get_snapshot();
    // The field should exist and be serializable
    let json = serde_json::to_value(&snap).expect("serialize perf snapshot");
    assert!(json.get("projection_corruption_count").is_some());
}

// ---------------------------------------------------------------------------
// 9. PBI-005: Query plan verification — composite indexes avoid temp B-tree
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grid_query_plan_uses_composite_index() {
    let harness = TestHarness::new().await;
    // Verify the primary grid query (status + imported_at DESC) uses the composite index.
    let plan = harness
        .db
        .with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "EXPLAIN QUERY PLAN SELECT file_id FROM file WHERE status = 1 ORDER BY imported_at DESC, file_id DESC LIMIT 50",
            )?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(3))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows.join("\n"))
        })
        .await
        .expect("explain query plan");

    assert!(
        !plan.contains("TEMP B-TREE"),
        "Grid query should not use temp B-tree sort. Plan:\n{plan}"
    );
    assert!(
        plan.contains("idx_file_status_imported"),
        "Grid query should use idx_file_status_imported index. Plan:\n{plan}"
    );
}

// ---------------------------------------------------------------------------
// Event contract tests — verify serialized shapes match TypeScript expectations
// ---------------------------------------------------------------------------

#[test]
fn event_names_are_kebab_case() {
    use picto_core::events::event_names;
    let names = [
        event_names::STATE_CHANGED,
        event_names::SIDEBAR_INVALIDATED,
        event_names::GRID_SNAPSHOT_INVALIDATED,
        event_names::SUBSCRIPTION_STARTED,
        event_names::SUBSCRIPTION_PROGRESS,
        event_names::SUBSCRIPTION_FINISHED,
        event_names::FLOW_STARTED,
        event_names::FLOW_PROGRESS,
        event_names::FLOW_FINISHED,
        event_names::PTR_SYNC_STARTED,
        event_names::PTR_SYNC_PROGRESS,
        event_names::PTR_SYNC_FINISHED,
        event_names::PTR_SYNC_PHASE_CHANGED,
        event_names::PTR_BOOTSTRAP_STARTED,
        event_names::PTR_BOOTSTRAP_PROGRESS,
        event_names::PTR_BOOTSTRAP_FINISHED,
        event_names::PTR_BOOTSTRAP_FAILED,
        event_names::LIBRARY_CLOSED,
        event_names::ZOOM_FACTOR_CHANGED,
        event_names::FILE_IMPORTED,
        event_names::OPEN_DETAIL_WINDOW,
        event_names::DUPLICATE_AUTO_MERGE_FINISHED,
    ];
    for name in names {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "Event name '{}' is not kebab-case",
            name
        );
    }
}

#[test]
fn subscription_started_event_contract() {
    let event = picto_core::events::SubscriptionStartedEvent {
        subscription_id: "sub_123".into(),
        subscription_name: "Test Sub".into(),
        mode: "subscription".into(),
        query_id: None,
        query_name: None,
    };
    let json: serde_json::Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["subscription_id"], "sub_123");
    assert_eq!(json["subscription_name"], "Test Sub");
    assert_eq!(json["mode"], "subscription");
    assert!(
        json.get("query_id").is_none(),
        "None fields should be skipped"
    );

    // With query_id
    let event2 = picto_core::events::SubscriptionStartedEvent {
        query_id: Some("q1".into()),
        ..event
    };
    let json2: serde_json::Value = serde_json::to_value(&event2).unwrap();
    assert_eq!(json2["query_id"], "q1");
}

#[test]
fn subscription_finished_event_contract() {
    let event = picto_core::events::SubscriptionFinishedEvent {
        subscription_id: "sub_1".into(),
        subscription_name: "My Sub".into(),
        mode: "subscription".into(),
        query_id: None,
        query_name: None,
        status: "succeeded".into(),
        files_downloaded: 42,
        files_skipped: 3,
        errors_count: 0,
        error: None,
        failure_kind: None,
        metadata_validated: 0,
        metadata_invalid: 0,
        last_metadata_error: None,
    };
    let json: serde_json::Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["files_downloaded"], 42);
    assert_eq!(json["files_skipped"], 3);
    assert_eq!(json["errors_count"], 0);
    assert_eq!(json["status"], "succeeded");
    assert!(json.get("error").is_none());
    assert!(json.get("query_id").is_none());
}

#[test]
fn flow_events_contract() {
    // FlowStartedEvent
    let started = picto_core::events::FlowStartedEvent {
        flow_id: "f_42".into(),
        subscription_count: 3,
    };
    let json: serde_json::Value = serde_json::to_value(&started).unwrap();
    assert_eq!(json["flow_id"], "f_42");
    assert_eq!(json["subscription_count"], 3);

    // FlowProgressEvent
    let progress = picto_core::events::FlowProgressEvent {
        flow_id: "f_42".into(),
        total: 3,
        done: 1,
        remaining: 2,
    };
    let json: serde_json::Value = serde_json::to_value(&progress).unwrap();
    assert_eq!(json["total"], 3);
    assert_eq!(json["done"], 1);
    assert_eq!(json["remaining"], 2);

    // FlowFinishedEvent — success
    let finished = picto_core::events::FlowFinishedEvent {
        flow_id: "f_42".into(),
        status: "succeeded".into(),
        started_count: Some(3),
        error: None,
    };
    let json: serde_json::Value = serde_json::to_value(&finished).unwrap();
    assert_eq!(json["status"], "succeeded");
    assert_eq!(json["started_count"], 3);
    assert!(json.get("error").is_none());

    // FlowFinishedEvent — failure
    let failed = picto_core::events::FlowFinishedEvent {
        flow_id: "f_42".into(),
        status: "failed".into(),
        started_count: None,
        error: Some("plugin not found".into()),
    };
    let json: serde_json::Value = serde_json::to_value(&failed).unwrap();
    assert_eq!(json["status"], "failed");
    assert_eq!(json["error"], "plugin not found");
    assert!(json.get("started_count").is_none());
}

#[test]
fn ptr_sync_finished_event_contract() {
    // Success path
    let success = picto_core::events::PtrSyncFinishedEvent {
        success: true,
        error: None,
        updates_processed: Some(100),
        tags_added: Some(50),
        schema_rebuild: None,
        index_rebuild: None,
        changed_hashes_truncated: Some(false),
    };
    let json: serde_json::Value = serde_json::to_value(&success).unwrap();
    assert_eq!(json["success"], true);
    assert!(json.get("error").is_none());
    assert_eq!(json["updates_processed"], 100);
    assert_eq!(json["tags_added"], 50);

    // Failure path
    let failure = picto_core::events::PtrSyncFinishedEvent {
        success: false,
        error: Some("Network error".into()),
        updates_processed: None,
        tags_added: None,
        schema_rebuild: None,
        index_rebuild: None,
        changed_hashes_truncated: None,
    };
    let json: serde_json::Value = serde_json::to_value(&failure).unwrap();
    assert_eq!(json["success"], false);
    assert_eq!(json["error"], "Network error");
    assert!(json.get("updates_processed").is_none());
}

#[test]
fn ptr_bootstrap_progress_union_shape() {
    // The 6 call sites populate different subsets. Default must work.
    let minimal = picto_core::events::PtrBootstrapProgressEvent {
        phase: "cancelling".into(),
        stage: Some("cancelling".into()),
        ..Default::default()
    };
    let json: serde_json::Value = serde_json::to_value(&minimal).unwrap();
    assert_eq!(json["phase"], "cancelling");
    assert_eq!(json["stage"], "cancelling");
    assert!(json.get("service_id").is_none());
    assert!(json.get("rows_done").is_none());

    // Full progress
    let full = picto_core::events::PtrBootstrapProgressEvent {
        phase: "importing".into(),
        stage: Some("compact_build".into()),
        rows_done: Some(5000),
        rows_total: Some(100000),
        rows_done_stage: Some(5000),
        rows_total_stage: Some(100000),
        rows_per_sec: Some(1234.5),
        eta_seconds: Some(77.0),
        ts: Some("2024-01-01T00:00:00Z".into()),
        ..Default::default()
    };
    let json: serde_json::Value = serde_json::to_value(&full).unwrap();
    assert_eq!(json["rows_done"], 5000);
    assert_eq!(json["rows_total"], 100000);
    assert!(json["rows_per_sec"].as_f64().unwrap() > 1000.0);
}

#[test]
fn state_changed_event_contract() {
    use picto_core::events::{Domain, Invalidate, StateChangedEvent};
    let event = StateChangedEvent {
        seq: 1,
        ts: "2024-01-01T00:00:00Z".into(),
        origin_command: "add_tags".into(),
        domains: vec![Domain::Tags, Domain::Files],
        file_hashes: Some(vec!["abc123".into()]),
        folder_ids: None,
        smart_folder_ids: None,
        invalidate: Invalidate {
            sidebar_tree: Some(true),
            grid_scopes: Some(vec!["system:all".into()]),
            ..Default::default()
        },
        compiler_batch_done: None,
        sidebar_counts: None,
    };
    let json: serde_json::Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["seq"], 1);
    assert_eq!(json["origin_command"], "add_tags");
    assert!(json["domains"].is_array());
    assert_eq!(json["domains"].as_array().unwrap().len(), 2);
    assert_eq!(json["file_hashes"][0], "abc123");
    assert_eq!(json["invalidate"]["sidebar_tree"], true);
    assert_eq!(json["invalidate"]["grid_scopes"][0], "system:all");
    assert!(json.get("folder_ids").is_none());
    assert!(json.get("compiler_batch_done").is_none());
}

#[test]
fn duplicate_auto_merge_finished_contract() {
    let event = picto_core::events::DuplicateAutoMergeFinishedEvent {
        winner_hash: "aaa".into(),
        loser_hash: "bbb".into(),
        distance: 3,
        tags_merged: 5,
    };
    let json: serde_json::Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["winner_hash"], "aaa");
    assert_eq!(json["loser_hash"], "bbb");
    assert_eq!(json["distance"], 3);
    assert_eq!(json["tags_merged"], 5);
}

#[test]
fn zoom_factor_changed_contract() {
    let event = picto_core::events::ZoomFactorChangedEvent { factor: 1.5 };
    let json: serde_json::Value = serde_json::to_value(&event).unwrap();
    assert_eq!(json["factor"], 1.5);
}

// ---------------------------------------------------------------------------
// Phase C: Composite index EXPLAIN QUERY PLAN tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grid_query_plan_rating_sort_uses_index() {
    let harness = TestHarness::new().await;
    let plan = harness
        .db
        .with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "EXPLAIN QUERY PLAN SELECT file_id FROM file WHERE status = 1 ORDER BY rating DESC, file_id DESC LIMIT 50",
            )?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(3))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows.join("\n"))
        })
        .await
        .expect("explain query plan");

    assert!(
        !plan.contains("TEMP B-TREE"),
        "Rating sort should not use temp B-tree. Plan:\n{plan}"
    );
    assert!(
        plan.contains("idx_file_status_rating"),
        "Rating sort should use idx_file_status_rating. Plan:\n{plan}"
    );
}

#[tokio::test]
async fn grid_query_plan_size_sort_uses_index() {
    let harness = TestHarness::new().await;
    let plan = harness
        .db
        .with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "EXPLAIN QUERY PLAN SELECT file_id FROM file WHERE status = 1 ORDER BY size DESC, file_id DESC LIMIT 50",
            )?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(3))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows.join("\n"))
        })
        .await
        .expect("explain query plan");

    assert!(
        !plan.contains("TEMP B-TREE"),
        "Size sort should not use temp B-tree. Plan:\n{plan}"
    );
    assert!(
        plan.contains("idx_file_status_size"),
        "Size sort should use idx_file_status_size. Plan:\n{plan}"
    );
}

#[tokio::test]
async fn grid_query_plan_viewcount_sort_uses_index() {
    let harness = TestHarness::new().await;
    let plan = harness
        .db
        .with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "EXPLAIN QUERY PLAN SELECT file_id FROM file WHERE status = 1 ORDER BY view_count DESC, file_id DESC LIMIT 50",
            )?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(3))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows.join("\n"))
        })
        .await
        .expect("explain query plan");

    assert!(
        !plan.contains("TEMP B-TREE"),
        "View count sort should not use temp B-tree. Plan:\n{plan}"
    );
    assert!(
        plan.contains("idx_file_status_viewcount"),
        "View count sort should use idx_file_status_viewcount. Plan:\n{plan}"
    );
}

#[tokio::test]
async fn grid_query_plan_name_sort_uses_index() {
    let harness = TestHarness::new().await;
    let plan = harness
        .db
        .with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "EXPLAIN QUERY PLAN SELECT file_id FROM file WHERE status = 1 ORDER BY name COLLATE NOCASE, file_id LIMIT 50",
            )?;
            let rows: Vec<String> = stmt
                .query_map([], |row| row.get::<_, String>(3))?
                .filter_map(|r| r.ok())
                .collect();
            Ok(rows.join("\n"))
        })
        .await
        .expect("explain query plan");

    assert!(
        !plan.contains("TEMP B-TREE"),
        "Name sort should not use temp B-tree. Plan:\n{plan}"
    );
    assert!(
        plan.contains("idx_file_status_name"),
        "Name sort should use idx_file_status_name. Plan:\n{plan}"
    );
}

// ---------------------------------------------------------------------------
// Phase C: Batch operation edge-case tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn remove_tags_batch_empty_inputs() {
    let harness = TestHarness::new().await;
    // Empty file_ids
    let result = harness
        .db
        .remove_tags_batch_by_entity_ids(vec![], vec!["tag:one".into()])
        .await;
    assert!(result.is_ok());

    // Empty tag_strings
    let result = harness
        .db
        .remove_tags_batch_by_entity_ids(vec![1, 2], vec![])
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn remove_tags_batch_by_entity_ids_correctness() {
    let harness = TestHarness::new().await;
    // Seed 3 files
    let f1 = harness.insert_test_file("hash_a", "a.png", 1).await;
    let f2 = harness.insert_test_file("hash_b", "b.png", 1).await;
    let f3 = harness.insert_test_file("hash_c", "c.png", 1).await;
    // Seed 2 tags with file_count
    let t1 = harness.insert_test_tag("", "red").await;
    let t2 = harness.insert_test_tag("", "blue").await;
    // Tag all 3 files with both tags and set file_counts
    for &fid in &[f1, f2, f3] {
        harness.tag_entity(fid, t1).await;
        harness.tag_entity(fid, t2).await;
    }
    let t1_copy = t1;
    let t2_copy = t2;
    harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "UPDATE tag SET file_count = 3 WHERE tag_id IN (?1, ?2)",
                rusqlite::params![t1_copy, t2_copy],
            )?;
            Ok(())
        })
        .await
        .expect("set file_counts");

    // Remove tag "red" from f1 and f2 only
    harness
        .db
        .remove_tags_batch_by_entity_ids(vec![f1, f2], vec![":red".into()])
        .await
        .expect("remove tags batch");

    // Verify: f1 and f2 should no longer have tag t1
    let remaining_t1: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM entity_tag_raw WHERE tag_id = ?1",
                [t1],
                |row| row.get(0),
            )
        })
        .await
        .expect("count t1 tags");
    assert_eq!(remaining_t1, 1, "Only f3 should still have tag 'red'");

    // Verify: file_count for t1 should be decremented by 2
    let count_t1: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT file_count FROM tag WHERE tag_id = ?1",
                [t1],
                |row| row.get(0),
            )
        })
        .await
        .expect("get t1 file_count");
    assert_eq!(count_t1, 1, "file_count for 'red' should be 1 (3-2)");

    // Verify: tag t2 should be untouched — all 3 files still tagged
    let remaining_t2: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM entity_tag_raw WHERE tag_id = ?1",
                [t2],
                |row| row.get(0),
            )
        })
        .await
        .expect("count t2 tags");
    assert_eq!(remaining_t2, 3, "All 3 files should still have tag 'blue'");
}

#[tokio::test]
async fn remove_files_from_folder_batch_correctness() {
    let harness = TestHarness::new().await;
    // Seed 3 files
    let f1 = harness.insert_test_file("hash_fa", "a.png", 1).await;
    let f2 = harness.insert_test_file("hash_fb", "b.png", 1).await;
    let f3 = harness.insert_test_file("hash_fc", "c.png", 1).await;
    // Create a folder and add all 3 files
    let folder_id: i64 = harness
        .db
        .with_conn(move |conn| {
            conn.execute(
                "INSERT INTO folder (name, created_at) VALUES ('test_folder', datetime('now'))",
                [],
            )?;
            let fid = conn.last_insert_rowid();
            let mut stmt = conn.prepare_cached(
                "INSERT INTO folder_entity (folder_id, entity_id, position_rank) VALUES (?1, ?2, ?3)",
            )?;
            stmt.execute(rusqlite::params![fid, f1, 1])?;
            stmt.execute(rusqlite::params![fid, f2, 2])?;
            stmt.execute(rusqlite::params![fid, f3, 3])?;
            Ok(fid)
        })
        .await
        .expect("create folder with files");

    // Remove f1 and f2 from the folder
    let removed = harness
        .db
        .remove_entities_from_folder_batch(folder_id, &["hash_fa".into(), "hash_fb".into()])
        .await
        .expect("remove files batch");
    assert_eq!(removed, 2, "Should have removed 2 files");

    // Verify: only f3 remains in the folder
    let remaining: i64 = harness
        .db
        .with_read_conn(move |conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM folder_entity WHERE folder_id = ?1",
                [folder_id],
                |row| row.get(0),
            )
        })
        .await
        .expect("count remaining folder files");
    assert_eq!(remaining, 1, "Only f3 should remain in the folder");
}

#[tokio::test]
async fn folder_controller_updates_sidebar_projection_immediately() {
    let harness = TestHarness::new().await;

    let parent = picto_core::folder_controller::FolderController::create_folder(
        &harness.db,
        "Parent".to_string(),
        None,
        Some("IconFolder".to_string()),
        Some("#aaaaaa".to_string()),
    )
    .await
    .expect("create parent folder");

    let child = picto_core::folder_controller::FolderController::create_folder(
        &harness.db,
        "Child".to_string(),
        Some(parent.folder_id),
        Some("IconPhoto".to_string()),
        Some("#ff0000".to_string()),
    )
    .await
    .expect("create child folder");

    let child_node_id = format!("folder:{}", child.folder_id);

    let child_node: (String, Option<String>, Option<String>, Option<String>) = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT name, parent_id, icon, color FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            }
        })
        .await
        .expect("read created child sidebar node");
    assert_eq!(child_node.0, "Child");
    assert_eq!(child_node.1, Some(format!("folder:{}", parent.folder_id)));
    assert_eq!(child_node.2, Some("IconPhoto".to_string()));
    assert_eq!(child_node.3, Some("#ff0000".to_string()));

    picto_core::folder_controller::FolderController::update_folder(
        &harness.db,
        child.folder_id,
        Some("Child Renamed".to_string()),
        Some("IconStar".to_string()),
        Some("#00ff00".to_string()),
    )
    .await
    .expect("update folder visuals");

    let updated_node: (String, Option<String>, Option<String>, Option<String>) = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT name, parent_id, icon, color FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            }
        })
        .await
        .expect("read updated child sidebar node");
    assert_eq!(updated_node.0, "Child Renamed");
    assert_eq!(updated_node.1, Some(format!("folder:{}", parent.folder_id)));
    assert_eq!(updated_node.2, Some("IconStar".to_string()));
    assert_eq!(updated_node.3, Some("#00ff00".to_string()));

    picto_core::folder_controller::FolderController::update_folder(
        &harness.db,
        child.folder_id,
        None,
        Some(String::new()),
        Some(String::new()),
    )
    .await
    .expect("clear folder visuals");

    let cleared_node: (String, Option<String>, Option<String>, Option<String>) = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT name, parent_id, icon, color FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            }
        })
        .await
        .expect("read cleared child sidebar node");
    assert_eq!(cleared_node.0, "Child Renamed");
    assert_eq!(cleared_node.1, Some(format!("folder:{}", parent.folder_id)));
    assert_eq!(cleared_node.2, None);
    assert_eq!(cleared_node.3, None);

    picto_core::folder_controller::FolderController::update_folder_parent(
        &harness.db,
        child.folder_id,
        None,
    )
    .await
    .expect("move child to root");

    let reparents_node_parent: Option<String> = harness
        .db
        .with_read_conn({
            let child_node_id = child_node_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT parent_id FROM sidebar_node WHERE node_id = ?1",
                    [child_node_id],
                    |row| row.get(0),
                )
            }
        })
        .await
        .expect("read reparented child sidebar node");
    assert_eq!(reparents_node_parent, Some("section:folders".to_string()));
}

// ---------------------------------------------------------------------------
// Phase D: Perf snapshot and SLO check include selection_summary
// ---------------------------------------------------------------------------

#[test]
fn perf_snapshot_includes_selection_summary() {
    let snap = picto_core::perf::get_snapshot();
    let json = serde_json::to_value(&snap).expect("serialize perf snapshot");
    assert!(
        json.get("selection_summary").is_some(),
        "PerfSnapshot must include selection_summary"
    );
    assert!(json.get("grid_page_slim").is_some());
    assert!(json.get("files_metadata_batch").is_some());
    assert!(json.get("sidebar_tree").is_some());
}

#[test]
fn slo_check_includes_selection_summary() {
    let result = picto_core::perf::check_default_slo();
    let json = serde_json::to_value(&result).expect("serialize slo check");
    assert!(
        json.get("selection_summary").is_some(),
        "SloCheckResult must include selection_summary"
    );
    // Verify target values
    let ss = &json["selection_summary"];
    assert_eq!(ss["target_p50_ms"], 60.0);
    assert_eq!(ss["target_p95_ms"], 120.0);
    assert_eq!(ss["target_p99_ms"], 200.0);
}

#[test]
fn slo_pass_fail_with_samples() {
    // Record fast samples into all 4 windows — well under SLO targets
    for _ in 0..20 {
        picto_core::perf::record_grid_page_slim(10.0);
        picto_core::perf::record_files_metadata_batch(8.0, 3.0, 3.0, 2.0, 10, 8, 2, 2, 0);
        picto_core::perf::record_sidebar_tree(12.0);
        picto_core::perf::record_selection_summary(15.0);
    }

    let result = picto_core::perf::check_default_slo();
    assert!(result.pass, "SLO should pass when all samples are fast");
    assert!(result.click_metadata.pass_p50);
    assert!(result.grid_first_page.pass_p50);
    assert!(result.sidebar_tree.pass_p50);
    assert!(result.selection_summary.pass_p50);

    // Now flood selection_summary with slow samples to blow the P50
    for _ in 0..600 {
        picto_core::perf::record_selection_summary(500.0);
    }

    let result = picto_core::perf::check_default_slo();
    assert!(
        !result.pass,
        "SLO should fail when selection_summary is slow"
    );
    assert!(
        !result.selection_summary.pass_p50,
        "selection_summary P50 should fail at 500ms vs 60ms target"
    );
}

// ---------------------------------------------------------------------------
// PBI-091: Mutation impact preset contract tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn file_lifecycle_preset_emits_correct_events() {
    let harness = TestHarness::new().await;
    harness.drain_events();

    let impact = events::MutationImpact::file_lifecycle(&harness.db);
    events::emit_state_changed("test_file_lifecycle", impact);

    // 1. state-changed should have domains containing "files"
    let state_evts = harness.find_events("state-changed");
    assert!(!state_evts.is_empty(), "should emit state-changed");
    let payload: serde_json::Value = serde_json::from_str(&state_evts.last().unwrap().1).unwrap();
    let domains: Vec<String> = payload["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        domains.contains(&"files".to_string()),
        "should include files domain"
    );

    // 2. sidebar-invalidated should fire (file_lifecycle sets sidebar_tree)
    let sidebar_evts = harness.find_events("sidebar-invalidated");
    assert!(
        !sidebar_evts.is_empty(),
        "file_lifecycle should trigger sidebar-invalidated"
    );

    // 3. grid-snapshot-invalidated should fire for system:all
    let grid_evts = harness.find_events("grid-snapshot-invalidated");
    assert!(
        !grid_evts.is_empty(),
        "file_lifecycle should trigger grid-snapshot-invalidated"
    );
    let grid_payload: serde_json::Value = serde_json::from_str(&grid_evts[0].1).unwrap();
    assert_eq!(grid_payload["scope_key"], "system:all");

    // 4. sidebar_counts should be present
    assert!(
        payload.get("sidebar_counts").is_some(),
        "file_lifecycle should include sidebar_counts"
    );
}

#[tokio::test]
async fn folder_sidebar_preset_emits_sidebar_only() {
    let harness = TestHarness::new().await;
    harness.drain_events();

    let impact = events::MutationImpact::sidebar(events::Domain::Folders);
    events::emit_state_changed("test_folder_sidebar", impact);

    // 1. state-changed should have domains containing "folders" and "sidebar"
    let state_evts = harness.find_events("state-changed");
    assert!(!state_evts.is_empty());
    let payload: serde_json::Value = serde_json::from_str(&state_evts.last().unwrap().1).unwrap();
    let domains: Vec<String> = payload["domains"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(domains.contains(&"folders".to_string()));
    assert!(domains.contains(&"sidebar".to_string()));

    // 2. sidebar-invalidated should fire
    let sidebar_evts = harness.find_events("sidebar-invalidated");
    assert!(
        !sidebar_evts.is_empty(),
        "sidebar preset should trigger sidebar-invalidated"
    );

    // 3. grid-snapshot-invalidated should NOT fire (sidebar preset doesn't set grid_scopes)
    let grid_evts = harness.find_events("grid-snapshot-invalidated");
    assert!(
        grid_evts.is_empty(),
        "sidebar preset should NOT trigger grid-snapshot-invalidated"
    );
}

#[tokio::test]
async fn batch_tags_preset_no_duplicate_events() {
    let harness = TestHarness::new().await;
    harness.drain_events();

    let impact = events::MutationImpact::batch_tags();
    events::emit_state_changed("test_batch_tags", impact);

    // Exactly 1 state-changed
    let state_evts = harness.find_events("state-changed");
    let own_evts: Vec<_> = state_evts
        .iter()
        .filter(|(_, p)| p.contains("test_batch_tags"))
        .collect();
    assert_eq!(
        own_evts.len(),
        1,
        "should emit exactly 1 state-changed for batch_tags"
    );

    // No sidebar-invalidated (batch_tags does NOT set sidebar_tree)
    let sidebar_evts = harness.find_events("sidebar-invalidated");
    let own_sidebar: Vec<_> = sidebar_evts
        .iter()
        .filter(|(_, p)| p.contains("test_batch_tags"))
        .collect();
    assert_eq!(
        own_sidebar.len(),
        0,
        "batch_tags should NOT trigger sidebar-invalidated"
    );

    // Exactly 1 grid-snapshot-invalidated for system:all
    let grid_evts = harness.find_events("grid-snapshot-invalidated");
    let own_grid: Vec<_> = grid_evts
        .iter()
        .filter(|(_, p)| p.contains("test_batch_tags"))
        .collect();
    assert_eq!(
        own_grid.len(),
        1,
        "should emit exactly 1 grid-snapshot-invalidated"
    );
    let grid_payload: serde_json::Value = serde_json::from_str(&own_grid[0].1).unwrap();
    assert_eq!(grid_payload["scope_key"], "system:all");
}
