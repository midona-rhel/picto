//! Shared execution for durable media derivative work.
//!
//! This module deliberately does not claim or complete queue rows. Callers
//! such as the durable worker, repair, and backfill paths all use this same
//! executor and keep queue ownership outside media processing.

use std::path::PathBuf;

use rusqlite::{params, OptionalExtension, Transaction};

use crate::blob_store::{mime_to_extension, BlobStore};
use crate::media_processing::{self, PreparedMediaSource, DEFAULT_THUMBNAIL_DIMENSIONS};
use crate::store::Store;
use crate::workers_v2::{WorkItem, WorkKind};

pub const TARGET_COLOR_ANALYSIS_VERSION: i64 = 2;
pub const TARGET_PHASH_ANALYSIS_VERSION: i64 = crate::store::schema::CURRENT_PHASH_ANALYSIS_VERSION;
const PHASH_VERSION_SETTING: &str = crate::store::schema::PHASH_VERSION_SETTING;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PerceptualHashRecovery {
    pub reset: bool,
    pub queued: usize,
}

/// Keep a library on one pHash algorithm and queue every missing supported
/// raster. The version switch is atomic: old hashes and their duplicate
/// distances disappear before new work can be observed, so algorithms are
/// never mixed inside one duplicate scan.
pub fn reconcile_perceptual_hash_work(
    store: &Store,
    now: &str,
) -> Result<PerceptualHashRecovery, String> {
    let target_version_json = TARGET_PHASH_ANALYSIS_VERSION.to_string();
    let (recovery, _, _) = store.transaction_if_changed(|transaction| {
        let stored_version = transaction
            .query_row(
                "SELECT value_json FROM setting WHERE key = ?1",
                [PHASH_VERSION_SETTING],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let reset = stored_version.as_deref() != Some(target_version_json.as_str());
        let mut changed = false;
        if reset {
            transaction.execute("DELETE FROM duplicate", [])?;
            transaction.execute(
                "DELETE FROM work_item WHERE work_type = 'perceptual_hash'",
                [],
            )?;
            transaction.execute(
                "UPDATE media_file SET perceptual_hash = NULL
                 WHERE perceptual_hash IS NOT NULL",
                [],
            )?;
            transaction.execute(
                "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                params![PHASH_VERSION_SETTING, target_version_json],
            )?;
            changed = true;
        }

        let mut statement = transaction.prepare(
            "SELECT mf.file_id, mf.mime_type, mf.frame_count
             FROM media_file mf
             WHERE mf.perceptual_hash IS NULL
               AND EXISTS (SELECT 1 FROM media_asset ma WHERE ma.file_id = mf.file_id)
               AND NOT EXISTS (
                   SELECT 1 FROM work_item wi
                   WHERE wi.file_id = mf.file_id AND wi.work_type = 'perceptual_hash'
               )
             ORDER BY mf.file_id",
        )?;
        let candidates = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let mut queued = 0;
        for (file_id, mime_type, frame_count) in candidates {
            let capabilities =
                crate::media_capabilities::capabilities_for_stored_media(&mime_type, frame_count);
            if !capabilities.can_perceptual_hash {
                continue;
            }
            queued += transaction.execute(
                "INSERT INTO work_item (
                     file_id, work_type, status, attempt_count,
                     available_at, created_at, updated_at
                 ) VALUES (?1, 'perceptual_hash', 'pending', 0, ?2, ?2, ?2)",
                params![file_id, now],
            )?;
        }
        changed |= queued != 0;
        Ok((PerceptualHashRecovery { reset, queued }, changed))
    })?;
    Ok(recovery)
}

