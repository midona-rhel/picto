//! Physical media I/O over the canonical library backend.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::blob_store::mime_to_extension;
use crate::dto::FileHash;
use crate::library_application::LibraryApplication;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ResolvedFilePath {
    pub file_hash: FileHash,
    #[ts(type = "string")]
    pub path: PathBuf,
}
pub fn resolve_file_paths_library(
    application: &LibraryApplication,
    file_hashes: &[FileHash],
) -> Result<Vec<ResolvedFilePath>, String> {
    let hashes = serde_json::to_string(
        &file_hashes
            .iter()
            .map(|file_hash| file_hash.0.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(|error| format!("Could not encode physical file selection: {error}"))?;
    let metadata = application
        .library()
        .auxiliary_read(
            picto_library::database::WorkPriority::VisibleRead,
            |connection| {
                let mut statement = connection.prepare(
                    "SELECT CAST(input.value AS TEXT),
                            (SELECT file.file_path
                             FROM media_file file
                             WHERE file.content_hash = CAST(input.value AS TEXT)
                             ORDER BY file.file_id LIMIT 1)
                     FROM json_each(?1) input
                     ORDER BY CAST(input.key AS INTEGER)",
                )?;
                let rows = statement
                    .query_map([hashes], |row| {
                        Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(Into::into);
                rows
            },
        )
        .map_err(|error| error.to_string())?;
    resolve_metadata_paths(metadata)
}

pub fn resolve_target_file_paths_library(
    application: &LibraryApplication,
    target: &picto_library::selection::SelectionTarget,
) -> Result<Vec<ResolvedFilePath>, String> {
    let metadata = application
        .library()
        .auxiliary_read_consistent(
            picto_library::database::WorkPriority::VisibleRead,
            |connection, projection| {
                let roots =
                    picto_library::selection::resolve_ordered(connection, projection, target)?;
                let media_ids = roots
                    .iter()
                    .flat_map(|root_id| {
                        projection
                            .collection_orders
                            .get(root_id)
                            .map(|members| members.iter().map(|member| member.0).collect())
                            .unwrap_or_else(|| vec![root_id.0])
                    })
                    .collect::<Vec<_>>();
                let encoded = serde_json::to_string(&media_ids)?;
                let mut statement = connection.prepare(
                    "WITH ordered_media(media_id, position) AS (
                         SELECT CAST(value AS INTEGER), CAST(key AS INTEGER)
                         FROM json_each(?1)
                     )
                     SELECT file.content_hash, file.file_path
                     FROM ordered_media
                     JOIN media_item media ON media.media_id = ordered_media.media_id
                     JOIN media_file file ON file.file_id = media.file_id
                     ORDER BY ordered_media.position",
                )?;
                let rows = statement
                    .query_map([encoded], |row| {
                        Ok((row.get::<_, String>(0)?, Some(row.get::<_, String>(1)?)))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(Into::into);
                rows
            },
        )
        .map_err(|error| error.to_string())?;
    resolve_metadata_paths(metadata)
}

fn resolve_metadata_paths(
    metadata: Vec<(String, Option<String>)>,
) -> Result<Vec<ResolvedFilePath>, String> {
    metadata
        .into_iter()
        .map(|(file_hash, file_path)| {
            let file_path =
                file_path.ok_or_else(|| format!("Physical file not found: {file_hash}"))?;
            let path = PathBuf::from(file_path);
            if !path.is_file() {
                return Err(format!(
                    "Original file is missing for physical file {file_hash}: {}",
                    path.display()
                ));
            }
            Ok(ResolvedFilePath {
                file_hash: FileHash(file_hash),
                path,
            })
        })
        .collect()
}
pub async fn render_thumbnail_now_library(
    application: &LibraryApplication,
    file_hash: &FileHash,
) -> Result<bool, String> {
    crate::library_media_runtime::render_thumbnail_now(application, &file_hash.0).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ThumbnailQueueResult {
    pub requested: usize,
    pub enqueued: usize,
    pub already_queued: usize,
}

pub fn enqueue_thumbnail_regeneration_library(
    application: &LibraryApplication,
    file_hashes: &[FileHash],
) -> Result<ThumbnailQueueResult, String> {
    let hashes = file_hashes
        .iter()
        .map(|file_hash| file_hash.0.clone())
        .collect::<Vec<_>>();
    let (requested, enqueued, _) = application
        .library()
        .enqueue_thumbnail_work(&hashes, &Utc::now().to_rfc3339())
        .map_err(|error| error.to_string())?;
    Ok(ThumbnailQueueResult {
        requested,
        enqueued,
        already_queued: requested - enqueued,
    })
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, Default)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    #[default]
    Original,
    Png,
    Jpeg,
    Webp,
    Avif,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRequest {
    pub target: picto_library::selection::SelectionTarget,
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
pub fn export_library(
    application: &LibraryApplication,
    request: &ExportRequest,
) -> Result<ExportResult, String> {
    reject_library_path(application.root(), &request.output_dir)?;
    fs::create_dir_all(&request.output_dir)
        .map_err(|error| format!("Failed to create export directory: {error}"))?;
    let (selected_item_count, media) = ordered_media_library(application, &request.target)?;
    let mut result = ExportResult {
        selected_item_count,
        selected_media_count: media.len(),
        exported: 0,
        skipped: 0,
        errors: Vec::new(),
    };
    for (index, media) in media.iter().enumerate() {
        match export_one(
            &request.output_dir,
            request.format,
            request.quality,
            request.width,
            request.height,
            request.keep_aspect,
            media,
            index,
        ) {
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
    file_path: PathBuf,
    mime_type: String,
    name: Option<String>,
    position: i64,
}

fn ordered_media_library(
    application: &LibraryApplication,
    target: &picto_library::selection::SelectionTarget,
) -> Result<(usize, Vec<ExportMedia>), String> {
    application
        .library()
        .auxiliary_read_consistent(
            picto_library::database::WorkPriority::VisibleRead,
            |connection, projection| {
                let roots =
                    picto_library::selection::resolve_ordered(connection, projection, target)?;
                let media_ids = roots
                    .iter()
                    .flat_map(|root_id| {
                        projection
                            .collection_orders
                            .get(root_id)
                            .map(|members| members.iter().map(|member| member.0).collect())
                            .unwrap_or_else(|| vec![root_id.0])
                    })
                    .collect::<Vec<_>>();
                let encoded = serde_json::to_string(&media_ids)?;
                let mut statement = connection.prepare(
                    "WITH ordered_media(media_id, position) AS (
                         SELECT CAST(value AS INTEGER), CAST(key AS INTEGER)
                         FROM json_each(?1)
                     )
                     SELECT file.content_hash, file.file_path, file.mime, media.media_name,
                            ordered_media.position
                     FROM ordered_media
                     JOIN media_item media ON media.media_id = ordered_media.media_id
                     JOIN media_file file ON file.file_id = media.file_id
                     ORDER BY ordered_media.position",
                )?;
                let media = statement
                    .query_map([encoded], |row| {
                        Ok(ExportMedia {
                            file_hash: FileHash(row.get(0)?),
                            file_path: PathBuf::from(row.get::<_, String>(1)?),
                            mime_type: row.get(2)?,
                            name: Some(row.get(3)?),
                            position: row.get(4)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok((roots.len(), media))
            },
        )
        .map_err(|error| error.to_string())
}

#[allow(clippy::too_many_arguments)]
fn export_one(
    output_dir: &Path,
    format: ExportFormat,
    quality: u8,
    width: Option<u32>,
    height: Option<u32>,
    keep_aspect: bool,
    media: &ExportMedia,
    index: usize,
) -> Result<(), String> {
    let extension = mime_to_extension(&media.mime_type);
    let bytes = fs::read(&media.file_path).map_err(|error| {
        format!(
            "Failed to read {} at {}: {error}",
            media.file_hash.0,
            media.file_path.display()
        )
    })?;
    let stem = sanitize_stem(
        media.name.as_deref().unwrap_or_default(),
        &format!(
            "{}-{}",
            &media.file_hash.0[..12],
            media.position.max(index as i64)
        ),
    );
    let output_extension = output_extension(format, extension);
    let output_path = unique_path(output_dir, &stem, output_extension);
    let output = match format {
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
            let image = resize(image, width, height, keep_aspect);
            encode(image, format, quality)?
        }
    };
    fs::write(&output_path, output)
        .map_err(|error| format!("Failed to write {}: {error}", output_path.display()))
}

fn output_extension(format: ExportFormat, original: &str) -> &str {
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
