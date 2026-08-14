//! Library database boundary.
//!
//! All SQL and storage details are contained within this module tree.
//! Code outside `db/` consumes typed methods and typed results only.
//! No SQL, no table names, no bitmap storage details leak out.

mod collection_ops;
pub mod core;
mod deferred_ops;
mod folder_ops;
pub mod projection;
pub mod query;
mod remote_ops;
mod smart_folder_ops;
mod tag_ops;
pub mod types;
mod view_ops;
pub mod write;

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::{Connection, OptionalExtension};

use crate::blob_store::BlobHashLease;

use self::projection::bitmaps::BitmapStore;
use self::types::*;

fn folder_uuid(conn: &Connection, folder_id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT uuid FROM folder WHERE folder_id = ?1",
        [folder_id],
        |row| row.get(0),
    )
    .optional()
}

fn smart_folder_uuid(conn: &Connection, smart_folder_id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT uuid FROM smart_folder WHERE smart_folder_id = ?1",
        [smart_folder_id],
        |row| row.get(0),
    )
    .optional()
}

/// Canonical tag key for ops: `namespace:subtag` (namespace may be empty;
/// replay splits at the first colon).
fn tag_op_key(conn: &Connection, tag_id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT namespace || ':' || subtag FROM tag WHERE tag_id = ?1",
        [tag_id],
        |row| row.get(0),
    )
    .optional()
}

fn entity_hashes_for_ids(conn: &Connection, ids: &[i64]) -> rusqlite::Result<Vec<String>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        let hash: Option<String> = conn
            .query_row(
                "SELECT entity_hash FROM media_entity WHERE entity_id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(hash) = hash {
            out.push(hash);
        }
    }
    Ok(out)
}

fn find_perceptual_hash_candidates_on_conn(
    conn: &Connection,
    perceptual_hash: &str,
    threshold: u32,
) -> rusqlite::Result<Vec<types::PerceptualHashCandidate>> {
    let Some(source) = crate::duplicates::phash::parse_supported_hash(perceptual_hash) else {
        return Ok(Vec::new());
    };

    let rows = if threshold <= crate::duplicates::phash::MAX_INDEXED_DISTANCE {
        let Some(partitions) = crate::duplicates::phash::indexed_partition_values(perceptual_hash)
        else {
            return Ok(Vec::new());
        };
        query::duplicates::list_indexed_perceptual_hash_sources(conn, &partitions)?
    } else {
        query::duplicates::list_perceptual_hash_sources(conn)?
    };

    let mut candidates = Vec::new();
    for row in rows {
        if !crate::media_capabilities::capabilities_for_stored_media(
            &row.mime_type,
            row.frame_count,
        )
        .can_perceptual_hash
        {
            continue;
        }
        let Some(candidate_hash) =
            crate::duplicates::phash::parse_supported_hash(&row.perceptual_hash)
        else {
            continue;
        };
        let distance = source.dist(&candidate_hash);
        if distance <= threshold {
            candidates.push(types::PerceptualHashCandidate {
                file_id: row.file_id,
                distance,
            });
        }
    }
    candidates.sort_by(|left, right| {
        left.distance
            .cmp(&right.distance)
            .then_with(|| left.file_id.cmp(&right.file_id))
    });
    Ok(candidates)
}

fn record_duplicate_review_candidates_on_conn(
    conn: &Connection,
    imported_file_id: i64,
    candidates: &[types::PerceptualHashCandidate],
) -> rusqlite::Result<()> {
    for candidate in candidates {
        write::duplicates::upsert_duplicate_pair_for_review(
            conn,
            imported_file_id,
            candidate.file_id,
            candidate.distance,
        )?;
    }
    Ok(())
}

/// Everything replay needs to materialize an ingested single (the blob itself
/// is fetched by hash). Derived fields (phash, colors) are excluded.
fn ingest_entity_created_payload(prepared: &types::IngestPreparedSingle) -> serde_json::Value {
    serde_json::json!({
        "kind": "single",
        "name": prepared.name,
        "status": prepared.status,
        "mime": prepared.mime_type,
        "size": prepared.size_bytes,
        "width": prepared.pixel_width,
        "height": prepared.pixel_height,
        "duration_ms": prepared.duration_ms,
        "frame_count": prepared.frame_count,
        "has_audio": prepared.has_audio,
        "date_created": prepared.date_created,
        "notes": prepared.notes,
        "source_urls": prepared.source_urls,
        "tags": prepared.tag_strings,
        "tag_provenance": prepared.tag_provenance_mask.to_string(),
    })
}

