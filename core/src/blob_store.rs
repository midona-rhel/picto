//! Loose content-addressed blob storage.
//!
//! Files are stored as plain files in a hash-addressed directory tree:
//! ```text
//! blobs/
//! ├── f/<ab>/<cd>/<fullhash>.<ext>   # originals (e.g. abc123.jpg)
//! └── t/<ab>/<cd>/<fullhash>.jpg     # thumbnails (always JPEG)
//! ```
//!
//! - Two-level hex sharding: `hash[0..2]` / `hash[2..4]`
//! - File extensions derived from MIME type; MIME derived from extension on read
//! - Idempotent writes — if the file already exists, skip
//! - The hash IS the filename, so no CRC32/checksums needed

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(thiserror::Error, Debug)]
pub enum BlobError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid hash (expected 64-char hex): {0}")]
    InvalidHash(String),
    #[error("Missing file extension for hash: {0}")]
    MissingExtension(String),
}

pub type BlobResult<T> = Result<T, BlobError>;

pub fn mime_to_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        "image/svg+xml" => "svg",
        "image/avif" => "avif",
        "image/heif" | "image/heic" => "heif",
        "image/jxl" => "jxl",
        "image/x-icon" => "ico",
        "image/vnd.adobe.photoshop" => "psd",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        "video/x-matroska" => "mkv",
        "video/quicktime" => "mov",
        "video/x-flv" => "flv",
        "video/x-msvideo" => "avi",
        "audio/flac" => "flac",
        "audio/x-wav" | "audio/wav" => "wav",
        "application/pdf" => "pdf",
        "application/epub+zip" => "epub",
        _ => "bin",
    }
}

pub fn extension_to_mime(ext: &str) -> &'static str {
    match ext {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tiff" | "tif" => "image/tiff",
        "svg" => "image/svg+xml",
        "avif" => "image/avif",
        "heif" | "heic" => "image/heif",
        "jxl" => "image/jxl",
        "ico" => "image/x-icon",
        "psd" => "image/vnd.adobe.photoshop",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mov" => "video/quicktime",
        "flv" => "video/x-flv",
        "avi" => "video/x-msvideo",
        "flac" => "audio/flac",
        "wav" => "audio/x-wav",
        "pdf" => "application/pdf",
        "epub" => "application/epub+zip",
        _ => "application/octet-stream",
    }
}

