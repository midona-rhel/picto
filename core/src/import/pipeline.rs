//! File import pipeline for Picto.
//!
//! Takes a file path -> hashes it -> detects MIME (by header bytes) -> extracts metadata
//! -> generates thumbnail (SIMD-accelerated) -> creates SQLite record -> writes to blob store.

use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use super::db as sqlite_import;
use crate::blob_store::BlobStore;
use crate::media_processing;
use crate::sqlite::SqliteDatabase;
use crate::tags::normalize as tags;

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
            initial_status: 0,
            skip_thumbnail: false,
        }
    }
}

/// A file that has been hashed, thumbnailed, and written to the blob store
/// but NOT yet inserted into the database. Use with batch commit.
pub struct PreparedFile {
    pub db_opts: super::db::ImportOptions,
    pub hex_hash: String,
    pub has_thumbnail: bool,
}

/// Work that can be processed in the background after the fast import path.
pub struct DeferredImportWork {
    pub hex_hash: String,
    pub file_data: Vec<u8>,
    pub thumbnail_data: Option<Vec<u8>>,
    pub mime: crate::constants::MimeType,
    pub mime_string: String,
    pub needs_thumbnail: bool,
}

pub struct ImportPipeline<'a> {
    db: &'a SqliteDatabase,
    blob_store: &'a BlobStore,
}

impl<'a> ImportPipeline<'a> {
    pub fn new(db: &'a SqliteDatabase, blob_store: &'a BlobStore) -> Self {
        Self { db, blob_store }
    }