fn insert_deferred_work_rows(
    conn: &Connection,
    entity_hash: &str,
    work_types: &[crate::background_work::DeferredWorkType],
) -> rusqlite::Result<()> {
    if work_types.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let mut stmt = conn.prepare(
        "INSERT INTO deferred_work_item
             (entity_hash, work_type, status, attempt_count, available_at, queued_at)
         VALUES
             (?1, ?2, 'pending', 0, ?3, ?3)
         ON CONFLICT(entity_hash, work_type) DO NOTHING",
    )?;
    for work_type in work_types {
        stmt.execute(rusqlite::params![entity_hash, work_type.as_db_str(), now])?;
    }
    Ok(())
}

/// Insert one prepared media entity inside the caller's write transaction.
///
/// Collection materialization deliberately passes no deferred work because its
/// caller batches those rows after the collection transaction commits.
fn insert_prepared_single(
    conn: &Connection,
    device_id: &str,
    prepared: &types::IngestPreparedSingle,
    deferred_work_types: &[crate::background_work::DeferredWorkType],
) -> rusqlite::Result<(i64, i64)> {
    let source_urls_json = if prepared.source_urls.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&prepared.source_urls).unwrap_or_default())
    };

    // Clean up any orphan media_file left by a deleted entity before inserting,
    // so we don't hit a UNIQUE constraint on file_hash.
    conn.execute(
        "DELETE FROM media_file WHERE file_hash = ?1
         AND file_id NOT IN (SELECT file_id FROM single_media_entity)",
        rusqlite::params![prepared.entity_hash],
    )?;
    let file_id = write::files::insert_file(
        conn,
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
        write::files::replace_file_phash(conn, file_id, Some(phash))?;
    }
    let entity_id = write::entities::insert_single(
        conn,
        &prepared.entity_hash,
        file_id,
        prepared.name.as_deref(),
        prepared.status,
        &prepared.date_created,
        &prepared.date_added,
    )?;
    if prepared.notes.is_some() || !prepared.source_urls.is_empty() {
        write::entities::patch_entity_metadata(
            conn,
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
            conn,
            &[entity_id],
            &prepared.tag_strings,
            prepared.tag_provenance_mask,
            types::ExpansionMode::EntityOnly,
        )?;
    }
    insert_deferred_work_rows(conn, &prepared.entity_hash, deferred_work_types)?;
    crate::oplog::record_op(
        conn,
        device_id,
        "entity_created",
        &prepared.entity_hash,
        &ingest_entity_created_payload(prepared),
    )?;

    Ok((entity_id, file_id))
}

/// Sync-relevant fields of an entity metadata patch (absent = unchanged,
/// null = cleared, mirroring the patch semantics).
fn entity_patch_payload(patch: &types::MediaEntityPatch) -> serde_json::Value {
    let mut fields = serde_json::Map::new();
    if let Some(v) = &patch.name {
        fields.insert("name".into(), v.clone().into());
    }
    if let Some(v) = patch.rating {
        fields.insert("rating".into(), v.into());
    }
    if let Some(v) = &patch.notes {
        fields.insert("notes".into(), serde_json::json!(v));
    }
    if let Some(v) = &patch.source_urls {
        fields.insert("source_urls".into(), serde_json::json!(v));
    }
    serde_json::Value::Object(fields)
}

fn emit_folder_membership_op(
    conn: &Connection,
    device_id: &str,
    op_type: &str,
    folder_id: i64,
    entity_ids: &[i64],
) -> rusqlite::Result<()> {
    if entity_ids.is_empty() {
        return Ok(());
    }
    let Some(uuid) = folder_uuid(conn, folder_id)? else {
        return Ok(());
    };
    let entities = entity_hashes_for_ids(conn, entity_ids)?;
    crate::oplog::record_op(
        conn,
        device_id,
        op_type,
        &uuid,
        &serde_json::json!({ "entities": entities }),
    )
}

