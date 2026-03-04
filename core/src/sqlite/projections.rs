//! Metadata projection read/write/invalidate.
//!
//! Pre-compiled per-file view data stored in `entity_metadata_projection`.
//! Compilers keep miss rate near zero; reads fall back to per-file SQL on miss.

use super::files::FileMetadataSlim;
use super::tags::FileTagInfo;
use super::SqliteDatabase;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Tracks file_ids with corrupt projection JSON for rebuild.
static CORRUPT_FILE_IDS: std::sync::OnceLock<std::sync::Mutex<Vec<i64>>> =
    std::sync::OnceLock::new();

fn push_corrupt_file_id(file_id: i64) {
    let vec = CORRUPT_FILE_IDS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    if let Ok(mut v) = vec.lock() {
        if v.len() < 10_000 {
            v.push(file_id);
        }
    }
}

/// Drain all recorded corrupt file_ids (used by repair command).
pub fn take_corrupt_file_ids() -> Vec<i64> {
    let vec = CORRUPT_FILE_IDS.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    if let Ok(mut v) = vec.lock() {
        std::mem::take(&mut *v)
    } else {
        Vec::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedMetadata {
    pub file: FileMetadataSlim,
    pub tags: Vec<FileTagInfo>,
}

/// Extended metadata returned by the batch endpoint — includes fields not in the projection.
#[derive(Debug, Clone)]
pub struct ResolvedMetadataFull {
    pub resolved: ResolvedMetadata,
    pub file_id: i64,
    pub source_urls_json: Option<String>,
    pub notes: Option<String>,
    pub colors: Vec<(String, f64, f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionRow {
    pub file_id: i64,
    pub epoch: i64,
    pub resolved_json: String,
    pub parents_json: String,
}

/// Get a pre-compiled projection for a file.
pub fn get_projection(conn: &Connection, file_id: i64) -> rusqlite::Result<Option<ProjectionRow>> {
    conn.query_row(
        "SELECT entity_id, epoch, resolved_json, parents_json
         FROM entity_metadata_projection WHERE entity_id = ?1",
        [file_id],
        |row| {
            Ok(ProjectionRow {
                file_id: row.get(0)?,
                epoch: row.get(1)?,
                resolved_json: row.get(2)?,
                parents_json: row.get(3)?,
            })
        },
    )
    .optional()
}

pub fn upsert_projection(
    conn: &Connection,
    file_id: i64,
    epoch: i64,
    resolved_json: &str,
    parents_json: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO entity_metadata_projection (entity_id, epoch, resolved_json, parents_json)
         VALUES (?1, ?2, ?3, ?4)",
        params![file_id, epoch, resolved_json, parents_json],
    )?;
    Ok(())
}

pub fn invalidate_projection(conn: &Connection, file_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM entity_metadata_projection WHERE entity_id = ?1",
        [file_id],
    )?;
    Ok(())
}

/// Build a projection for a single file from raw SQL data.
pub fn build_projection_for_file(
    conn: &Connection,
    file_id: i64,
    epoch: i64,
) -> rusqlite::Result<()> {
    let file = super::files::get_file_by_id(conn, file_id)?;
    let file = match file {
        Some(f) => f,
        None => return Ok(()),
    };

    let slim = FileMetadataSlim {
        file_id: file.file_id,
        entity_id: file.file_id,
        is_collection: false,
        collection_item_count: None,
        hash: file.hash,
        name: file.name,
        mime: file.mime,
        width: file.width,
        height: file.height,
        size: file.size,
        status: file.status as u8,
        rating: file.rating,
        blurhash: file.blurhash,
        imported_at: file.imported_at,
        dominant_color_hex: file.dominant_color_hex,
        duration_ms: file.duration_ms,
        num_frames: file.num_frames,
        has_audio: file.has_audio,
        view_count: file.view_count,
        position_rank: None,
    };

    let tags = super::tags::get_entity_tags(conn, file_id)?;

    let resolved = ResolvedMetadata { file: slim, tags };
    let resolved_json = serde_json::to_string(&resolved).unwrap_or_default();

    let parents_json = "[]".to_string();

    upsert_projection(conn, file_id, epoch, &resolved_json, &parents_json)?;

    Ok(())
}

/// Batch build projections for multiple files using a single JOIN query.
pub fn build_projections_batch(
    conn: &Connection,
    file_ids: &[i64],
    epoch: i64,
) -> rusqlite::Result<()> {
    if file_ids.is_empty() {
        return Ok(());
    }

    // Process in chunks to avoid exceeding SQLite parameter limits
    const CHUNK_SIZE: usize = 500;
    for chunk in file_ids.chunks(CHUNK_SIZE) {
        build_projections_batch_chunk(conn, chunk, epoch)?;
    }
    Ok(())
}

fn build_projections_batch_chunk(
    conn: &Connection,
    file_ids: &[i64],
    epoch: i64,
) -> rusqlite::Result<()> {
    if file_ids.is_empty() {
        return Ok(());
    }

    let placeholders: Vec<String> = (1..=file_ids.len()).map(|i| format!("?{i}")).collect();
    let ph_str = placeholders.join(",");

    let sql = format!(
        "SELECT f.file_id, f.hash, f.name, f.mime, f.width, f.height, f.size, f.status,
                f.rating, f.blurhash, f.imported_at, f.dominant_color_hex,
                f.duration_ms, f.num_frames, f.has_audio, f.view_count,
                t.tag_id, t.namespace, t.subtag, td.display_ns, td.display_st, etr.source
         FROM file f
         LEFT JOIN entity_tag_raw etr ON etr.entity_id = f.file_id
         LEFT JOIN tag t ON t.tag_id = etr.tag_id
         LEFT JOIN tag_display td ON td.tag_id = t.tag_id
         WHERE f.file_id IN ({ph_str})
         ORDER BY f.file_id, t.namespace, t.subtag"
    );

    let params: Vec<&dyn rusqlite::types::ToSql> = file_ids
        .iter()
        .map(|id| id as &dyn rusqlite::types::ToSql)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(params.as_slice())?;

    let mut current_file_id: Option<i64> = None;
    let mut current_slim: Option<FileMetadataSlim> = None;
    let mut current_tags: Vec<FileTagInfo> = Vec::new();

    let mut upsert_stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO entity_metadata_projection (entity_id, epoch, resolved_json, parents_json)
         VALUES (?1, ?2, ?3, ?4)",
    )?;

    let flush = |upsert: &mut rusqlite::Statement,
                 file_id: i64,
                 slim: FileMetadataSlim,
                 tags: Vec<FileTagInfo>|
     -> rusqlite::Result<()> {
        let resolved = ResolvedMetadata { file: slim, tags };
        let resolved_json = serde_json::to_string(&resolved).unwrap_or_default();
        upsert.execute(params![file_id, epoch, resolved_json, "[]"])?;
        Ok(())
    };

    while let Some(row) = rows.next()? {
        let file_id: i64 = row.get(0)?;

        if current_file_id != Some(file_id) {
            // Flush previous file
            if let (Some(prev_fid), Some(slim)) = (current_file_id, current_slim.take()) {
                let tags = std::mem::take(&mut current_tags);
                flush(&mut upsert_stmt, prev_fid, slim, tags)?;
            }

            current_file_id = Some(file_id);
            current_slim = Some(FileMetadataSlim {
                file_id,
                entity_id: file_id,
                is_collection: false,
                collection_item_count: None,
                hash: row.get(1)?,
                name: row.get(2)?,
                mime: row.get(3)?,
                width: row.get(4)?,
                height: row.get(5)?,
                size: row.get(6)?,
                status: row.get::<_, i64>(7)? as u8,
                rating: row.get(8)?,
                blurhash: row.get(9)?,
                imported_at: row.get(10)?,
                dominant_color_hex: row.get(11)?,
                duration_ms: row.get(12)?,
                num_frames: row.get(13)?,
                has_audio: row.get::<_, i64>(14)? != 0,
                view_count: row.get(15)?,
                position_rank: None,
            });
        }

        // Collect tag if present (LEFT JOIN may produce NULL tag_id)
        let tag_id: Option<i64> = row.get(16)?;
        if let Some(tid) = tag_id {
            current_tags.push(FileTagInfo {
                tag_id: tid,
                namespace: row.get(17)?,
                subtag: row.get(18)?,
                display_ns: row.get(19)?,
                display_st: row.get(20)?,
                source: row.get(21)?,
            });
        }
    }

    if let (Some(prev_fid), Some(slim)) = (current_file_id, current_slim.take()) {
        flush(&mut upsert_stmt, prev_fid, slim, current_tags)?;
    }

    Ok(())
}

impl SqliteDatabase {
    /// Repair corrupt projection rows by rebuilding them from source data.
    /// Returns the number of projections rebuilt.
    pub async fn repair_corrupt_projections(&self) -> Result<usize, String> {
        let corrupt_ids = take_corrupt_file_ids();
        if corrupt_ids.is_empty() {
            return Ok(0);
        }
        let count = corrupt_ids.len();
        let epoch = self
            .manifest
            .published_artifact_version("metadata_projection") as i64;
        self.with_conn(move |conn| {
            build_projections_batch(conn, &corrupt_ids, epoch)?;
            Ok(count)
        })
        .await
    }

    /// Get batch metadata using projections (fast path) with SQL fallback.
    /// Returns `ResolvedMetadataFull` which includes source_urls_json and notes
    /// from the file table (not stored in projections).
    pub async fn get_files_metadata_batch(
        &self,
        hashes: Vec<String>,
    ) -> Result<Vec<ResolvedMetadataFull>, String> {
        if hashes.is_empty() {
            return Ok(Vec::new());
        }

        let projection_version =
            self.manifest
                .published_artifact_version("metadata_projection") as i64;
        self.with_read_conn(move |conn| {
            #[derive(Debug)]
            struct FallbackRow {
                file_id: i64,
                file: super::files::FileRecord,
            }

            let placeholders = std::iter::repeat_n("?", hashes.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT f.file_id, f.hash, f.name, f.size, f.mime, f.width, f.height, f.duration_ms, f.num_frames,
                        f.has_audio, f.blurhash, f.status, f.rating, f.view_count, f.phash, f.imported_at,
                        f.notes, f.source_urls_json, f.dominant_color_hex,
                        p.epoch, p.resolved_json
                 FROM file f
                 LEFT JOIN entity_metadata_projection p ON p.entity_id = f.file_id
                 WHERE f.hash IN ({})",
                placeholders
            );

            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(hashes.iter()), |row| {
                let file = super::files::FileRecord {
                    file_id: row.get(0)?,
                    hash: row.get(1)?,
                    name: row.get(2)?,
                    size: row.get(3)?,
                    mime: row.get(4)?,
                    width: row.get(5)?,
                    height: row.get(6)?,
                    duration_ms: row.get(7)?,
                    num_frames: row.get(8)?,
                    has_audio: row.get::<_, i64>(9)? != 0,
                    blurhash: row.get(10)?,
                    status: row.get(11)?,
                    rating: row.get(12)?,
                    view_count: row.get(13)?,
                    phash: row.get(14)?,
                    imported_at: row.get(15)?,
                    notes: row.get(16)?,
                    source_urls_json: row.get(17)?,
                    dominant_color_hex: row.get(18)?,
                };
                let proj_epoch: Option<i64> = row.get(19)?;
                let proj_resolved_json: Option<String> = row.get(20)?;
                Ok((file, proj_epoch, proj_resolved_json))
            })?;

            let mut results = Vec::new();
            let mut fallbacks: Vec<FallbackRow> = Vec::new();

            for row in rows {
                let (file, proj_epoch, proj_resolved_json) = row?;
                let file_id = file.file_id;
                let source_urls_json = file.source_urls_json.clone();
                let notes = file.notes.clone();

                if let (Some(epoch), Some(ref resolved_json)) = (proj_epoch, &proj_resolved_json) {
                    if epoch == projection_version {
                        match serde_json::from_str::<ResolvedMetadata>(resolved_json) {
                            Ok(resolved) => {
                                results.push(ResolvedMetadataFull {
                                    resolved,
                                    file_id,
                                    source_urls_json,
                                    notes,
                                    colors: Vec::new(), // filled below
                                });
                                continue;
                            }
                            Err(e) => {
                                // PBI-012: Corruption detected — log, count, and queue rebuild.
                                tracing::warn!(
                                    target: "picto::core::projections",
                                    "corrupt projection JSON for file_id={file_id}: {e} (first 200 chars: {:?})",
                                    &resolved_json[..resolved_json.len().min(200)]
                                );
                                push_corrupt_file_id(file_id);
                                crate::perf::record_projection_corruption(1);
                            }
                        }
                    }
                }

                fallbacks.push(FallbackRow {
                    file_id: file.file_id,
                    file,
                });
            }

            if !fallbacks.is_empty() {
                let fallback_ids: Vec<i64> = fallbacks.iter().map(|f| f.file_id).collect();
                let mut tags_by_file: HashMap<i64, Vec<super::tags::FileTagInfo>> =
                    super::tags::get_entities_tags(conn, &fallback_ids)?;

                for fallback in fallbacks {
                    let source_urls_json = fallback.file.source_urls_json.clone();
                    let notes = fallback.file.notes.clone();
                    let file = fallback.file;
                    let slim = FileMetadataSlim {
                        file_id: fallback.file_id,
                        entity_id: fallback.file_id,
                        is_collection: false,
                        collection_item_count: None,
                        hash: file.hash,
                        name: file.name,
                        mime: file.mime,
                        width: file.width,
                        height: file.height,
                        size: file.size,
                        status: file.status as u8,
                        rating: file.rating,
                        blurhash: file.blurhash,
                        imported_at: file.imported_at,
                        dominant_color_hex: file.dominant_color_hex,
                        duration_ms: file.duration_ms,
                        num_frames: file.num_frames,
                        has_audio: file.has_audio,
                        view_count: file.view_count,
                        position_rank: None,
                    };
                    let tags = tags_by_file.remove(&fallback.file_id).unwrap_or_default();
                    results.push(ResolvedMetadataFull {
                        resolved: ResolvedMetadata { file: slim, tags },
                        file_id: fallback.file_id,
                        source_urls_json,
                        notes,
                        colors: Vec::new(), // filled below
                    });
                }
            }

            let all_file_ids: Vec<i64> = results.iter().map(|r| r.file_id).collect();
            let mut colors_map = super::files::get_files_colors_batch(conn, &all_file_ids)?;
            for r in &mut results {
                if let Some(colors) = colors_map.remove(&r.file_id) {
                    r.colors = colors;
                }
            }

            Ok(results)
        })
        .await
    }
}