    /// Prepare a file for import: hash, MIME detect, thumbnail, blob write — but NO database insert.
    /// Use with `commit_prepared_batch` to insert everything in one transaction.
    pub async fn prepare_file(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> ImportResult<PreparedFile> {
        let file_data = tokio::fs::read(path).await?;
        if file_data.is_empty() {
            return Err(ImportError::ZeroSizeFile(path.display().to_string()));
        }
        let file_size = file_data.len() as u64;

        let hash = media_processing::get_hash_from_bytes(&file_data);
        let hex_hash = hex::encode(&hash);

        if self
            .db
            .file_exists(&hex_hash)
            .await
            .map_err(ImportError::Db)?
        {
            return Err(ImportError::AlreadyImported(hex_hash));
        }

        let file_info = media_processing::get_file_info(path, None).await?;
        let mime_string = file_info.mime.mime_string().to_string();

        if media_processing::is_image(file_info.mime) {
            if let Ok(true) = media_processing::is_decompression_bomb(path) {
                return Err(ImportError::UnsupportedFile(
                    "Decompression bomb".to_string(),
                ));
            }
        }

        let thumbnail_result = if options.skip_thumbnail {
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

        // Write blobs now (idempotent, safe to write before DB)
        let blob_ext = crate::blob_store::mime_to_extension(&mime_string);
        self.blob_store
            .write_original(&hex_hash, &file_data, Some(blob_ext))?;
        if let Some((ref thumb_bytes, ref thumb_ext)) = thumbnail_result {
            let _ = self
                .blob_store
                .write_thumbnail(&hex_hash, thumb_bytes, thumb_ext);
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
        for (ns, st) in &options.tags {
            let full_tag = tags::combine_tag(ns, st);
            if let Some((ns, st)) = tags::parse_tag(&full_tag) {
                tag_tuples.push((ns, st));
            }
        }

        Ok(PreparedFile {
            db_opts: sqlite_import::ImportOptions {
                hash: hex_hash.clone(),
                name,
                size: file_size as i64,
                mime: mime_string,
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
                created_at: options.created_at.clone().or_else(|| {
                    std::fs::metadata(path).ok().and_then(|meta| {
                        let ts = meta.created().or_else(|_| meta.modified()).ok()?;
                        let dt: chrono::DateTime<chrono::Utc> = ts.into();
                        Some(dt.to_rfc3339())
                    })
                }),
                dominant_color_hex: None,
                dominant_palette_blob: None,
                tags: tag_tuples,
                tag_source: "local".to_string(),
                colors: Vec::new(),
            },
            hex_hash,
            has_thumbnail: thumbnail_result.is_some(),
        })
    }

    fn cleanup_partial_blob_write(&self, hex_hash: &str, mime_string: &str) {
        let _ = mime_string;
        if let Err(err) = self.blob_store.delete(hex_hash) {
            warn!(hash = %hex_hash, error = %err, "Failed to clean up partial blob writes");
        }
    }

    /// Import a single file from disk.
    ///
    /// Returns the imported file metadata and optional deferred work that can
    /// be processed in the background (dominant colors, phash).
    pub async fn import_file(
        &self,
        path: &Path,
        options: &ImportOptions,
    ) -> ImportResult<(ImportedFile, Option<DeferredImportWork>)> {
        let t0 = std::time::Instant::now();

        let file_data = tokio::fs::read(path).await?;
        if file_data.is_empty() {
            return Err(ImportError::ZeroSizeFile(path.display().to_string()));
        }
        let file_size = file_data.len() as u64;

        let t_read = t0.elapsed();

        let hash = media_processing::get_hash_from_bytes(&file_data);
        let hex_hash = hex::encode(&hash);
        let t_hash = t0.elapsed();

        info!(hash = %hex_hash, path = %path.display(), "Starting file import");

        if self
            .db
            .file_exists(&hex_hash)
            .await
            .map_err(ImportError::Db)?
        {
            return Err(ImportError::AlreadyImported(hex_hash));
        }

        let file_info = match media_processing::get_file_info(path, None).await {
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
        let t_info = t0.elapsed();

        if media_processing::is_image(file_info.mime) {
            if let Ok(true) = media_processing::is_decompression_bomb(path) {
                warn!(hash = %hex_hash, "Skipping decompression bomb");
                return Err(ImportError::UnsupportedFile(
                    "Image has extreme dimensions (decompression bomb)".to_string(),
                ));
            }
        }

        let thumbnail_result = if options.skip_thumbnail {
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
        let t_thumb = t0.elapsed();

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
        let t_blob = t0.elapsed();

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
            created_at: options.created_at.clone().or_else(|| {
                std::fs::metadata(path).ok().and_then(|meta| {
                    let ts = meta.created().or_else(|_| meta.modified()).ok()?;
                    let dt: chrono::DateTime<chrono::Utc> = ts.into();
                    Some(dt.to_rfc3339())
                })
            }),
            dominant_color_hex: None,
            dominant_palette_blob: None,
            tags: tag_tuples,
            tag_source: "local".to_string(),
            colors: Vec::new(),
        };

        if let Err(err) = self.db.import_file(import_opts).await {
            self.cleanup_partial_blob_write(&hex_hash, &mime_string);
            return Err(ImportError::Db(err));
        }
        let t_db = t0.elapsed();

        let is_image = media_processing::is_image(file_info.mime);
        let deferred = if is_image {
            Some(DeferredImportWork {
                hex_hash: hex_hash.clone(),
                file_data,
                thumbnail_data: thumbnail_result.as_ref().map(|(b, _)| b.clone()),
                mime: file_info.mime,
                mime_string: mime_string.clone(),
                needs_thumbnail: options.skip_thumbnail,
            })
        } else {
            None
        };

        debug!(
            hash = %hex_hash,
            size = file_size,
            read_ms = t_read.as_millis() as u64,
            hash_ms = (t_hash - t_read).as_millis() as u64,
            info_ms = (t_info - t_hash).as_millis() as u64,
            thumb_ms = (t_thumb - t_info).as_millis() as u64,
            blob_ms = (t_blob - t_thumb).as_millis() as u64,
            db_ms = (t_db - t_blob).as_millis() as u64,
            total_ms = t_db.as_millis() as u64,
            skip_thumbnail = options.skip_thumbnail,
            "Import pipeline timing"
        );

        info!(
            hash = %hex_hash,
            mime = %mime_string,
            size = file_size,
            tags = tags_applied.len(),
            thumbnail = thumbnail_result.is_some(),
            elapsed_ms = t_db.as_millis() as u64,
            "File imported successfully"
        );

        Ok((
            ImportedFile {
                hex_hash,
                mime: mime_string,
                size: file_size,
                has_thumbnail: thumbnail_result.is_some(),
                tags_applied,
            },
            deferred,
        ))
    }

    /// Process deferred import work (dominant colors, phash).
    /// Safe to call from a background worker.
    pub async fn process_deferred(&self, work: DeferredImportWork) {
        let t0 = std::time::Instant::now();
        let is_image = media_processing::is_image(work.mime);

        // Thumbnail (for files that skipped it in the fast path)
        if work.needs_thumbnail && is_image {
            let ext = crate::blob_store::mime_to_extension(&work.mime_string);
            if let Ok(Some((blob_path, _))) =
                self.blob_store.find_original(&work.hex_hash, Some(ext))
            {
                if let Ok((thumb_bytes, thumb_ext)) = media_processing::generate_thumbnail_bytes(
                    &blob_path,
                    media_processing::DEFAULT_THUMBNAIL_DIMENSIONS,
                    work.mime,
                    None,
                    None,
                    35,
                )
                .await
                {
                    let _ =
                        self.blob_store
                            .write_thumbnail(&work.hex_hash, &thumb_bytes, &thumb_ext);
                }
            }
        }

        // Dominant colors
        if is_image {
            if let Ok(img) = image::load_from_memory(&work.file_data) {
                let colors = media_processing::colors::extract_dominant_colors(&img, 8);
                if !colors.is_empty() {
                    let hex = Some(colors[0].hex.clone());
                    let lab: Vec<(String, f32, f32, f32)> = colors
                        .iter()
                        .map(|c| (c.hex.clone(), c.l as f32, c.a as f32, c.b as f32))
                        .collect();
                    let _ = self.db.set_file_colors(&work.hex_hash, lab, hex).await;
                }
            }
        }

        // Phash
        if is_image {
            let phash_data = work.thumbnail_data.as_deref().unwrap_or(&work.file_data);
            match crate::duplicates::phash::compute_phash_base64(phash_data) {
                Ok(phash_b64) => {
                    let _ = self.db.set_phash(&work.hex_hash, &phash_b64).await;
                }
                Err(e) => {
                    warn!(hash = %work.hex_hash, error = %e, "Deferred phash failed (non-fatal)");
                }
            }
        }

        debug!(
            hash = %work.hex_hash,
            elapsed_ms = t0.elapsed().as_millis() as u64,
            "Deferred import work complete"
        );
    }

    /// Import multiple files from a list of paths.
    pub async fn import_files(
        &self,
        paths: &[PathBuf],
        options: &ImportOptions,
    ) -> Vec<Result<(ImportedFile, Option<DeferredImportWork>), ImportError>> {
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