fn emit_collection_membership_op(
    conn: &Connection,
    device_id: &str,
    collection_id: i64,
    change: &types::CollectionMembershipChange,
) -> rusqlite::Result<()> {
    let hash = change
        .collection_hash
        .clone()
        .or(query::collections::get_collection_hash(
            conn,
            collection_id,
        )?);
    let Some(hash) = hash else {
        return Ok(());
    };
    if change.deleted_collection {
        return crate::oplog::record_op(
            conn,
            device_id,
            "collection_split",
            &hash,
            &serde_json::json!({}),
        );
    }
    if !change.added.is_empty() {
        let members = entity_hashes_for_ids(conn, &change.added)?;
        crate::oplog::record_op(
            conn,
            device_id,
            "collection_members_added",
            &hash,
            &serde_json::json!({ "members": members }),
        )?;
    }
    if !change.removed.is_empty() {
        let members = entity_hashes_for_ids(conn, &change.removed)?;
        crate::oplog::record_op(
            conn,
            device_id,
            "collection_members_removed",
            &hash,
            &serde_json::json!({ "members": members }),
        )?;
    }
    Ok(())
}

/// Record the same op once per entity hash (per-entity keying keeps replay
/// merge rules simple; a bulk action fans out to one op per entity).
fn emit_per_entity(
    conn: &Connection,
    device_id: &str,
    op_type: &str,
    hashes: &[String],
    payload: &serde_json::Value,
) -> rusqlite::Result<()> {
    for hash in hashes {
        crate::oplog::record_op(conn, device_id, op_type, hash, payload)?;
    }
    Ok(())
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
    /// This installation's stable device identity, stamped on outbox ops.
    device_id: String,
}

impl Drop for LibraryDatabase {
    fn drop(&mut self) {
        // Backstop only — close_library checkpoints explicitly, because a
        // detached worker can hold an Arc past close and delay this drop.
        self.checkpoint();
    }
}

impl LibraryDatabase {
    /// Open or create a library database at the given root path.
    pub fn open(library_root: &Path) -> Result<Self, String> {
        Self::open_with_device_id(library_root, crate::oplog::device_id())
    }

    /// Open with an explicit device identity (tests simulate multiple
    /// devices in one process; a future host may pass its own identity).
    pub fn open_with_device_id(library_root: &Path, device_id: String) -> Result<Self, String> {
        let db_path = library_root.join("library.db");
        let write_conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open write connection: {e}"))?;
        let read_conn = Connection::open(&db_path)
            .map_err(|e| format!("Failed to open read connection: {e}"))?;

        // Configure connections. The write connection runs synchronous=FULL so
        // an acknowledged commit survives power loss, not just process crash.
        write_conn
            .execute_batch(
                "PRAGMA journal_mode = WAL; PRAGMA synchronous = FULL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = 5000;",
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
            device_id,
        };

        // Pre-1.0 libraries must match this build's schema exactly. We create
        // empty libraries, but never mutate or delete an incompatible database.
        {
            let conn = db.write_conn.lock().unwrap();
            core::schema::initialize_schema(&conn)?;
            projection::compiler::full_rebuild(&conn, &db.bitmaps);
            let _ = std::fs::remove_file(library_root.join("bitmaps.delta"));
        }

        Ok(db)
    }

    // ── Internal connection access (crate-private) ─────────────────

    /// Execute a write operation. Only accessible within db/.
    ///
    /// The closure runs inside a single transaction: an error rolls back every
    /// statement it issued. Closures must not open their own transactions.
    pub(crate) fn with_write<F, R>(&self, f: F) -> Result<R, String>
    where
        F: FnOnce(&Connection) -> rusqlite::Result<R>,
    {
        let conn = self.write_conn.lock().unwrap();
        debug_assert!(
            conn.is_autocommit(),
            "with_write entered with a transaction already open"
        );
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let result = f(&tx).map_err(|e| e.to_string())?;
        debug_assert!(
            !tx.is_autocommit(),
            "with_write closure must not commit or roll back the outer transaction"
        );
        tx.commit().map_err(|e| e.to_string())?;
        Ok(result)
    }

