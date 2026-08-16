//! File import pipeline for Picto.
//!
//! Takes a file path -> hashes it -> detects MIME (by header bytes) -> extracts metadata
//! -> generates thumbnail (SIMD-accelerated) -> writes to blob store.

use std::path::Path;

use crate::blob_store::BlobStore;
use crate::media_processing::{self, PreparedMediaSource};
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
    /// Skip thumbnail generation when the caller will defer it.
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
    pub thumbnail: Option<(Vec<u8>, String)>,
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
        path: &Path,
        options: &ImportOptions,
    ) -> ImportResult<PreparedBlobImport> {
        let mut source = PreparedMediaSource::prepare_ingest(path).await?;
        let file_data = source.file_bytes.take().expect("ingest source bytes");
        let file_size = source.size_bytes.expect("ingest source size");

        let hash = media_processing::get_hash_from_bytes(&file_data);
        let hex_hash = hex::encode(&hash);

        if !source.caps.ingest_supported {
            return Err(ImportError::UnsupportedFile(format!(
                "Unsupported file type: {}",
                path.display()
            )));
        }

        let thumbnail_result =
            if options.skip_thumbnail || !source.caps.should_inline_thumbnail_on_ingest() {
                None
            } else {
                source
                    .render_thumbnail_bytes(options.thumbnail_dimensions, 35)
                    .await
                    .ok()
            };

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
            mime: source.mime_type,
            size: file_size,
            pixel_width: source.pixel_width.map(|w| w as i64),
            pixel_height: source.pixel_height.map(|h| h as i64),
            duration_ms: source.duration_ms.map(|d| d as i64),
            num_frames: source.num_frames.map(|n| n as i64),
            has_audio: source.has_audio,
            has_thumbnail: thumbnail_result.is_some(),
            thumbnail: thumbnail_result,
            name,
            created_at,
            tags_applied,
            file_bytes: file_data,
        })
    }

    pub fn persist_blob_import(
        blob_store: &BlobStore,
        prepared: &PreparedBlobImport,
    ) -> ImportResult<()> {
        let blob_ext = crate::blob_store::mime_to_extension(&prepared.mime);
        blob_store.write_original(&prepared.hex_hash, &prepared.file_bytes, Some(blob_ext))?;
        if let Some((thumb_bytes, thumb_ext)) = &prepared.thumbnail {
            blob_store.write_thumbnail(&prepared.hex_hash, thumb_bytes, thumb_ext)?;
        }
        Ok(())
    }
}
