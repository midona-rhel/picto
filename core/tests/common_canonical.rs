//! Shared canonical LibraryDatabase test harness for scope and smart-folder tests.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use picto_core::db::projection::bitmaps::BitmapKey;
use picto_core::db::types::{ExpansionMode, TAG_PROVENANCE_MANUAL};
use picto_core::db::LibraryDatabase;

pub struct TestHarness {
    _tmp: TempDir,
    pub db: Arc<LibraryDatabase>,
    tag_strings: Mutex<HashMap<i64, String>>,
}

impl TestHarness {
    pub async fn new() -> Self {
        let tmp = TempDir::new().expect("create temp dir");
        let db = Arc::new(LibraryDatabase::open(tmp.path()).expect("open canonical library db"));
        Self {
            _tmp: tmp,
            db,
            tag_strings: Mutex::new(HashMap::new()),
        }
    }

    pub async fn insert_test_file(&self, hash: &str, name: &str, status: i64) -> i64 {
        let now = chrono::Utc::now().to_rfc3339();
        let file_id = self
            .db
            .insert_file(
                hash,
                "image/png",
                1024,
                Some(100),
                Some(100),
                None,
                None,
                false,
                &now,
            )
            .expect("insert file");
        self.db
            .insert_single(hash, file_id, Some(name), status, &now, &now)
            .expect("insert single entity")
    }

    pub async fn insert_test_tag(&self, namespace: &str, subtag: &str) -> i64 {
        let tag_string = if namespace.is_empty() {
            format!("general:{subtag}")
        } else {
            format!("{namespace}:{subtag}")
        };
        let tag_id = self.db.ensure_tag(&tag_string).expect("ensure tag");
        self.tag_strings
            .lock()
            .expect("lock tag strings")
            .insert(tag_id, tag_string);
        tag_id
    }

    pub async fn tag_entity(&self, entity_id: i64, tag_id: i64) {
        let tag = self
            .tag_strings
            .lock()
            .expect("lock tag strings")
            .get(&tag_id)
            .cloned()
            .expect("known tag string");
        self.db
            .add_tags(
                &[entity_id],
                &[tag],
                TAG_PROVENANCE_MANUAL,
                ExpansionMode::EntityOnly,
            )
            .expect("add tag");
    }

    pub fn bitmaps_insert_effective_tag(&self, tag_id: i64, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::EffectiveTag(tag_id), entity_id as u32);
    }

    pub fn bitmaps_mark_active(&self, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::Status(1), entity_id as u32);
    }

    pub fn bitmaps_mark_inbox(&self, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::Status(0), entity_id as u32);
    }

    pub fn bitmaps_mark_trash(&self, entity_id: i64) {
        self.db
            .bitmaps
            .insert(&BitmapKey::Status(2), entity_id as u32);
    }

    pub fn bitmaps_mark_tagged(&self, entity_id: i64) {
        self.db.bitmaps.insert(&BitmapKey::Tagged, entity_id as u32);
    }
}