/// Manages reading and writing content-addressed blobs.
pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    /// Open or create a blob store at `<library_root>/blobs/`.
    pub fn open(library_root: &Path) -> BlobResult<Self> {
        let root = library_root.join("blobs");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    /// Write an original file with extension. Skips if already exists (idempotent).
    pub fn write_original(&self, hex_hash: &str, data: &[u8], ext: Option<&str>) -> BlobResult<()> {
        // Check new path (with extension) first
        let path = self.original_path_with_ext(hex_hash, ext)?;
        if path.exists() {
            return Ok(());
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)?;
        Ok(())
    }

    /// Read an original file's bytes.
    pub fn read_original(&self, hex_hash: &str, ext: Option<&str>) -> BlobResult<Vec<u8>> {
        let path = self.original_path_with_ext(hex_hash, ext)?;
        Ok(fs::read(&path)?)
    }

    /// Find the original file on disk. Returns (path, extension) if found.
    /// Strict mode requires an extension hint and performs no directory scans.
    pub fn find_original(
        &self,
        hex_hash: &str,
        ext_hint: Option<&str>,
    ) -> BlobResult<Option<(PathBuf, Option<String>)>> {
        let ext = match ext_hint {
            Some(e) if !e.is_empty() => e,
            _ => return Err(BlobError::MissingExtension(hex_hash.to_string())),
        };
        let path = self.original_path_with_ext(hex_hash, Some(ext))?;
        if path.exists() {
            return Ok(Some((path, Some(ext.to_string()))));
        }
        Ok(None)
    }

    /// Write a thumbnail with the given extension (e.g. `"jpg"` or `"png"`).
    /// Skips if a thumbnail already exists (any extension).
    pub fn write_thumbnail(&self, hex_hash: &str, data: &[u8], ext: &str) -> BlobResult<()> {
        // Skip if any thumbnail variant already exists.
        if self.find_thumbnail_path(hex_hash)?.is_some() {
            return Ok(());
        }
        let path = self.thumbnail_path_with_ext(hex_hash, ext)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, data)?;
        Ok(())
    }

    /// Read a thumbnail, returning `Ok(None)` if missing.
    pub fn read_thumbnail(&self, hex_hash: &str) -> BlobResult<Option<Vec<u8>>> {
        if let Some(path) = self.find_thumbnail_path(hex_hash)? {
            return Ok(Some(fs::read(&path)?));
        }
        Ok(None)
    }

    /// Delete all thumbnail variants for a hash (both `.jpg` and `.png`).
    pub fn delete_thumbnail(&self, hex_hash: &str) -> BlobResult<()> {
        let _ = fs::remove_file(self.thumbnail_path_with_ext(hex_hash, "jpg")?);
        let _ = fs::remove_file(self.thumbnail_path_with_ext(hex_hash, "png")?);
        Ok(())
    }

    /// Remove all files and thumbnails, then recreate empty directories.
    pub fn wipe(&self) -> BlobResult<()> {
        let f_dir = self.root.join("f");
        let t_dir = self.root.join("t");
        if f_dir.exists() {
            fs::remove_dir_all(&f_dir)?;
        }
        if t_dir.exists() {
            fs::remove_dir_all(&t_dir)?;
        }
        fs::create_dir_all(&f_dir)?;
        fs::create_dir_all(&t_dir)?;
        Ok(())
    }

    /// Delete both original and thumbnail for a hash.
    pub fn delete(&self, hex_hash: &str) -> BlobResult<()> {
        // Delete originals matching `<hash>.<ext>` in shard dir.
        let (ab, cd) = shard_prefix(hex_hash)?;
        let orig_dir = self.root.join("f").join(ab).join(cd);
        if orig_dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&orig_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with(&format!("{}.", hex_hash)) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }
        self.delete_thumbnail(hex_hash)?;
        Ok(())
    }

    /// Path to original with extension: `blobs/f/<ab>/<cd>/<hash>.<ext>`
    pub fn original_path_with_ext(&self, hex_hash: &str, ext: Option<&str>) -> BlobResult<PathBuf> {
        let (ab, cd) = shard_prefix(hex_hash)?;
        let e = match ext {
            Some(e) if !e.is_empty() => e,
            _ => return Err(BlobError::MissingExtension(hex_hash.to_string())),
        };
        let filename = format!("{}.{}", hex_hash, e);
        Ok(self.root.join("f").join(ab).join(cd).join(filename))
    }

    /// Path to a thumbnail with a specific extension: `blobs/t/<ab>/<cd>/<hash>.<ext>`
    pub fn thumbnail_path_with_ext(&self, hex_hash: &str, ext: &str) -> BlobResult<PathBuf> {
        let (ab, cd) = shard_prefix(hex_hash)?;
        Ok(self
            .root
            .join("t")
            .join(ab)
            .join(cd)
            .join(format!("{}.{}", hex_hash, ext)))
    }

    /// Legacy path to the thumbnail (`.jpg`): `blobs/t/<ab>/<cd>/<hash>.jpg`
    pub fn thumbnail_path(&self, hex_hash: &str) -> BlobResult<PathBuf> {
        self.thumbnail_path_with_ext(hex_hash, "jpg")
    }

    /// Find thumbnail path, checking `.jpg` first then `.png` for backwards
    /// compatibility with existing libraries.
    pub fn find_thumbnail_path(&self, hex_hash: &str) -> BlobResult<Option<PathBuf>> {
        let jpg = self.thumbnail_path_with_ext(hex_hash, "jpg")?;
        if jpg.exists() {
            return Ok(Some(jpg));
        }
        let png = self.thumbnail_path_with_ext(hex_hash, "png")?;
        if png.exists() {
            return Ok(Some(png));
        }
        Ok(None)
    }
}

