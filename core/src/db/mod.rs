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

use rusqlite::{params, Connection, OptionalExtension};

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
    if has_column(conn, "tag", "site_mask").map_err(|e| e.to_string())? == false {
        conn.execute_batch("ALTER TABLE tag ADD COLUMN site_mask INTEGER NOT NULL DEFAULT 0")
            .map_err(|e| format!("Failed to add tag.site_mask to canonical db: {e}"))?;
    }
    if has_column(conn, "entity_tag", "provenance_mask").map_err(|e| e.to_string())? == false {
        conn.execute_batch(
            "ALTER TABLE entity_tag ADD COLUMN provenance_mask INTEGER NOT NULL DEFAULT 0",
        )
        .map_err(|e| format!("Failed to add entity_tag.provenance_mask to canonical db: {e}"))?;
        conn.execute(
            "UPDATE entity_tag SET provenance_mask = ?1 WHERE source = 'local' AND provenance_mask = 0",
            [types::mask_to_db_bits(types::TAG_PROVENANCE_MANUAL)],
        )
        .map_err(|e| format!("Failed to backfill entity_tag.provenance_mask: {e}"))?;
    }
    conn.execute_batch(
        "DROP TABLE IF EXISTS rejected_fingerprint;
         DROP TABLE IF EXISTS rejected_media;",
    )
    .map_err(|e| format!("Failed to remove rejected-media schema: {e}"))?;
    Ok(())
}

