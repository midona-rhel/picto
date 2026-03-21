//! File import pipeline for Picto.
//!
//! Takes a file path -> hashes it -> detects MIME (by header bytes) -> extracts metadata
//! -> generates thumbnail (SIMD-accelerated) -> creates SQLite record -> writes to blob store.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::blob_store::BlobStore;
use crate::media_processing;
use crate::sqlite::SqliteDatabase;
use crate::tags::normalize as tags;
use super::db as sqlite_import;

#[derive(thiserror::Error, Debug)]
pub enum ImportError {
    #[error("Database error: {0}")]
    Db(String),
    #[error("Blob storage error: {0}")]
    Blob(#[from] crate::blob_store::BlobError),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("File processing error: {0}")]
    FileProcessing(#[from] media_processing::FileError),
    #[error("File already imported: {0}")]
    AlreadyImported(String),
    #[error("Zero-size file: {0}")]
    ZeroSizeFile(String),
    #[error("Unsupported file type: {0}")]
    UnsupportedFile(String),
}

pub type ImportResult<T> = Result<T, ImportError>;

/// Result of a successful file import.
#[derive(Debug, Clone)]
pub struct ImportedFile {
    pub hex_hash: String,
    pub mime: String,
    pub size: u64,
    pub has_thumbnail: bool,
    pub tags_applied: Vec<String>,
}

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
    /// Initial status for imported files (0=inbox, 1=active). Defaults to 0 (inbox).
    pub initial_status: i64,
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
            initial_status: 0,
        }
    }
}

pub struct ImportPipeline<'a> {
    db: &'a SqliteDatabase,
    blob_store: &'a BlobStore,
}

impl<'a> ImportPipeline<'a> {
    pub fn new(db: &'a SqliteDatabase, blob_store: &'a BlobStore) -> Self {
        Self { db, blob_store }
    }

    fn cleanup_partial_blob_write(&self, hex_hash: &str, mime_string: &str) {
        let _ = mime_string;
        if let Err(err) = self.blob_store.delete(hex_hash) {
            warn!(hash = %hex_hash, error = %err, "Failed to clean up partial blob writes");
        }
    }

    /// Import a single file from disk.
    pub async fn import_file(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> ImportResult<ImportedFile> {
        let file_data = tokio::fs::read(path).await?;
        if file_data.is_empty() {
            return Err(ImportError::ZeroSizeFile(path.display().to_string()));
        }
        let file_size = file_data.len() as u64;

        let hash = media_processing::get_hash_from_bytes(&file_data);
        let hex_hash = hex::encode(&hash);

        info!(hash = %hex_hash, path = %path.display(), "Starting file import");

        if self
            .db
            .file_exists(&hex_hash)
            .await
            .map_err(ImportError::Db)?
        {
            return Err(ImportError::AlreadyImported(hex_hash));
        }

        let file_info = match media_processing::get_file_info(path, None) {
            Ok(info) => {
                info!(hash = %hex_hash, mime = %info.mime.mime_string(), "Detected MIME type");
                info
            }
            Err(e) => {
                warn!(hash = %hex_hash, path = %path.display(), error = %e, "MIME detection / file info failed");
                return Err(ImportError::FileProcessing(e));
            }
        };
        let mime_string = file_info.mime.mime_string().to_string();

        if media_processing::is_image(file_info.mime) {
            if let Ok(true) = media_processing::is_decompression_bomb(path) {
                warn!(hash = %hex_hash, "Skipping decompression bomb");
                return Err(ImportError::UnsupportedFile(
                    "Image has extreme dimensions (decompression bomb)".to_string(),
                ));
            }
        }

        let thumbnail_result = media_processing::generate_thumbnail_bytes(
            path,
            options.thumbnail_dimensions,
            file_info.mime,
            file_info.duration_ms,
            file_info.num_frames,
            35, // percentage_in: 35% into file for video/animation
        )
        .ok();

        let mut colors_lab: Vec<(String, f32, f32, f32)> = Vec::new();
        let mut dominant_color_hex: Option<String> = None;
        if media_processing::is_image(file_info.mime) {
            if let Ok(img) = image::load_from_memory(&file_data) {
                let colors = media_processing::colors::extract_dominant_colors(&img, 8);
                if !colors.is_empty() {
                    dominant_color_hex = Some(colors[0].hex.clone());
                    colors_lab = colors
                        .iter()
                        .map(|c| (c.hex.clone(), c.l as f32, c.a as f32, c.b as f32))
                        .collect();
                }
            }
        }

        let name = options.name.clone().or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        });