/// Queue palette repair for existing media that predates its current
/// thumbnail-backed color capability. New imports enqueue this work directly.
pub fn enqueue_missing_dominant_color_work(store: &Store, now: &str) -> Result<usize, String> {
    let (inserted, _, _) = store.transaction_if_changed(|transaction| {
        let mut statement = transaction.prepare(
            "SELECT mf.file_id, mf.mime_type, mf.frame_count
             FROM media_file mf
             WHERE (mf.color_analysis_version < ?1 OR mf.dominant_palette_blob IS NULL)
               AND EXISTS (SELECT 1 FROM media_asset ma WHERE ma.file_id = mf.file_id)
               AND NOT EXISTS (
                   SELECT 1 FROM work_item wi
                   WHERE wi.file_id = mf.file_id AND wi.work_type = 'dominant_colors'
               )
             ORDER BY mf.file_id",
        )?;
        let candidates = statement
            .query_map([TARGET_COLOR_ANALYSIS_VERSION], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);

        let mut inserted = 0;
        for (file_id, mime_type, frame_count) in candidates {
            let capabilities =
                crate::media_capabilities::capabilities_for_stored_media(&mime_type, frame_count);
            if !capabilities.can_dominant_colors {
                continue;
            }
            inserted += transaction.execute(
                "INSERT INTO work_item (
                     file_id, work_type, status, attempt_count,
                     available_at, created_at, updated_at
                 ) VALUES (?1, 'dominant_colors', 'pending', 0, ?2, ?2, ?2)",
                params![file_id, now],
            )?;
        }
        Ok((inserted, inserted != 0))
    })?;
    Ok(inserted)
}

/// The storage operations needed by derivative generation.
///
/// Keeping this boundary small means the executor can run against the real
/// content-addressed store or a temporary test store without knowing how
/// library paths are owned by the application.
pub trait BlobSource {
    fn original_path(&self, file_hash: &str, mime_type: &str) -> Result<PathBuf, String>;
    fn thumbnail_exists(&self, file_hash: &str) -> Result<bool, String>;
    fn read_thumbnail(&self, _file_hash: &str) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
    fn write_thumbnail(&self, file_hash: &str, bytes: &[u8], extension: &str)
        -> Result<(), String>;
    fn delete(&self, file_hash: &str) -> Result<(), String>;
}

impl BlobSource for BlobStore {
    fn original_path(&self, file_hash: &str, mime_type: &str) -> Result<PathBuf, String> {
        let extension = mime_to_extension(mime_type);
        self.find_original(file_hash, Some(extension))
            .map_err(|error| format!("Original blob lookup failed: {error}"))?
            .map(|(path, _)| path)
            .ok_or_else(|| format!("Original file not found for hash {file_hash}"))
    }

    fn thumbnail_exists(&self, file_hash: &str) -> Result<bool, String> {
        self.find_thumbnail_path(file_hash)
            .map(|path| path.is_some())
            .map_err(|error| format!("Thumbnail lookup failed: {error}"))
    }

    fn read_thumbnail(&self, file_hash: &str) -> Result<Option<Vec<u8>>, String> {
        BlobStore::read_thumbnail(self, file_hash)
            .map_err(|error| format!("Thumbnail read failed: {error}"))
    }

    fn write_thumbnail(
        &self,
        file_hash: &str,
        bytes: &[u8],
        extension: &str,
    ) -> Result<(), String> {
        BlobStore::write_thumbnail(self, file_hash, bytes, extension)
            .map_err(|error| format!("Thumbnail write failed: {error}"))
    }

    fn delete(&self, file_hash: &str) -> Result<(), String> {
        BlobStore::delete(self, file_hash).map_err(|error| format!("Blob deletion failed: {error}"))
    }
}