/// Import legacy data from an old SqliteDatabase file via ATTACH.
/// Fatal — returns Err if the import fails.
fn import_from_legacy_db(conn: &Connection, old_db_path: &Path) -> Result<(), String> {
    tracing::info!(
        "Importing from legacy database at {}",
        old_db_path.display()
    );
    let old_db_str = old_db_path.to_string_lossy().to_string();

    conn.execute(
        "ATTACH DATABASE ?1 AS old_db",
        rusqlite::params![old_db_str],
    )
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
        write_conn
            .execute_batch(
                "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
            )
            .map_err(|e| format!("Failed to configure write connection: {e}"))?;
        read_conn
            .execute_batch(
                "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
            )
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
            let old_db_path = library_root.join("db").join("library.sqlite");
            let legacy_exists = old_db_path.exists();

            if migration_legacy::needs_migration(&conn) {
                // In-place migration (old tables exist in library.db itself)
                tracing::info!(
                    "Legacy schema detected in library.db, running in-place migration..."
                );
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
                    let replayed =
                        projection::bitmap_delta::replay_deltas(&delta_path, &db.bitmaps)
                            .unwrap_or(0);
                    if replayed == 0 {
                        let conn_r = db.read_conn.lock().unwrap();
                        projection::compiler::full_rebuild(&conn_r, &db.bitmaps);
                    }
                }
            }

            // Reconcile schema: add columns that may be missing from older schema versions
            reconcile_open_schema(&conn)?;
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
            write::entities::insert_single(
                conn,
                entity_hash,
                file_id,
                name,
                status,
                date_created,
                date_added,
            )
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
            write::files::insert_file(
                conn,
                file_hash,
                mime_type,
                size_bytes,
                pixel_width,
                pixel_height,
                duration_ms,
                frame_count,
                has_audio,
                date_added,
            )
        })
    }

    pub fn get_existing_import_target_by_file_hash(
        &self,
        file_hash: &str,
    ) -> Result<Option<query::ingest::ExistingImportTarget>, String> {
        let hash = file_hash.to_string();
        self.with_read(|conn| query::ingest::get_existing_import_target_by_file_hash(conn, &hash))
    }

    pub fn get_existing_import_target_by_file_hash_write(
        &self,
        file_hash: &str,
    ) -> Result<Option<query::ingest::ExistingImportTarget>, String> {
        let hash = file_hash.to_string();
        self.with_write(|conn| query::ingest::get_existing_import_target_by_file_hash(conn, &hash))
    }

    pub fn get_existing_import_target_by_entity_hash(
        &self,
        entity_hash: &str,
    ) -> Result<Option<query::ingest::ExistingImportTarget>, String> {
        let hash = entity_hash.to_string();
        self.with_read(|conn| query::ingest::get_existing_import_target_by_entity_hash(conn, &hash))
    }

    pub fn get_derivative_target_by_entity_hash(
        &self,
        entity_hash: &str,
    ) -> Result<Option<query::ingest::DerivativeTarget>, String> {
        let hash = entity_hash.to_string();
        self.with_read(|conn| query::ingest::get_derivative_target_by_entity_hash(conn, &hash))
    }

    pub fn find_perceptual_hash_candidates(
        &self,
        perceptual_hash: &str,
        threshold: u32,
    ) -> Result<Vec<types::PerceptualHashCandidate>, String> {
        let perceptual_hash = perceptual_hash.to_string();
        self.with_read(move |conn| {
            use img_hash::ImageHash;

            let source = match ImageHash::<Vec<u8>>::from_base64(&perceptual_hash) {
                Ok(value) => value,
                Err(_) => return Ok(Vec::new()),
            };

            let mut stmt = conn.prepare(
                "SELECT
                     mf.file_id,
                     me.entity_id,
                     me.entity_hash,
                     mf.file_hash,
                     mf.mime_type,
                     mf.size_bytes,
                     mf.pixel_width,
                     mf.pixel_height,
                     mf.frame_count,
                     mf.perceptual_hash
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE mf.perceptual_hash IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?;

            let mut candidates = Vec::new();
            for row in rows {
                let (
                    file_id,
                    entity_id,
                    entity_hash,
                    file_hash,
                    mime_type,
                    size_bytes,
                    pixel_width,
                    pixel_height,
                    frame_count,
                    candidate_phash,
                ) = row?;
                if !crate::media_capabilities::capabilities_for_stored_media(
                    &mime_type,
                    frame_count,
                )
                .can_perceptual_hash
                {
                    continue;
                }
                let Ok(candidate_hash) = ImageHash::<Vec<u8>>::from_base64(&candidate_phash) else {
                    continue;
                };
                let distance = source.dist(&candidate_hash);
                if distance <= threshold {
                    candidates.push(types::PerceptualHashCandidate {
                        file_id,
                        entity_id,
                        entity_hash,
                        file_hash,
                        mime_type,
                        size_bytes,
                        pixel_width,
                        pixel_height,
                        frame_count,
                        perceptual_hash: candidate_phash,
                        distance,
                    });
                }
            }

            candidates.sort_by(|left, right| {
                left.distance
                    .cmp(&right.distance)
                    .then_with(|| left.entity_hash.cmp(&right.entity_hash))
            });
            Ok(candidates)
        })
    }

    pub fn upsert_duplicate_pair_for_review(
        &self,
        file_id_a: i64,
        file_id_b: i64,
        distance: u32,
    ) -> Result<(), String> {
        let (file_id_a, file_id_b) = if file_id_a < file_id_b {
            (file_id_a, file_id_b)
        } else {
            (file_id_b, file_id_a)
        };
        self.with_write(move |conn| {
            conn.execute(
                "INSERT INTO duplicate (
                     file_id_a, file_id_b, distance, status, decision_at, decision_source, decision_reason, winner_file_id, loser_file_id
                 ) VALUES (?1, ?2, ?3, 'detected', NULL, NULL, NULL, NULL, NULL)
                 ON CONFLICT(file_id_a, file_id_b) DO UPDATE SET
                     distance = excluded.distance,
                     status = 'detected',
                     decision_at = NULL,
                     decision_source = NULL,
                     decision_reason = NULL,
                     winner_file_id = NULL,
                     loser_file_id = NULL",
                params![file_id_a, file_id_b, distance as i64],
            )?;
            Ok(())
        })
    }

    pub fn insert_ingested_single(
        &self,
        prepared: &types::IngestPreparedSingle,
    ) -> Result<i64, String> {
        let prepared = prepared.clone();
        self.with_write(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let source_urls_json = if prepared.source_urls.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&prepared.source_urls).unwrap_or_default())
            };
            // Clean up any orphan media_file left by a deleted entity before
            // inserting, so we don't hit a UNIQUE constraint on file_hash.
            tx.execute(
                "DELETE FROM media_file WHERE file_hash = ?1
                 AND file_id NOT IN (SELECT file_id FROM single_media_entity)",
                rusqlite::params![prepared.entity_hash],
            )?;
            let file_id = write::files::insert_file(
                &tx,
                &prepared.entity_hash,
                &prepared.mime_type,
                prepared.size_bytes,
                prepared.pixel_width,
                prepared.pixel_height,
                prepared.duration_ms,
                prepared.frame_count,
                prepared.has_audio,
                &prepared.date_added,
            )?;
            if let Some(phash) = prepared.perceptual_hash.as_deref() {
                write::files::replace_file_phash(&tx, file_id, Some(phash))?;
            }
            let entity_id = write::entities::insert_single(
                &tx,
                &prepared.entity_hash,
                file_id,
                prepared.name.as_deref(),
                prepared.status,
                &prepared.date_created,
                &prepared.date_added,
            )?;
            if prepared.notes.is_some() || !prepared.source_urls.is_empty() {
                write::entities::patch_entity_metadata(
                    &tx,
                    &[entity_id],
                    None,
                    None,
                    prepared.notes.as_deref().map(Some),
                    source_urls_json.as_deref(),
                    &prepared.date_added,
                    types::ExpansionMode::EntityOnly,
                )?;
            }
            if !prepared.tag_strings.is_empty() {
                write::tags::add_tags(
                    &tx,
                    &[entity_id],
                    &prepared.tag_strings,
                    prepared.tag_provenance_mask,
                    types::ExpansionMode::EntityOnly,
                )?;
            }
            tx.commit()?;
            Ok(entity_id)
        })
    }

    pub fn materialize_ingested_collection(
        &self,
        name: &str,
        new_members: &[types::IngestPreparedSingle],
        existing_member_ids: &[i64],
        existing_collection_id: Option<i64>,
        force_collection: bool,
    ) -> Result<(i64, String, Vec<String>), String> {
        let collection_name = name.to_string();
        let prepared = new_members.to_vec();
        let existing_ids = existing_member_ids.to_vec();
        self.with_write(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let mut member_ids = existing_ids;
            let mut new_hashes = Vec::with_capacity(prepared.len());

            for member in &prepared {
                let source_urls_json = if member.source_urls.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&member.source_urls).unwrap_or_default())
                };
                // Clean up any orphan media_file left by a deleted entity before
                // inserting, so we don't hit a UNIQUE constraint on file_hash.
                tx.execute(
                    "DELETE FROM media_file WHERE file_hash = ?1
                     AND file_id NOT IN (SELECT file_id FROM single_media_entity)",
                    rusqlite::params![member.entity_hash],
                )?;
                let file_id = write::files::insert_file(
                    &tx,
                    &member.entity_hash,
                    &member.mime_type,
                    member.size_bytes,
                    member.pixel_width,
                    member.pixel_height,
                    member.duration_ms,
                    member.frame_count,
                    member.has_audio,
                    &member.date_added,
                )?;
                let entity_id = write::entities::insert_single(
                    &tx,
                    &member.entity_hash,
                    file_id,
                    member.name.as_deref(),
                    member.status,
                    &member.date_created,
                    &member.date_added,
                )?;
                if member.notes.is_some() || !member.source_urls.is_empty() {
                    write::entities::patch_entity_metadata(
                        &tx,
                        &[entity_id],
                        None,
                        None,
                        member.notes.as_deref().map(Some),
                        source_urls_json.as_deref(),
                        &member.date_added,
                        types::ExpansionMode::EntityOnly,
                    )?;
                }
                if !member.tag_strings.is_empty() {
                    write::tags::add_tags(
                        &tx,
                        &[entity_id],
                        &member.tag_strings,
                        member.tag_provenance_mask,
                        types::ExpansionMode::EntityOnly,
                    )?;
                }
                member_ids.push(entity_id);
                new_hashes.push(member.entity_hash.clone());
            }

            if existing_collection_id.is_none() && member_ids.len() < 2 && !force_collection {
                return Err(rusqlite::Error::InvalidQuery);
            }

            let collection_id = if let Some(collection_id) = existing_collection_id {
                if !member_ids.is_empty() {
                    write::collections::add_members(&tx, collection_id, &member_ids)?;
                }
                collection_id
            } else {
                let now = chrono::Utc::now().to_rfc3339();
                let collection_id =
                    write::collections::create_collection(&tx, &collection_name, &now)?;
                if !member_ids.is_empty() {
                    write::collections::add_members(&tx, collection_id, &member_ids)?;
                }
                collection_id
            };

            let collection_hash = query::collections::get_collection_hash(&tx, collection_id)?
                .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
            tx.commit()?;
            Ok((collection_id, collection_hash, new_hashes))
        })
    }

    // ── Collection operations ────────────────────────────────────

    pub fn add_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        self.with_write(|conn| {
            write::collections::add_members(conn, collection_id, member_entity_ids)
        })
    }

    pub fn create_collection(&self, name: &str) -> Result<i64, String> {
        let n = name.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| write::collections::create_collection(conn, &n, &now))
    }

    pub fn update_collection_name(&self, collection_id: i64, name: &str) -> Result<(), String> {
        let n = name.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            write::collections::update_collection_name(conn, collection_id, &n, &now)
        })
    }

    pub fn remove_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        self.with_write(|conn| {
            write::collections::remove_members(conn, collection_id, member_entity_ids)
        })
    }

    pub fn reorder_collection_members(
        &self,
        collection_id: i64,
        ordered_entity_ids: &[i64],
    ) -> Result<(), String> {
        let ids = ordered_entity_ids.to_vec();
        self.with_write(move |conn| write::collections::reorder_members(conn, collection_id, &ids))
    }

    pub fn reorder_collection_members_by_hashes(
        &self,
        collection_id: i64,
        ordered_hashes: &[String],
    ) -> Result<(), String> {
        let hashes = ordered_hashes.to_vec();
        self.with_write(move |conn| {
            let current_rows =
                query::collections::list_collection_member_hash_rows(conn, collection_id)?;
            if current_rows.is_empty() {
                return Ok(());
            }

            let mut by_hash = std::collections::HashMap::<String, i64>::new();
            let mut current_hash_order = Vec::with_capacity(current_rows.len());
            for (entity_id, hash) in current_rows {
                by_hash.insert(hash.clone(), entity_id);
                current_hash_order.push(hash);
            }

            let mut seen = std::collections::HashSet::<String>::new();
            let mut final_order = Vec::with_capacity(current_hash_order.len());
            for hash in &hashes {
                if by_hash.contains_key(hash) && seen.insert(hash.clone()) {
                    final_order.push(*by_hash.get(hash).unwrap());
                }
            }
            for hash in current_hash_order {
                if seen.insert(hash.clone()) {
                    final_order.push(*by_hash.get(&hash).unwrap());
                }
            }

            write::collections::reorder_members(conn, collection_id, &final_order)
        })
    }

    pub fn add_collection_members_by_hashes(
        &self,
        collection_id: i64,
        hashes: &[String],
    ) -> Result<CollectionMembershipChange, String> {
        let ids = self.resolve_entity_hashes(hashes)?;
        self.add_collection_members(collection_id, &ids)
    }

    pub fn remove_collection_members_by_hashes(
        &self,
        collection_id: i64,
        hashes: &[String],
    ) -> Result<CollectionMembershipChange, String> {
        let ids = self.resolve_entity_hashes(hashes)?;
        self.remove_collection_members(collection_id, &ids)
    }

    pub fn delete_collection(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        self.with_write(|conn| write::collections::delete_collection(conn, collection_id))
    }

    pub fn split_collection(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        self.with_write(|conn| write::collections::split_collection(conn, collection_id))
    }

    pub fn get_collections(&self) -> Result<Vec<CollectionRecord>, String> {
        self.with_read(query::collections::list_collections)
    }

    pub fn get_collection_summary(&self, collection_id: i64) -> Result<CollectionSummary, String> {
        self.with_read(|conn| query::collections::get_collection_summary(conn, collection_id))
    }

    pub fn list_collection_member_hashes(&self, collection_id: i64) -> Result<Vec<String>, String> {
        self.with_read(|conn| {
            query::collections::list_collection_member_hashes(conn, collection_id)
        })
    }

    pub fn get_collection_hash(&self, collection_id: i64) -> Result<Option<String>, String> {
        self.with_read(|conn| query::collections::get_collection_hash(conn, collection_id))
    }

    pub fn get_collection_folder_ids(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        self.with_read(|conn| query::collections::get_collection_folder_ids(conn, collection_id))
    }

    pub fn get_folder_ids_for_entities(&self, entity_ids: &[i64]) -> Result<Vec<i64>, String> {
        let ids = entity_ids.to_vec();
        self.with_read(|conn| query::collections::get_folder_ids_for_entities(conn, &ids))
    }

    pub fn get_folder_entity_count(&self, folder_id: i64) -> Result<Option<i64>, String> {
        self.with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM folder_member WHERE folder_id = ?1",
                [folder_id],
                |row| row.get(0),
            )
            .optional()
        })
    }

    // ── Tag operations ───────────────────────────────────────────

    pub fn add_tags(
        &self,
        entity_ids: &[i64],
        tag_strings: &[String],
        provenance_mask: u64,
        expansion: ExpansionMode,
    ) -> Result<TagChange, String> {
        self.with_write(|conn| {
            write::tags::add_tags(conn, entity_ids, tag_strings, provenance_mask, expansion)
        })
    }

    pub fn remove_tags(
        &self,
        entity_ids: &[i64],
        tag_strings: &[String],
        expansion: ExpansionMode,
    ) -> Result<TagChange, String> {
        self.with_write(|conn| write::tags::remove_tags(conn, entity_ids, tag_strings, expansion))
    }

    pub fn rename_tag(&self, tag_id: i64, new_name: &str) -> Result<TagStructureChange, String> {
        self.with_write(|conn| write::tags::rename_tag(conn, tag_id, new_name))
    }

    pub fn delete_tag(&self, tag_id: i64) -> Result<TagStructureChange, String> {
        self.with_write(|conn| write::tags::delete_tag(conn, tag_id))
    }

    pub fn merge_tags(
        &self,
        from_tag_id: i64,
        to_tag_id: i64,
    ) -> Result<TagStructureChange, String> {
        self.with_write(|conn| write::tags::merge_tags(conn, from_tag_id, to_tag_id))
    }

    pub fn manage_tag_alias(&self, from_tag_id: i64, to_tag_id: Option<i64>) -> Result<(), String> {
        self.with_write(|conn| write::tags::manage_alias(conn, from_tag_id, to_tag_id))
    }

    pub fn manage_tag_implication(
        &self,
        child_tag_id: i64,
        parent_tag_id: i64,
        add: bool,
    ) -> Result<(), String> {
        self.with_write(|conn| {
            write::tags::manage_implication(conn, child_tag_id, parent_tag_id, add)
        })
    }

    pub fn set_tag_site_mask(&self, tag_id: i64, site_mask: u64) -> Result<(), String> {
        self.with_write(|conn| write::tags::set_tag_site_mask(conn, tag_id, site_mask))
    }

    pub fn ensure_tag(&self, tag_str: &str) -> Result<i64, String> {
        self.with_write(|conn| write::tags::ensure_tag(conn, tag_str))
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
        self.with_write(|conn| {
            write::folders::create_folder(conn, name, parent_id, icon, color, &now)
        })
    }

    pub fn update_folder(&self, folder_id: i64, patch: &types::FolderPatch) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let p = patch.clone();
        self.with_write(move |conn| write::folders::update_folder(conn, folder_id, &p, &now))
    }

    pub fn delete_folder(&self, folder_id: i64) -> Result<(), String> {
        self.with_write(|conn| write::folders::delete_folder(conn, folder_id))
    }

    pub fn move_folder(&self, folder_id: i64, new_parent_id: Option<i64>) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            write::folders::move_folder(conn, folder_id, new_parent_id, &now)
        })
    }

    pub fn reorder_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::folders::reorder_folders(conn, &m))
    }

    pub fn reorder_folder_items(&self, folder_id: i64, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::folders::reorder_members(conn, folder_id, &m))
    }

    pub fn get_folder(&self, folder_id: i64) -> Result<Option<query::folders::FolderRow>, String> {
        self.with_read(|conn| query::folders::get_folder(conn, folder_id))
    }

    pub fn collect_descendant_smart_folder_ids(&self, root_id: i64) -> Result<Vec<i64>, String> {
        self.with_read(|conn| query::folders::collect_descendant_smart_folder_ids(conn, root_id))
    }

    pub fn get_smart_folder(
        &self,
        smart_folder_id: i64,
    ) -> Result<Option<query::folders::SmartFolderRow>, String> {
        self.with_read(|conn| query::folders::get_smart_folder(conn, smart_folder_id))
    }

    pub fn find_child_folder_id(&self, parent_id: i64, name: &str) -> Result<Option<i64>, String> {
        let child_name = name.to_string();
        self.with_read(move |conn| {
            query::ingest::find_child_folder_id(conn, parent_id, &child_name)
        })
    }

    pub fn list_folders_canonical(&self) -> Result<Vec<query::folders::FolderRow>, String> {
        self.with_read(|conn| query::folders::list_folders(conn))
    }

    pub fn list_smart_folders_canonical(
        &self,
    ) -> Result<Vec<query::folders::SmartFolderRow>, String> {
        self.with_read(|conn| query::folders::list_smart_folders(conn))
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
        self.with_write(|conn| {
            write::folders::remove_members(conn, folder_id, entity_ids, expansion)
        })
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
                conn,
                smart_folder_id,
                n.as_deref(),
                p.as_deref(),
                i.as_deref(),
                c.as_deref(),
                notes.as_deref(),
                sf.as_deref(),
                so.as_deref(),
                &now,
            )
        })
    }

    pub fn delete_smart_folder(
        &self,
        smart_folder_id: i64,
    ) -> Result<(Vec<i64>, Option<i64>), String> {
        self.with_write(|conn| write::smart_folders::delete_smart_folder(conn, smart_folder_id))
    }

    pub fn move_smart_folder(
        &self,
        smart_folder_id: i64,
        new_parent_id: Option<i64>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            write::smart_folders::move_smart_folder(conn, smart_folder_id, new_parent_id, &now)
        })
    }

    pub fn reorder_smart_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::smart_folders::reorder_smart_folders(conn, &m))
    }

    // ── Bulk target operations (for engine query_results targets) ──

    pub fn resolve_entity_hashes(&self, hashes: &[String]) -> Result<Vec<i64>, String> {
        self.with_read(|conn| {
            let mut ids = Vec::with_capacity(hashes.len());
            let mut stmt =
                conn.prepare_cached("SELECT entity_id FROM media_entity WHERE entity_hash = ?1")?;
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
            let params: Vec<&dyn rusqlite::types::ToSql> = ids
                .iter()
                .map(|id| id as &dyn rusqlite::types::ToSql)
                .collect();
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
                patch.notes.as_ref().map(|notes| notes.as_deref()),
                patch
                    .source_urls
                    .as_ref()
                    .map(|urls| serde_json::to_string(urls).unwrap_or_default())
                    .as_deref(),
                &now,
                types::ExpansionMode::EntityAndDescendants,
            )
        })
    }

    pub fn set_entity_date_created(
        &self,
        entity_hash: &str,
        date_created: &str,
    ) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let created = date_created.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            let entity_id: i64 = conn.query_row(
                "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
                [&hash],
                |row| row.get(0),
            )?;
            write::entities::set_entity_date_created(conn, entity_id, &created, &now)
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
                conn,
                &ids,
                p.name.as_deref(),
                p.rating.map(Some),
                p.notes.as_ref().map(|notes| notes.as_deref()),
                p.source_urls
                    .as_ref()
                    .map(|u| serde_json::to_string(u).unwrap_or_default())
                    .as_deref(),
                &now,
                types::ExpansionMode::EntityOnly,
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
            write::entities::set_entity_status(
                conn,
                &ids,
                status,
                types::ExpansionMode::EntityOnly,
                &now,
            )
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
        provenance_mask: u64,
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
            write::tags::add_tags(
                conn,
                &ids,
                &t,
                provenance_mask,
                types::ExpansionMode::EntityOnly,
            )
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

    pub fn enqueue_deferred_jobs(
        &self,
        entity_hash: &str,
        work_types: &[crate::background_work::DeferredWorkType],
    ) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let work_types: Vec<String> = work_types
            .iter()
            .map(|work_type| work_type.as_db_str().to_string())
            .collect();
        self.with_write(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let mut stmt = conn.prepare(
                "INSERT INTO deferred_work_item
                     (entity_hash, work_type, status, attempt_count, available_at, queued_at)
                 VALUES
                     (?1, ?2, 'pending', 0, ?3, ?3)
                 ON CONFLICT(entity_hash, work_type) DO UPDATE SET
                     status = 'pending',
                     attempt_count = 0,
                     available_at = excluded.available_at,
                     last_error = NULL,
                     queued_at = excluded.queued_at,
                     started_at = NULL,
                     finished_at = NULL,
                     last_error_at = NULL",
            )?;
            for work_type in &work_types {
                stmt.execute(rusqlite::params![hash, work_type, now])?;
            }
            Ok(())
        })
    }

    pub fn enqueue_deferred_jobs_batch(
        &self,
        items: Vec<(String, Vec<crate::background_work::DeferredWorkType>)>,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            let now = chrono::Utc::now().to_rfc3339();
            let mut stmt = conn.prepare(
                "INSERT INTO deferred_work_item
                     (entity_hash, work_type, status, attempt_count, available_at, queued_at)
                 VALUES
                     (?1, ?2, 'pending', 0, ?3, ?3)
                 ON CONFLICT(entity_hash, work_type) DO UPDATE SET
                     status = 'pending',
                     attempt_count = 0,
                     available_at = excluded.available_at,
                     last_error = NULL,
                     queued_at = excluded.queued_at,
                     started_at = NULL,
                     finished_at = NULL,
                     last_error_at = NULL",
            )?;
            for (entity_hash, work_types) in &items {
                for work_type in work_types {
                    stmt.execute(rusqlite::params![entity_hash, work_type.as_db_str(), now])?;
                }
            }
            Ok(())
        })
    }

    pub fn list_deferred_work_items(
        &self,
        filter: crate::background_work::DeferredWorkFilter,
    ) -> Result<Vec<crate::background_work::DeferredWorkItemInfo>, String> {
        self.with_read(move |conn| {
            let mut sql = String::from(
                "SELECT entity_hash, work_type, status, attempt_count, available_at, queued_at, started_at, finished_at, last_error, last_error_at
                 FROM deferred_work_item",
            );
            let mut conditions = Vec::<String>::new();
            let mut params: Vec<String> = Vec::new();

            if let Some(entity_hash) = &filter.entity_hash {
                conditions.push("entity_hash = ?".to_string());
                params.push(entity_hash.clone());
            }
            if let Some(work_type) = filter.work_type {
                conditions.push("work_type = ?".to_string());
                params.push(work_type.as_db_str().to_string());
            }
            if let Some(status) = filter.status {
                conditions.push("status = ?".to_string());
                params.push(status.as_db_str().to_string());
            }
            if !conditions.is_empty() {
                sql.push_str(" WHERE ");
                sql.push_str(&conditions.join(" AND "));
            }
            sql.push_str(" ORDER BY available_at ASC, work_id ASC");
            let limit = filter.limit.unwrap_or(200).clamp(1, 1000);
            sql.push_str(&format!(" LIMIT {limit}"));

            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt
                .query_map(rusqlite::params_from_iter(params.iter()), |row| {
                    let work_type_raw: String = row.get(1)?;
                    let status_raw: String = row.get(2)?;
                    Ok(crate::background_work::DeferredWorkItemInfo {
                        entity_hash: row.get(0)?,
                        work_type: crate::background_work::DeferredWorkType::from_db_str(&work_type_raw)
                            .ok_or(rusqlite::Error::InvalidQuery)?,
                        status: crate::background_work::DeferredWorkStatus::from_db_str(&status_raw)
                            .ok_or(rusqlite::Error::InvalidQuery)?,
                        attempt_count: row.get(3)?,
                        available_at: row.get(4)?,
                        queued_at: row.get(5)?,
                        started_at: row.get(6)?,
                        finished_at: row.get(7)?,
                        last_error: row.get(8)?,
                        last_error_at: row.get(9)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    pub fn reset_running_deferred_work_items(&self) -> Result<usize, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            conn.execute(
                "UPDATE deferred_work_item
                 SET status = 'pending',
                     started_at = NULL,
                     finished_at = NULL,
                     queued_at = ?1
                 WHERE status = 'running'",
                [&now],
            )
        })
    }

    pub fn claim_next_deferred_work_items(
        &self,
    ) -> Result<Vec<types::ClaimedDeferredWorkItem>, String> {
        self.with_write(|conn| {
            let tx = conn.unchecked_transaction()?;
            let now = chrono::Utc::now().to_rfc3339();
            let next_hash: Option<String> = tx
                .query_row(
                    "SELECT entity_hash
                     FROM deferred_work_item
                     WHERE status = 'pending' AND available_at <= ?1
                     ORDER BY work_id ASC
                     LIMIT 1",
                    [&now],
                    |row| row.get(0),
                )
                .optional()?;

            let Some(next_hash) = next_hash else {
                tx.commit()?;
                return Ok(Vec::new());
            };

            let mut stmt = tx.prepare(
                "SELECT work_id, entity_hash, work_type, attempt_count
                 FROM deferred_work_item
                 WHERE entity_hash = ?1 AND status = 'pending' AND available_at <= ?2
                 ORDER BY work_id ASC",
            )?;
            let rows = stmt
                .query_map(rusqlite::params![next_hash, now], |row| {
                    Ok(types::ClaimedDeferredWorkItem {
                        work_id: row.get(0)?,
                        entity_hash: row.get(1)?,
                        work_type: row.get(2)?,
                        attempt_count: row.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            drop(stmt);

            for row in &rows {
                tx.execute(
                    "UPDATE deferred_work_item
                     SET status = 'running',
                         started_at = ?2
                     WHERE work_id = ?1",
                    rusqlite::params![row.work_id, now],
                )?;
            }
            tx.commit()?;
            Ok(rows)
        })
    }

    pub fn complete_deferred_work_item(&self, work_id: i64) -> Result<(), String> {
        self.with_write(move |conn| {
            conn.execute(
                "DELETE FROM deferred_work_item WHERE work_id = ?1",
                [work_id],
            )?;
            Ok(())
        })
    }

    pub fn retry_deferred_work_item(
        &self,
        work_id: i64,
        next_attempt: i64,
        error: &str,
    ) -> Result<(), String> {
        let error = error.to_string();
        let available_at = {
            let exp = (next_attempt.saturating_sub(1)).clamp(0, 10) as u32;
            let delay_secs = (30_i64.saturating_mul(1_i64 << exp)).min(60 * 60);
            (chrono::Utc::now() + chrono::Duration::seconds(delay_secs)).to_rfc3339()
        };
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            conn.execute(
                "UPDATE deferred_work_item
                 SET status = 'pending',
                     attempt_count = ?2,
                     available_at = ?3,
                     last_error = ?4,
                     queued_at = ?5,
                     started_at = NULL,
                     last_error_at = ?5
                 WHERE work_id = ?1",
                rusqlite::params![work_id, next_attempt, available_at, error, now],
            )?;
            Ok(())
        })
    }

    pub fn set_phash_for_entity_hash(&self, entity_hash: &str, phash: &str) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let value = phash.to_string();
        self.with_write(move |conn| {
            let file_id: i64 = conn.query_row(
                "SELECT mf.file_id
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE me.entity_hash = ?1",
                [&hash],
                |row| row.get(0),
            )?;
            write::files::update_file_analysis(conn, file_id, Some(&value), None, None)
        })
    }

    pub fn replace_file_phash(&self, file_id: i64, phash: Option<&str>) -> Result<(), String> {
        let value = phash.map(str::to_string);
        self.with_write(move |conn| {
            write::files::replace_file_phash(conn, file_id, value.as_deref())
        })
    }

    pub fn set_file_colors_for_entity_hash(
        &self,
        entity_hash: &str,
        colors: &[(String, f32, f32, f32)],
        dominant_color_hex: Option<&str>,
    ) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let colors = colors.to_vec();
        let dominant = dominant_color_hex.map(str::to_string);
        self.with_write(move |conn| {
            let file_id: i64 = conn.query_row(
                "SELECT mf.file_id
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE me.entity_hash = ?1",
                [&hash],
                |row| row.get(0),
            )?;
            write::files::save_file_colors(conn, file_id, &colors)?;
            write::files::update_file_analysis(conn, file_id, None, dominant.as_deref(), None)
        })
    }

    pub fn replace_file_colors(
        &self,
        file_id: i64,
        colors: &[(String, f32, f32, f32)],
        dominant_color_hex: Option<&str>,
    ) -> Result<(), String> {
        let colors = colors.to_vec();
        let dominant = dominant_color_hex.map(str::to_string);
        self.with_write(move |conn| {
            write::files::save_file_colors(conn, file_id, &colors)?;
            write::files::replace_file_dominant_color(conn, file_id, dominant.as_deref())
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
                let bitmap = self
                    .bitmaps
                    .get(&projection::bitmaps::BitmapKey::SmartFolder(sf_id));
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
        offset: i64,
    ) -> Result<Vec<types::TagRecord>, String> {
        self.with_read(|conn| query::tags::search_tags(conn, query_str, limit, offset))
    }

    pub fn get_all_tags_with_counts(&self) -> Result<Vec<types::TagRecord>, String> {
        self.with_read(query::tags::get_all_tags_with_counts)
    }

    pub fn get_entity_tags(&self, entity_hash: &str) -> Result<Vec<types::TagInfo>, String> {
        self.with_read(|conn| query::tags::get_entity_tags(conn, entity_hash))
    }

    pub fn get_aliases_for_tag(&self, tag_id: i64) -> Result<Vec<types::TagRelation>, String> {
        self.with_read(|conn| query::tags::get_aliases_for_tag(conn, tag_id))
    }

    pub fn get_implications_for_tag(&self, tag_id: i64) -> Result<Vec<types::TagRelation>, String> {
        self.with_read(|conn| query::tags::get_implications_for_tag(conn, tag_id))
    }

    pub fn get_tags_paginated(
        &self,
        namespace: Option<String>,
        search: Option<String>,
        cursor: Option<String>,
        limit: i64,
    ) -> Result<Vec<types::TagRecord>, String> {
        self.with_read(|conn| {
            query::tags::get_tags_paginated(
                conn,
                namespace.as_deref(),
                search.as_deref(),
                cursor.as_deref(),
                limit,
            )
        })
    }

    pub fn get_namespace_summary(&self) -> Result<Vec<types::NamespaceSummary>, String> {
        self.with_read(query::tags::get_namespace_summary)
    }

    pub fn find_similar(
        &self,
        source_hash: &str,
    ) -> Result<crate::types::FindSimilarResponse, String> {
        let source_hash = source_hash.to_string();
        let source = self.with_read(|conn| {
            conn.query_row(
                "SELECT mf.perceptual_hash, mf.mime_type, mf.frame_count
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE me.entity_hash = ?1",
                [source_hash.as_str()],
                |row| {
                    Ok((
                        row.get::<_, Option<String>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .optional()
        })?;

        let Some((Some(source_phash), source_mime, source_frame_count)) = source else {
            return Ok(crate::types::FindSimilarResponse {
                source_hash,
                items: Vec::new(),
            });
        };

        if !crate::media_capabilities::capabilities_for_stored_media(
            &source_mime,
            source_frame_count,
        )
        .can_perceptual_hash
        {
            return Ok(crate::types::FindSimilarResponse {
                source_hash,
                items: Vec::new(),
            });
        }

        let candidates: Vec<(String, String, String, Option<i64>)> = self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT me.entity_hash, mf.perceptual_hash, mf.mime_type, mf.frame_count
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                  WHERE mf.perceptual_hash IS NOT NULL
                   AND me.entity_hash != ?1",
            )?;
            let rows = stmt.query_map([source_hash.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?;
            rows.collect()
        })?;

        use img_hash::ImageHash;

        let Ok(source_hash_image) = ImageHash::<Vec<u8>>::from_base64(&source_phash) else {
            return Ok(crate::types::FindSimilarResponse {
                source_hash,
                items: Vec::new(),
            });
        };

        let mut items: Vec<crate::types::SimilarItem> = candidates
            .into_iter()
            .filter_map(|(hash, phash_b64, mime_type, frame_count)| {
                if !crate::media_capabilities::capabilities_for_stored_media(
                    &mime_type,
                    frame_count,
                )
                .can_perceptual_hash
                {
                    return None;
                }
                let candidate = ImageHash::<Vec<u8>>::from_base64(&phash_b64).ok()?;
                Some(crate::types::SimilarItem {
                    hash,
                    distance: source_hash_image.dist(&candidate),
                })
            })
            .collect();
        items.sort_by_key(|item| item.distance);

        Ok(crate::types::FindSimilarResponse { source_hash, items })
    }

    pub fn scan_duplicates(
        &self,
        threshold: Option<u32>,
        review_threshold: Option<u32>,
    ) -> Result<types::DuplicateScanSummary, String> {
        let threshold = threshold.unwrap_or(crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD);
        let review_threshold = review_threshold.unwrap_or(threshold);
        let files: Vec<(i64, String, String, String, Option<i64>)> = self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT mf.file_id, me.entity_hash, mf.perceptual_hash, mf.mime_type, mf.frame_count
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE mf.perceptual_hash IS NOT NULL",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })?;
            rows.collect()
        })?;

        use img_hash::ImageHash;
        let parsed: Vec<(i64, String, ImageHash<Vec<u8>>)> = files
            .iter()
            .filter_map(|(file_id, entity_hash, phash, mime_type, frame_count)| {
                if !crate::media_capabilities::capabilities_for_stored_media(
                    mime_type,
                    *frame_count,
                )
                .can_perceptual_hash
                {
                    return None;
                }
                Some((
                    *file_id,
                    entity_hash.clone(),
                    ImageHash::<Vec<u8>>::from_base64(phash).ok()?,
                ))
            })
            .collect();

        let mut candidate_pairs = Vec::<(i64, i64, u32)>::new();
        let mut closest_distance: Option<u32> = None;
        for index in 0..parsed.len() {
            let (file_id_a, _, ref hash_a) = parsed[index];
            for (file_id_b, _, hash_b) in parsed.iter().skip(index + 1) {
                let distance = hash_a.dist(hash_b);
                if distance <= threshold {
                    candidate_pairs.push((file_id_a, *file_id_b, distance));
                    closest_distance = Some(match closest_distance {
                        Some(current) => current.min(distance),
                        None => distance,
                    });
                }
            }
        }

        let pairs_inserted = self.with_write(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut inserted = 0usize;
            for (file_id_a, file_id_b, distance) in &candidate_pairs {
                let (a, b) = if file_id_a < file_id_b {
                    (*file_id_a, *file_id_b)
                } else {
                    (*file_id_b, *file_id_a)
                };
                inserted += tx.execute(
                    "INSERT OR IGNORE INTO duplicate (file_id_a, file_id_b, distance)
                     VALUES (?1, ?2, ?3)",
                    params![a, b, *distance as i64],
                )? as usize;
            }
            tx.commit()?;
            Ok(inserted)
        })?;

        let reviewable_detected_total = self.with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM duplicate WHERE status = 'detected' AND distance <= ?1",
                [review_threshold as i64],
                |row| row.get::<_, i64>(0),
            )
        })? as usize;

        let reviewable_detected_new = candidate_pairs
            .iter()
            .filter(|(_, _, distance)| *distance <= review_threshold)
            .count()
            .min(pairs_inserted);

        Ok(types::DuplicateScanSummary {
            candidates_found: candidate_pairs.len(),
            pairs_inserted,
            reviewable_detected_total,
            reviewable_detected_new,
            total_files: parsed.len(),
            files_with_phash: parsed.len(),
            files_scanned: files.len(),
            closest_distance,
        })
    }

    pub fn get_duplicate_pairs(
        &self,
        cursor: Option<String>,
        limit: usize,
        status: Option<String>,
        max_distance: Option<f64>,
    ) -> Result<types::DuplicatePairPage, String> {
        let status_filter = status.unwrap_or_else(|| "detected".to_string());
        let limit = limit.clamp(1, 200);
        let cursor = cursor.clone();
        self.with_read(move |conn| {
            let total: i64 = if let Some(max_distance) = max_distance {
                conn.query_row(
                    "SELECT COUNT(*) FROM duplicate WHERE status = ?1 AND distance <= ?2",
                    params![status_filter, max_distance as i64],
                    |row| row.get(0),
                )?
            } else {
                conn.query_row(
                    "SELECT COUNT(*) FROM duplicate WHERE status = ?1",
                    [status_filter.as_str()],
                    |row| row.get(0),
                )?
            };

            let mut params_vec: Vec<rusqlite::types::Value> = vec![status_filter.clone().into()];
            let cursor_clause = if let Some(cursor) = cursor.as_deref() {
                let parts: Vec<&str> = cursor.split(',').collect();
                if parts.len() == 3 {
                    let distance = parts[0].parse::<i64>().unwrap_or(0);
                    let file_id_a = parts[1].parse::<i64>().unwrap_or(0);
                    let file_id_b = parts[2].parse::<i64>().unwrap_or(0);
                    params_vec.push(distance.into());
                    params_vec.push(file_id_a.into());
                    params_vec.push(file_id_b.into());
                    " AND (d.distance > ?2 OR (d.distance = ?2 AND d.file_id_a > ?3) OR (d.distance = ?2 AND d.file_id_a = ?3 AND d.file_id_b > ?4))"
                } else {
                    ""
                }
            } else {
                ""
            };

            let distance_clause = if let Some(max_distance) = max_distance {
                if cursor_clause.is_empty() {
                    params_vec.push((max_distance as i64).into());
                    " AND d.distance <= ?2"
                } else {
                    params_vec.push((max_distance as i64).into());
                    " AND d.distance <= ?5"
                }
            } else {
                ""
            };

            params_vec.push((limit as i64).into());
            let limit_param = format!("?{}", params_vec.len());
            let sql = format!(
                "SELECT
                     me_a.entity_hash,
                     me_b.entity_hash,
                     d.distance,
                     d.status,
                     d.file_id_a,
                     d.file_id_b
                 FROM duplicate d
                 JOIN single_media_entity sme_a ON sme_a.file_id = d.file_id_a
                 JOIN media_entity me_a ON me_a.entity_id = sme_a.entity_id
                 JOIN single_media_entity sme_b ON sme_b.file_id = d.file_id_b
                 JOIN media_entity me_b ON me_b.entity_id = sme_b.entity_id
                 WHERE d.status = ?1{cursor_clause}{distance_clause}
                 ORDER BY d.distance ASC, d.file_id_a ASC, d.file_id_b ASC
                 LIMIT {limit_param}"
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(params_vec.iter()), |row| {
                let distance: i64 = row.get(2)?;
                Ok((
                    types::DuplicatePairRecord {
                        hash_a: row.get(0)?,
                        hash_b: row.get(1)?,
                        distance: distance as f64,
                        similarity_pct: ((1.0 - distance as f64 / 64.0) * 100.0).round(),
                        status: row.get(3)?,
                    },
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    distance,
                ))
            })?;
            let rows: Vec<_> = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            let next_cursor = if rows.len() == limit {
                rows.last()
                    .map(|(_, file_id_a, file_id_b, distance)| format!("{distance},{file_id_a},{file_id_b}"))
            } else {
                None
            };
            let has_more = next_cursor.is_some();
            Ok(types::DuplicatePairPage {
                items: rows.into_iter().map(|(row, _, _, _)| row).collect(),
                next_cursor,
                has_more,
                total,
            })
        })
    }

    pub fn get_duplicate_count(&self) -> Result<i64, String> {
        self.with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
                [],
                |row| row.get(0),
            )
        })
    }

    pub fn resolve_duplicate_pair(
        &self,
        action: &str,
        hash_a: &str,
        hash_b: &str,
        preferred_collection_id: Option<i64>,
    ) -> Result<types::DuplicateResolutionResult, String> {
        #[derive(Clone)]
        struct SingleRef {
            entity_id: i64,
            file_id: i64,
            entity_hash: String,
            status: i64,
            mime_type: String,
            size_bytes: i64,
            pixel_width: Option<i64>,
            pixel_height: Option<i64>,
            frame_count: Option<i64>,
            notes: Option<String>,
            source_urls_json: Option<String>,
            rating: Option<i64>,
            date_created: String,
            parent_collection_entity_id: Option<i64>,
            collection_ordinal: Option<i64>,
        }

        fn merged_status(left: i64, right: i64) -> i64 {
            if left == 1 || right == 1 {
                1
            } else if left == 0 || right == 0 {
                0
            } else {
                left
            }
        }

        fn merge_notes(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
            let existing = existing.unwrap_or("").trim();
            let incoming = incoming.unwrap_or("").trim();
            match (existing.is_empty(), incoming.is_empty()) {
                (true, true) => None,
                (true, false) => Some(incoming.to_string()),
                (false, true) => Some(existing.to_string()),
                (false, false) if existing.contains(incoming) => Some(existing.to_string()),
                (false, false) => Some(format!("{existing}\n\n{incoming}")),
            }
        }

        fn merge_source_urls(existing: Option<&str>, incoming: Option<&str>) -> Option<String> {
            let mut merged = Vec::<String>::new();
            let mut seen = std::collections::HashSet::<String>::new();
            for raw in [existing, incoming].into_iter().flatten() {
                let urls: Vec<String> = serde_json::from_str(raw).unwrap_or_default();
                for url in urls {
                    if !url.trim().is_empty() && seen.insert(url.clone()) {
                        merged.push(url);
                    }
                }
            }
            if merged.is_empty() {
                None
            } else {
                serde_json::to_string(&merged).ok()
            }
        }

        let action = action.to_string();
        let hash_a = hash_a.to_string();
        let hash_b = hash_b.to_string();
        self.with_write(move |conn| {
            let tx = conn.unchecked_transaction()?;
            let load_single = |entity_hash: &str| -> rusqlite::Result<SingleRef> {
                tx.query_row(
                    "SELECT
                         me.entity_id,
                         sme.file_id,
                         me.entity_hash,
                         me.status,
                         mf.mime_type,
                         mf.size_bytes,
                         mf.pixel_width,
                         mf.pixel_height,
                         mf.frame_count,
                         me.notes,
                         me.source_urls_json,
                         me.rating,
                         me.date_created,
                         me.parent_collection_entity_id,
                         me.collection_ordinal
                     FROM media_entity me
                     JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                     JOIN media_file mf ON mf.file_id = sme.file_id
                     WHERE me.entity_hash = ?1",
                    [entity_hash],
                    |row| {
                        Ok(SingleRef {
                            entity_id: row.get(0)?,
                            file_id: row.get(1)?,
                            entity_hash: row.get(2)?,
                            status: row.get(3)?,
                            mime_type: row.get(4)?,
                            size_bytes: row.get(5)?,
                            pixel_width: row.get(6)?,
                            pixel_height: row.get(7)?,
                            frame_count: row.get(8)?,
                            notes: row.get(9)?,
                            source_urls_json: row.get(10)?,
                            rating: row.get(11)?,
                            date_created: row.get(12)?,
                            parent_collection_entity_id: row.get(13)?,
                            collection_ordinal: row.get(14)?,
                        })
                    },
                )
            };

            let left = load_single(&hash_a)?;
            let right = load_single(&hash_b)?;

            if action == "not_duplicate" {
                tx.execute(
                    "UPDATE duplicate
                     SET status = 'ignored_false_positive',
                         decision_at = datetime('now'),
                         decision_source = 'manual',
                         decision_reason = 'User marked as not duplicate'
                     WHERE (file_id_a = ?1 AND file_id_b = ?2) OR (file_id_a = ?2 AND file_id_b = ?1)",
                    params![left.file_id, right.file_id],
                )?;
                tx.commit()?;
                return Ok(types::DuplicateResolutionResult {
                    status: types::DuplicateResolveStatus::Resolved,
                    winner_hash: None,
                    loser_hash: None,
                    action,
                    affected_folder_ids: Vec::new(),
                    affected_collection_ids: Vec::new(),
                    tags_merged: 0,
                    conflict: None,
                });
            }

            if action == "keep_both" {
                tx.execute(
                    "UPDATE duplicate
                     SET status = 'dismissed_keep_both',
                         decision_at = datetime('now'),
                         decision_source = 'manual',
                         decision_reason = 'User chose to keep both'
                     WHERE (file_id_a = ?1 AND file_id_b = ?2) OR (file_id_a = ?2 AND file_id_b = ?1)",
                    params![left.file_id, right.file_id],
                )?;
                tx.commit()?;
                return Ok(types::DuplicateResolutionResult {
                    status: types::DuplicateResolveStatus::Resolved,
                    winner_hash: None,
                    loser_hash: None,
                    action,
                    affected_folder_ids: Vec::new(),
                    affected_collection_ids: Vec::new(),
                    tags_merged: 0,
                    conflict: None,
                });
            }

            let (winner, loser) = match action.as_str() {
                "keep_left" => (left.clone(), right.clone()),
                "keep_right" => (right.clone(), left.clone()),
                "smart_merge" => {
                    let decision = crate::duplicates::quality::compare_static_image_quality(
                        &crate::duplicates::quality::ComparableImageCandidate {
                            mime_type: &left.mime_type,
                            size_bytes: left.size_bytes,
                            pixel_width: left.pixel_width,
                            pixel_height: left.pixel_height,
                            frame_count: left.frame_count,
                        },
                        &crate::duplicates::quality::ComparableImageCandidate {
                            mime_type: &right.mime_type,
                            size_bytes: right.size_bytes,
                            pixel_width: right.pixel_width,
                            pixel_height: right.pixel_height,
                            frame_count: right.frame_count,
                        },
                    );
                    match decision {
                        crate::duplicates::quality::ImageQualityDecision::LeftBetter => {
                            (left.clone(), right.clone())
                        }
                        crate::duplicates::quality::ImageQualityDecision::RightBetter => {
                            (right.clone(), left.clone())
                        }
                        crate::duplicates::quality::ImageQualityDecision::Ambiguous => {
                            if left.entity_hash <= right.entity_hash {
                                (left.clone(), right.clone())
                            } else {
                                (right.clone(), left.clone())
                            }
                        }
                    }
                }
                other => {
                    return Err(rusqlite::Error::InvalidParameterName(format!(
                        "Invalid duplicate action: {other}"
                    )))
                }
            };

            if winner.parent_collection_entity_id.is_some()
                && loser.parent_collection_entity_id.is_some()
                && winner.parent_collection_entity_id != loser.parent_collection_entity_id
                && preferred_collection_id.is_none()
            {
                return Ok(types::DuplicateResolutionResult {
                    status: types::DuplicateResolveStatus::Conflict,
                    winner_hash: Some(winner.entity_hash.clone()),
                    loser_hash: Some(loser.entity_hash.clone()),
                    action,
                    affected_folder_ids: Vec::new(),
                    affected_collection_ids: Vec::new(),
                    tags_merged: 0,
                    conflict: Some(types::DuplicateCollectionConflict {
                        winner_hash: winner.entity_hash.clone(),
                        loser_hash: loser.entity_hash.clone(),
                        winner_collection_id: winner.parent_collection_entity_id,
                        loser_collection_id: loser.parent_collection_entity_id,
                    }),
                });
            }

            if let Some(chosen_collection_id) = preferred_collection_id {
                let valid = [winner.parent_collection_entity_id, loser.parent_collection_entity_id]
                    .into_iter()
                    .flatten()
                    .any(|value| value == chosen_collection_id);
                if !valid {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "preferred_collection_id must match one of the duplicate owners".into(),
                    ));
                }
            }

            let final_collection_id = preferred_collection_id
                .or(winner.parent_collection_entity_id)
                .or(loser.parent_collection_entity_id);

            let tags_merged: usize = tx
                .query_row(
                    "SELECT COUNT(*) FROM entity_tag et
                     WHERE et.entity_id = ?1
                       AND NOT EXISTS (
                           SELECT 1 FROM entity_tag existing
                           WHERE existing.entity_id = ?2
                             AND existing.tag_id = et.tag_id
                       )",
                    params![loser.entity_id, winner.entity_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize;

            tx.execute(
                "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source)
                 SELECT ?1, et.tag_id, et.provenance_mask, et.source
                 FROM entity_tag et
                 WHERE et.entity_id = ?2
                 ON CONFLICT(entity_id, tag_id, source)
                 DO UPDATE SET provenance_mask = entity_tag.provenance_mask | excluded.provenance_mask",
                params![winner.entity_id, loser.entity_id],
            )?;

            let merged_notes = merge_notes(winner.notes.as_deref(), loser.notes.as_deref());
            let merged_urls = merge_source_urls(
                winner.source_urls_json.as_deref(),
                loser.source_urls_json.as_deref(),
            );
            let merged_rating = match (winner.rating, loser.rating) {
                (Some(left), Some(right)) => Some(left.max(right)),
                (Some(value), None) | (None, Some(value)) => Some(value),
                (None, None) => None,
            };
            let merged_created_at = winner.date_created.min(loser.date_created);
            let merged_status = merged_status(winner.status, loser.status);
            tx.execute(
                "UPDATE media_entity
                 SET status = ?1,
                     notes = ?2,
                     source_urls_json = ?3,
                     rating = ?4,
                     date_created = ?5,
                     date_modified = ?6
                 WHERE entity_id = ?7",
                params![
                    merged_status,
                    merged_notes.as_deref(),
                    merged_urls.as_deref(),
                    merged_rating,
                    merged_created_at,
                    chrono::Utc::now().to_rfc3339(),
                    winner.entity_id
                ],
            )?;

            let affected_folder_ids = {
                let mut stmt = tx.prepare(
                    "SELECT folder_id FROM folder_member WHERE entity_id = ?1 ORDER BY folder_id",
                )?;
                let ids = stmt
                    .query_map([loser.entity_id], |row| row.get::<_, i64>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                tx.execute(
                    "INSERT OR IGNORE INTO folder_member (folder_id, entity_id, position_rank)
                     SELECT folder_id, ?1, position_rank
                     FROM folder_member
                     WHERE entity_id = ?2",
                    params![winner.entity_id, loser.entity_id],
                )?;
                tx.execute("DELETE FROM folder_member WHERE entity_id = ?1", [loser.entity_id])?;
                ids
            };

            tx.execute(
                "INSERT OR IGNORE INTO subscription_entity (subscription_id, entity_id)
                 SELECT subscription_id, ?1
                 FROM subscription_entity
                 WHERE entity_id = ?2",
                params![winner.entity_id, loser.entity_id],
            )?;
            tx.execute(
                "DELETE FROM subscription_entity WHERE entity_id = ?1",
                [loser.entity_id],
            )?;

            let mut affected_collection_ids = Vec::<i64>::new();
            for collection_id in [winner.parent_collection_entity_id, loser.parent_collection_entity_id, final_collection_id]
                .into_iter()
                .flatten()
            {
                if !affected_collection_ids.contains(&collection_id) {
                    affected_collection_ids.push(collection_id);
                }
            }

            match final_collection_id {
                Some(collection_id) => {
                    let ordinal = if loser.parent_collection_entity_id == Some(collection_id) {
                        loser.collection_ordinal.unwrap_or(1)
                    } else if winner.parent_collection_entity_id == Some(collection_id) {
                        winner.collection_ordinal.unwrap_or(1)
                    } else {
                        tx.query_row(
                            "SELECT COALESCE(MAX(collection_ordinal), 0) + 1
                             FROM media_entity
                             WHERE parent_collection_entity_id = ?1",
                            [collection_id],
                            |row| row.get::<_, i64>(0),
                        )?
                    };
                    tx.execute(
                        "UPDATE media_entity
                         SET parent_collection_entity_id = ?1,
                             collection_ordinal = ?2
                         WHERE entity_id = ?3",
                        params![collection_id, ordinal, winner.entity_id],
                    )?;
                }
                None => {
                    tx.execute(
                        "UPDATE media_entity
                         SET parent_collection_entity_id = NULL,
                             collection_ordinal = NULL
                         WHERE entity_id = ?1",
                        [winner.entity_id],
                    )?;
                }
            }

            tx.execute(
                "DELETE FROM duplicate WHERE file_id_a = ?1 OR file_id_b = ?1",
                [loser.file_id],
            )?;
            tx.execute(
                "DELETE FROM single_media_entity WHERE entity_id = ?1",
                [loser.entity_id],
            )?;
            tx.execute("DELETE FROM entity_tag WHERE entity_id = ?1", [loser.entity_id])?;
            tx.execute("DELETE FROM media_entity WHERE entity_id = ?1", [loser.entity_id])?;
            tx.execute("DELETE FROM media_file WHERE file_id = ?1", [loser.file_id])?;

            for collection_id in &affected_collection_ids {
                write::collections::sync_aggregates(&tx, *collection_id)?;
            }

            tx.commit()?;
            Ok(types::DuplicateResolutionResult {
                status: types::DuplicateResolveStatus::Resolved,
                winner_hash: Some(winner.entity_hash),
                loser_hash: Some(loser.entity_hash),
                action,
                affected_folder_ids,
                affected_collection_ids,
                tags_merged,
                conflict: None,
            })
        })
    }

    pub fn get_tag_string(&self, tag_id: i64) -> Result<Option<String>, String> {
        self.with_read(|conn| query::tags::get_tag_string(conn, tag_id))
    }

    pub fn find_tag_id(&self, tag_str: &str) -> Result<Option<i64>, String> {
        self.with_read(|conn| query::tags::find_tag_id(conn, tag_str))
    }

    pub fn resolve_entity_ids_to_hashes(&self, entity_ids: &[i64]) -> Result<Vec<String>, String> {
        let ids = entity_ids.to_vec();
        self.with_read(move |conn| {
            if ids.is_empty() {
                return Ok(Vec::new());
            }
            let placeholders = std::iter::repeat_n("?", ids.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql =
                format!("SELECT entity_hash FROM media_entity WHERE entity_id IN ({placeholders})");
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |row| row.get(0))?;
            rows.collect()
        })
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