        let notes_json = options
            .notes
            .as_ref()
            .map(|n| serde_json::to_string(n).unwrap_or_default());

        let mut tag_tuples = Vec::new();
        let mut tags_applied = Vec::new();
        for (ns, st) in &options.tags {
            let full_tag = tags::combine_tag(ns, st);
            if let Some((ns, st)) = tags::parse_tag(&full_tag) {
                tags_applied.push(tags::combine_tag(&ns, &st));
                tag_tuples.push((ns, st));
            }
        }

        // Write blob before DB record so failures don't leave orphan rows.
        let blob_ext = crate::blob_store::mime_to_extension(&mime_string);
        self.blob_store
            .write_original(&hex_hash, &file_data, Some(blob_ext))?;

        if let Some((ref thumb_bytes, ref thumb_ext)) = thumbnail_result {
            if let Err(err) = self
                .blob_store
                .write_thumbnail(&hex_hash, thumb_bytes, thumb_ext)
            {
                self.cleanup_partial_blob_write(&hex_hash, &mime_string);
                return Err(ImportError::Blob(err));
            }
        }

        let import_opts = sqlite_import::ImportOptions {
            hash: hex_hash.clone(),
            name,
            size: file_size as i64,
            mime: mime_string.clone(),
            width: file_info.width.map(|w| w as i64),
            height: file_info.height.map(|h| h as i64),
            duration_ms: file_info.duration_ms.map(|d| d as i64),
            num_frames: file_info.num_frames.map(|n| n as i64),
            has_audio: file_info.has_audio,
            status: options.initial_status,
            notes: notes_json,
            source_urls: if options.source_urls.is_empty() {
                None
            } else {
                Some(options.source_urls.clone())
            },
            created_at: options.created_at.clone(),
            dominant_color_hex,
            dominant_palette_blob: None,
            tags: tag_tuples,
            tag_source: "local".to_string(),
            colors: colors_lab,
        };

        if let Err(err) = self.db.import_file(import_opts).await {
            self.cleanup_partial_blob_write(&hex_hash, &mime_string);
            return Err(ImportError::Db(err));
        }

        // Compute phash from thumbnail (faster than full image) for duplicate detection
        if media_processing::is_image(file_info.mime) {
            let phash_data = thumbnail_result
                .as_ref()
                .map(|(b, _)| b.as_slice())
                .unwrap_or(&file_data);
            match crate::duplicates::phash::compute_phash_base64(phash_data) {
                Ok(phash_b64) => {
                    if let Err(e) = self.db.set_phash(&hex_hash, &phash_b64).await {
                        warn!(hash = %hex_hash, error = %e, "Failed to store phash (non-fatal)");
                    }
                }
                Err(e) => {
                    warn!(hash = %hex_hash, error = %e, "Failed to compute phash (non-fatal)");
                }
            }
        }

        info!(
            hash = %hex_hash,
            mime = %mime_string,
            size = file_size,
            tags = tags_applied.len(),
            thumbnail = thumbnail_result.is_some(),
            "File imported successfully"
        );

        Ok(ImportedFile {
            hex_hash,
            mime: mime_string,
            size: file_size,
            has_thumbnail: thumbnail_result.is_some(),
            tags_applied,
        })
    }

    /// Import multiple files from a list of paths.
    pub async fn import_files(
        &self,
        paths: &[PathBuf],
        options: &ImportOptions,
    ) -> Vec<Result<ImportedFile, ImportError>> {
        let mut results = Vec::new();
        for path in paths {
            results.push(self.import_file(path, options).await);
        }
        results
    }

    /// Export a file from the blob store to a destination path.
    pub async fn export_file(&self, hex_hash: &str, dest: &Path) -> ImportResult<()> {
        let record = self
            .db
            .get_file_by_hash(hex_hash)
            .await
            .map_err(ImportError::Db)?
            .ok_or_else(|| ImportError::Db(format!("File not found in database: {hex_hash}")))?;
        let ext = crate::blob_store::mime_to_extension(&record.mime);
        let data = self.blob_store.read_original(hex_hash, Some(ext))?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(dest, &data)?;
        Ok(())
    }
}
