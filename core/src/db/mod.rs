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

fn has_column(conn: &Connection, table: &str, column: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

fn reconcile_open_schema(conn: &Connection) -> Result<(), String> {
    if has_column(conn, "folder", "notes").map_err(|e| e.to_string())? == false {
        conn.execute_batch("ALTER TABLE folder ADD COLUMN notes TEXT")
            .map_err(|e| format!("Failed to add folder.notes to canonical db: {e}"))?;
    }
    if has_column(conn, "smart_folder", "notes").map_err(|e| e.to_string())? == false {
        conn.execute_batch("ALTER TABLE smart_folder ADD COLUMN notes TEXT")
            .map_err(|e| format!("Failed to add smart_folder.notes to canonical db: {e}"))?;
    }
    Ok(())
}

/// Import legacy data from an old SqliteDatabase file via ATTACH.
/// Fatal — returns Err if the import fails.
fn import_from_legacy_db(conn: &Connection, old_db_path: &Path) -> Result<(), String> {
    tracing::info!("Importing from legacy database at {}", old_db_path.display());
    let old_db_str = old_db_path.to_string_lossy().to_string();

    conn.execute("ATTACH DATABASE ?1 AS old_db", rusqlite::params![old_db_str])
        .map_err(|e| format!("Failed to attach legacy database: {e}"))?;

    let result = migration_legacy::migrate_from_attached(conn);
    let _ = conn.execute("DETACH DATABASE old_db", []);

    match result {
        Ok(r) => {
            tracing::info!("Legacy import complete: {r}");
            Ok(())
        }
        Err(e) => Err(format!(
            "Failed to import legacy data from {}: {e}. \
             The library cannot open with an empty canonical database while legacy data exists.",
            old_db_path.display()
        )),
    }
}

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

        // Bootstrap: migrate, import, or load existing schema
        {
            let conn = db.write_conn.lock().unwrap();
            reconcile_open_schema(&conn)?;
            let old_db_path = library_root.join("db").join("library.sqlite");
            let legacy_exists = old_db_path.exists();

            if migration_legacy::needs_migration(&conn) {
                // In-place migration (old tables exist in library.db itself)
                tracing::info!("Legacy schema detected in library.db, running in-place migration...");
                let result = migration_legacy::migrate(&conn)?;
                tracing::info!("{result}");
                projection::compiler::full_rebuild(&conn, &db.bitmaps);
                tracing::info!("Post-migration projection rebuild complete");

            } else if !migration_legacy::is_new_schema(&conn) {
                // Fresh library.db — create schema and import from legacy if it exists
                conn.execute_batch(core::schema::LIBRARY_DDL)
                    .map_err(|e| format!("Failed to create schema: {e}"))?;

                if legacy_exists {
                    import_from_legacy_db(&conn, &old_db_path)?;
                }

                projection::compiler::full_rebuild(&conn, &db.bitmaps);

            } else {
                // Existing new-schema library.db — check for empty-with-legacy-data
                let canonical_count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM media_entity", [], |r| r.get(0))
                    .unwrap_or(0);

                if canonical_count == 0 && legacy_exists {
                    // New schema exists but is empty while legacy data exists.
                    // This means a previous import failed or was skipped. Repair.
                    tracing::warn!(
                        "Canonical DB has new schema but 0 entities while legacy DB exists. Repairing..."
                    );
                    import_from_legacy_db(&conn, &old_db_path)?;
                    projection::compiler::full_rebuild(&conn, &db.bitmaps);
                } else {
                    // Normal startup — load bitmaps
                    let delta_path = library_root.join("bitmaps.delta");
                    let replayed = projection::bitmap_delta::replay_deltas(&delta_path, &db.bitmaps)
                        .unwrap_or(0);
                    if replayed == 0 {
                        let conn_r = db.read_conn.lock().unwrap();
                        projection::compiler::full_rebuild(&conn_r, &db.bitmaps);
                    }
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

    pub fn reorder_collection_members(
        &self,
        collection_id: i64,
        ordered_entity_ids: &[i64],
    ) -> Result<(), String> {
        let ids = ordered_entity_ids.to_vec();
        self.with_write(move |conn| write::collections::reorder_members(conn, collection_id, &ids))
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

    pub fn update_folder(
        &self,
        folder_id: i64,
        name: Option<&str>,
        icon: Option<&str>,
        color: Option<&str>,
        auto_tags: Option<&str>,
        notes: Option<&str>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let n = name.map(str::to_string);
        let i = icon.map(str::to_string);
        let c = color.map(str::to_string);
        let a = auto_tags.map(str::to_string);
        let notes = notes.map(str::to_string);
        self.with_write(move |conn| {
            write::folders::update_folder(
                conn,
                folder_id,
                n.as_deref(),
                i.as_deref(),
                c.as_deref(),
                a.as_deref(),
                notes.as_deref(),
                &now,
            )
        })
    }

    pub fn delete_folder(&self, folder_id: i64) -> Result<(), String> {
        self.with_write(|conn| write::folders::delete_folder(conn, folder_id))
    }

    pub fn upsert_folder_record(&self, record: &FolderMirrorRecord) -> Result<(), String> {
        let record = record.clone();
        self.with_write(move |conn| write::folders::upsert_folder_record(conn, &record))
    }

    pub fn move_folder(&self, folder_id: i64, new_parent_id: Option<i64>) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| write::folders::move_folder(conn, folder_id, new_parent_id, &now))
    }

    pub fn reorder_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::folders::reorder_folders(conn, &m))
    }

    pub fn reorder_folder_items(&self, folder_id: i64, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::folders::reorder_members(conn, folder_id, &m))
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
        notes: Option<&str>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            write::smart_folders::create_smart_folder(
                conn,
                name,
                parent_id,
                predicate_json,
                icon,
                color,
                notes,
                &now,
            )
        })
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: i64,
        name: Option<&str>,
        predicate_json: Option<&str>,
        icon: Option<&str>,
        color: Option<&str>,
        notes: Option<&str>,
        sort_field: Option<&str>,
        sort_order: Option<&str>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let n = name.map(str::to_string);
        let p = predicate_json.map(str::to_string);
        let i = icon.map(str::to_string);
        let c = color.map(str::to_string);
        let notes = notes.map(str::to_string);
        let sf = sort_field.map(str::to_string);
        let so = sort_order.map(str::to_string);
        self.with_write(move |conn| {
            write::smart_folders::update_smart_folder(
                conn, smart_folder_id,
                n.as_deref(), p.as_deref(), i.as_deref(), c.as_deref(), notes.as_deref(),
                sf.as_deref(), so.as_deref(), &now,
            )
        })
    }

    pub fn delete_smart_folder(&self, smart_folder_id: i64) -> Result<(), String> {
        self.with_write(|conn| write::smart_folders::delete_smart_folder(conn, smart_folder_id))
    }

    pub fn upsert_smart_folder_record(&self, record: &SmartFolderMirrorRecord) -> Result<(), String> {
        let record = record.clone();
        self.with_write(move |conn| write::smart_folders::upsert_smart_folder_record(conn, &record))
    }

    pub fn move_smart_folder(&self, smart_folder_id: i64, new_parent_id: Option<i64>) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| write::smart_folders::move_smart_folder(conn, smart_folder_id, new_parent_id, &now))
    }

    pub fn reorder_smart_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::smart_folders::reorder_smart_folders(conn, &m))
    }

    // ── Bulk target operations (for engine query_results targets) ──

    pub fn resolve_entity_hashes(&self, hashes: &[String]) -> Result<Vec<i64>, String> {
        self.with_read(|conn| {
            let mut ids = Vec::with_capacity(hashes.len());
            let mut stmt = conn.prepare_cached(
                "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
            )?;
            for hash in hashes {
                if let Ok(id) = stmt.query_row([hash], |row| row.get::<_, i64>(0)) {
                    ids.push(id);
                }
            }
            Ok(ids)
        })
    }

    pub fn get_entity_hashes_by_ids(&self, ids: &[i64]) -> Result<Vec<String>, String> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        self.with_read(|conn| {
            let placeholders: Vec<String> = (1..=ids.len()).map(|i| format!("?{i}")).collect();
            let sql = format!(
                "SELECT entity_hash FROM media_entity WHERE entity_id IN ({})",
                placeholders.join(",")
            );
            let mut stmt = conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> =
                ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
            let rows = stmt.query_map(params.as_slice(), |row| row.get::<_, String>(0))?;
            rows.collect()
        })
    }

    pub fn get_entity_grid_items(
        &self,
        entity_hashes: &[String],
    ) -> Result<Vec<types::EntityGridItem>, String> {
        let hashes = entity_hashes.to_vec();
        self.with_read(|conn| query::grid::get_entity_grid_items_by_hash(conn, &hashes))
    }

    pub fn patch_entity_metadata(
        &self,
        entity_ids: &[i64],
        patch: &types::MediaEntityPatch,
    ) -> Result<types::EntityChange, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            write::entities::patch_entity_metadata(
                conn,
                entity_ids,
                patch.name.as_deref(),
                patch.rating.map(Some),
                patch.notes.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()).as_deref(),
                patch.source_urls.as_ref().map(|urls| serde_json::to_string(urls).unwrap_or_default()).as_deref(),
                &now,
                types::ExpansionMode::EntityAndDescendants,
            )
        })
    }

    pub fn patch_entity_metadata_bulk(
        &self,
        query: &types::EntityViewQuery,
        exclusions: &[String],
        patch: &types::MediaEntityPatch,
    ) -> Result<types::EntityChange, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let q = query.clone();
        let excl = exclusions.to_vec();
        let p = patch.clone();
        let bm = self.bitmaps.clone();
        self.with_write(move |conn| {
            write::bulk::populate_bulk_target(conn, &q, &excl, &bm)?;
            write::bulk::expand_bulk_target(conn, types::ExpansionMode::EntityAndDescendants)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            write::entities::patch_entity_metadata(
                conn, &ids,
                p.name.as_deref(), p.rating.map(Some),
                p.notes.as_ref().map(|v| serde_json::to_string(v).unwrap_or_default()).as_deref(),
                p.source_urls.as_ref().map(|u| serde_json::to_string(u).unwrap_or_default()).as_deref(),
                &now, types::ExpansionMode::EntityOnly,
            )
        })
    }

    pub fn set_entity_status_bulk(
        &self,
        query: &types::EntityViewQuery,
        exclusions: &[String],
        status: i64,
    ) -> Result<types::StatusChange, String> {
        let now = chrono::Utc::now().to_rfc3339();
        let q = query.clone();
        let excl = exclusions.to_vec();
        let bm = self.bitmaps.clone();
        self.with_write(move |conn| {
            write::bulk::populate_bulk_target(conn, &q, &excl, &bm)?;
            write::bulk::expand_bulk_target(conn, types::ExpansionMode::EntityAndDescendants)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            write::entities::set_entity_status(conn, &ids, status, types::ExpansionMode::EntityOnly, &now)
        })
    }

    pub fn delete_entities_bulk(
        &self,
        query: &types::EntityViewQuery,
        exclusions: &[String],
    ) -> Result<types::EntityChange, String> {
        let q = query.clone();
        let excl = exclusions.to_vec();
        let bm = self.bitmaps.clone();
        self.with_write(move |conn| {
            write::bulk::populate_bulk_target(conn, &q, &excl, &bm)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            write::entities::delete_entities(conn, &ids)
        })
    }

    pub fn add_tags_bulk(
        &self,
        query: &types::EntityViewQuery,
        exclusions: &[String],
        tags: &[String],
        expansion: types::ExpansionMode,
    ) -> Result<types::TagChange, String> {
        let q = query.clone();
        let excl = exclusions.to_vec();
        let t = tags.to_vec();
        let bm = self.bitmaps.clone();
        self.with_write(move |conn| {
            write::bulk::populate_bulk_target(conn, &q, &excl, &bm)?;
            write::bulk::expand_bulk_target(conn, expansion)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            write::tags::add_tags(conn, &ids, &t, types::ExpansionMode::EntityOnly)
        })
    }

    pub fn remove_tags_bulk(
        &self,
        query: &types::EntityViewQuery,
        exclusions: &[String],
        tags: &[String],
        expansion: types::ExpansionMode,
    ) -> Result<types::TagChange, String> {
        let q = query.clone();
        let excl = exclusions.to_vec();
        let t = tags.to_vec();
        let bm = self.bitmaps.clone();
        self.with_write(move |conn| {
            write::bulk::populate_bulk_target(conn, &q, &excl, &bm)?;
            write::bulk::expand_bulk_target(conn, expansion)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            write::tags::remove_tags(conn, &ids, &t, types::ExpansionMode::EntityOnly)
        })
    }

    pub fn add_folder_members_bulk(
        &self,
        folder_id: i64,
        query: &types::EntityViewQuery,
        exclusions: &[String],
        expansion: types::ExpansionMode,
    ) -> Result<types::FolderMembershipChange, String> {
        let q = query.clone();
        let excl = exclusions.to_vec();
        let bm = self.bitmaps.clone();
        self.with_write(move |conn| {
            write::bulk::populate_bulk_target(conn, &q, &excl, &bm)?;
            write::bulk::expand_bulk_target(conn, expansion)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            write::folders::add_members(conn, folder_id, &ids, types::ExpansionMode::EntityOnly)
        })
    }

    pub fn remove_folder_members_bulk(
        &self,
        folder_id: i64,
        query: &types::EntityViewQuery,
        exclusions: &[String],
        expansion: types::ExpansionMode,
    ) -> Result<types::FolderMembershipChange, String> {
        let q = query.clone();
        let excl = exclusions.to_vec();
        let bm = self.bitmaps.clone();
        self.with_write(move |conn| {
            write::bulk::populate_bulk_target(conn, &q, &excl, &bm)?;
            write::bulk::expand_bulk_target(conn, expansion)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            write::folders::remove_members(conn, folder_id, &ids, types::ExpansionMode::EntityOnly)
        })
    }

    pub fn get_selection_summary_from_query(
        &self,
        query: &types::EntityViewQuery,
        exclusions: &[String],
    ) -> Result<crate::engine::selection::SelectionSummary, String> {
        // Use the same query builder as the grid with no pagination
        // to get an accurate total count including all filters.
        let mut unbounded = query.clone();
        unbounded.page = types::QueryPage { limit: i64::MAX, cursor: None };

        let result = self.query_entity_view(&unbounded)?;
        let total = result.total_count.unwrap_or(result.items.len() as i64);

        // Subtract exclusions from the full set
        let excluded_count = if exclusions.is_empty() {
            0
        } else {
            let excl_set: std::collections::HashSet<&str> =
                exclusions.iter().map(|s| s.as_str()).collect();
            result.items.iter().filter(|i| excl_set.contains(i.entity_hash.as_str())).count() as i64
        };

        Ok(crate::engine::selection::SelectionSummary {
            total_count: total - excluded_count,
            entity_hashes: Vec::new(),
        })
    }

    // ── Deferred work ────────────────────────────────────────────

    pub fn get_deferred_work_summary(
        &self,
    ) -> Result<crate::engine::deferred::DeferredWorkSummary, String> {
        self.with_read(|conn| {
            let pending: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item WHERE status = 'pending'",
                [],
                |r| r.get(0),
            )?;
            let running: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item WHERE status = 'running'",
                [],
                |r| r.get(0),
            )?;
            let failed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item WHERE status = 'pending' AND attempt_count > 0",
                [],
                |r| r.get(0),
            )?;
            Ok(crate::engine::deferred::DeferredWorkSummary {
                pending_count: pending,
                running_count: running,
                failed_count: failed,
            })
        })
    }

    pub fn retry_deferred_work(&self, entity_hash: &str) -> Result<(), String> {
        let h = entity_hash.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            conn.execute(
                "UPDATE deferred_work_item
                 SET status = 'pending', attempt_count = 0, available_at = ?1, last_error = NULL
                 WHERE entity_hash = ?2",
                rusqlite::params![now, h],
            )?;
            Ok(())
        })
    }

    /// Get the entity_hash of a collection's primary member (first by ordinal).
    pub fn get_primary_member_hash(&self, collection_hash: &str) -> Result<Option<String>, String> {
        let h = collection_hash.to_string();
        self.with_read(|conn| {
            use rusqlite::OptionalExtension;
            conn.query_row(
                "SELECT pm.entity_hash FROM media_entity me
                 JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
                 WHERE me.entity_hash = ?1 AND me.entity_kind = 'collection'",
                [&h],
                |row| row.get(0),
            )
            .optional()
        })
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
