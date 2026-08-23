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

use crate::app::{resources, Application, FileHash, ItemTarget, MutationReceipt};
use crate::blob_store::{mime_to_extension, BlobStore};
use crate::media_processing_v2;
use crate::store::Store;
use crate::workers_v2::WorkKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedFilePath {
    pub file_hash: FileHash,
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

/// Ensure one thumbnail through the shared derivative executor.
///
/// Existing thumbnails are treated as success, so retries and repeated
/// protocol requests are safe and do not create duplicate generation paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnsureThumbnailResult {
    pub created: bool,
}

pub async fn ensure_thumbnail(
    store: &Store,
    blobs: &BlobStore,
    file_hash: &FileHash,
) -> Result<EnsureThumbnailResult, String> {
    let file_id = file_id_for_hash(store, file_hash)?;
    let outcome = media_processing_v2::execute(
        store,
        blobs,
        media_processing_v2::DerivativeRequest {
            media_item_id: None,
            file_id: Some(file_id),
            kind: WorkKind::Thumbnail,
        },
    )
    .await?;
    Ok(EnsureThumbnailResult {
        created: outcome.thumbnail_written,
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
            let known: usize = transaction.query_row(
                "SELECT COUNT(*) FROM media_file
                 WHERE file_hash IN (SELECT CAST(value AS TEXT) FROM json_each(?1))",
                [&encoded],
                |row| row.get(0),
            )?;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRequest {
    pub target: ItemTarget,
    pub output_dir: PathBuf,
    pub format: ExportFormat,
    pub quality: u8,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub keep_aspect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    blobs: &BlobStore,
    library_root: &Path,
    request: &ExportRequest,
) -> Result<ExportResult, String> {
    reject_library_path(library_root, &request.output_dir)?;
    fs::create_dir_all(&request.output_dir)
        .map_err(|error| format!("Failed to create export directory: {error}"))?;

    let item_ids = store.read_result(|connection| {
        crate::query_v2::resolve_target_ids(connection, &request.target)
            .map_err(|error| error.to_string())
    })?;
    let media = ordered_media(store, &item_ids)?;
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

fn ordered_media(store: &Store, item_ids: &[i64]) -> Result<Vec<ExportMedia>, String> {
    let encoded = serde_json::to_string(item_ids)
        .map_err(|error| format!("Could not encode export targets: {error}"))?;
    store.read(|connection| {
        let mut statement = connection.prepare(
            "WITH roots(root_item_id, root_order) AS (
                 SELECT CAST(value AS INTEGER), CAST(key AS INTEGER) FROM json_each(?1)
             ), ordered_media(root_order, media_item_id, position) AS (
                 SELECT roots.root_order, ma.item_id, 0
                 FROM roots JOIN media_asset ma ON ma.item_id = roots.root_item_id
                 UNION ALL
                 SELECT roots.root_order, cm.media_item_id, cm.position_rank
                 FROM roots
                 JOIN collection_member cm ON cm.collection_id = roots.root_item_id
             )
             SELECT mf.file_hash, mf.mime_type, ma.name, ordered_media.position
             FROM ordered_media
             JOIN media_asset ma ON ma.item_id = ordered_media.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             ORDER BY ordered_media.root_order, ordered_media.position,
                      ordered_media.media_item_id",
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
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb};
    use sha2::{Digest, Sha256};
    use std::io::Cursor;
    use tempfile::TempDir;

    const NOW: &str = "2026-01-01T00:00:00Z";

    struct Fixture {
        directory: TempDir,
        application: Application,
        hashes: Vec<FileHash>,
    }

    fn fixture() -> Fixture {
        let directory = tempfile::tempdir().unwrap();
        let store = Arc::new(Store::open(directory.path()).unwrap());
        let application = Application::new(Arc::clone(&store));
        let mut hashes = Vec::new();
        store
            .transaction(|transaction| {
                for (file_id, color, name) in [(1, [255, 0, 0], "same.png"), (2, [0, 0, 255], "same.png")] {
                    let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(16, 8, Rgb(color)));
                    let mut bytes = Vec::new();
                    image.write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png).unwrap();
                    let hash = hex::encode(Sha256::digest(&bytes));
                    application
                        .blobs()
                        .write_original(&hash, &bytes, Some("png"))
                        .unwrap();
                    transaction.execute(
                        "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes,
                             pixel_width, pixel_height, created_at)
                         VALUES (?1, ?2, 'image/png', ?3, 16, 8, ?4)",
                        rusqlite::params![file_id, hash, bytes.len() as i64, NOW],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_item (item_id, item_key, kind, label, created_at, updated_at)
                         VALUES (?1, ?2, 'media', ?3, ?4, ?4)",
                        rusqlite::params![file_id, format!("item-{file_id}"), name, NOW],
                    )?;
                    transaction.execute(
                        "INSERT INTO media_asset (item_id, file_id, name, imported_at, updated_at)
                         VALUES (?1, ?1, ?2, ?3, ?3)",
                        rusqlite::params![file_id, name, NOW],
                    )?;
                    transaction.execute(
                        "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                        [file_id],
                    )?;
                    hashes.push(FileHash(hash));
                }
                transaction.execute(
                    "INSERT INTO library_item (item_id, item_key, kind, label, created_at, updated_at)
                     VALUES (10, 'collection-10', 'collection', 'ordered', ?1, ?1)",
                    [NOW],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (10, 'active')",
                    [],
                )?;
                transaction.execute(
                    "DELETE FROM library_root WHERE item_id IN (1, 2)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (10, 2, 10), (10, 1, 20)",
                    [],
                )?;
                transaction.execute(
                    "UPDATE library_item SET cover_media_item_id = 2 WHERE item_id = 10",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        Fixture {
            directory,
            application,
            hashes,
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

    #[tokio::test]
    async fn ensure_thumbnail_is_idempotent() {
        let fixture = fixture();
        let first = ensure_thumbnail(
            fixture.application.store(),
            fixture.application.blobs(),
            &fixture.hashes[0],
        )
        .await
        .unwrap();
        let second = ensure_thumbnail(
            fixture.application.store(),
            fixture.application.blobs(),
            &fixture.hashes[0],
        )
        .await
        .unwrap();
        assert!(first.created);
        assert!(!second.created);
        assert!(fixture
            .application
            .blobs()
            .find_thumbnail_path(&fixture.hashes[0].0)
            .unwrap()
            .is_some());
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
                item_ids: vec![ItemId(10)],
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
            fixture.application.blobs(),
            fixture.directory.path(),
            &request,
        )
        .unwrap_err();
        assert!(error.contains("library"));
    }
}
