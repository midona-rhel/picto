//! Library database boundary.
//!
//! All SQL and storage details are contained within this module tree.
//! Code outside `db/` consumes typed methods and typed results only.
//! No SQL, no table names, no bitmap storage details leak out.

pub mod core;
pub mod migration_legacy;
pub mod projection;
pub mod query;
pub mod types;
pub mod write;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use self::projection::bitmaps::BitmapStore;
use self::types::*;

/// The single typed database boundary. All storage access goes through here.
/// Code outside `core/src/db/` must not issue SQL or know table names.
pub struct LibraryDatabase {
    /// Single writer connection (mutex-protected).
    write_conn: Mutex<Connection>,
    /// Read pool connection (for parallel reads).
    read_conn: Mutex<Connection>,
    /// In-memory bitmap store (derived, rebuildable).
    pub bitmaps: Arc<BitmapStore>,
    /// Library root path (for bitmap snapshot files).
    library_root: std::path::PathBuf,
}

impl LibraryDatabase {
    /// Open or create a library database at the given root path.
    pub fn open(library_root: &Path) -> Result<Self, String> {
        let db_path = library_root.join("library.db");
        let write_conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open write connection: {e}"))?;
        let read_conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open read connection: {e}"))?;

        // Configure connections
        write_conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
            .map_err(|e| format!("Failed to configure write connection: {e}"))?;
        read_conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;")
            .map_err(|e| format!("Failed to configure read connection: {e}"))?;

        let bitmaps = Arc::new(BitmapStore::new());

        let db = Self {
            write_conn: Mutex::new(write_conn),
            read_conn: Mutex::new(read_conn),
            bitmaps,
            library_root: library_root.to_path_buf(),
        };

        // Check if migration is needed
        {
            let conn = db.write_conn.lock().unwrap();
            if migration_legacy::needs_migration(&conn) {
                tracing::info!("Legacy schema detected, running migration...");
                let result = migration_legacy::migrate(&conn)?;
                tracing::info!("{result}");
                // Full projection rebuild after migration
                projection::compiler::full_rebuild(&conn, &db.bitmaps);
                tracing::info!("Post-migration projection rebuild complete");
            } else if !migration_legacy::is_new_schema(&conn) {
                // Fresh database — create new schema
                conn.execute_batch(core::schema::LIBRARY_DDL)
                    .map_err(|e| format!("Failed to create schema: {e}"))?;
                projection::compiler::full_rebuild(&conn, &db.bitmaps);
            } else {
                // Existing new-schema database — load bitmap snapshot + replay deltas
                let delta_path = library_root.join("bitmaps.delta");
                let replayed = projection::bitmap_delta::replay_deltas(&delta_path, &db.bitmaps)
                    .unwrap_or(0);
                if replayed == 0 {
                    // No deltas or snapshot — full rebuild
                    let conn_r = db.read_conn.lock().unwrap();
                    projection::compiler::full_rebuild(&conn_r, &db.bitmaps);
                }
            }
        }

        Ok(db)
    }

    // ── Internal connection access (crate-private) ─────────────────

    /// Execute a write operation. Only accessible within db/.
    fn with_write<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R>,
    {
        let conn = self.write_conn.lock().unwrap();
        f(&conn).map_err(|e| e.to_string())
    }

    /// Execute a read operation. Only accessible within db/.
    fn with_read<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R>,
    {
        let conn = self.read_conn.lock().unwrap();
        f(&conn).map_err(|e| e.to_string())
    }

    // ── Entity operations ────────────────────────────────────────

    pub fn insert_single(
        &self,
        entity_hash: &str,
        file_id: i64,
        name: Option<&str>,
        status: i64,
        date_created: &str,
        date_added: &str,
    ) -> Result<i64, String> {
        self.with_write(|conn| {
            write::entities::insert_single(conn, entity_hash, file_id, name, status, date_created, date_added)
        })
    }

    pub fn insert_collection(
        &self,
        entity_hash: &str,
        name: &str,
        date_created: &str,
        date_added: &str,
    ) -> Result<i64, String> {
        self.with_write(|conn| {
            write::entities::insert_collection(conn, entity_hash, name, date_created, date_added)
        })
    }