    // ── Sync op outbox ───────────────────────────────────────────

    /// This installation's device identity (stamped on outbox ops).
    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn kv_get(&self, key: &str) -> Result<Option<String>, String> {
        let key = key.to_string();
        self.with_read(move |conn| write::settings::get_kv(conn, &key))
    }

    pub fn kv_set(&self, key: &str, value: &str) -> Result<(), String> {
        let key = key.to_string();
        let value = value.to_string();
        self.with_write(move |conn| write::settings::set_kv(conn, &key, &value))
    }

    pub fn kv_delete(&self, key: &str) -> Result<(), String> {
        let key = key.to_string();
        self.with_write(move |conn| {
            conn.execute("DELETE FROM kv_settings WHERE key = ?1", [&key])
                .map(|_| ())
        })
    }

    /// Stable identity of this library's truth lineage. Minted on first use;
    /// shared across all devices syncing the same library.
    pub fn library_uuid(&self) -> Result<String, String> {
        if let Some(existing) = self.kv_get("library_uuid")? {
            if !existing.is_empty() {
                return Ok(existing);
            }
        }
        let minted = crate::oplog::new_uuid();
        self.kv_set("library_uuid", &minted)?;
        Ok(minted)
    }

    /// Adopt a remote library's lineage. Only legal when this library has no
    /// identity yet, already matches, or holds no truth (fresh install).
    pub fn adopt_library_uuid(&self, uuid: &str) -> Result<(), String> {
        match self.kv_get("library_uuid")? {
            Some(existing) if !existing.is_empty() => {
                if existing == uuid {
                    return Ok(());
                }
                if !self.truth_is_empty()? {
                    return Err(
                        "This local library already belongs to a different sync lineage. \
                         Connecting it here would merge two unrelated libraries. Use an \
                         empty library to connect, or connect to this library's own remote."
                            .to_string(),
                    );
                }
                self.kv_set("library_uuid", uuid)
            }
            _ => self.kv_set("library_uuid", uuid),
        }
    }

    /// True when the library holds no truth rows (safe to adopt any lineage).
    pub fn truth_is_empty(&self) -> Result<bool, String> {
        self.with_read(|conn| {
            let entities: i64 =
                conn.query_row("SELECT COUNT(*) FROM media_entity", [], |r| r.get(0))?;
            let folders: i64 = conn.query_row("SELECT COUNT(*) FROM folder", [], |r| r.get(0))?;
            let smart: i64 =
                conn.query_row("SELECT COUNT(*) FROM smart_folder", [], |r| r.get(0))?;
            Ok(entities == 0 && folders == 0 && smart == 0)
        })
    }

