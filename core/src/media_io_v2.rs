//! Physical media I/O for the replacement backend.
//!
//! Logical item selection is resolved through `query_v2`; physical storage is
//! always addressed by `media_file.file_hash`. This keeps collection expansion
//! and file access separate, while making export and shell actions use the
//! same ordered media list.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{resources, Application, FileHash, ItemTarget, MutationReceipt};
use crate::blob_store::{mime_to_extension, BlobStore};
use crate::media_processing_v2;
use crate::store::Store;
use crate::workers_v2::WorkKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ResolvedFilePath {
    pub file_hash: FileHash,
    #[ts(type = "string")]
    pub path: PathBuf,
}

/// Resolve physical files in the caller's order without creating work or
/// changing SQLite. Missing hashes fail the whole operation instead of being
/// silently dropped from a drag/export request.
pub fn resolve_file_paths(
    store: &Store,
    blobs: &BlobStore,
    file_hashes: &[FileHash],
) -> Result<Vec<ResolvedFilePath>, String> {
    let hashes = serde_json::to_string(
        &file_hashes
            .iter()
            .map(|file_hash| file_hash.0.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| format!("Could not encode physical file selection: {error}"))?;
    let metadata = store.read(|connection| {
        let mut statement = connection.prepare(
            "SELECT CAST(input.value AS TEXT), mf.mime_type
             FROM json_each(?1) input
             LEFT JOIN media_file mf ON mf.file_hash = CAST(input.value AS TEXT)
             ORDER BY CAST(input.key AS INTEGER)",
        )?;
        let rows = statement
            .query_map([hashes], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>();
        rows
    })?;
    metadata
        .into_iter()
        .map(|(file_hash, mime_type)| {
            let mime_type =
                mime_type.ok_or_else(|| format!("Physical file not found: {file_hash}"))?;
            let extension = mime_to_extension(&mime_type);
            let path = blobs
                .find_original(&file_hash, Some(extension))
                .map_err(|error| format!("Failed to resolve {file_hash}: {error}"))?
                .map(|(path, _)| path)
                .ok_or_else(|| format!("Original blob is missing for physical file {file_hash}"))?;
            Ok(ResolvedFilePath {
                file_hash: FileHash(file_hash),
                path,
            })
        })
        .collect()
}

/// Resolve every physical file represented by a logical item target.
/// Collections expand in their stored member order.
pub fn resolve_target_file_paths(
    store: &Store,
    projections: &crate::projection_v2::ProjectionStore,
    blobs: &BlobStore,
    target: &ItemTarget,
) -> Result<Vec<ResolvedFilePath>, String> {
    let selection_snapshot = projections.selection_snapshot();
    let item_ids = store.read_result(|connection| {
        crate::query_v2::resolve_target_ids(connection, &selection_snapshot, target)
            .map_err(|error| error.to_string())
    })?;
    let hashes = ordered_media(store, projections, &item_ids)?
        .into_iter()
        .map(|media| media.file_hash)
        .collect::<Vec<_>>();
    resolve_file_paths(store, blobs, &hashes)
}

/// Request one missing thumbnail without decoding media on the caller's thread.
/// Visible requests replace a deferred per-item row with one immediately
/// eligible file-level row; retries retain their backoff.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct RequestThumbnailResult {
    pub ready: bool,
    pub supported: bool,
    pub queued: bool,
}

pub fn request_thumbnail(
    store: &Store,
    blobs: &BlobStore,
    file_hash: &FileHash,
) -> Result<RequestThumbnailResult, String> {
    if blobs
        .find_thumbnail_path(&file_hash.0)
        .map_err(|error| format!("Thumbnail lookup failed: {error}"))?
        .is_some()
    {
        return Ok(RequestThumbnailResult {
            ready: true,
            supported: true,
            queued: false,
        });
    }

    let (file_id, mime_type, frame_count) = store.read(|connection| {
        connection.query_row(
            "SELECT file_id, mime_type, frame_count FROM media_file WHERE file_hash = ?1",
            [&file_hash.0],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
    })?;
    if !crate::media_capabilities::capabilities_for_stored_media(&mime_type, frame_count)
        .can_thumbnail()
    {
        return Ok(RequestThumbnailResult {
            ready: false,
            supported: false,
            queued: false,
        });
    }

    const VISIBLE_PRIORITY: &str = "0001-01-01T00:00:00Z";
    let now = Utc::now().to_rfc3339();
    let (queued, _, _) = store.transaction_if_changed(|transaction| {
        let rows = {
            let mut statement = transaction.prepare(
                "SELECT media_item_id, status, attempt_count, available_at
                 FROM work_item WHERE file_id = ?1 AND work_type = 'thumbnail'",
            )?;
            let rows = statement
                .query_map([file_id], |row| {
                    Ok((
                        row.get::<_, Option<i64>>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        };
        if rows.iter().any(|(_, status, _, _)| status == "running")
            || rows.iter().any(|(_, _, attempts, _)| *attempts > 0)
        {
            return Ok((false, false));
        }
        if rows.len() == 1 && rows[0].0.is_none() && rows[0].3 == VISIBLE_PRIORITY {
            return Ok((false, false));
        }

        transaction.execute(
            "DELETE FROM work_item
             WHERE file_id = ?1 AND work_type = 'thumbnail' AND status = 'pending'",
            [file_id],
        )?;
        transaction.execute(
            "INSERT INTO work_item (
                 file_id, work_type, status, attempt_count,
                 available_at, created_at, updated_at
             ) VALUES (?1, 'thumbnail', 'pending', 0, ?2, ?3, ?3)",
            params![file_id, VISIBLE_PRIORITY, now],
        )?;
        Ok((true, true))
    })?;
    Ok(RequestThumbnailResult {
        ready: false,
        supported: true,
        queued,
    })
}

/// Explicit regeneration is allowed to wait; viewport thumbnail requests are not.
pub async fn render_thumbnail_now(
    store: &Store,
    blobs: &BlobStore,
    file_hash: &FileHash,
) -> Result<bool, String> {
    let file_id = file_id_for_hash(store, file_hash)?;
    Ok(media_processing_v2::execute(
        store,
        blobs,
        media_processing_v2::DerivativeRequest {
            media_item_id: None,
            file_id: Some(file_id),
            kind: WorkKind::Thumbnail,
        },
    )
    .await?
    .thumbnail_written)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ThumbnailQueueResult {
    pub requested: usize,
    pub enqueued: usize,
    pub already_queued: usize,
    pub receipt: MutationReceipt,
}

/// Queue thumbnail regeneration without performing it inline. Queue identity
/// is the physical file, so shared blobs are regenerated once.
pub fn enqueue_thumbnail_regeneration(
    application: &Application,
    file_hashes: &[FileHash],
) -> Result<ThumbnailQueueResult, String> {
    let unique = file_hashes
        .iter()
        .map(|file_hash| file_hash.0.as_str())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let encoded = serde_json::to_string(&unique)
        .map_err(|error| format!("Could not encode thumbnail targets: {error}"))?;
    let now = Utc::now().to_rfc3339();
    let ((known, enqueued), revision, _) =
        application.store().transaction_if_changed(|transaction| {
            let known: i64 = transaction.query_row(
                "SELECT COUNT(*) FROM media_file
                 WHERE file_hash IN (SELECT CAST(value AS TEXT) FROM json_each(?1))",
                [&encoded],
                |row| row.get(0),
            )?;
            let known = usize::try_from(known)
                .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, known))?;
            if known != unique.len() {
                return Err(rusqlite::Error::InvalidParameterName(
                    "A thumbnail target is not a physical file".to_string(),
                ));
            }
            let enqueued = transaction.execute(
                "INSERT INTO work_item (
                     file_id, work_type, status, attempt_count,
                     available_at, created_at, updated_at
                 )
                 SELECT mf.file_id, 'thumbnail', 'pending', 0, ?2, ?2, ?2
                 FROM media_file mf
                 JOIN json_each(?1) target
                   ON mf.file_hash = CAST(target.value AS TEXT)
                 WHERE 1
                 ON CONFLICT(file_id, work_type)
                   WHERE media_item_id IS NULL AND file_id IS NOT NULL
                 DO NOTHING",
                params![encoded, now],
            )?;
            Ok(((known, enqueued), enqueued != 0))
        })?;
    Ok(ThumbnailQueueResult {
        requested: known,
        enqueued,
        already_queued: known - enqueued,
        receipt: MutationReceipt {
            revision,
            resources: vec![resources::TASKS.to_string()],
            item_ids: Vec::new(),
        },
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Original,
    Png,
    Jpeg,
    Webp,
    Avif,
}

impl Default for ExportFormat {
    fn default() -> Self {
        Self::Original
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ExportRequest {
    pub target: ItemTarget,
    #[ts(type = "string")]
    pub output_dir: PathBuf,
    pub format: ExportFormat,
    pub quality: u8,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub keep_aspect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ExportResult {
    pub selected_item_count: usize,
    pub selected_media_count: usize,
    pub exported: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

/// Export every physical media member selected by `target`.
///
/// Query resolution owns collection expansion and stored member ordering;
/// this function only reads the resulting blobs and writes outside the
/// library. Conversion is limited to image formats supported by `image`.
pub fn export(
    store: &Store,
    projections: &crate::projection_v2::ProjectionStore,
    blobs: &BlobStore,
    library_root: &Path,
    request: &ExportRequest,
) -> Result<ExportResult, String> {
    reject_library_path(library_root, &request.output_dir)?;
    fs::create_dir_all(&request.output_dir)
        .map_err(|error| format!("Failed to create export directory: {error}"))?;

    let selection_snapshot = projections.selection_snapshot();
    let item_ids = store.read_result(|connection| {
        crate::query_v2::resolve_target_ids(connection, &selection_snapshot, &request.target)
            .map_err(|error| error.to_string())
    })?;
    let media = ordered_media(store, projections, &item_ids)?;
    let mut result = ExportResult {
        selected_item_count: item_ids.len(),
        selected_media_count: media.len(),
        exported: 0,
        skipped: 0,
        errors: Vec::new(),
    };

    for (index, media) in media.iter().enumerate() {
        match export_one(blobs, &request.output_dir, request, media, index) {
            Ok(()) => result.exported += 1,
            Err(error) => {
                result.skipped += 1;
                result.errors.push(error);
            }
        }
    }
    Ok(result)
}

#[derive(Debug, Clone)]
struct ExportMedia {
    file_hash: FileHash,
    mime_type: String,
    name: Option<String>,
    position: i64,
}

fn ordered_media(
    store: &Store,
    projections: &crate::projection_v2::ProjectionStore,
    item_ids: &[i64],
) -> Result<Vec<ExportMedia>, String> {
    let media_ids = item_ids
        .iter()
        .flat_map(|root_id| {
            projections
                .group_order(*root_id)
                .unwrap_or_else(|| vec![*root_id])
        })
        .collect::<Vec<_>>();
    let encoded = serde_json::to_string(&media_ids)
        .map_err(|error| format!("Could not encode export targets: {error}"))?;
    store.read(|connection| {
        let mut statement = connection.prepare(
            "WITH ordered_media(media_item_id, position) AS (
                 SELECT CAST(value AS INTEGER), CAST(key AS INTEGER) FROM json_each(?1)
             )
             SELECT mf.file_hash, mf.mime_type, ma.name, ordered_media.position
             FROM ordered_media
             JOIN media_asset ma ON ma.item_id = ordered_media.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             ORDER BY ordered_media.position",
        )?;
        let media = statement
            .query_map([encoded], |row| {
                Ok(ExportMedia {
                    file_hash: FileHash(row.get(0)?),
                    mime_type: row.get(1)?,
                    name: row.get(2)?,
                    position: row.get(3)?,
                })
            })?
            .collect();
        media
    })
}

fn export_one(
    blobs: &BlobStore,
    output_dir: &Path,
    request: &ExportRequest,
    media: &ExportMedia,
    index: usize,
) -> Result<(), String> {
    let extension = mime_to_extension(&media.mime_type);
    let bytes = blobs
        .read_original(&media.file_hash.0, Some(extension))
        .map_err(|error| format!("Failed to read {}: {error}", media.file_hash.0))?;
    let stem = sanitize_stem(
        media.name.as_deref().unwrap_or_default(),
        &format!(
            "{}-{}",
            &media.file_hash.0[..12],
            media.position.max(index as i64)
        ),
    );
    let output_extension = output_extension(request.format, extension);
    let output_path = unique_path(output_dir, &stem, output_extension);
    let output = match request.format {
        ExportFormat::Original => bytes,
        format => {
            if !media.mime_type.starts_with("image/") {
                return Err(format!(
                    "Cannot convert non-image media {}",
                    media.file_hash.0
                ));
            }
            let image = image::load_from_memory(&bytes)
                .map_err(|error| format!("Failed to decode {}: {error}", media.file_hash.0))?;
            let image = resize(image, request.width, request.height, request.keep_aspect);
            encode(image, format, request.quality)?
        }
    };
    fs::write(&output_path, output)
        .map_err(|error| format!("Failed to write {}: {error}", output_path.display()))
}

fn file_id_for_hash(store: &Store, file_hash: &FileHash) -> Result<i64, String> {
    store.read_result(|connection| {
        connection
            .query_row(
                "SELECT file_id FROM media_file WHERE file_hash = ?1",
                [&file_hash.0],
                |row| row.get(0),
            )
            .map_err(|_| format!("Physical file not found: {}", file_hash.0))
    })
}

fn output_extension<'a>(format: ExportFormat, original: &'a str) -> &'a str {
    match format {
        ExportFormat::Original => original,
        ExportFormat::Png => "png",
        ExportFormat::Jpeg => "jpg",
        ExportFormat::Webp => "webp",
        ExportFormat::Avif => "avif",
    }
}

fn resize(
    image: image::DynamicImage,
    width: Option<u32>,
    height: Option<u32>,
    keep_aspect: bool,
) -> image::DynamicImage {
    if width.unwrap_or(0) == 0 && height.unwrap_or(0) == 0 {
        return image;
    }
    if keep_aspect {
        image.resize(
            width.unwrap_or(u32::MAX),
            height.unwrap_or(u32::MAX),
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        image.resize_exact(
            width.unwrap_or_else(|| image.width()),
            height.unwrap_or_else(|| image.height()),
            image::imageops::FilterType::Lanczos3,
        )
    }
}

fn encode(
    image: image::DynamicImage,
    format: ExportFormat,
    quality: u8,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    match format {
        ExportFormat::Original => unreachable!(),
        ExportFormat::Png => {
            use image::ImageEncoder;
            let rgba = image.to_rgba8();
            image::codecs::png::PngEncoder::new(&mut output)
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|error| format!("PNG encode failed: {error}"))?;
        }
        ExportFormat::Jpeg => {
            let rgb = image.to_rgb8();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality.clamp(1, 100))
                .encode_image(&image::DynamicImage::ImageRgb8(rgb))
                .map_err(|error| format!("JPEG encode failed: {error}"))?;
        }
        ExportFormat::Webp => {
            let rgba = image.to_rgba8();
            output.extend_from_slice(
                webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height())
                    .encode(quality.clamp(1, 100) as f32)
                    .as_ref(),
            );
        }
        ExportFormat::Avif => image
            .write_to(
                &mut std::io::Cursor::new(&mut output),
                image::ImageFormat::Avif,
            )
            .map_err(|error| format!("AVIF encode failed: {error}"))?,
    }
    Ok(output)
}

fn sanitize_stem(name: &str, fallback: &str) -> String {
    let raw = if name.trim().is_empty() {
        fallback
    } else {
        name.trim()
    };
    let stem = Path::new(raw)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(raw);
    let sanitized = stem
        .chars()
        .map(|character| match character {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            character if character.is_control() => '_',
            character => character,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string();
    if sanitized.is_empty() {
        fallback.to_string()
    } else {
        sanitized
    }
}

fn unique_path(directory: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut path = directory.join(format!("{stem}.{extension}"));
    let mut suffix = 2;
    while path.exists() {
        path = directory.join(format!("{stem} ({suffix}).{extension}"));
        suffix += 1;
    }
    path
}

fn reject_library_path(library_root: &Path, output: &Path) -> Result<(), String> {
    let library_root = fs::canonicalize(library_root)
        .map_err(|error| format!("Failed to resolve library path: {error}"))?;
    let candidate = if output.exists() {
        fs::canonicalize(output)
    } else {
        output
            .parent()
            .map(fs::canonicalize)
            .unwrap_or_else(|| Err(std::io::Error::from(std::io::ErrorKind::NotFound)))
    }
    .map_err(|error| format!("Failed to resolve export path: {error}"))?;
    if candidate.starts_with(&library_root) {
        return Err("Cannot export into the library directory".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::app::ItemId;
    use crate::operations_v2::OrganizeIntoCollectionInput;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T00:00:00Z";

    struct Fixture {
        directory: TempDir,
        application: Application,
        hashes: Vec<FileHash>,
        group_id: ItemId,
    }

    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let application = Application::new(Arc::clone(&store));
        let mut hashes = Vec::new();
        let mut item_ids = Vec::new();
        for color in [[255, 0, 0], [0, 0, 255]] {
            let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(16, 8, Rgb(color)));
            let mut bytes = Vec::new();
            image
                .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
                .unwrap();
            let hash = hex::encode(Sha256::digest(&bytes));
            application
                .blobs()
                .write_original(&hash, &bytes, Some("png"))
                .unwrap();
            let item = application
                .ingest_prepared(&crate::ingest_v2::PreparedMediaInput {
                    file_hash: hash.clone(),
                    mime_type: "image/png".to_string(),
                    size_bytes: bytes.len() as i64,
                    pixel_width: Some(16),
                    pixel_height: Some(8),
                    duration_ms: None,
                    frame_count: Some(1),
                    has_audio: false,
                    name: Some("same.png".to_string()),
                    notes: None,
                    rating: None,
                    source_urls: Vec::new(),
                    tags: Vec::new(),
                    lifecycle: crate::app::Lifecycle::Active,
                    captured_at: None,
                    source: None,
                    target_folder_id: None,
                    target_folder_ids: Vec::new(),
                })
                .unwrap();
            hashes.push(FileHash(hash));
            item_ids.push(item.root_item_id);
        }
        application
            .store()
            .transaction(|transaction| {
                transaction.execute("DELETE FROM work_item", [])?;
                Ok(())
            })
            .unwrap();
        let group_id = application
            .organize_into_collection(OrganizeIntoCollectionInput {
                target: ItemTarget::Explicit {
                    item_ids: vec![item_ids[1], item_ids[0]],
                },
                label: Some("ordered".to_string()),
                winning_collection_id: None,
            })
            .unwrap()
            .collection_id;
        Fixture {
            directory,
            application,
            hashes,
            group_id,
        }
    }

    #[test]
    fn resolves_physical_paths_in_input_order_without_queue_side_effects() {
        let fixture = fixture();
        let reversed = vec![fixture.hashes[1].clone(), fixture.hashes[0].clone()];
        let resolved = resolve_file_paths(
            fixture.application.store(),
            fixture.application.blobs(),
            &reversed,
        )
        .unwrap();
        assert_eq!(
            resolved
                .iter()
                .map(|item| &item.file_hash)
                .collect::<Vec<_>>(),
            reversed.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            fixture
                .application
                .store()
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM work_item",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            0
        );
    }

    #[test]
    fn target_path_resolution_expands_a_collection_in_member_order() {
        let fixture = fixture();
        let resolved = resolve_target_file_paths(
            fixture.application.store(),
            fixture.application.projections(),
            fixture.application.blobs(),
            &ItemTarget::Explicit {
                item_ids: vec![fixture.group_id],
            },
        )
        .unwrap();
        assert_eq!(
            resolved
                .iter()
                .map(|item| &item.file_hash)
                .collect::<Vec<_>>(),
            vec![&fixture.hashes[1], &fixture.hashes[0]],
        );
    }

    #[test]
    fn visible_thumbnail_request_promotes_deferred_work_without_decoding() {
        let fixture = fixture();
        fixture
            .application
            .store()
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO work_item (
                     media_item_id, file_id, work_type, status, attempt_count,
                     available_at, created_at, updated_at
                 ) VALUES (1, 1, 'thumbnail', 'pending', 0, ?1, ?1, ?1)",
                    [NOW],
                )?;
                Ok(())
            })
            .unwrap();

        let first = request_thumbnail(
            fixture.application.store(),
            fixture.application.blobs(),
            &fixture.hashes[0],
        )
        .unwrap();
        let second = request_thumbnail(
            fixture.application.store(),
            fixture.application.blobs(),
            &fixture.hashes[0],
        )
        .unwrap();
        assert_eq!(
            first,
            RequestThumbnailResult {
                ready: false,
                supported: true,
                queued: true
            }
        );
        assert_eq!(
            second,
            RequestThumbnailResult {
                ready: false,
                supported: true,
                queued: false
            }
        );
        assert!(fixture
            .application
            .blobs()
            .find_thumbnail_path(&fixture.hashes[0].0)
            .unwrap()
            .is_none());
        let target = fixture
            .application
            .store()
            .read(|connection| {
                connection.query_row(
                    "SELECT media_item_id, available_at, COUNT(*) OVER ()
                 FROM work_item WHERE file_id = 1 AND work_type = 'thumbnail'",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, Option<i64>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, i64>(2)?,
                        ))
                    },
                )
            })
            .unwrap();
        assert_eq!(target, (None, "0001-01-01T00:00:00Z".to_string(), 1));
    }

    #[test]
    fn regeneration_is_durable_and_deduplicated_by_physical_file() {
        let fixture = fixture();
        let hashes = vec![fixture.hashes[0].clone(), fixture.hashes[0].clone()];
        let first = enqueue_thumbnail_regeneration(&fixture.application, &hashes).unwrap();
        let second = enqueue_thumbnail_regeneration(&fixture.application, &hashes).unwrap();
        assert_eq!(first.enqueued, 1);
        assert_eq!(second.already_queued, 1);
        assert_eq!(
            fixture
                .application
                .store()
                .read(|connection| connection.query_row(
                    "SELECT COUNT(*) FROM work_item",
                    [],
                    |row| row.get::<_, i64>(0)
                ))
                .unwrap(),
            1
        );
    }

    #[test]
    fn collection_export_uses_stored_member_order() {
        let fixture = fixture();
        let output = tempfile::tempdir().unwrap();
        let request = ExportRequest {
            target: ItemTarget::Explicit {
                item_ids: vec![fixture.group_id],
            },
            output_dir: output.path().to_path_buf(),
            format: ExportFormat::Original,
            quality: 82,
            width: None,
            height: None,
            keep_aspect: true,
        };
        let result = export(
            fixture.application.store(),
            fixture.application.projections(),
            fixture.application.blobs(),
            fixture.directory.path(),
            &request,
        )
        .unwrap();
        assert_eq!(result.selected_item_count, 1);
        assert_eq!(result.selected_media_count, 2);
        assert_eq!(result.exported, 2);
        assert!(output.path().join("same.png").exists());
        assert!(output.path().join("same (2).png").exists());
        let first = image::open(output.path().join("same.png"))
            .unwrap()
            .to_rgb8();
        let second = image::open(output.path().join("same (2).png"))
            .unwrap()
            .to_rgb8();
        assert_eq!(first.get_pixel(0, 0), &Rgb([0, 0, 255]));
        assert_eq!(second.get_pixel(0, 0), &Rgb([255, 0, 0]));
    }

    #[test]
    fn export_rejects_library_destination() {
        let fixture = fixture();
        let request = ExportRequest {
            target: ItemTarget::Explicit {
                item_ids: vec![ItemId(1)],
            },
            output_dir: fixture.directory.path().join("blobs"),
            format: ExportFormat::Original,
            quality: 82,
            width: None,
            height: None,
            keep_aspect: true,
        };
        let error = export(
            fixture.application.store(),
            fixture.application.projections(),
            fixture.application.blobs(),
            fixture.directory.path(),
            &request,
        )
        .unwrap_err();
        assert!(error.contains("library"));
    }
}
