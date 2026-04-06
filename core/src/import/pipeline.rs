//! File import pipeline for Picto.
//!
//! Takes a file path -> hashes it -> detects MIME (by header bytes) -> extracts metadata
//! -> generates thumbnail (SIMD-accelerated) -> writes to blob store.

use std::path::Path;

use crate::blob_store::BlobStore;
use crate::media_capabilities::capabilities_for_detected_mime;
use crate::media_processing;
use crate::tags::normalize as tags;

#[derive(thiserror::Error, Debug)]
pub enum ImportError {
    #[error("Blob storage error: {0}")]
    Blob(#[from] crate::blob_store::BlobError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("File processing error: {0}")]
    FileProcessing(#[from] media_processing::FileError),
    #[error("Zero-size file: {0}")]
    ZeroSizeFile(String),
    #[error("Unsupported file type: {0}")]
    UnsupportedFile(String),
}

pub type ImportResult<T> = Result<T, ImportError>;

#[derive(Debug, Clone)]
pub struct ImportOptions {
    pub tags: Vec<(String, String)>, // (namespace, subtag)
    pub source_urls: Vec<String>,
    pub created_at: Option<String>,
    pub thumbnail_dimensions: (u32, u32),
    /// Override the default file-stem name.
    pub name: Option<String>,
    /// Notes to store on the file (key → text).
    pub notes: Option<std::collections::HashMap<String, String>>,
    /// Skip thumbnail generation (e.g. for non-cover collection members).
    pub skip_thumbnail: bool,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            tags: Vec::new(),
            source_urls: Vec::new(),
            created_at: None,
            thumbnail_dimensions: media_processing::DEFAULT_THUMBNAIL_DIMENSIONS,
            name: None,
            notes: None,
            skip_thumbnail: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreparedBlobImport {
    pub hex_hash: String,
    pub mime: String,
    pub size: u64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub has_thumbnail: bool,
    pub name: Option<String>,
    pub created_at: Option<String>,
    pub tags_applied: Vec<String>,
    /// Cached file bytes from the initial read, available for downstream use
    /// (e.g. perceptual hashing) to avoid re-reading from disk.
    pub file_bytes: Vec<u8>,
}

pub struct ImportPipeline;

impl ImportPipeline {
    pub async fn prepare_blob_import(
        blob_store: &BlobStore,
        path: &Path,
        options: &ImportOptions,
    ) -> ImportResult<PreparedBlobImport> {
        let file_data = tokio::fs::read(path).await?;
        if file_data.is_empty() {
            return Err(ImportError::ZeroSizeFile(path.display().to_string()));
        }
        let file_size = file_data.len() as u64;

        let hash = media_processing::get_hash_from_bytes(&file_data);
        let hex_hash = hex::encode(&hash);

        let file_info = media_processing::get_file_info(path, None).await?;
        let mime_string = file_info.mime.mime_string().to_string();
        let caps = capabilities_for_detected_mime(file_info.mime);

        if !caps.ingest_supported {
            return Err(ImportError::UnsupportedFile(format!(
                "Unsupported file type: {}",
                path.display()
            )));
        }

        if media_processing::is_image(file_info.mime) {
            if let Ok(true) = media_processing::is_decompression_bomb(path) {
                return Err(ImportError::UnsupportedFile(
                    "Decompression bomb".to_string(),
                ));
            }
        }

        let thumbnail_result =
            if options.skip_thumbnail || !caps.should_inline_thumbnail_on_ingest() {
                None
            } else {
                media_processing::generate_thumbnail_bytes(
                    path,
                    options.thumbnail_dimensions,
                    file_info.mime,
                    file_info.duration_ms,
                    file_info.num_frames,
                    35,
                )
                .await
                .ok()
            };

        let blob_ext = crate::blob_store::mime_to_extension(&mime_string);
        blob_store.write_original(&hex_hash, &file_data, Some(blob_ext))?;
        if let Some((ref thumb_bytes, ref thumb_ext)) = thumbnail_result {
            blob_store.write_thumbnail(&hex_hash, thumb_bytes, thumb_ext)?;
        }

        let name = options.name.clone().or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        });
        let created_at = options.created_at.clone().or_else(|| {
            std::fs::metadata(path).ok().and_then(|meta| {
                let ts = meta.created().or_else(|_| meta.modified()).ok()?;
                let dt: chrono::DateTime<chrono::Utc> = ts.into();
                Some(dt.to_rfc3339())
            })
        });

        let mut tags_applied = Vec::new();
        for (ns, st) in &options.tags {
            let full_tag = tags::combine_tag(ns, st);
            if let Some((ns, st)) = tags::parse_tag(&full_tag) {
                tags_applied.push(tags::combine_tag(&ns, &st));
            }
        }

        Ok(PreparedBlobImport {
            hex_hash,
            mime: mime_string,
            size: file_size,
            pixel_width: file_info.width.map(|w| w as i64),
            pixel_height: file_info.height.map(|h| h as i64),
            duration_ms: file_info.duration_ms.map(|d| d as i64),
            num_frames: file_info.num_frames.map(|n| n as i64),
            has_audio: file_info.has_audio,
            has_thumbnail: thumbnail_result.is_some(),
            name,
            created_at,
            tags_applied,
            file_bytes: file_data,
        })
    }
}
