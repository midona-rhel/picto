//! Single-transaction file import with tags.

use rusqlite::Connection;

use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::ReadModelEvent;
use crate::sqlite::files::{self, NewFile};
use crate::tags::db as tags_db;
use crate::sqlite::SqliteDatabase;

/// Options for importing a file.
pub struct ImportOptions {
    pub hash: String,
    pub name: Option<String>,
    pub size: i64,
    pub mime: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub status: i64,
    pub notes: Option<String>,
    pub source_urls: Option<Vec<String>>,
    pub created_at: Option<String>,
    pub dominant_color_hex: Option<String>,
    pub dominant_palette_blob: Option<Vec<u8>>,
    pub tags: Vec<(String, String)>, // (namespace, subtag)
    pub tag_source: String,
    pub colors: Vec<(String, f32, f32, f32)>, // (hex, l, a, b)
}

/// Result of a single-transaction import.
pub struct ImportResult {
    pub file_id: i64,
    pub tag_ids: Vec<i64>,
    pub was_duplicate: bool,
}

pub fn import_file_with_tags(
    conn: &mut Connection,
    opts: &ImportOptions,
) -> rusqlite::Result<ImportResult> {
    if files::file_exists(conn, &opts.hash)? {
        let file_id = conn.query_row(
            "SELECT file_id FROM file WHERE hash = ?1",
            [&opts.hash],
            |row| row.get::<_, i64>(0),
        )?;
        return Ok(ImportResult {
            file_id,
            tag_ids: Vec::new(),
            was_duplicate: true,
        });
    }

    let tx = conn.transaction()?;

    let now = chrono::Utc::now().to_rfc3339();
    let urls_json = opts
        .source_urls
        .as_ref()
        .map(|urls| serde_json::to_string(urls).unwrap_or_default());

    let new_file = NewFile {
        hash: opts.hash.clone(),
        name: opts.name.clone(),
        size: opts.size,
        mime: opts.mime.clone(),
        width: opts.width,
        height: opts.height,
        duration_ms: opts.duration_ms,
        num_frames: opts.num_frames,
        has_audio: opts.has_audio,
        status: opts.status,
        imported_at: now,
        entity_created_at: opts.created_at.clone(),
        notes: opts.notes.clone(),
        source_urls_json: urls_json,
        dominant_color_hex: opts.dominant_color_hex.clone(),
        dominant_palette_blob: opts.dominant_palette_blob.clone(),
    };

    let file_id = files::insert_file(&tx, &new_file)?;

    let mut tag_ids = Vec::new();
    for (ns, st) in &opts.tags {
        let tag_id = tags_db::get_or_create_tag(&tx, ns, st)?;
        tags_db::tag_entity(&tx, file_id, tag_id, &opts.tag_source)?;
        tag_ids.push(tag_id);
    }

    if !opts.colors.is_empty() {
        files::save_file_colors(&tx, file_id, &opts.colors)?;
    }

    tx.commit()?;

    Ok(ImportResult {
        file_id,
        tag_ids,
        was_duplicate: false,
    })
}

impl SqliteDatabase {
    pub async fn import_file(&self, opts: ImportOptions) -> Result<ImportResult, String> {
        let bitmaps = self.bitmaps.clone();
        let hash_index = self.hash_index.clone();
        let hash = opts.hash.clone();
        let status = opts.status;

        let result = self
            .with_conn_mut(move |conn| import_file_with_tags(conn, &opts))
            .await?;

        if !result.was_duplicate {
            hash_index.insert(hash, result.file_id);

            // When events are held (collection member import), skip bitmaps
            // and events entirely. They'll be populated by the compiler when
            // release_events fires the FileInserted event after the collection
            // has parent_collection_id set.
            if !self.events_held.load(std::sync::atomic::Ordering::SeqCst) {
                let file_id_u32 = result.file_id as u32;
                bitmaps.insert(&BitmapKey::Status(status), file_id_u32);
                for &tag_id in &result.tag_ids {
                    bitmaps.insert(&BitmapKey::Tag(tag_id), file_id_u32);
                }
            }

            self.emit_read_model_event(ReadModelEvent::FileInserted {
                file_id: result.file_id,
            });
        }

        Ok(result)
    }

    /// Import multiple files as a collection in a single transaction.
    /// All files + the collection entity + member assignments are committed atomically,
    /// so the grid only ever sees the final collection — never individual loose files.
    pub async fn import_collection_batch(
        &self,
        files: Vec<ImportOptions>,
        collection_name: &str,
    ) -> Result<BatchCollectionResult, String> {
        let bitmaps = self.bitmaps.clone();
        let hash_index = self.hash_index.clone();
        let cname = collection_name.to_string();

        let result = self
            .with_conn_mut(move |conn| {
                let mut file_ids: Vec<i64> = Vec::with_capacity(files.len());
                let mut hashes: Vec<String> = Vec::with_capacity(files.len());

                // Insert all files
                for opts in &files {
                    match import_file_with_tags(conn, opts) {
                        Ok(r) if !r.was_duplicate => {
                            file_ids.push(r.file_id);
                            hashes.push(opts.hash.clone());
                        }
                        Ok(_) => {
                            // Duplicate — still include in collection
                            if let Ok(fid) = conn.query_row(
                                "SELECT file_id FROM file WHERE hash = ?1",
                                [&opts.hash],
                                |row| row.get::<_, i64>(0),
                            ) {
                                file_ids.push(fid);
                                hashes.push(opts.hash.clone());
                            }
                        }
                        Err(e) => {
                            tracing::warn!(hash = %opts.hash, error = %e, "Batch import: file failed (skipped)");
                        }
                    }
                }

                // Create collection entity
                let collection_id = crate::folders::collections_db::create_collection(conn, &cname)?;

                // Assign members (set parent_collection_id + ordinal)
                for (ordinal, &fid) in file_ids.iter().enumerate() {
                    conn.execute(
                        "UPDATE media_entity
                         SET parent_collection_id = ?1,
                             collection_ordinal = ?2,
                             updated_at = CURRENT_TIMESTAMP
                         WHERE entity_id = ?3 AND kind = 'single'",
                        rusqlite::params![collection_id, ordinal as i64 + 1, fid],
                    )?;
                }

                // Sync collection metadata (cover, count, size, tags, dates)
                crate::folders::collections_db::sync_collection_aggregate_metadata(conn, collection_id)?;

                Ok(BatchCollectionResult {
                    collection_id,
                    file_ids,
                    hashes,
                })
            })
            .await?;

        // Update in-memory indexes outside the transaction
        for (hash, &fid) in result.hashes.iter().zip(&result.file_ids) {
            hash_index.insert(hash.clone(), fid);
        }
        // Only add the collection to the status bitmap — members are hidden
        bitmaps.insert(&BitmapKey::Status(0), result.collection_id as u32);

        self.emit_read_model_event(ReadModelEvent::StatusBatchChanged);

        Ok(result)
    }
}

pub struct BatchCollectionResult {
    pub collection_id: i64,
    pub file_ids: Vec<i64>,
    pub hashes: Vec<String>,
}