/// Extract two-level shard prefix from a hex hash: `("ab", "cd")` from `"abcd..."`.
///
/// Validates that the hash is exactly 64 lowercase hex characters to prevent
/// path traversal attacks (e.g., a hash containing `../`).
fn shard_prefix(hex_hash: &str) -> BlobResult<(&str, &str)> {
    if hex_hash.len() != 64 || !hex_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(BlobError::InvalidHash(hex_hash.to_string()));
    }
    Ok((&hex_hash[0..2], &hex_hash[2..4]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_hash() -> String {
        "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string()
    }

    #[test]
    fn test_write_and_read_original_with_ext() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        store
            .write_original(&hash, b"hello world", Some("jpg"))
            .unwrap();
        let data = store.read_original(&hash, Some("jpg")).unwrap();
        assert_eq!(data, b"hello world");

        // Verify file has .jpg extension
        let path = store.original_path_with_ext(&hash, Some("jpg")).unwrap();
        assert!(path.to_string_lossy().ends_with(".jpg"));
        assert!(path.exists());
    }

    #[test]
    fn test_write_idempotent() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        store.write_original(&hash, b"first", Some("png")).unwrap();
        store.write_original(&hash, b"second", Some("png")).unwrap();
        let data = store.read_original(&hash, Some("png")).unwrap();
        assert_eq!(data, b"first");
    }

    #[test]
    fn test_find_original_with_ext() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        store.write_original(&hash, b"data", Some("png")).unwrap();
        let result = store.find_original(&hash, Some("png")).unwrap();
        assert!(result.is_some());
        let (_, ext) = result.unwrap();
        assert_eq!(ext, Some("png".to_string()));
    }

    #[test]
    fn test_thumbnail_with_jpg_extension() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        store.write_thumbnail(&hash, b"thumb bytes", "jpg").unwrap();
        let path = store.thumbnail_path(&hash).unwrap();
        assert!(path.to_string_lossy().ends_with(".jpg"));
        assert!(path.exists());

        let data = store.read_thumbnail(&hash).unwrap();
        assert_eq!(data, Some(b"thumb bytes".to_vec()));
    }

    #[test]
    fn test_missing_thumbnail() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        let data = store.read_thumbnail(&hash).unwrap();
        assert_eq!(data, None);
    }

    #[test]
    fn test_delete_with_extension() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        store.write_original(&hash, b"data", Some("jpg")).unwrap();
        store.write_thumbnail(&hash, b"thumb", "jpg").unwrap();
        assert!(store
            .original_path_with_ext(&hash, Some("jpg"))
            .unwrap()
            .exists());
        assert!(store.thumbnail_path(&hash).unwrap().exists());

        store.delete(&hash).unwrap();
        assert!(!store
            .original_path_with_ext(&hash, Some("jpg"))
            .unwrap()
            .exists());
        assert_eq!(store.read_thumbnail(&hash).unwrap(), None);
    }

    #[test]
    fn test_shard_paths() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        let orig = store.original_path_with_ext(&hash, Some("jpg")).unwrap();
        assert!(orig.to_string_lossy().contains("/f/ab/cd/"));
        assert!(orig.to_string_lossy().ends_with(".jpg"));

        let thumb = store.thumbnail_path(&hash).unwrap();
        assert!(thumb.to_string_lossy().contains("/t/ab/cd/"));
        assert!(thumb.to_string_lossy().ends_with(".jpg"));
    }

    #[test]
    fn test_invalid_hash() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        assert!(store.original_path_with_ext("ab", Some("jpg")).is_err()); // too short
    }

    #[test]
    fn test_mime_to_extension() {
        assert_eq!(mime_to_extension("image/jpeg"), "jpg");
        assert_eq!(mime_to_extension("image/png"), "png");
        assert_eq!(mime_to_extension("video/mp4"), "mp4");
        assert_eq!(mime_to_extension("application/pdf"), "pdf");
        assert_eq!(mime_to_extension("unknown/type"), "bin");
    }

    #[test]
    fn test_extension_to_mime() {
        assert_eq!(extension_to_mime("jpg"), "image/jpeg");
        assert_eq!(extension_to_mime("jpeg"), "image/jpeg");
        assert_eq!(extension_to_mime("png"), "image/png");
        assert_eq!(extension_to_mime("mp4"), "video/mp4");
        assert_eq!(extension_to_mime("pdf"), "application/pdf");
        assert_eq!(extension_to_mime("xyz"), "application/octet-stream");
    }
}