    /// Outbox ops not yet shipped to the remote.
    pub fn pending_op_count(&self) -> Result<i64, String> {
        self.with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM op_outbox WHERE uploaded_seq IS NULL",
                [],
                |r| r.get(0),
            )
        })
    }

    /// Oldest not-yet-uploaded outbox ops, as `(op_id, record)`.
    pub fn pending_ops(&self, limit: usize) -> Result<Vec<(i64, crate::oplog::OpRecord)>, String> {
        self.with_read(|conn| {
            let mut stmt = conn.prepare(
                "SELECT op_id, op_version, op_type, entity_key, payload_json, hlc, device_id
                 FROM op_outbox WHERE uploaded_seq IS NULL ORDER BY op_id LIMIT ?1",
            )?;
            let rows = stmt
                .query_map([limit as i64], |row| {
                    let payload_json: String = row.get(4)?;
                    Ok((
                        row.get::<_, i64>(0)?,
                        crate::oplog::OpRecord {
                            op_version: row.get(1)?,
                            op_type: row.get(2)?,
                            entity_key: row.get(3)?,
                            payload: serde_json::from_str(&payload_json)
                                .unwrap_or(serde_json::Value::Null),
                            hlc: row.get(5)?,
                            device_id: row.get(6)?,
                        },
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Next segment sequence number for this device's remote prefix.
    pub fn next_segment_seq(&self) -> Result<i64, String> {
        self.with_read(|conn| {
            conn.query_row(
                "SELECT COALESCE(MAX(uploaded_seq), 0) + 1 FROM op_outbox",
                [],
                |row| row.get(0),
            )
        })
    }

    /// Highest contiguous remote segment already applied for a peer device.
    pub fn ingest_cursor(&self, peer_device_id: &str) -> Result<i64, String> {
        let device = peer_device_id.to_string();
        self.with_read(move |conn| {
            conn.query_row(
                "SELECT consumed_seq FROM sync_ingest_cursor WHERE device_id = ?1",
                [&device],
                |row| row.get(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0))
        })
    }

    /// Apply remote ops and advance peer cursors in ONE transaction, so a
    /// crash can never apply without advancing (or vice versa). Remote
    /// application goes through the low-level writers and never records to
    /// the outbox — remote ops must not echo back out as our own.
    pub fn apply_remote_ops(
        &self,
        ops: &[crate::oplog::OpRecord],
        cursor_updates: &[(String, i64)],
    ) -> Result<Option<usize>, String> {
        let conn = self.write_conn.lock().unwrap();
        let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
        let mut applied = 0usize;
        for op in ops {
            match remote_ops::apply_remote_op(&tx, op).map_err(|e| e.to_string())? {
                remote_ops::RemoteOpOutcome::Applied => applied += 1,
                remote_ops::RemoteOpOutcome::Pending(reason) => {
                    tx.rollback().map_err(|e| e.to_string())?;
                    tracing::info!(
                        op_type = op.op_type,
                        entity_key = op.entity_key,
                        reason,
                        "parking remote sync segment until its prerequisite exists"
                    );
                    return Ok(None);
                }
            }
        }
        for (device, seq) in cursor_updates {
            tx.execute(
                "INSERT INTO sync_ingest_cursor (device_id, consumed_seq) VALUES (?1, ?2)
                 ON CONFLICT(device_id) DO UPDATE SET consumed_seq = excluded.consumed_seq",
                rusqlite::params![device, seq],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        drop(conn);
        if applied > 0 {
            // Projections are derived; rebuild after remote truth changed.
            self.full_rebuild();
        }
        Ok(Some(applied))
    }

    /// Mark ops as shipped in the given segment.
    pub fn mark_ops_uploaded(&self, op_ids: &[i64], segment_seq: i64) -> Result<(), String> {
        let ids = op_ids.to_vec();
        self.with_write(move |conn| {
            let mut stmt =
                conn.prepare("UPDATE op_outbox SET uploaded_seq = ?1 WHERE op_id = ?2")?;
            for id in &ids {
                stmt.execute(rusqlite::params![segment_seq, id])?;
            }
            Ok(())
        })
    }

    /// Fold the WAL back into the main database file. Called explicitly on
    /// library close; `Drop` re-runs it as a backstop for stray references.
    pub fn checkpoint(&self) {
        if let Ok(conn) = self.write_conn.lock() {
            match conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
                row.get::<_, i64>(0)
            }) {
                Ok(0) => {}
                Ok(busy) => tracing::warn!(busy, "WAL checkpoint could not complete"),
                Err(e) => tracing::warn!("WAL checkpoint failed: {e}"),
            }
        }
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
            let id = write::entities::insert_collection(
                conn,
                entity_hash,
                name,
                date_created,
                date_added,
            )?;
            crate::oplog::record_op(
                conn,
                &self.device_id,
                "collection_created",
                entity_hash,
                &serde_json::json!({ "name": name, "date_created": date_created }),
            )?;
            Ok(id)
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
            let change =
                write::entities::set_entity_status(conn, entity_ids, status, expansion, &now)?;
            emit_per_entity(
                conn,
                &self.device_id,
                "entity_status_changed",
                &change.entity_hashes,
                &serde_json::json!({ "status": status }),
            )?;
            Ok(change)
        })
    }

    pub fn delete_entities(&self, entity_ids: &[i64]) -> Result<EntityChange, String> {
        self.with_write(|conn| {
            let change = write::entities::delete_entities(conn, entity_ids)?;
            emit_per_entity(
                conn,
                &self.device_id,
                "entity_deleted",
                &change.entity_hashes,
                &serde_json::json!({}),
            )?;
            Ok(change)
        })
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

    pub fn upsert_duplicate_pair_for_review(
        &self,
        file_id_a: i64,
        file_id_b: i64,
        distance: u32,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            write::duplicates::upsert_duplicate_pair_for_review(
                conn, file_id_a, file_id_b, distance,
            )
        })
    }

    pub(crate) fn insert_ingested_single_with_blob_lease(
        &self,
        prepared: &types::IngestPreparedSingle,
        deferred_work_types: &[crate::background_work::DeferredWorkType],
        duplicate_threshold: u32,
        _blob_lease: BlobHashLease,
    ) -> Result<(i64, bool), String> {
        let prepared = prepared.clone();
        let deferred_work = deferred_work_types.to_vec();
        self.with_write(move |conn| {
            let candidates = prepared
                .perceptual_hash
                .as_deref()
                .map(|phash| {
                    find_perceptual_hash_candidates_on_conn(conn, phash, duplicate_threshold)
                })
                .transpose()?
                .unwrap_or_default();
            let (entity_id, file_id) =
                insert_prepared_single(conn, &self.device_id, &prepared, &deferred_work)?;
            record_duplicate_review_candidates_on_conn(conn, file_id, &candidates)?;
            Ok((entity_id, !candidates.is_empty()))
        })
    }

    pub fn materialize_ingested_collection(
        &self,
        name: &str,
        new_members: &[types::IngestPreparedSingle],
        existing_member_ids: &[i64],
        existing_collection_id: Option<i64>,
    ) -> Result<(i64, String, Vec<String>), String> {
        let deferred_work = vec![Vec::new(); new_members.len()];
        let (collection_id, collection_hash, new_hashes, _) = self
            .materialize_ingested_collection_with_blob_leases(
                name,
                new_members,
                &deferred_work,
                existing_member_ids,
                existing_collection_id,
                crate::duplicates::phash::MAX_INDEXED_DISTANCE,
                Vec::new(),
            )?;
        Ok((collection_id, collection_hash, new_hashes))
    }

    pub(crate) fn materialize_ingested_collection_with_blob_leases(
        &self,
        name: &str,
        new_members: &[types::IngestPreparedSingle],
        deferred_work_by_member: &[Vec<crate::background_work::DeferredWorkType>],
        existing_member_ids: &[i64],
        existing_collection_id: Option<i64>,
        duplicate_threshold: u32,
        blob_leases: Vec<BlobHashLease>,
    ) -> Result<(i64, String, Vec<String>, bool), String> {
        if new_members.len() != deferred_work_by_member.len() {
            return Err("Collection ingest deferred work must match its members".into());
        }
        let collection_name = name.to_string();
        let prepared = new_members.to_vec();
        let deferred_work = deferred_work_by_member.to_vec();
        let existing_ids = existing_member_ids.to_vec();
        let blob_leases = blob_leases;
        self.with_write(move |conn| {
            let _blob_leases = blob_leases;
            let mut member_ids = existing_ids;
            let mut new_hashes = Vec::with_capacity(prepared.len());
            let mut duplicates_changed = false;

            for (member, member_work) in prepared.iter().zip(&deferred_work) {
                let candidates = member
                    .perceptual_hash
                    .as_deref()
                    .map(|phash| {
                        find_perceptual_hash_candidates_on_conn(conn, phash, duplicate_threshold)
                    })
                    .transpose()?
                    .unwrap_or_default();
                let (entity_id, file_id) =
                    insert_prepared_single(conn, &self.device_id, member, member_work)?;
                record_duplicate_review_candidates_on_conn(conn, file_id, &candidates)?;
                duplicates_changed |= !candidates.is_empty();
                member_ids.push(entity_id);
                new_hashes.push(member.entity_hash.clone());
            }

            if member_ids.is_empty() {
                return Err(rusqlite::Error::InvalidQuery);
            }

            let collection_id = if let Some(collection_id) = existing_collection_id {
                write::collections::add_members(&conn, collection_id, &member_ids)?;
                collection_id
            } else {
                let now = chrono::Utc::now().to_rfc3339();
                let collection_id =
                    write::collections::create_collection(&conn, &collection_name, &now)?;
                write::collections::add_members(&conn, collection_id, &member_ids)?;
                collection_id
            };

            let collection_hash = query::collections::get_collection_hash(&conn, collection_id)?
                .ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)?;
            if existing_collection_id.is_none() {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "collection_created",
                    &collection_hash,
                    &serde_json::json!({ "name": collection_name }),
                )?;
            }
            if !member_ids.is_empty() {
                let members = entity_hashes_for_ids(conn, &member_ids)?;
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "collection_members_added",
                    &collection_hash,
                    &serde_json::json!({ "members": members }),
                )?;
            }
            Ok((
                collection_id,
                collection_hash,
                new_hashes,
                duplicates_changed,
            ))
        })
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
            let change = write::entities::patch_entity_metadata(
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
                types::ExpansionMode::EntityOnly,
            )?;
            let payload = entity_patch_payload(patch);
            if payload.as_object().is_some_and(|o| !o.is_empty()) {
                emit_per_entity(
                    conn,
                    &self.device_id,
                    "entity_updated",
                    &change.entity_hashes,
                    &payload,
                )?;
            }
            Ok(change)
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
            write::entities::set_entity_date_created(conn, entity_id, &created, &now)?;
            crate::oplog::record_op(
                conn,
                &self.device_id,
                "entity_updated",
                &hash,
                &serde_json::json!({ "date_created": created }),
            )?;
            Ok(())
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
            // Metadata belongs to the selected entity only. Collections keep
            // structural fields, while child content remains child-owned.
            write::bulk::expand_bulk_target(conn, types::ExpansionMode::EntityOnly)?;
            let ids = write::bulk::collect_bulk_ids(conn)?;
            let change = write::entities::patch_entity_metadata(
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
            )?;
            let payload = entity_patch_payload(&p);
            if payload.as_object().is_some_and(|o| !o.is_empty()) {
                emit_per_entity(
                    conn,
                    &self.device_id,
                    "entity_updated",
                    &change.entity_hashes,
                    &payload,
                )?;
            }
            Ok(change)
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
            let change = write::entities::set_entity_status(
                conn,
                &ids,
                status,
                types::ExpansionMode::EntityOnly,
                &now,
            )?;
            emit_per_entity(
                conn,
                &self.device_id,
                "entity_status_changed",
                &change.entity_hashes,
                &serde_json::json!({ "status": status }),
            )?;
            Ok(change)
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
            let change = write::entities::delete_entities(conn, &ids)?;
            emit_per_entity(
                conn,
                &self.device_id,
                "entity_deleted",
                &change.entity_hashes,
                &serde_json::json!({}),
            )?;
            Ok(change)
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
            let change = write::tags::add_tags(
                conn,
                &ids,
                &t,
                provenance_mask,
                types::ExpansionMode::EntityOnly,
            )?;
            if !change.tags_added.is_empty() {
                let hashes = entity_hashes_for_ids(conn, &change.entity_ids)?;
                emit_per_entity(
                    conn,
                    &self.device_id,
                    "entity_tags_added",
                    &hashes,
                    &serde_json::json!({ "tags": change.tags_added, "provenance": provenance_mask.to_string() }),
                )?;
            }
            Ok(change)
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
            let change =
                write::tags::remove_tags(conn, &ids, &t, types::ExpansionMode::EntityOnly)?;
            if !change.tags_removed.is_empty() {
                let hashes = entity_hashes_for_ids(conn, &change.entity_ids)?;
                emit_per_entity(
                    conn,
                    &self.device_id,
                    "entity_tags_removed",
                    &hashes,
                    &serde_json::json!({ "tags": change.tags_removed }),
                )?;
            }
            Ok(change)
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
            let change = write::folders::add_members(
                conn,
                folder_id,
                &ids,
                types::ExpansionMode::EntityOnly,
            )?;
            emit_folder_membership_op(
                conn,
                &self.device_id,
                "folder_members_added",
                folder_id,
                &change.entity_ids,
            )?;
            Ok(change)
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
            let change = write::folders::remove_members(
                conn,
                folder_id,
                &ids,
                types::ExpansionMode::EntityOnly,
            )?;
            emit_folder_membership_op(
                conn,
                &self.device_id,
                "folder_members_removed",
                folder_id,
                &change.entity_ids,
            )?;
            Ok(change)
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

    pub fn get_all_tag_keys(&self) -> Result<Vec<(i64, String, String)>, String> {
        self.with_read(query::tags::get_all_tag_keys)
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
        let parent_tags =
            self.with_read(|conn| query::metadata::get_implied_tags(conn, entity_hash))?;

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
    ) -> Result<types::TagPage, String> {
        let mut page = self.with_read(|conn| {
            query::tags::get_tags_paginated(
                conn,
                namespace.as_deref(),
                search.as_deref(),
                cursor.as_deref(),
                limit,
            )
        })?;
        let active = self.bitmaps.get(&projection::bitmaps::BitmapKey::Status(1));
        for tag in &mut page.items {
            tag.file_count = self
                .bitmaps
                .get(&projection::bitmaps::BitmapKey::EffectiveTag(tag.tag_id))
                .intersection_len(&active) as i64;
        }
        Ok(page)
    }

    pub fn get_namespace_summary(&self) -> Result<Vec<types::NamespaceSummary>, String> {
        self.with_read(query::tags::get_namespace_summary)
    }

    pub fn scan_duplicates(
        &self,
        threshold: Option<u32>,
        review_threshold: Option<u32>,
    ) -> Result<types::DuplicateScanSummary, String> {
        let threshold = threshold.unwrap_or(crate::duplicates::phash::DEFAULT_DISTANCE_THRESHOLD);
        let review_threshold = review_threshold.unwrap_or(threshold);
        let files = self.with_read(query::duplicates::list_duplicate_scan_sources)?;

        let parsed: Vec<_> = files
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
                    crate::duplicates::phash::parse_supported_hash(&row.perceptual_hash)?,
                ))
            })
            .collect();

        let candidate_pairs = crate::duplicates::phash::find_candidate_pairs(&parsed, threshold);
        let closest_distance = candidate_pairs
            .iter()
            .map(|(_, _, distance)| *distance)
            .min();

        let newly_detected = self.with_write(|conn| {
            write::duplicates::reconcile_detected_duplicate_pairs(conn, &candidate_pairs)
        })?;
        let pairs_inserted = newly_detected.len();

        let reviewable_detected_total = self.with_read(|conn| {
            query::duplicates::count_duplicate_pairs_with_max_distance(
                conn,
                "detected",
                review_threshold as i64,
            )
        })? as usize;

        let reviewable_detected_new = newly_detected
            .iter()
            .filter(|(_, _, distance)| *distance <= review_threshold)
            .count();

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
            let result = write::duplicates::resolve_duplicate_pair(
                conn,
                &action,
                left,
                right,
                preferred_collection_id,
            )?;
            if matches!(result.status, types::DuplicateResolveStatus::Resolved) {
                // The detected pair is recomputable; the user's decision is truth.
                let (a, b) = if hash_a <= hash_b {
                    (&hash_a, &hash_b)
                } else {
                    (&hash_b, &hash_a)
                };
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "duplicate_decided",
                    &format!("{a}|{b}"),
                    &serde_json::json!({
                        "action": action,
                        "winner": result.winner_hash,
                        "loser": result.loser_hash,
                    }),
                )?;
            }
            Ok(result)
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
        let conn = self.write_conn.lock().unwrap();
        projection::compiler::execute_plan(&conn, &self.bitmaps, &plan);
    }

    /// Full rebuild of all projections from authoritative data.
    pub fn full_rebuild(&self) {
        let conn = self.write_conn.lock().unwrap();
        projection::compiler::full_rebuild(&conn, &self.bitmaps);
    }

    /// Get bitmap length for a key (used by sidebar counts).
    pub fn bitmap_len(&self, key: &projection::bitmaps::BitmapKey) -> u64 {
        self.bitmaps.len(key)
    }
}

#[cfg(test)]
mod tests;
