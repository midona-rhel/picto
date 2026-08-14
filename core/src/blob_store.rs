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
//! - Writes are atomic: staged in `blobs/tmp/`, then renamed onto the final
//!   path. A file at a content-addressed path is therefore always complete —
//!   which is what makes the existence-skip and hash-as-filename safe.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanBlobCandidate {
    pub hash: String,
    pub file_count: u64,
    pub bytes: u64,
}

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
    /// This instance's staging directory, `blobs/tmp/<pid>-<nonce>/`.
    /// Unique per open, so a re-open never disturbs an in-flight writer
    /// still holding the previous instance.
    staging: PathBuf,
    hash_locks: Arc<Mutex<HashMap<String, Weak<Semaphore>>>>,
}

pub(crate) struct BlobHashLease {
    _permit: OwnedSemaphorePermit,
}

impl Drop for BlobStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging);
    }
}

impl BlobStore {
    /// Open or create a blob store at `<library_root>/blobs/`.
    ///
    /// Sweeps staging leftovers from other (dead) processes. Same-pid entries
    /// are left alone: they may belong to a still-live writer from an earlier
    /// open of this library in this process.
    pub fn open(library_root: &Path) -> BlobResult<Self> {
        let root = library_root.join("blobs");
        fs::create_dir_all(&root)?;
        let tmp_root = root.join("tmp");
        fs::create_dir_all(&tmp_root)?;

        let own_prefix = format!("{}-", std::process::id());
        if let Ok(entries) = fs::read_dir(&tmp_root) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&own_prefix) {
                    continue;
                }
                let path = entry.path();
                let _ = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };
            }
        }

        let staging = tmp_root.join(format!(
            "{}-{:08x}",
            std::process::id(),
            rand::random::<u32>()
        ));
        fs::create_dir_all(&staging)?;
        Ok(Self {
            root,
            staging,
            hash_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn hash_lock(&self, hex_hash: &str) -> Arc<Semaphore> {
        let mut locks = self.hash_locks.lock().unwrap();
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(hex_hash).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Semaphore::new(1));
        locks.insert(hex_hash.to_string(), Arc::downgrade(&lock));
        lock
    }

    /// Serialize blob publication and the following database reference write.
    pub(crate) async fn acquire_hash_lease(&self, hex_hash: &str) -> BlobHashLease {
        let permit = self
            .hash_lock(hex_hash)
            .acquire_owned()
            .await
            .expect("blob hash semaphore remains open");
        BlobHashLease { _permit: permit }
    }

    /// Cleanup must not wait behind an import while holding the DB writer.
    /// Returning `None` leaves the durable job for a later retry.
    pub(crate) fn try_acquire_hash_lease(&self, hex_hash: &str) -> Option<BlobHashLease> {
        let permit = self.hash_lock(hex_hash).try_acquire_owned().ok()?;
        Some(BlobHashLease { _permit: permit })
    }

    /// Write an original file with extension. Skips if already exists (idempotent).
    pub fn write_original(&self, hex_hash: &str, data: &[u8], ext: Option<&str>) -> BlobResult<()> {
        // Check new path (with extension) first
        let path = self.original_path_with_ext(hex_hash, ext)?;
        if path.exists() {
            return Ok(());
        }
        // Originals are irreplaceable — fsync before rename.
        self.write_atomic(&path, data, true, true)
    }

    /// Atomically replace a corrupt original with bytes already verified
    /// against `hex_hash` by the sync boundary.
    pub(crate) fn replace_original(
        &self,
        hex_hash: &str,
        data: &[u8],
        ext: Option<&str>,
    ) -> BlobResult<()> {
        let path = self.original_path_with_ext(hex_hash, ext)?;
        self.write_atomic(&path, data, true, false)
    }

    /// Stage `data` in this instance's staging dir and atomically rename onto
    /// `final_path`, so a partial file can never exist at a content-addressed
    /// path. `durable` additionally fsyncs the file (and its directory) so the
    /// blob survives power loss once this returns. The temp file is cleaned
    /// up on every error path (RAII via `NamedTempFile`).
    fn write_atomic(
        &self,
        final_path: &Path,
        data: &[u8],
        durable: bool,
        existing_is_success: bool,
    ) -> BlobResult<()> {
        let mut file = tempfile::NamedTempFile::new_in(&self.staging)?;
        file.write_all(data)?;
        if durable {
            file.as_file().sync_all()?;
        }

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(err) = file.persist(final_path) {
            // A concurrent writer of the same hash landing first is success:
            // content-addressed, so the bytes are identical.
            if !existing_is_success || !final_path.exists() {
                return Err(err.error.into());
            }
        }
        if durable {
            #[cfg(unix)]
            if let Some(parent) = final_path.parent() {
                if let Ok(dir) = fs::File::open(parent) {
                    let _ = dir.sync_all();
                }
            }
        }
        Ok(())
    }

    /// Enumerate stored originals as `(hash, extension)` pairs.
    pub fn list_originals(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let f_root = self.root.join("f");
        let mut stack = vec![f_root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if let Some((hash, ext)) = name.split_once('.') {
                        if hash.len() == 64 {
                            out.push((hash.to_string(), ext.to_string()));
                        }
                    }
                }
            }
        }
        out
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
        // Thumbnails are regenerable — atomic rename without fsync.
        self.write_atomic(&path, data, false, true)
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
        remove_file_if_exists(self.thumbnail_path_with_ext(hex_hash, "jpg")?)?;
        remove_file_if_exists(self.thumbnail_path_with_ext(hex_hash, "png")?)?;
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

    /// Enumerate unreferenced, old blob hashes without deleting anything.
    /// Physical deletion is owned by the database-backed cleanup contract so
    /// the reference check cannot become stale between enumeration and delete.
    pub fn orphan_candidates(
        &self,
        referenced: &std::collections::HashSet<String>,
        min_age: std::time::Duration,
    ) -> BlobResult<Vec<OrphanBlobCandidate>> {
        let mut candidates: std::collections::HashMap<String, (u64, u64, bool)> =
            std::collections::HashMap::new();
        let now = std::time::SystemTime::now();
        for top in ["f", "t"] {
            let top_dir = self.root.join(top);
            let shard_a = match fs::read_dir(&top_dir) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for ab in shard_a {
                let ab = ab?;
                let shard_b = match fs::read_dir(ab.path()) {
                    Ok(entries) => entries,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error.into()),
                };
                for cd in shard_b {
                    let cd = cd?;
                    let files = match fs::read_dir(cd.path()) {
                        Ok(entries) => entries,
                        Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                        Err(error) => return Err(error.into()),
                    };
                    for file in files {
                        let file = file?;
                        let name = file.file_name();
                        let name_str = name.to_string_lossy();
                        let hash = name_str.split('.').next().unwrap_or("");
                        if hash.len() != 64
                            || !hash.chars().all(|c| c.is_ascii_hexdigit())
                            || referenced.contains(hash)
                        {
                            continue;
                        }
                        let meta = file.metadata()?;
                        if !meta.is_file() {
                            continue;
                        }
                        let mtime = meta.modified()?;
                        let old_enough = now
                            .duration_since(mtime)
                            .map(|age| age >= min_age)
                            .unwrap_or(false);
                        let entry = candidates.entry(hash.to_string()).or_insert((0, 0, true));
                        entry.0 += 1;
                        entry.1 += meta.len();
                        entry.2 &= old_enough;
                    }
                }
            }
        }

        Ok(candidates
            .into_iter()
            .filter_map(|(hash, (file_count, bytes, all_old))| {
                all_old.then_some(OrphanBlobCandidate {
                    hash,
                    file_count,
                    bytes,
                })
            })
            .collect())
    }

    /// Delete both original and thumbnail for a hash.
    pub(crate) fn delete(&self, hex_hash: &str) -> BlobResult<()> {
        // Delete originals matching `<hash>.<ext>` in shard dir.
        let (ab, cd) = shard_prefix(hex_hash)?;
        let orig_dir = self.root.join("f").join(ab).join(cd);
        match fs::read_dir(&orig_dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy();
                    if name_str.starts_with(&format!("{}.", hex_hash)) {
                        remove_file_if_exists(entry.path())?;
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
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

    /// Compute disk usage for originals (`blobs/f/`) and thumbnails (`blobs/t/`).
    pub fn disk_usage(&self) -> (u64, u64) {
        let originals = dir_size(&self.root.join("f"));
        let thumbnails = dir_size(&self.root.join("t"));
        (originals, thumbnails)
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

/// Recursively sum file sizes under a directory.
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_dir() {
                total += dir_size(&entry.path());
            } else if ft.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
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

fn remove_file_if_exists(path: PathBuf) -> BlobResult<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_hash() -> String {
        "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string()
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
    fn replace_original_atomically_replaces_corrupt_bytes() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        store
            .write_original(&hash, b"corrupt", Some("png"))
            .unwrap();
        store
            .replace_original(&hash, b"verified", Some("png"))
            .unwrap();

        assert_eq!(
            store.read_original(&hash, Some("png")).unwrap(),
            b"verified"
        );
    }

    #[test]
    fn test_invalid_hash() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        assert!(store.original_path_with_ext("ab", Some("jpg")).is_err()); // too short
    }

    #[test]
    fn test_no_staging_leftovers_after_write() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();

        store.write_original(&hash, b"data", Some("png")).unwrap();
        store.write_thumbnail(&hash, b"thumb", "jpg").unwrap();

        assert_eq!(fs::read_dir(&store.staging).unwrap().count(), 0);
    }

    #[test]
    fn test_open_clears_dead_process_staging_only() {
        let dir = TempDir::new().unwrap();
        let tmp = dir.path().join("blobs").join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        // Crash leftover from another (dead) process.
        let dead = tmp.join("999999999-deadbeef");
        fs::create_dir_all(&dead).unwrap();
        fs::write(dead.join("partial.png"), b"partial").unwrap();
        // In-flight staging dir from a still-live store in this process.
        let live = tmp.join(format!("{}-cafebabe", std::process::id()));
        fs::create_dir_all(&live).unwrap();
        fs::write(live.join("inflight.png"), b"inflight").unwrap();

        let _store = BlobStore::open(dir.path()).unwrap();
        assert!(!dead.exists(), "dead-process leftover must be swept");
        assert!(
            live.join("inflight.png").exists(),
            "same-process in-flight staging must survive a re-open"
        );
    }

    #[test]
    fn test_drop_removes_own_staging_dir() {
        let dir = TempDir::new().unwrap();
        let staging = {
            let store = BlobStore::open(dir.path()).unwrap();
            store.staging.clone()
        };
        assert!(!staging.exists());
    }

    #[test]
    fn delete_is_idempotent_when_blob_is_missing() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        store.delete(&test_hash()).unwrap();
    }

    #[test]
    fn delete_reports_filesystem_failures() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();
        let path = store.original_path_with_ext(&hash, Some("png")).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::create_dir(path).unwrap();

        let error = store
            .delete(&hash)
            .expect_err("directory cannot be deleted as a file");
        assert!(matches!(error, BlobError::Io(_)));
    }

    #[test]
    fn orphan_enumeration_reports_filesystem_failures() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let shard = dir.path().join("blobs").join("f").join("aa");
        fs::create_dir_all(shard.parent().unwrap()).unwrap();
        fs::write(&shard, b"not a directory").unwrap();

        let error = store
            .orphan_candidates(&std::collections::HashSet::new(), std::time::Duration::ZERO)
            .expect_err("invalid shard layout must be reported");
        assert!(matches!(error, BlobError::Io(_)));
    }
}
