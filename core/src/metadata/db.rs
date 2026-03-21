//! Metadata-owned projection reads and repair helpers.

use crate::sqlite::SqliteDatabase;
use crate::sqlite::files::{FileMetadataSlim, FileRecord};
use crate::sqlite::projections::build_projections_batch;
use crate::tags::db::FileTagInfo;
use rusqlite::params_from_iter;
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

fn take_corrupt_file_ids() -> Vec<i64> {
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

/// Extended metadata returned by the batch endpoint.
#[derive(Debug, Clone)]
pub struct ResolvedMetadataFull {
    pub resolved: ResolvedMetadata,
    pub file_id: i64,
    pub source_urls_json: Option<String>,
    pub notes: Option<String>,
    pub colors: Vec<(String, f64, f64, f64)>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
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
                file: FileRecord,
                created_at: Option<String>,
                updated_at: Option<String>,
            }

            let placeholders = std::iter::repeat_n("?", hashes.len())
                .collect::<Vec<_>>()
                .join(", ");
            let sql = format!(
                "SELECT f.file_id, f.hash, f.name, f.size, f.mime, f.width, f.height, f.duration_ms, f.num_frames,
                        f.has_audio, f.status, f.rating, f.view_count, f.phash, f.imported_at,
                        f.notes, f.source_urls_json, f.dominant_color_hex,
                        p.epoch, p.resolved_json,
                        me.created_at, me.updated_at
                 FROM file f
                 LEFT JOIN entity_metadata_projection p ON p.entity_id = f.file_id
                 LEFT JOIN entity_file ef ON ef.file_id = f.file_id
                 LEFT JOIN media_entity me ON me.entity_id = ef.entity_id
                 WHERE f.hash IN ({})",
                placeholders
            );

            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params_from_iter(hashes.iter()), |row| {
                let file = FileRecord {
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
                    status: row.get(10)?,
                    rating: row.get(11)?,
                    view_count: row.get(12)?,
                    phash: row.get(13)?,
                    imported_at: row.get(14)?,
                    notes: row.get(15)?,
                    source_urls_json: row.get(16)?,
                    dominant_color_hex: row.get(17)?,
                };
                let proj_epoch: Option<i64> = row.get(18)?;
                let proj_resolved_json: Option<String> = row.get(19)?;
                let created_at: Option<String> = row.get(20)?;
                let updated_at: Option<String> = row.get(21)?;
                Ok((file, proj_epoch, proj_resolved_json, created_at, updated_at))
            })?;

            let mut results = Vec::new();
            let mut fallbacks: Vec<FallbackRow> = Vec::new();

            for row in rows {
                let (file, proj_epoch, proj_resolved_json, created_at, updated_at) = row?;
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
                                    colors: Vec::new(),
                                    created_at,
                                    updated_at,
                                });
                                continue;
                            }
                            Err(e) => {
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

                fallbacks.push(FallbackRow { file_id, file, created_at, updated_at });
            }

            if !fallbacks.is_empty() {
                let fallback_ids: Vec<i64> = fallbacks.iter().map(|f| f.file_id).collect();
                let mut tags_by_file: HashMap<i64, Vec<FileTagInfo>> =
                    crate::tags::db::get_entities_tags(conn, &fallback_ids)?;

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
                        colors: Vec::new(),
                        created_at: fallback.created_at,
                        updated_at: fallback.updated_at,
                    });
                }
            }

            let all_file_ids: Vec<i64> = results.iter().map(|r| r.file_id).collect();
            let mut colors_map = crate::sqlite::files::get_files_colors_batch(conn, &all_file_ids)?;
            for result in &mut results {
                if let Some(colors) = colors_map.remove(&result.file_id) {
                    result.colors = colors;
                }
            }

            Ok(results)
        })
        .await
    }
}