pub fn execute_blob_delete<B: BlobSource>(
    store: &Store,
    blobs: &B,
    item: &WorkItem,
) -> Result<(), String> {
    if item.kind != WorkKind::BlobDelete {
        return Err("Blob deletion requires a blob_delete work item".to_string());
    }
    let file_hash = item
        .file_hash
        .as_deref()
        .ok_or_else(|| "Blob deletion work is missing its persisted file hash".to_string())?;
    store.read_result(|connection| {
        let referenced: bool = connection
            .query_row(
                "SELECT EXISTS(
                     SELECT 1
                     FROM media_asset ma
                     JOIN media_file mf ON mf.file_id = ma.file_id
                     WHERE mf.file_hash = ?1
                 )",
                [file_hash],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if referenced {
            return Ok(());
        }
        blobs.delete(file_hash)
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DerivativeOutcome {
    pub changed: bool,
    pub thumbnail_written: bool,
    pub dominant_colors_written: bool,
    pub perceptual_hash_written: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DerivativeRequest {
    pub media_item_id: Option<i64>,
    pub file_id: Option<i64>,
    pub kind: WorkKind,
}

#[derive(Debug, Clone)]
struct DerivativeTarget {
    file_id: i64,
    file_hash: String,
    mime_type: String,
    duration_ms: Option<i64>,
    frame_count: Option<i64>,
    perceptual_hash: Option<String>,
    color_analysis_version: i64,
    has_dominant_palette: bool,
}

/// Execute one derivative work item from persisted media and blob state.
///
/// The operation is intentionally idempotent. A completed thumbnail, pHash,
/// or current color analysis is treated as success without doing the work a
/// second time. Queue claim, retry, and completion remain the caller's job.
pub async fn execute_work<B: BlobSource>(
    store: &Store,
    blobs: &B,
    item: &WorkItem,
) -> Result<DerivativeOutcome, String> {
    execute_work_group(store, blobs, std::slice::from_ref(item))
        .await
        .pop()
        .expect("one derivative result")
}

/// Execute claimed derivative rows for one physical file with one decoded source.
/// Queue rows remain independently retryable; only expensive media preparation is shared.
pub async fn execute_work_group<B: BlobSource>(
    store: &Store,
    blobs: &B,
    items: &[WorkItem],
) -> Vec<Result<DerivativeOutcome, String>> {
    let Some(first) = items.first() else {
        return Vec::new();
    };
    let first_request = DerivativeRequest {
        media_item_id: first.media_item_id,
        file_id: first.file_id,
        kind: first.kind,
    };
    let target = match load_target(store, first_request) {
        Ok(target) => target,
        Err(error) => {
            return items
                .iter()
                .map(|item| Err(format!("work {}: {error}", item.work_id)))
                .collect()
        }
    };
    if items.iter().any(|item| {
        item.file_id
            .is_some_and(|file_id| file_id != target.file_id)
    }) {
        return items
            .iter()
            .map(|item| {
                Err(format!(
                    "work {}: derivative group mixes physical files",
                    item.work_id
                ))
            })
            .collect();
    }
    let original_path = match blobs.original_path(&target.file_hash, &target.mime_type) {
        Ok(path) => path,
        Err(error) => {
            return items
                .iter()
                .map(|item| Err(format!("work {}: {error}", item.work_id)))
                .collect()
        }
    };
    let mut source = PreparedMediaSource::from_stored_metadata(
        original_path,
        &target.mime_type,
        target.duration_ms,
        target.frame_count,
    );
    let mut rendered_thumbnail = None;
    let mut results = Vec::with_capacity(items.len());
    for item in items {
        results.push(
            execute_kind(
                store,
                blobs,
                &target,
                &mut source,
                &mut rendered_thumbnail,
                item.kind,
            )
            .await
            .map_err(|error| format!("work {}: {error}", item.work_id)),
        );
    }
    results
}

/// Execute a derivative directly for repair and backfill paths.
pub async fn execute<B: BlobSource>(
    store: &Store,
    blobs: &B,
    request: DerivativeRequest,
) -> Result<DerivativeOutcome, String> {
    let target = load_target(store, request)?;
    let original_path = blobs.original_path(&target.file_hash, &target.mime_type)?;
    let mut source = PreparedMediaSource::from_stored_metadata(
        original_path,
        &target.mime_type,
        target.duration_ms,
        target.frame_count,
    );
    let mut rendered_thumbnail = None;
    execute_kind(
        store,
        blobs,
        &target,
        &mut source,
        &mut rendered_thumbnail,
        request.kind,
    )
    .await
}

async fn execute_kind<B: BlobSource>(
    store: &Store,
    blobs: &B,
    target: &DerivativeTarget,
    source: &mut PreparedMediaSource,
    rendered_thumbnail: &mut Option<(Vec<u8>, String)>,
    kind: WorkKind,
) -> Result<DerivativeOutcome, String> {
    match kind {
        WorkKind::Thumbnail => {
            if blobs.thumbnail_exists(&target.file_hash)? {
                return Ok(DerivativeOutcome::default());
            }
            if !source.caps.can_thumbnail() {
                return Err(format!("No thumbnail backend for {}", target.mime_type));
            }
            if rendered_thumbnail.is_none() {
                *rendered_thumbnail = Some(
                    source
                        .render_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS, 35)
                        .await
                        .map_err(|error| format!("Thumbnail generation failed: {error}"))?,
                );
            }
            let (bytes, extension) = rendered_thumbnail
                .as_ref()
                .expect("rendered thumbnail available");
            blobs.write_thumbnail(&target.file_hash, bytes, extension)?;
            Ok(DerivativeOutcome {
                changed: true,
                thumbnail_written: true,
                ..DerivativeOutcome::default()
            })
        }
        WorkKind::DominantColors => {
            if !source.caps.can_dominant_colors {
                return Ok(DerivativeOutcome::default());
            }
            let needs_colors = target.color_analysis_version < TARGET_COLOR_ANALYSIS_VERSION
                || !target.has_dominant_palette;
            let needs_phash = source.caps.can_perceptual_hash && target.perceptual_hash.is_none();
            if !needs_colors && !needs_phash {
                return Ok(DerivativeOutcome::default());
            }
            if rendered_thumbnail.is_none() {
                *rendered_thumbnail = match blobs.read_thumbnail(&target.file_hash)? {
                    Some(bytes) => Some((bytes, String::new())),
                    None => Some(
                        source
                            .render_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS, 35)
                            .await
                            .map_err(|error| format!("Dominant color thumbnail failed: {error}"))?,
                    ),
                };
            }
            let decoded = image::load_from_memory(
                &rendered_thumbnail
                    .as_ref()
                    .expect("rendered thumbnail available")
                    .0,
            )
            .map_err(|error| format!("Dominant color thumbnail decode failed: {error}"))?;
            let colors_changed = if needs_colors {
                let palette = media_processing::colors::extract_dominant_colors(&decoded, 10);
                let colors = palette
                    .iter()
                    .map(|color| {
                        (
                            color.hex.clone(),
                            color.l as f32,
                            color.a as f32,
                            color.b as f32,
                        )
                    })
                    .collect::<Vec<_>>();
                let palette_blob = media_processing::colors::serialize_dominant_palette_blob(
                    &palette,
                )
                .map_err(|error| format!("Dominant palette serialization failed: {error}"))?;
                let dominant = palette.first().map(|color| color.hex.as_str());
                update_colors(store, target.file_id, &colors, dominant, &palette_blob)?
            } else {
                false
            };
            let phash_changed = if needs_phash {
                let perceptual_hash =
                    media_processing::compute_phash_base64_from_image(&decoded)
                        .map_err(|error| format!("Perceptual hash analysis failed: {error}"))?;
                update_phash(store, target.file_id, &perceptual_hash)?
            } else {
                false
            };
            Ok(DerivativeOutcome {
                changed: colors_changed || phash_changed,
                dominant_colors_written: colors_changed,
                perceptual_hash_written: phash_changed,
                ..DerivativeOutcome::default()
            })
        }
        WorkKind::PerceptualHash => {
            if !source.caps.can_perceptual_hash || target.perceptual_hash.is_some() {
                return Ok(DerivativeOutcome::default());
            }
            if rendered_thumbnail.is_none() {
                *rendered_thumbnail = match blobs.read_thumbnail(&target.file_hash)? {
                    Some(bytes) => Some((bytes, String::new())),
                    None => Some(
                        source
                            .render_thumbnail_bytes(DEFAULT_THUMBNAIL_DIMENSIONS, 35)
                            .await
                            .map_err(|error| {
                                format!("Perceptual hash thumbnail failed: {error}")
                            })?,
                    ),
                };
            }
            let decoded = image::load_from_memory(
                &rendered_thumbnail
                    .as_ref()
                    .expect("rendered thumbnail available")
                    .0,
            )
            .map_err(|error| format!("Perceptual hash thumbnail decode failed: {error}"))?;
            let perceptual_hash = media_processing::compute_phash_base64_from_image(&decoded)
                .map_err(|error| format!("Perceptual hash analysis failed: {error}"))?;
            let changed = update_phash(store, target.file_id, &perceptual_hash)?;
            Ok(DerivativeOutcome {
                changed,
                perceptual_hash_written: changed,
                ..DerivativeOutcome::default()
            })
        }
        WorkKind::BlobDelete | WorkKind::AiTag => Err(format!(
            "Derivative executor does not handle {:?} work",
            kind
        )),
    }
}

fn load_target(store: &Store, request: DerivativeRequest) -> Result<DerivativeTarget, String> {
    store
        .read(|connection| {
            let row = if let Some(file_id) = request.file_id {
                connection
                    .query_row(
                        "SELECT file_id, file_hash, mime_type, duration_ms, frame_count,
                            perceptual_hash, color_analysis_version,
                            dominant_palette_blob IS NOT NULL
                     FROM media_file
                     WHERE file_id = ?1",
                        [file_id],
                        target_from_row,
                    )
                    .optional()?
            } else if let Some(media_item_id) = request.media_item_id {
                connection
                    .query_row(
                        "SELECT mf.file_id, mf.file_hash, mf.mime_type, mf.duration_ms,
                            mf.frame_count, mf.perceptual_hash,
                            mf.color_analysis_version,
                            mf.dominant_palette_blob IS NOT NULL
                     FROM media_asset ma
                     JOIN media_file mf ON mf.file_id = ma.file_id
                     WHERE ma.item_id = ?1",
                        [media_item_id],
                        target_from_row,
                    )
                    .optional()?
            } else {
                None
            };
            row.ok_or_else(|| rusqlite::Error::QueryReturnedNoRows)
        })
        .map_err(|error| match error {
            error if error.contains("Query returned no rows") => {
                "Derivative target not found".to_string()
            }
            error => error,
        })
}

fn target_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DerivativeTarget> {
    Ok(DerivativeTarget {
        file_id: row.get(0)?,
        file_hash: row.get(1)?,
        mime_type: row.get(2)?,
        duration_ms: row.get(3)?,
        frame_count: row.get(4)?,
        perceptual_hash: row.get(5)?,
        color_analysis_version: row.get(6)?,
        has_dominant_palette: row.get::<_, i64>(7)? != 0,
    })
}

fn update_phash(store: &Store, file_id: i64, perceptual_hash: &str) -> Result<bool, String> {
    let value = perceptual_hash.to_string();
    store
        .transaction_if_changed(|transaction| {
            let changed = transaction.execute(
                "UPDATE media_file
                 SET perceptual_hash = ?1
                 WHERE file_id = ?2 AND perceptual_hash IS NULL",
                params![value, file_id],
            )?;
            Ok((changed == 1, changed == 1))
        })
        .map(|(_, _, changed)| changed)
}

fn update_colors(
    store: &Store,
    file_id: i64,
    colors: &[(String, f32, f32, f32)],
    dominant_color_hex: Option<&str>,
    palette_blob: &[u8],
) -> Result<bool, String> {
    let dominant_color_hex = dominant_color_hex.map(str::to_string);
    let palette_blob = palette_blob.to_vec();
    store
        .transaction_if_changed(|transaction| {
            let complete: bool = transaction.query_row(
                "SELECT color_analysis_version >= ?1
                        AND dominant_palette_blob IS NOT NULL
                 FROM media_file WHERE file_id = ?2",
                params![TARGET_COLOR_ANALYSIS_VERSION, file_id],
                |row| row.get(0),
            )?;
            if complete {
                return Ok((false, false));
            }
            save_file_colors(transaction, file_id, colors)?;
            let changed = transaction.execute(
                "UPDATE media_file
                 SET dominant_color_hex = ?1,
                     dominant_palette_blob = ?2,
                     color_analysis_version = ?3
                 WHERE file_id = ?4
                   AND (color_analysis_version < ?3
                        OR dominant_palette_blob IS NULL)",
                params![
                    dominant_color_hex,
                    palette_blob,
                    TARGET_COLOR_ANALYSIS_VERSION,
                    file_id
                ],
            )?;
            Ok((changed == 1, changed == 1))
        })
        .map(|(_, _, changed)| changed)
}

fn save_file_colors(
    transaction: &Transaction<'_>,
    file_id: i64,
    colors: &[(String, f32, f32, f32)],
) -> rusqlite::Result<()> {
    transaction.execute("DELETE FROM file_color WHERE file_id = ?1", [file_id])?;
    for (hex, l, a, b) in colors {
        transaction.execute(
            "INSERT INTO file_color (file_id, hex, l, a, b)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![file_id, hex, l, a, b],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        enqueue_missing_dominant_color_work, execute_work, reconcile_perceptual_hash_work,
        BlobSource, DerivativeOutcome, TARGET_COLOR_ANALYSIS_VERSION,
        TARGET_PHASH_ANALYSIS_VERSION,
    };
    use crate::blob_store::BlobStore;
    use crate::store::Store;
    use crate::workers_v2::{WorkItem, WorkKind, WorkStatus};
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T00:00:00Z";

    fn fixture() -> (TempDir, Store, BlobStore, i64, String) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let blobs = BlobStore::open(directory.path()).unwrap();
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_fn(32, 32, |x, y| {
            if (x + y) % 2 == 0 {
                Rgb([255, 20, 20])
            } else {
                Rgb([20, 20, 255])
            }
        }));
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        let hash = hex::encode(Sha256::digest(&bytes));
        blobs.write_original(&hash, &bytes, Some("png")).unwrap();
        let file_id = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file (
                         file_hash, mime_type, size_bytes, pixel_width, pixel_height, created_at
                     ) VALUES (?1, 'image/png', ?2, 32, 32, ?3)",
                    rusqlite::params![hash, bytes.len() as i64, NOW],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item (item_key, kind, created_at, updated_at)
                     VALUES ('fixture', 'media', ?1, ?1)",
                    [NOW],
                )?;
                let item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                     VALUES (?1, ?2, ?3, ?3)",
                    rusqlite::params![item_id, file_id, NOW],
                )?;
                Ok(file_id)
            })
            .unwrap()
            .0;
        (directory, store, blobs, file_id, hash)
    }

    #[test]
    fn existing_media_missing_a_palette_is_queued_once() {
        let (_directory, store, _blobs, file_id, _hash) = fixture();

        assert_eq!(enqueue_missing_dominant_color_work(&store, NOW).unwrap(), 1);
        assert_eq!(enqueue_missing_dominant_color_work(&store, NOW).unwrap(), 0);
        let queued: (i64, String) = store
            .read(|connection| {
                connection.query_row("SELECT file_id, work_type FROM work_item", [], |row| {
                    Ok((row.get(0)?, row.get(1)?))
                })
            })
            .unwrap();
        assert_eq!(queued, (file_id, "dominant_colors".to_string()));
    }

    #[test]
    fn p_hash_version_switch_is_atomic_and_backfill_is_idempotent() {
        let (_directory, store, _blobs, file_id, _hash) = fixture();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE setting SET value_json = '1' WHERE key = ?1",
                    [super::PHASH_VERSION_SETTING],
                )?;
                transaction.execute(
                    "INSERT INTO media_file (
                         file_id, file_hash, mime_type, size_bytes, perceptual_hash, created_at
                     ) VALUES (999, 'second-hash', 'image/png', 1, 'second-old-hash', ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO duplicate (file_id_a, file_id_b, distance)
                     VALUES (?1, 999, 4)",
                    [file_id],
                )?;
                transaction.execute(
                    "UPDATE media_file SET perceptual_hash = 'old-hash' WHERE file_id = ?1",
                    [file_id],
                )?;
                Ok(())
            })
            .unwrap();

        let first = reconcile_perceptual_hash_work(&store, NOW).unwrap();
        assert_eq!(first.reset, true);
        assert_eq!(first.queued, 1);
        let state: (Option<String>, i64, String, i64) = store
            .read(|connection| {
                connection.query_row(
                    "SELECT mf.perceptual_hash,
                            (SELECT COUNT(*) FROM work_item
                             WHERE file_id = mf.file_id
                               AND work_type = 'perceptual_hash'),
                            (SELECT value_json FROM setting
                             WHERE key = 'media.perceptual_hash_version'),
                            (SELECT COUNT(*) FROM duplicate)
                     FROM media_file mf WHERE mf.file_id = ?1",
                    [file_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
            })
            .unwrap();
        assert_eq!(
            state,
            (None, 1, TARGET_PHASH_ANALYSIS_VERSION.to_string(), 0)
        );

        assert_eq!(
            reconcile_perceptual_hash_work(&store, NOW).unwrap(),
            super::PerceptualHashRecovery {
                reset: false,
                queued: 0,
            }
        );

        store
            .transaction(|transaction| {
                transaction.execute(
                    "DELETE FROM work_item WHERE file_id = ?1
                       AND work_type = 'perceptual_hash'",
                    [file_id],
                )?;
                transaction.execute(
                    "UPDATE media_file SET perceptual_hash = 'new-hash' WHERE file_id = ?1",
                    [file_id],
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            reconcile_perceptual_hash_work(&store, NOW).unwrap(),
            super::PerceptualHashRecovery {
                reset: false,
                queued: 0,
            }
        );
        let retained: String = store
            .read(|connection| {
                connection.query_row(
                    "SELECT perceptual_hash FROM media_file WHERE file_id = ?1",
                    [file_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(retained, "new-hash");
    }

    fn work(file_id: i64, kind: WorkKind) -> WorkItem {
        WorkItem {
            work_id: 1,
            media_item_id: None,
            file_id: Some(file_id),
            file_hash: None,
            kind,
            status: WorkStatus::Running,
            attempt_count: 0,
            available_at: NOW.to_string(),
            last_error: None,
        }
    }

    #[tokio::test]
    async fn derivatives_write_exact_fields_and_rerun_idempotently() {
        let (_directory, store, blobs, file_id, hash) = fixture();

        let colors = execute_work(&store, &blobs, &work(file_id, WorkKind::DominantColors))
            .await
            .unwrap();
        assert_eq!(
            colors,
            DerivativeOutcome {
                changed: true,
                dominant_colors_written: true,
                perceptual_hash_written: true,
                ..DerivativeOutcome::default()
            }
        );
        let fields = store
            .read(|connection| {
                connection.query_row(
                    "SELECT dominant_color_hex, dominant_palette_blob IS NOT NULL,
                            color_analysis_version, COUNT(*)
                     FROM media_file mf
                     LEFT JOIN file_color fc ON fc.file_id = mf.file_id
                     WHERE mf.file_id = ?1
                     GROUP BY mf.file_id",
                    [file_id],
                    |row| {
                        Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, i64>(1)? != 0,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
            })
            .unwrap();
        assert!(fields.0.is_some());
        assert!(fields.1);
        assert_eq!(fields.2, TARGET_COLOR_ANALYSIS_VERSION);
        assert!(fields.3 > 0);
        assert_eq!(
            execute_work(&store, &blobs, &work(file_id, WorkKind::DominantColors))
                .await
                .unwrap(),
            DerivativeOutcome::default()
        );

        let phash = execute_work(&store, &blobs, &work(file_id, WorkKind::PerceptualHash))
            .await
            .unwrap();
        assert_eq!(phash, DerivativeOutcome::default());
        assert!(store
            .read(|connection| connection.query_row(
                "SELECT perceptual_hash FROM media_file WHERE file_id = ?1",
                [file_id],
                |row| row.get::<_, Option<String>>(0)
            ))
            .unwrap()
            .is_some());
        assert_eq!(
            execute_work(&store, &blobs, &work(file_id, WorkKind::PerceptualHash))
                .await
                .unwrap(),
            DerivativeOutcome::default()
        );

        let thumbnail = execute_work(&store, &blobs, &work(file_id, WorkKind::Thumbnail))
            .await
            .unwrap();
        assert!(thumbnail.thumbnail_written);
        assert!(blobs.find_thumbnail_path(&hash).unwrap().is_some());
        assert_eq!(
            execute_work(&store, &blobs, &work(file_id, WorkKind::Thumbnail))
                .await
                .unwrap(),
            DerivativeOutcome::default()
        );
    }

    struct TestBlobSource;

    impl BlobSource for TestBlobSource {
        fn original_path(
            &self,
            _file_hash: &str,
            _mime_type: &str,
        ) -> Result<std::path::PathBuf, String> {
            Err("not used".to_string())
        }

        fn thumbnail_exists(&self, _file_hash: &str) -> Result<bool, String> {
            Ok(true)
        }

        fn write_thumbnail(
            &self,
            _file_hash: &str,
            _bytes: &[u8],
            _extension: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        fn delete(&self, _file_hash: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn blob_source_boundary_is_small_and_explicit() {
        let source = TestBlobSource;
        assert!(source.thumbnail_exists("hash").unwrap());
    }

    struct ThumbnailOnlyBlobSource {
        thumbnail: Vec<u8>,
    }

    impl BlobSource for ThumbnailOnlyBlobSource {
        fn original_path(
            &self,
            _file_hash: &str,
            _mime_type: &str,
        ) -> Result<std::path::PathBuf, String> {
            Ok(std::path::PathBuf::from("/original-must-not-be-read"))
        }

        fn thumbnail_exists(&self, _file_hash: &str) -> Result<bool, String> {
            Ok(true)
        }

        fn read_thumbnail(&self, _file_hash: &str) -> Result<Option<Vec<u8>>, String> {
            Ok(Some(self.thumbnail.clone()))
        }

        fn write_thumbnail(
            &self,
            _file_hash: &str,
            _bytes: &[u8],
            _extension: &str,
        ) -> Result<(), String> {
            Ok(())
        }

        fn delete(&self, _file_hash: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn palette_repair_reuses_the_persisted_thumbnail() {
        let (_directory, store, blobs, file_id, hash) = fixture();
        let thumbnail = blobs.read_original(&hash, Some("png")).unwrap();
        let source = ThumbnailOnlyBlobSource { thumbnail };

        let outcome = execute_work(&store, &source, &work(file_id, WorkKind::DominantColors))
            .await
            .unwrap();

        assert!(outcome.dominant_colors_written);
    }

    #[tokio::test]
    async fn perceptual_hash_repair_reuses_the_persisted_thumbnail() {
        let (_directory, store, blobs, file_id, hash) = fixture();
        let thumbnail = blobs.read_original(&hash, Some("png")).unwrap();
        let source = ThumbnailOnlyBlobSource { thumbnail };

        let outcome = execute_work(&store, &source, &work(file_id, WorkKind::PerceptualHash))
            .await
            .unwrap();

        assert!(outcome.perceptual_hash_written);
    }
}
