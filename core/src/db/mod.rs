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

use rusqlite::{Connection, OptionalExtension};

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

fn table_exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table],
        |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
}

fn reconcile_ingest_queue_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS ingest_queue (
            queue_id        INTEGER PRIMARY KEY,
            queue_kind      TEXT    NOT NULL,
            source_kind     TEXT    NOT NULL,
            subscription_id INTEGER REFERENCES subscription(subscription_id) ON DELETE CASCADE,
            query_id        INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
            query_run_id    INTEGER,
            cleanup_root    TEXT,
            post_id         TEXT,
            category        TEXT,
            preferred_name  TEXT,
            expected_count  INTEGER,
            status          TEXT    NOT NULL DEFAULT 'pending',
            last_error      TEXT,
            created_at      TEXT    NOT NULL,
            updated_at      TEXT    NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ingest_queue_item (
            item_id              INTEGER PRIMARY KEY,
            queue_id             INTEGER NOT NULL REFERENCES ingest_queue(queue_id) ON DELETE CASCADE,
            source_path          TEXT    NOT NULL,
            page_num             INTEGER NOT NULL DEFAULT 0,
            payload_json         TEXT    NOT NULL,
            delete_after_ingest  INTEGER NOT NULL DEFAULT 0,
            status               TEXT    NOT NULL DEFAULT 'pending',
            result_kind          TEXT,
            resolved_entity_hash TEXT,
            resolved_file_hash   TEXT,
            last_error           TEXT,
            created_at           TEXT    NOT NULL,
            updated_at           TEXT    NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_ingest_queue_ready
            ON ingest_queue(status, created_at, queue_id);
        CREATE INDEX IF NOT EXISTS idx_ingest_queue_subscription
            ON ingest_queue(subscription_id, status, queue_id);
        CREATE INDEX IF NOT EXISTS idx_ingest_queue_item_queue
            ON ingest_queue_item(queue_id, status, page_num, item_id);",
    )
    .map_err(|e| format!("Failed to reconcile ingest queue tables: {e}"))?;

    if table_exists(conn, "ingest_queue").map_err(|e| e.to_string())? {
        if !has_column(conn, "ingest_queue", "created_at").map_err(|e| e.to_string())? {
            conn.execute_batch(
                "ALTER TABLE ingest_queue ADD COLUMN created_at TEXT NOT NULL DEFAULT '';
                 ALTER TABLE ingest_queue ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';",
            )
            .map_err(|e| format!("Failed to add ingest_queue created/updated columns: {e}"))?;
            conn.execute(
                "UPDATE ingest_queue
                 SET created_at = COALESCE(NULLIF(date_added, ''), datetime('now')),
                     updated_at = COALESCE(NULLIF(date_modified, ''), COALESCE(NULLIF(date_added, ''), datetime('now')))
                 WHERE created_at = '' OR updated_at = ''",
                [],
            )
            .map_err(|e| format!("Failed to backfill ingest_queue timestamps: {e}"))?;
        }
    }

    if table_exists(conn, "ingest_queue_item").map_err(|e| e.to_string())? {
        if !has_column(conn, "ingest_queue_item", "created_at").map_err(|e| e.to_string())? {
            conn.execute_batch(
                "ALTER TABLE ingest_queue_item ADD COLUMN created_at TEXT NOT NULL DEFAULT '';
                 ALTER TABLE ingest_queue_item ADD COLUMN updated_at TEXT NOT NULL DEFAULT '';",
            )
            .map_err(|e| {
                format!("Failed to add ingest_queue_item created/updated columns: {e}")
            })?;
            conn.execute(
                "UPDATE ingest_queue_item
                 SET created_at = COALESCE(NULLIF(date_added, ''), datetime('now')),
                     updated_at = COALESCE(NULLIF(date_modified, ''), COALESCE(NULLIF(date_added, ''), datetime('now')))
                 WHERE created_at = '' OR updated_at = ''",
                [],
            )
            .map_err(|e| format!("Failed to backfill ingest_queue_item timestamps: {e}"))?;
        }
    }

    Ok(())
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
    if has_column(conn, "media_file", "color_analysis_version").map_err(|e| e.to_string())? == false
    {
        conn.execute_batch(
            "ALTER TABLE media_file
             ADD COLUMN color_analysis_version INTEGER NOT NULL DEFAULT 0",
        )
        .map_err(|e| {
            format!("Failed to add media_file.color_analysis_version to canonical db: {e}")
        })?;
    }
    if has_column(conn, "subscription_query", "notes").map_err(|e| e.to_string())? == false {
        conn.execute_batch("ALTER TABLE subscription_query ADD COLUMN notes TEXT")
            .map_err(|e| format!("Failed to add subscription_query.notes: {e}"))?;
    }
    if has_column(conn, "subscription_query", "site_id").map_err(|e| e.to_string())? == false {
        conn.execute_batch(
            "ALTER TABLE subscription_query ADD COLUMN site_id TEXT NOT NULL DEFAULT ''",
        )
        .map_err(|e| format!("Failed to add subscription_query.site_id: {e}"))?;
        conn.execute(
            "UPDATE subscription_query
             SET site_id = COALESCE((
                 SELECT NULLIF(subscription.site_id, '')
                 FROM subscription
                 WHERE subscription.subscription_id = subscription_query.subscription_id
             ), site_id)
             WHERE site_id = ''",
            [],
        )
        .map_err(|e| format!("Failed to backfill subscription_query.site_id: {e}"))?;
    }
    for column in [
        "last_success_at",
        "last_failure_at",
        "last_failure_kind",
        "last_failure_message",
    ] {
        if has_column(conn, "subscription_query", column).map_err(|e| e.to_string())? == false {
            conn.execute_batch(&format!(
                "ALTER TABLE subscription_query ADD COLUMN {column} TEXT"
            ))
            .map_err(|e| format!("Failed to add subscription_query.{column}: {e}"))?;
        }
    }
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS subscription_run (
            run_id INTEGER PRIMARY KEY,
            subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            status TEXT NOT NULL DEFAULT 'running',
            failure_kind TEXT,
            error_message TEXT,
            files_downloaded INTEGER NOT NULL DEFAULT 0,
            files_skipped INTEGER NOT NULL DEFAULT 0,
            metadata_validated INTEGER NOT NULL DEFAULT 0,
            metadata_invalid INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS subscription_query_run (
            query_run_id INTEGER PRIMARY KEY,
            run_id INTEGER REFERENCES subscription_run(run_id) ON DELETE SET NULL,
            subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
            query_id INTEGER NOT NULL REFERENCES subscription_query(query_id) ON DELETE CASCADE,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            status TEXT NOT NULL DEFAULT 'running',
            failure_kind TEXT,
            error_message TEXT,
            posts_processed INTEGER NOT NULL DEFAULT 0,
            files_downloaded INTEGER NOT NULL DEFAULT 0,
            files_skipped INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS subscription_issue (
            issue_id INTEGER PRIMARY KEY,
            subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
            query_id INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
            issue_kind TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'open',
            message TEXT NOT NULL,
            detail TEXT,
            first_seen_at TEXT NOT NULL,
            last_seen_at TEXT NOT NULL,
            resolved_at TEXT,
            UNIQUE (subscription_id, query_id, issue_kind, message)
        );
        CREATE TABLE IF NOT EXISTS subscription_download_attempt (
            attempt_id INTEGER PRIMARY KEY,
            subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
            query_id INTEGER REFERENCES subscription_query(query_id) ON DELETE CASCADE,
            query_run_id INTEGER REFERENCES subscription_query_run(query_run_id) ON DELETE SET NULL,
            item_key TEXT NOT NULL,
            site_category TEXT,
            post_id TEXT,
            page_num INTEGER,
            canonical_post_url TEXT,
            media_url TEXT,
            retry_url TEXT,
            retry_count INTEGER NOT NULL DEFAULT 0,
            status TEXT NOT NULL DEFAULT 'pending',
            failure_kind TEXT,
            last_error TEXT,
            next_retry_at TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            resolved_at TEXT,
            UNIQUE (subscription_id, query_id, item_key)
        );
        CREATE TABLE IF NOT EXISTS subscription_post_member (
            subscription_id INTEGER NOT NULL REFERENCES subscription(subscription_id) ON DELETE CASCADE,
            site_id TEXT NOT NULL,
            post_id TEXT NOT NULL,
            item_key TEXT NOT NULL,
            page_num INTEGER,
            canonical_post_url TEXT,
            media_url TEXT,
            entity_hash TEXT,
            status TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            PRIMARY KEY (subscription_id, site_id, post_id, item_key)
        );",
    )
    .map_err(|e| format!("Failed to create canonical subscription runtime tables: {e}"))?;
    conn.execute_batch(
        "DROP TABLE IF EXISTS rejected_fingerprint;
         DROP TABLE IF EXISTS rejected_media;",
    )
    .map_err(|e| format!("Failed to remove rejected-media schema: {e}"))?;
    reconcile_ingest_queue_schema(conn)?;
    if has_column(conn, "folder", "total_size_bytes").map_err(|e| e.to_string())? == false {
        conn.execute_batch("ALTER TABLE folder ADD COLUMN total_size_bytes INTEGER NOT NULL DEFAULT 0")
            .map_err(|e| format!("Failed to add folder.total_size_bytes: {e}"))?;
    }
    if has_column(conn, "smart_folder", "total_size_bytes").map_err(|e| e.to_string())? == false {
        conn.execute_batch("ALTER TABLE smart_folder ADD COLUMN total_size_bytes INTEGER NOT NULL DEFAULT 0")
            .map_err(|e| format!("Failed to add smart_folder.total_size_bytes: {e}"))?;
    }
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
    pub(crate) fn with_write<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R>,
    {
        let conn = self.write_conn.lock().unwrap();
        f(&conn).map_err(|e| e.to_string())
    }

    /// Execute a read operation. Only accessible within db/.
    pub(crate) fn with_read<F, R>(&self, f: F) -> Result<R, String>
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

    pub fn get_derivative_targets_by_entity_hashes(
        &self,
        entity_hashes: &[String],
    ) -> Result<Vec<query::ingest::DerivativeTarget>, String> {
        let hashes = entity_hashes.to_vec();
        self.with_read(|conn| query::ingest::get_derivative_targets_by_entity_hashes(conn, &hashes))
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

            let mut candidates = Vec::new();
            for row in query::duplicates::list_perceptual_hash_sources(conn, None)? {
                let candidate_phash = row.perceptual_hash;
                if !crate::media_capabilities::capabilities_for_stored_media(
                    &row.mime_type,
                    row.frame_count,
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
                        file_id: row.file_id,
                        entity_id: row.entity_id,
                        entity_hash: row.entity_hash,
                        file_hash: row.file_hash,
                        mime_type: row.mime_type,
                        size_bytes: row.size_bytes,
                        pixel_width: row.pixel_width,
                        pixel_height: row.pixel_height,
                        frame_count: row.frame_count,
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

    pub fn plan_ingest_duplicate_review(
        &self,
        prepared: &types::IngestPreparedSingle,
        threshold: u32,
    ) -> Result<types::IngestDuplicatePlan, String> {
        let Some(perceptual_hash) = prepared.perceptual_hash.as_deref() else {
            return Ok(types::IngestDuplicatePlan::default());
        };

        let candidates = self.find_perceptual_hash_candidates(perceptual_hash, threshold)?;
        let exact_matches: Vec<types::PerceptualHashCandidate> = candidates
            .iter()
            .filter(|candidate| candidate.distance == 0)
            .cloned()
            .collect();
        let near_matches: Vec<types::PerceptualHashCandidate> = candidates
            .iter()
            .filter(|candidate| candidate.distance > 0)
            .cloned()
            .collect();

        let mut plan = types::IngestDuplicatePlan::default();
        let mut review_candidates = Vec::<types::PerceptualHashCandidate>::new();
        let mut seen_review_files = std::collections::HashSet::<i64>::new();
        let mut push_review_candidate = |candidate: types::PerceptualHashCandidate| {
            if seen_review_files.insert(candidate.file_id) {
                review_candidates.push(candidate);
            }
        };

        if exact_matches.len() == 1 {
            let candidate = &exact_matches[0];
            if let Some(existing) = self.get_existing_import_target_by_file_hash(&candidate.file_hash)? {
                let quality = crate::duplicates::quality::compare_static_image_quality(
                    &crate::duplicates::quality::ComparableImageCandidate {
                        mime_type: &existing.mime_type,
                        size_bytes: existing.size_bytes,
                        pixel_width: existing.pixel_width,
                        pixel_height: existing.pixel_height,
                        frame_count: existing.frame_count,
                    },
                    &crate::duplicates::quality::ComparableImageCandidate {
                        mime_type: &prepared.mime_type,
                        size_bytes: prepared.size_bytes,
                        pixel_width: prepared.pixel_width,
                        pixel_height: prepared.pixel_height,
                        frame_count: prepared.frame_count,
                    },
                );
                plan.action = match quality {
                    crate::duplicates::quality::ImageQualityDecision::LeftBetter => {
                        types::IngestDuplicateAction::ReuseExisting {
                            entity_hash: existing.entity_hash,
                        }
                    }
                    crate::duplicates::quality::ImageQualityDecision::RightBetter => {
                        types::IngestDuplicateAction::PreferNewOverExisting {
                            existing_entity_hash: existing.entity_hash,
                        }
                    }
                    crate::duplicates::quality::ImageQualityDecision::Ambiguous => {
                        push_review_candidate(candidate.clone());
                        types::IngestDuplicateAction::None
                    }
                };
            }
        } else {
            for candidate in exact_matches {
                push_review_candidate(candidate);
            }
        }

        for candidate in near_matches {
            push_review_candidate(candidate);
        }

        plan.review_candidates = review_candidates;
        Ok(plan)
    }

    pub fn upsert_duplicate_pair_for_review(
        &self,
        file_id_a: i64,
        file_id_b: i64,
        distance: u32,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            write::duplicates::upsert_duplicate_pair_for_review(conn, file_id_a, file_id_b, distance)
        })
    }

    pub fn record_duplicate_review_candidates(
        &self,
        imported_file_id: i64,
        candidates: &[types::PerceptualHashCandidate],
    ) -> Result<(), String> {
        let review_candidates = candidates.to_vec();
        self.with_write(move |conn| {
            for candidate in &review_candidates {
                write::duplicates::upsert_duplicate_pair_for_review(
                    conn,
                    imported_file_id,
                    candidate.file_id,
                    candidate.distance,
                )?;
            }
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
        if new_parent_id == Some(folder_id) {
            return Err("Cannot move a folder into itself".into());
        }
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            // Prevent cycles: check if new_parent_id is a descendant of folder_id
            if let Some(target) = new_parent_id {
                if write::folders::is_ancestor_of(conn, target, folder_id)? {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "Cannot move a folder into one of its descendants".into(),
                    ));
                }
            }
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
            let dominant_pending: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item
                 WHERE work_type = 'dominant_colors' AND status = 'pending' AND attempt_count = 0",
                [],
                |r| r.get(0),
            )?;
            let dominant_running: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item
                 WHERE work_type = 'dominant_colors' AND status = 'running'",
                [],
                |r| r.get(0),
            )?;
            let dominant_failed: i64 = conn.query_row(
                "SELECT COUNT(*) FROM deferred_work_item
                 WHERE work_type = 'dominant_colors' AND status = 'pending' AND attempt_count > 0",
                [],
                |r| r.get(0),
            )?;
            Ok(crate::engine::deferred::DeferredWorkSummary {
                pending_count: pending,
                running_count: running,
                failed_count: failed,
                dominant_colors_pending_count: dominant_pending,
                dominant_colors_running_count: dominant_running,
                dominant_colors_failed_count: dominant_failed,
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

    pub fn ensure_deferred_jobs_present(
        &self,
        entity_hash: &str,
        work_types: &[crate::background_work::DeferredWorkType],
    ) -> Result<(), String> {
        self.ensure_deferred_jobs_present_batch(vec![(
            entity_hash.to_string(),
            work_types.to_vec(),
        )])
    }

    pub fn ensure_deferred_jobs_present_batch(
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
                 ON CONFLICT(entity_hash, work_type) DO NOTHING",
            )?;
            for (entity_hash, work_types) in &items {
                for work_type in work_types {
                    stmt.execute(rusqlite::params![entity_hash, work_type.as_db_str(), now])?;
                }
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
        dominant_palette_blob: Option<&[u8]>,
        color_analysis_version: i64,
    ) -> Result<(), String> {
        let hash = entity_hash.to_string();
        let colors = colors.to_vec();
        let dominant = dominant_color_hex.map(str::to_string);
        let palette_blob = dominant_palette_blob.map(|blob| blob.to_vec());
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
            write::files::replace_file_color_analysis(
                conn,
                file_id,
                &colors,
                dominant.as_deref(),
                palette_blob.as_deref(),
                color_analysis_version,
            )
        })
    }

    pub fn replace_file_colors(
        &self,
        file_id: i64,
        colors: &[(String, f32, f32, f32)],
        dominant_color_hex: Option<&str>,
        dominant_palette_blob: Option<&[u8]>,
        color_analysis_version: i64,
    ) -> Result<(), String> {
        let colors = colors.to_vec();
        let dominant = dominant_color_hex.map(str::to_string);
        let palette_blob = dominant_palette_blob.map(|blob| blob.to_vec());
        self.with_write(move |conn| {
            write::files::replace_file_color_analysis(
                conn,
                file_id,
                &colors,
                dominant.as_deref(),
                palette_blob.as_deref(),
                color_analysis_version,
            )
        })
    }

    pub fn get_file_colors_for_entity_hash(
        &self,
        entity_hash: &str,
    ) -> Result<Vec<(String, f64, f64, f64)>, String> {
        let hash = entity_hash.to_string();
        self.with_read(|conn| {
            let row = conn
                .query_row(
                    "SELECT mf.file_id, mf.dominant_palette_blob
                     FROM media_entity me
                     JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                     JOIN media_file mf ON mf.file_id = sme.file_id
                     WHERE me.entity_hash = ?1",
                    [&hash],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<Vec<u8>>>(1)?)),
                )
                .optional()?;
            let Some((file_id, dominant_palette_blob)) = row else {
                return Ok(Vec::new());
            };

            if let Some(blob) = dominant_palette_blob.as_deref() {
                match crate::media_processing::colors::deserialize_dominant_palette_blob(blob) {
                    Ok(colors) => {
                        return Ok(colors
                            .into_iter()
                            .map(|color| (color.hex, color.l, color.a, color.b))
                            .collect());
                    }
                    Err(error) => {
                        tracing::warn!(
                            entity_hash = %hash,
                            file_id,
                            error = %error,
                            "Failed to decode dominant_palette_blob, falling back to file_color"
                        );
                    }
                }
            }

            let mut stmt = conn.prepare_cached(
                "SELECT hex, l, a, b FROM file_color WHERE file_id = ?1 ORDER BY rowid",
            )?;
            let colors = stmt
                .query_map([file_id], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, f64>(1)?,
                        row.get::<_, f64>(2)?,
                        row.get::<_, f64>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(colors)
        })
    }

    pub fn enqueue_stale_color_analysis_jobs(&self, target_version: i64) -> Result<usize, String> {
        let candidates = self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT me.entity_hash, mf.mime_type, mf.frame_count
                 FROM media_entity me
                 JOIN single_media_entity sme ON sme.entity_id = me.entity_id
                 JOIN media_file mf ON mf.file_id = sme.file_id
                 WHERE mf.color_analysis_version < ?1
                    OR (mf.color_analysis_version >= ?1 AND mf.dominant_palette_blob IS NULL)",
            )?;
            let rows = stmt
                .query_map([target_version], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })?;

        let items = candidates
            .into_iter()
            .filter_map(|(entity_hash, mime_type, frame_count)| {
                let caps = crate::media_capabilities::capabilities_for_stored_media(
                    &mime_type,
                    frame_count,
                );
                caps.can_dominant_colors.then_some((
                    entity_hash,
                    vec![crate::background_work::DeferredWorkType::DominantColors],
                ))
            })
            .collect::<Vec<_>>();

        let count = items.len();
        if count > 0 {
            self.ensure_deferred_jobs_present_batch(items)?;
        }
        Ok(count)
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
        let Some(mut details) =
            self.with_read(|conn| query::details::get_entity_details(conn, entity_hash))?
        else {
            return Ok(None);
        };

        let colors = self.get_file_colors_for_entity_hash(entity_hash)?;
        if !colors.is_empty() {
            details.dominant_colors = Some(
                colors
                    .into_iter()
                    .map(|(hex, l, a, b)| crate::types::DominantColorDto { hex, l, a, b })
                    .collect(),
            );
        }

        Ok(Some(details))
    }

    pub fn get_sidebar_tree(&self) -> Result<Vec<query::sidebar::SidebarNode>, String> {
        self.with_read(|conn| query::sidebar::get_sidebar_tree(conn))
    }

    pub fn get_sidebar_tree_epoch(&self) -> Result<u64, String> {
        self.with_read(query::sidebar::get_sidebar_tree_epoch)
    }

    pub fn get_scope_counts(&self) -> Result<query::stats::ScopeCounts, String> {
        self.with_read(|conn| query::stats::get_scope_counts(conn))
    }

    pub fn count_media_files(&self) -> Result<i64, String> {
        self.with_read(query::stats::count_media_files)
    }

    pub fn aggregate_file_stats(&self) -> Result<types::FileStats, String> {
        self.with_read(query::stats::aggregate_file_stats)
    }

    pub fn aggregate_media_type_breakdown(&self) -> Result<types::MediaTypeBreakdown, String> {
        self.with_read(query::stats::aggregate_media_type_breakdown)
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

    pub fn get_entity_all_metadata(
        &self,
        entity_hash: &str,
    ) -> Result<Option<crate::types::EntityAllMetadata>, String> {
        let Some(entity) = self.get_entity_details(entity_hash)? else {
            return Ok(None);
        };
        let entity_id = self.with_read(|conn| {
            conn.query_row(
                "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
                [entity_hash],
                |row| row.get::<_, i64>(0),
            )
        })?;

        let colors = self.get_file_colors_for_entity_hash(entity_hash)?;
        let parent_tags = self.with_read(|conn| query::metadata::get_implied_tags(conn, entity_hash))?;

        let tags = entity
            .tags
            .iter()
            .map(|tag| {
                let raw_tag = crate::types::tag_display_key(&tag.namespace, &tag.subtag);
                crate::types::ResolvedTagInfo {
                    raw_tag: raw_tag.clone(),
                    display_tag: raw_tag,
                    namespace: tag.namespace.clone(),
                    subtag: tag.subtag.clone(),
                    source: tag.source.clone(),
                    read_only: tag.source != "local",
                }
            })
            .collect();

        let has_thumbnail = !entity.thumbnail_hash.is_empty();
        let entity_dto = crate::types::EntityDetails {
            entity_id,
            kind: entity.entity_kind.as_str().to_string(),
            hash: entity.entity_hash.clone(),
            thumbnail_hash: entity.thumbnail_hash,
            member_count: entity.member_count,
            name: entity.name,
            size: entity.size_bytes,
            mime: entity.mime_type,
            width: entity.pixel_width,
            height: entity.pixel_height,
            duration_ms: entity.duration_ms,
            num_frames: entity.frame_count,
            has_audio: entity.has_audio,
            status: crate::types::status_to_string(entity.status).to_string(),
            rating: entity.rating,
            view_count: 0,
            source_urls: entity
                .source_urls
                .as_ref()
                .and_then(|urls| serde_json::to_value(urls).ok()),
            imported_at: entity.date_added,
            has_thumbnail,
            dominant_color_hex: entity.dominant_color_hex,
            dominant_colors: Some(
                colors
                    .into_iter()
                    .map(|(hex, l, a, b)| crate::types::DominantColorDto { hex, l, a, b })
                    .collect(),
            ),
            notes: entity
                .notes
                .as_ref()
                .and_then(|notes| serde_json::from_str(notes).ok()),
            created_at: Some(entity.date_created),
            updated_at: Some(entity.date_modified),
        };

        Ok(Some(crate::types::EntityAllMetadata {
            entity: entity_dto,
            tags,
            parent_tags,
        }))
    }

    pub fn get_view_pref(
        &self,
        scope: &str,
    ) -> Result<Option<crate::settings::types::ViewPref>, String> {
        self.with_read(|conn| query::settings::get_view_pref_with_fallback(conn, scope))
    }

    pub fn set_view_pref(&self, pref: crate::settings::types::ViewPref) -> Result<(), String> {
        self.with_write(|conn| query::settings::set_view_pref(conn, &pref))
    }

    pub fn get_folder_entity_hashes(&self, folder_id: i64) -> Result<Vec<String>, String> {
        self.with_read(|conn| query::folders::get_folder_entity_hashes(conn, folder_id))
    }

    pub fn get_folder_cover_hash(&self, folder_id: i64) -> Result<Option<String>, String> {
        self.with_read(|conn| query::folders::get_folder_cover_hash(conn, folder_id))
    }

    pub fn get_entity_folder_memberships(
        &self,
        entity_hash: &str,
    ) -> Result<Vec<types::FolderMembership>, String> {
        let hash = entity_hash.to_string();
        self.with_read(|conn| {
            let entity_id: Option<i64> = conn
                .query_row(
                    "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
                    [&hash],
                    |row| row.get(0),
                )
                .optional()?;
            let Some(entity_id) = entity_id else {
                return Ok(Vec::new());
            };
            query::folders::get_entity_folder_memberships(conn, entity_id)
        })
    }

    pub fn get_entity_folder_memberships_by_entity_id(
        &self,
        entity_id: i64,
    ) -> Result<Vec<types::FolderMembership>, String> {
        self.with_read(|conn| query::folders::get_entity_folder_memberships(conn, entity_id))
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
            query::duplicates::get_duplicate_find_source(conn, &source_hash)
        })?;

        let Some(source) = source else {
            return Ok(crate::types::FindSimilarResponse {
                source_hash,
                items: Vec::new(),
            });
        };
        let Some(source_phash) = source.perceptual_hash else {
            return Ok(crate::types::FindSimilarResponse {
                source_hash,
                items: Vec::new(),
            });
        };

        if !crate::media_capabilities::capabilities_for_stored_media(
            &source.mime_type,
            source.frame_count,
        )
        .can_perceptual_hash
        {
            return Ok(crate::types::FindSimilarResponse {
                source_hash,
                items: Vec::new(),
            });
        }

        let candidates = self.with_read(|conn| {
            query::duplicates::list_perceptual_hash_sources(conn, Some(&source_hash))
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
            .filter_map(|candidate| {
                if !crate::media_capabilities::capabilities_for_stored_media(
                    &candidate.mime_type,
                    candidate.frame_count,
                )
                .can_perceptual_hash
                {
                    return None;
                }
                let candidate_hash =
                    ImageHash::<Vec<u8>>::from_base64(&candidate.perceptual_hash).ok()?;
                Some(crate::types::SimilarItem {
                    hash: candidate.entity_hash,
                    distance: source_hash_image.dist(&candidate_hash),
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
        let files = self.with_read(query::duplicates::list_duplicate_scan_sources)?;

        use img_hash::ImageHash;
        let parsed: Vec<(i64, String, ImageHash<Vec<u8>>)> = files
            .iter()
            .filter_map(|row| {
                if !crate::media_capabilities::capabilities_for_stored_media(
                    &row.mime_type,
                    row.frame_count,
                )
                .can_perceptual_hash
                {
                    return None;
                }
                Some((
                    row.file_id,
                    row.entity_hash.clone(),
                    ImageHash::<Vec<u8>>::from_base64(&row.perceptual_hash).ok()?,
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
            write::duplicates::insert_duplicate_pairs_for_scan(conn, &candidate_pairs)
        })?;

        let reviewable_detected_total = self.with_read(|conn| {
            query::duplicates::count_duplicate_pairs_with_max_distance(
                conn,
                "detected",
                review_threshold as i64,
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
        self.with_read(move |conn| {
            query::duplicates::get_duplicate_pairs_paginated(
                conn,
                cursor.as_deref(),
                limit,
                &status_filter,
                max_distance,
            )
        })
    }

    pub fn get_duplicate_count(&self) -> Result<i64, String> {
        self.with_read(|conn| query::duplicates::count_duplicate_pairs(conn, "detected"))
    }

    pub fn resolve_duplicate_pair(
        &self,
        action: &str,
        hash_a: &str,
        hash_b: &str,
        preferred_collection_id: Option<i64>,
    ) -> Result<types::DuplicateResolutionResult, String> {
        let action = action.to_string();
        let hash_a = hash_a.to_string();
        let hash_b = hash_b.to_string();
        self.with_write(move |conn| {
            let left = query::duplicates::get_duplicate_single_ref_by_hash(conn, &hash_a)?;
            let right = query::duplicates::get_duplicate_single_ref_by_hash(conn, &hash_b)?;
            write::duplicates::resolve_duplicate_pair(
                conn,
                &action,
                left,
                right,
                preferred_collection_id,
            )
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

#[cfg(test)]
mod tests {
    use super::LibraryDatabase;
    use crate::background_work::{DeferredWorkFilter, DeferredWorkType};
    use crate::db::core::schema::LIBRARY_DDL;
    use crate::db::types::{BaseScope, DuplicateResolveStatus, EntityViewQuery, QueryFilters, QueryPage, QuerySort, ScopeKind};
    use crate::media_analysis::ensure_missing_color_analysis_jobs;
    use crate::media_analysis::TARGET_COLOR_ANALYSIS_VERSION;
    use crate::media_processing::colors::{serialize_dominant_palette_blob, DominantColor};
    use rusqlite::params;
    use tempfile::TempDir;

    fn open_test_db() -> LibraryDatabase {
        let tmp = TempDir::new().expect("tempdir");
        let db = LibraryDatabase::open(tmp.path()).expect("open library db");
        std::mem::forget(tmp);
        db
    }

    #[test]
    fn get_file_colors_for_entity_hash_prefers_blob_and_falls_back_to_index() {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
                 ) VALUES (1, 'with_blob', 'single', 1, 'Blob', '2026-04-01', '2026-04-01', '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
                 ) VALUES (2, 'fallback', 'single', 1, 'Fallback', '2026-04-01', '2026-04-01', '2026-04-01')",
                [],
            )?;

            let blob = serialize_dominant_palette_blob(&[DominantColor {
                hex: "#abcdef".into(),
                l: 10.0,
                a: 1.0,
                b: 2.0,
            }])
            .expect("serialize");

            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, has_audio, dominant_color_hex,
                    dominant_palette_blob, color_analysis_version, date_added
                 ) VALUES (1, 'file_blob', 'image/png', 1, 0, '#abcdef', ?1, ?2, '2026-04-01')",
                rusqlite::params![blob, TARGET_COLOR_ANALYSIS_VERSION],
            )?;
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, has_audio, dominant_color_hex,
                    color_analysis_version, date_added
                 ) VALUES (2, 'file_fallback', 'image/png', 1, 0, '#123456', 0, '2026-04-01')",
                [],
            )?;
            conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1)", [])?;
            conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (2, 2)", [])?;
            conn.execute(
                "INSERT INTO file_color (file_id, hex, l, a, b) VALUES (2, '#fedcba', 20.0, 3.0, 4.0)",
                [],
            )?;
            Ok(())
        })
        .expect("seed db");

        let blob_colors = db
            .get_file_colors_for_entity_hash("with_blob")
            .expect("get blob colors");
        let fallback_colors = db
            .get_file_colors_for_entity_hash("fallback")
            .expect("get fallback colors");

        assert_eq!(blob_colors[0].0, "#abcdef");
        assert_eq!(fallback_colors[0].0, "#fedcba");
    }

    #[test]
    fn open_repairs_missing_ingest_queue_tables_for_existing_canonical_db() {
        let tmp = TempDir::new().expect("tempdir");
        let db_path = tmp.path().join("library.db");
        let conn = rusqlite::Connection::open(&db_path).expect("open raw db");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute_batch(
            "DROP INDEX IF EXISTS idx_ingest_queue_ready;
             DROP INDEX IF EXISTS idx_ingest_queue_subscription;
             DROP INDEX IF EXISTS idx_ingest_queue_item_queue;
             DROP TABLE IF EXISTS ingest_queue_item;
             DROP TABLE IF EXISTS ingest_queue;",
        )
        .expect("drop queue tables");
        drop(conn);

        let db = LibraryDatabase::open(tmp.path()).expect("repair schema");
        db.with_read(|conn| {
            let queue_exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'ingest_queue'",
                [],
                |row| row.get(0),
            )?;
            let item_exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'ingest_queue_item'",
                [],
                |row| row.get(0),
            )?;
            let ready_index_exists: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = 'idx_ingest_queue_ready'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(queue_exists, 1);
            assert_eq!(item_exists, 1);
            assert_eq!(ready_index_exists, 1);
            Ok(())
        })
        .expect("inspect repaired schema");
    }

    #[test]
    fn smart_folder_scope_query_matches_runtime_compiled_bitmap_and_sidebar_count() {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, rating, date_created, date_added, date_modified
                 ) VALUES
                    (1, 'entity_1', 'single', 1, 'Landscape', 5, '2026-04-01', '2026-04-01', '2026-04-01'),
                    (2, 'entity_2', 'single', 1, 'Portrait', 2, '2026-04-02', '2026-04-02', '2026-04-02')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, has_audio, date_added
                 ) VALUES
                    (1, 'file_1', 'image/png', 100, 1000, 500, 0, '2026-04-01'),
                    (2, 'file_2', 'image/jpeg', 100, 500, 1000, 0, '2026-04-02')",
                [],
            )?;
            conn.execute(
                "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO tag (tag_id, namespace, subtag, file_count) VALUES (1, 'general', 'landscape', 1)",
                [],
            )?;
            conn.execute(
                "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source) VALUES (1, 1, 1, 'local')",
                [],
            )?;
            conn.execute(
                "INSERT INTO file_color (file_id, hex, l, a, b) VALUES (1, '#ff0000', 50.0, 60.0, 70.0)",
                [],
            )?;
            conn.execute(
                "INSERT INTO smart_folder (
                    smart_folder_id, name, predicate_json, date_added, date_modified
                 ) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![
                    7_i64,
                    "Smart Landscape",
                    serde_json::json!({
                        "groups": [{
                            "match_mode": "all",
                            "negate": false,
                            "rules": [
                                { "field": "tags", "op": "include_all", "values": ["landscape"] },
                                { "field": "color", "op": "contains", "values": ["#ff0000"] },
                                { "field": "rating", "op": "gte", "value": 4 }
                            ]
                        }]
                    }).to_string(),
                    "2026-04-01T00:00:00Z",
                ],
            )?;
            Ok(())
        })
        .expect("seed smart folder data");

        db.full_rebuild();

        let page = db
            .query_entity_view(&EntityViewQuery {
                base_scope: BaseScope {
                    kind: ScopeKind::SmartFolder,
                    key: None,
                    id: Some(7),
                },
                filters: QueryFilters::default(),
                sort: QuerySort::default(),
                page: QueryPage::default(),
            })
            .expect("query smart folder scope");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].entity_hash, "entity_1");
        assert_eq!(
            db.bitmap_len(&crate::db::projection::bitmaps::BitmapKey::SmartFolder(7)),
            1
        );
        db.with_read(|conn| {
            let count: i64 = conn.query_row(
                "SELECT count FROM sidebar_node WHERE node_id = 'smart:7'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(count, 1);
            Ok(())
        })
        .expect("read sidebar count");
    }

    #[test]
    fn resolve_duplicate_pair_requires_explicit_collection_choice_for_cross_collection_members() {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
                 ) VALUES
                    (1, 'collection_left', 'collection', 1, 'Left Collection', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (2, 'collection_right', 'collection', 1, 'Right Collection', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (3, 'left_single', 'single', 1, 'Left Single', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (4, 'right_single', 'single', 1, 'Right Single', '2026-04-01', '2026-04-01', '2026-04-01')",
                [],
            )?;
            conn.execute(
                "UPDATE media_entity
                 SET parent_collection_entity_id = 1, collection_ordinal = 1
                 WHERE entity_id = 3",
                [],
            )?;
            conn.execute(
                "UPDATE media_entity
                 SET parent_collection_entity_id = 2, collection_ordinal = 1
                 WHERE entity_id = 4",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, has_audio,
                    perceptual_hash, date_added
                 ) VALUES
                    (1, 'file_left', 'image/png', 1, 100, 100, 0, 'hash_left', '2026-04-01'),
                    (2, 'file_right', 'image/png', 1, 100, 100, 0, 'hash_right', '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO single_media_entity (entity_id, file_id) VALUES (3, 1), (4, 2)",
                [],
            )?;
            Ok(())
        })
        .expect("seed duplicate conflict data");

        let result = db
            .resolve_duplicate_pair("keep_left", "left_single", "right_single", None)
            .expect("resolve duplicate conflict");

        assert!(matches!(result.status, DuplicateResolveStatus::Conflict));
        let conflict = result.conflict.expect("conflict payload");
        assert_eq!(conflict.winner_collection_id, Some(1));
        assert_eq!(conflict.loser_collection_id, Some(2));
    }

    #[test]
    fn enqueue_stale_color_analysis_jobs_only_queues_stale_color_capable_rows() {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
                 ) VALUES
                    (1, 'stale_image', 'single', 1, 'Stale', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (2, 'fresh_image', 'single', 1, 'Fresh', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (3, 'audio_only', 'single', 1, 'Audio', '2026-04-01', '2026-04-01', '2026-04-01')",
                [],
            )?;

            let blob = serialize_dominant_palette_blob(&[DominantColor {
                hex: "#010203".into(),
                l: 1.0,
                a: 2.0,
                b: 3.0,
            }])
            .expect("serialize");

            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, has_audio, color_analysis_version, date_added
                 ) VALUES
                    (1, 'stale_file', 'image/png', 1, 0, 0, '2026-04-01'),
                    (3, 'audio_file', 'audio/mpeg', 1, 1, 0, '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, has_audio, dominant_palette_blob,
                    color_analysis_version, date_added
                 ) VALUES (2, 'fresh_file', 'image/png', 1, 0, ?1, ?2, '2026-04-01')",
                rusqlite::params![blob, TARGET_COLOR_ANALYSIS_VERSION],
            )?;
            conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1)", [])?;
            conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (2, 2)", [])?;
            conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (3, 3)", [])?;
            Ok(())
        })
        .expect("seed db");

        let queued = db
            .enqueue_stale_color_analysis_jobs(TARGET_COLOR_ANALYSIS_VERSION)
            .expect("enqueue stale colors");
        let jobs = db
            .list_deferred_work_items(DeferredWorkFilter::default())
            .expect("list jobs");

        assert_eq!(queued, 1);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].entity_hash, "stale_image");
        assert_eq!(jobs[0].work_type, DeferredWorkType::DominantColors);
    }

    #[test]
    fn ensure_deferred_jobs_present_does_not_reset_existing_running_job() {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO deferred_work_item (
                    entity_hash, work_type, status, attempt_count, available_at, queued_at, started_at
                 ) VALUES (?1, 'dominant_colors', 'running', 3, '2026-04-01', '2026-04-01', '2026-04-01T00:00:01Z')",
                ["hash_a"],
            )?;
            Ok(())
        })
        .expect("seed deferred work");

        db.ensure_deferred_jobs_present("hash_a", &[DeferredWorkType::DominantColors])
            .expect("ensure deferred work");

        let jobs = db
            .list_deferred_work_items(DeferredWorkFilter::default())
            .expect("list jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].entity_hash, "hash_a");
        assert_eq!(jobs[0].work_type, DeferredWorkType::DominantColors);
        assert_eq!(jobs[0].status, crate::background_work::DeferredWorkStatus::Running);
        assert_eq!(jobs[0].attempt_count, 3);
        assert_eq!(jobs[0].started_at.as_deref(), Some("2026-04-01T00:00:01Z"));
    }

    #[test]
    fn ensure_missing_color_analysis_jobs_only_queues_missing_colors_once() {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO media_entity (
                    entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
                 ) VALUES
                    (1, 'stale_image', 'single', 1, 'Stale', '2026-04-01', '2026-04-01', '2026-04-01'),
                    (2, 'fresh_image', 'single', 1, 'Fresh', '2026-04-01', '2026-04-01', '2026-04-01')",
                [],
            )?;
            let blob = serialize_dominant_palette_blob(&[DominantColor {
                hex: "#010203".into(),
                l: 1.0,
                a: 2.0,
                b: 3.0,
            }])
            .expect("serialize");
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, has_audio, color_analysis_version, date_added
                 ) VALUES (1, 'stale_file', 'image/png', 1, 0, 0, '2026-04-01')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_file (
                    file_id, file_hash, mime_type, size_bytes, has_audio, dominant_palette_blob,
                    color_analysis_version, date_added
                 ) VALUES (2, 'fresh_file', 'image/png', 1, 0, ?1, ?2, '2026-04-01')",
                rusqlite::params![blob, TARGET_COLOR_ANALYSIS_VERSION],
            )?;
            conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1)", [])?;
            conn.execute("INSERT INTO single_media_entity (entity_id, file_id) VALUES (2, 2)", [])?;
            Ok(())
        })
        .expect("seed db");

        let hashes = vec![
            "stale_image".to_string(),
            "fresh_image".to_string(),
            "stale_image".to_string(),
        ];
        ensure_missing_color_analysis_jobs(&db, &hashes).expect("first ensure");
        ensure_missing_color_analysis_jobs(&db, &hashes).expect("second ensure");

        let jobs = db
            .list_deferred_work_items(DeferredWorkFilter::default())
            .expect("list jobs");
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].entity_hash, "stale_image");
        assert_eq!(jobs[0].work_type, DeferredWorkType::DominantColors);
    }

    #[test]
    fn deferred_work_summary_reports_dominant_color_backlog_counts() {
        let db = open_test_db();
        db.with_write(|conn| {
            conn.execute_batch(LIBRARY_DDL)?;
            conn.execute(
                "INSERT INTO deferred_work_item (
                    entity_hash, work_type, status, attempt_count, available_at, queued_at
                 ) VALUES
                    ('pending_color', 'dominant_colors', 'pending', 0, '2026-04-01', '2026-04-01'),
                    ('failed_color', 'dominant_colors', 'pending', 2, '2026-04-01', '2026-04-01'),
                    ('running_color', 'dominant_colors', 'running', 0, '2026-04-01', '2026-04-01'),
                    ('thumb_job', 'thumbnail', 'pending', 0, '2026-04-01', '2026-04-01')",
                [],
            )?;
            Ok(())
        })
        .expect("seed deferred summary");

        let summary = db
            .get_deferred_work_summary()
            .expect("get deferred summary");

        assert_eq!(summary.pending_count, 3);
        assert_eq!(summary.running_count, 1);
        assert_eq!(summary.failed_count, 1);
        assert_eq!(summary.dominant_colors_pending_count, 1);
        assert_eq!(summary.dominant_colors_running_count, 1);
        assert_eq!(summary.dominant_colors_failed_count, 1);
    }
}