    pub fn set_entity_status(
        &self,
        entity_ids: &[i64],
        status: i64,
        expansion: ExpansionMode,
    ) -> Result<StatusChange, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            write::entities::set_entity_status(conn, entity_ids, status, expansion, &now)
        })
    }

    pub fn delete_entities(&self, entity_ids: &[i64]) -> Result<EntityChange, String> {
        self.with_write(|conn| write::entities::delete_entities(conn, entity_ids))
    }

    // ── File operations ──────────────────────────────────────────

    pub fn insert_file(
        &self,
        file_hash: &str,
        mime_type: &str,
        size_bytes: i64,
        pixel_width: Option<i64>,
        pixel_height: Option<i64>,
        duration_ms: Option<i64>,
        frame_count: Option<i64>,
        has_audio: bool,
        date_added: &str,
    ) -> Result<i64, String> {
        self.with_write(|conn| {
            write::files::insert_file(conn, file_hash, mime_type, size_bytes, pixel_width, pixel_height, duration_ms, frame_count, has_audio, date_added)
        })
    }

    // ── Collection operations ────────────────────────────────────

    pub fn add_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        self.with_write(|conn| write::collections::add_members(conn, collection_id, member_entity_ids))
    }

    pub fn remove_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        self.with_write(|conn| write::collections::remove_members(conn, collection_id, member_entity_ids))
    }

    pub fn split_collection(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        self.with_write(|conn| write::collections::split_collection(conn, collection_id))
    }

    // ── Tag operations ───────────────────────────────────────────

    pub fn add_tags(
        &self,
        entity_ids: &[i64],
        tag_strings: &[String],
        expansion: ExpansionMode,
    ) -> Result<TagChange, String> {
        self.with_write(|conn| write::tags::add_tags(conn, entity_ids, tag_strings, expansion))
    }

    pub fn remove_tags(
        &self,
        entity_ids: &[i64],
        tag_strings: &[String],
        expansion: ExpansionMode,
    ) -> Result<TagChange, String> {
        self.with_write(|conn| write::tags::remove_tags(conn, entity_ids, tag_strings, expansion))
    }

    pub fn rename_tag(&self, tag_id: i64, new_name: &str) -> Result<Option<i64>, String> {
        self.with_write(|conn| write::tags::rename_tag(conn, tag_id, new_name))
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<Vec<i64>, String> {
        self.with_write(|conn| write::tags::delete_tag(conn, tag_id))
    }

    pub fn merge_tags(&self, from_tag_id: i64, to_tag_id: i64) -> Result<Vec<i64>, String> {
        self.with_write(|conn| write::tags::merge_tags(conn, from_tag_id, to_tag_id))
    }

    // ── Folder operations ────────────────────────────────────────

    pub fn create_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        icon: Option<&str>,
        color: Option<&str>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| write::folders::create_folder(conn, name, parent_id, icon, color, &now))
    }

    pub fn delete_folder(&self, folder_id: i64) -> Result<(), String> {
        self.with_write(|conn| write::folders::delete_folder(conn, folder_id))
    }

    pub fn add_folder_members(
        &self,
        folder_id: i64,
        entity_ids: &[i64],
        expansion: ExpansionMode,
    ) -> Result<FolderMembershipChange, String> {
        self.with_write(|conn| write::folders::add_members(conn, folder_id, entity_ids, expansion))
    }

    pub fn remove_folder_members(
        &self,
        folder_id: i64,
        entity_ids: &[i64],
        expansion: ExpansionMode,
    ) -> Result<FolderMembershipChange, String> {
        self.with_write(|conn| write::folders::remove_members(conn, folder_id, entity_ids, expansion))
    }

    // ── Smart folder operations ──────────────────────────────────

    pub fn create_smart_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        predicate_json: &str,
        icon: Option<&str>,
        color: Option<&str>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            write::smart_folders::create_smart_folder(conn, name, parent_id, predicate_json, icon, color, &now)
        })
    }

    pub fn delete_smart_folder(&self, smart_folder_id: i64) -> Result<(), String> {
        self.with_write(|conn| write::smart_folders::delete_smart_folder(conn, smart_folder_id))
    }

    // ── Query operations ─────────────────────────────────────────

    /// Single entry point for all grid queries. Routes by scope, applies filters.
    /// Pre-resolves bitmap-backed scopes (SmartFolder) before passing to the query builder.
    pub fn query_entity_view(
        &self,
        view_query: &types::EntityViewQuery,
    ) -> Result<types::EntityViewPage, String> {
        // Pre-resolve SmartFolder bitmap to entity_ids (doesn't need DB connection)
        let preresolved = match view_query.base_scope.kind {
            types::ScopeKind::SmartFolder => {
                let sf_id = view_query.base_scope.id.unwrap_or(0);
                let bitmap = self.bitmaps.get(
                    &projection::bitmaps::BitmapKey::SmartFolder(sf_id),
                );
                Some(bitmap.iter().map(|id| id as i64).collect::<Vec<_>>())
            }
            _ => None,
        };
        self.with_read(|conn| {
            query::grid::query_entity_view(conn, view_query, preresolved.as_deref())
        })
    }

    pub fn get_entity_details(
        &self,
        entity_hash: &str,
    ) -> Result<Option<types::EntityDetails>, String> {
        self.with_read(|conn| query::details::get_entity_details(conn, entity_hash))
    }

    pub fn get_sidebar_tree(&self) -> Result<Vec<query::sidebar::SidebarNode>, String> {
        self.with_read(|conn| query::sidebar::get_sidebar_tree(conn))
    }

    pub fn get_scope_counts(&self) -> Result<query::stats::ScopeCounts, String> {
        self.with_read(|conn| query::stats::get_scope_counts(conn))
    }

    pub fn search_tags(
        &self,
        query_str: &str,
        limit: i64,
    ) -> Result<Vec<query::search::TagSearchResult>, String> {
        self.with_read(|conn| query::search::search_tags(conn, query_str, limit))
    }

    // ── Projection operations ────────────────────────────────────

    /// Run a compiler plan (called after write operations).
    pub fn run_compiler(&self, plan: projection::compiler::CompilerPlan) {
        let conn = self.read_conn.lock().unwrap();
        projection::compiler::execute_plan(&conn, &self.bitmaps, &plan);
    }

    /// Full rebuild of all projections from authoritative data.
    pub fn full_rebuild(&self) {
        let conn = self.read_conn.lock().unwrap();
        projection::compiler::full_rebuild(&conn, &self.bitmaps);
    }

    /// Flush pending bitmap deltas to the delta log file.
    pub fn flush_bitmap_deltas(&self) -> Result<usize, String> {
        let delta_path = self.library_root.join("bitmaps.delta");
        projection::bitmap_delta::flush_deltas(&delta_path, &self.bitmaps)
            .map_err(|e| format!("Failed to flush bitmap deltas: {e}"))
    }

    /// Get bitmap length for a key (used by sidebar counts).
    pub fn bitmap_len(&self, key: &projection::bitmaps::BitmapKey) -> u64 {
        self.bitmaps.len(key)
    }
}
