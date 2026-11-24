//! The blob store: the folder `blobs` of a library, which holds the
//! library's original files and the pictures that are made from them.
//!
//! Every file is stored as a plain file, under the SHA-256 hash of the
//! original's content, written as 64 hexadecimal characters:
//! ```text
//! blobs/
//! ├── f/<ab>/<cd>/<hash>.<ext>              an original, for example <hash>.mp4
//! └── t/<ab>/<cd>/<hash>.jpg or <hash>.png  the original's thumbnail
//!     t/<ab>/<cd>/<hash>.first-frame.jpg    a video's first frame
//! ```
//!
//! - `<ab>` is the first two characters of the hash and `<cd>` is the next
//!   two.
//! - An original's extension is the one that belongs to its MIME type
//!   ([`mime_to_extension`]).
//! - A thumbnail is the `.jpg` when that file exists, and the `.png`
//!   otherwise ([`BlobStore::find_thumbnail_path`]).
//! - The first-frame picture exists only for a video. It is the video's
//!   first frame at the size of a thumbnail. The viewers draw it while the
//!   video loads, because playback starts at that frame, and a video's
//!   thumbnail is taken from a later point in the video.
//! - The thumbnail and the first-frame picture are made from the original
//!   and can be made again at any time. They are deleted with the original
//!   (`BlobStore::delete`).
//! - A write of an original or of a thumbnail does nothing when the file
//!   already exists.
//! - A file is written to a temporary file in `blobs/tmp/` and then renamed
//!   to its final path. A file at its final path is therefore always
//!   complete. This is why a write may skip a file that exists, and why a
//!   file's name can be trusted to be the hash of the original's content.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::io::Read as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[derive(thiserror::Error, Debug)]
pub enum BlobError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid hash (expected 64-char hex): {0}")]
    InvalidHash(String),
    #[error("Missing file extension for hash: {0}")]
    MissingExtension(String),
    #[error("Content hash mismatch: expected {expected}, found {actual}")]
    HashMismatch { expected: String, actual: String },
}

pub type BlobResult<T> = Result<T, BlobError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanBlobCandidate {
    pub hash: String,
    pub file_count: u64,
    pub bytes: u64,
}

pub fn mime_to_extension(mime: &str) -> &'static str {
    if let Some(extension) = crate::media_processing::formats::extension_for_mime(mime) {
        return extension;
    }
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
    if let Some(format) = crate::media_processing::formats::format_for_extension(ext) {
        return format.mime_type;
    }
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

/// Reads and writes the files of one library's `blobs` folder.
pub struct BlobStore {
    root: PathBuf,
    /// The folder in which this store writes its temporary files:
    /// `blobs/tmp/<process id>-<random number>/`. Every `BlobStore` has a
    /// folder of its own. When a library is opened again in the same
    /// process, a write that the earlier `BlobStore` has not finished
    /// therefore keeps its temporary file.
    staging: PathBuf,
}

impl Drop for BlobStore {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.staging);
    }
}

impl BlobStore {
    /// Opens the blob store at `<library_root>/blobs/`, and creates the
    /// folder when it does not exist.
    ///
    /// The temporary folders that other processes left in `blobs/tmp/` are
    /// deleted. A folder whose name starts with this process's id is kept,
    /// because an earlier `BlobStore` of this process may still be writing
    /// in it.
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
        Ok(Self { root, staging })
    }

    /// Writes `data` as the original with the hash `hex_hash` and the
    /// extension `ext`. Does nothing when that file already exists.
    pub fn write_original(&self, hex_hash: &str, data: &[u8], ext: Option<&str>) -> BlobResult<()> {
        let path = self.original_path_with_ext(hex_hash, ext)?;
        if path.exists() {
            return Ok(());
        }
        // An original cannot be made again, so its bytes are flushed to the
        // disk before the file is renamed to its final path.
        self.write_atomic(&path, data, true, true)
    }

    /// Copies the file `source` into the store as the original with the
    /// hash `hex_hash`. The hash of the bytes is computed while they are
    /// copied. When it differs from `hex_hash`, nothing is stored and the
    /// error is `BlobError::HashMismatch`.
    pub fn write_original_from_path(
        &self,
        hex_hash: &str,
        source: &Path,
        ext: Option<&str>,
    ) -> BlobResult<()> {
        let final_path = self.original_path_with_ext(hex_hash, ext)?;
        fs::create_dir_all(&self.staging)?;
        let mut input = fs::File::open(source)?;
        let mut staged = if final_path.exists() {
            None
        } else {
            Some(tempfile::NamedTempFile::new_in(&self.staging)?)
        };
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 1024 * 1024];
        loop {
            let read = input.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            if let Some(file) = staged.as_mut() {
                file.write_all(&buffer[..read])?;
            }
        }
        let actual = hex::encode(hasher.finalize());
        if !hex_hash.eq_ignore_ascii_case(&actual) {
            return Err(BlobError::HashMismatch {
                expected: hex_hash.to_string(),
                actual,
            });
        }
        let Some(file) = staged else {
            return Ok(());
        };
        file.as_file().sync_all()?;
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(error) = file.persist(&final_path) {
            if !final_path.exists() {
                return Err(error.error.into());
            }
        }
        #[cfg(unix)]
        if let Some(parent) = final_path.parent() {
            if let Ok(directory) = fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        Ok(())
    }

    /// Moves the file `source` into the store as the original with the hash
    /// `hex_hash`, by renaming it. The caller must have written `source`
    /// itself, computed `hex_hash` from the bytes it wrote, and flushed the
    /// file to the disk, because this function does not read the file. A
    /// file from anywhere else is stored with `write_original_from_path`,
    /// which checks the hash while it copies.
    pub fn promote_verified_original(
        &self,
        hex_hash: &str,
        source: &Path,
        ext: Option<&str>,
    ) -> BlobResult<PathBuf> {
        let final_path = self.original_path_with_ext(hex_hash, ext)?;
        if final_path.exists() {
            fs::remove_file(source)?;
            return Ok(final_path);
        }
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(error) = fs::rename(source, &final_path) {
            if final_path.exists() {
                fs::remove_file(source)?;
            } else {
                return Err(error.into());
            }
        }
        #[cfg(unix)]
        if let Some(parent) = final_path.parent() {
            if let Ok(directory) = fs::File::open(parent) {
                let _ = directory.sync_all();
            }
        }
        Ok(final_path)
    }

    /// Writes `data` to a temporary file in this store's staging folder and
    /// renames that file to `final_path`, so that a file at `final_path` is
    /// never incomplete. A file that is already at `final_path` is replaced.
    ///
    /// With `durable`, the file and its folder are flushed to the disk, so
    /// that the file survives a loss of power after this function returns.
    /// With `existing_is_success`, a rename that fails counts as success
    /// when a file is at `final_path` afterwards. The temporary file is
    /// deleted when the function returns an error.
    fn write_atomic(
        &self,
        final_path: &Path,
        data: &[u8],
        durable: bool,
        existing_is_success: bool,
    ) -> BlobResult<()> {
        // Another Picto process deletes this staging folder when it opens
        // the same library, so the folder is created again when it is
        // missing.
        fs::create_dir_all(&self.staging)?;
        let mut file = tempfile::NamedTempFile::new_in(&self.staging)?;
        file.write_all(data)?;
        if durable {
            file.as_file().sync_all()?;
        }

        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if let Err(err) = file.persist(final_path) {
            // The rename failed. When a file is at this path all the same,
            // another writer put it there. A file is named by the hash of
            // its original, so that file was made from the same original,
            // and the write counts as done.
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

    /// Returns every original in the store, as its hash and its extension.
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
                if entry_is_dir(&entry) {
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

    /// Returns the hashes of the originals that have a thumbnail, read from
    /// one listing of the folder `t`. A thumbnail counts when its name is
    /// exactly `<hash>.jpg` or `<hash>.png` and it is in the folder
    /// `t/<ab>/<cd>/` of its own hash. `find_thumbnail_path` finds a
    /// thumbnail for every hash in the result. A first-frame picture
    /// (`<hash>.first-frame.jpg`) is not a thumbnail and is not counted.
    /// Symbolic links are left out, because only a look at a link's target
    /// tells whether the link leads to a file. `find_thumbnail_path` takes
    /// that look.
    pub fn list_thumbnails(&self) -> HashSet<String> {
        let mut out = HashSet::new();
        let shards = |dir: &Path| -> Vec<(String, PathBuf)> {
            let Ok(entries) = fs::read_dir(dir) else {
                return Vec::new();
            };
            entries
                .flatten()
                .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
                .filter_map(|entry| Some((entry.file_name().into_string().ok()?, entry.path())))
                .filter(|(name, _)| name.len() == 2)
                .collect()
        };
        for (ab, outer) in shards(&self.root.join("t")) {
            for (cd, inner) in shards(&outer) {
                let Ok(entries) = fs::read_dir(&inner) else {
                    continue;
                };
                for entry in entries.flatten() {
                    if entry.file_type().map_or(true, |kind| kind.is_symlink()) {
                        continue;
                    }
                    let Ok(name) = entry.file_name().into_string() else {
                        continue;
                    };
                    let Some((hash, "jpg" | "png")) = name.split_once('.') else {
                        continue;
                    };
                    if hash.len() == 64
                        && hash.get(0..2) == Some(ab.as_str())
                        && hash.get(2..4) == Some(cd.as_str())
                    {
                        out.insert(hash.to_owned());
                    }
                }
            }
        }
        out
    }

    /// Returns the bytes of the original with the hash `hex_hash` and the
    /// extension `ext`.
    pub fn read_original(&self, hex_hash: &str, ext: Option<&str>) -> BlobResult<Vec<u8>> {
        let path = self.original_path_with_ext(hex_hash, ext)?;
        Ok(fs::read(&path)?)
    }

    /// Returns the path and the extension of the original with the hash
    /// `hex_hash`, or `None` when the store has no such file. The caller
    /// must give the extension in `ext_hint`, because no folder is searched
    /// for the file. Without an extension the error is
    /// `BlobError::MissingExtension`.
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

    /// Writes `data` as the thumbnail of the original with the hash
    /// `hex_hash`, with the extension `ext` (`"jpg"` or `"png"`). Does
    /// nothing when the original already has a thumbnail with either
    /// extension.
    pub fn write_thumbnail(&self, hex_hash: &str, data: &[u8], ext: &str) -> BlobResult<()> {
        if self.find_thumbnail_path(hex_hash)?.is_some() {
            return Ok(());
        }
        let path = self.thumbnail_path_with_ext(hex_hash, ext)?;
        // A thumbnail can be made again, so it is not flushed to the disk.
        self.write_atomic(&path, data, false, true)
    }

    /// Copies the thumbnail file `source`, which is another library's
    /// thumbnail of the original with the hash `hex_hash`, into this store.
    /// Does nothing when this store already holds a thumbnail for the hash.
    pub fn adopt_thumbnail(&self, hex_hash: &str, source: &Path) -> BlobResult<()> {
        if self.find_thumbnail_path(hex_hash)?.is_some() {
            return Ok(());
        }
        let extension = source
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("jpg");
        self.write_thumbnail(hex_hash, &fs::read(source)?, extension)
    }

    /// Returns the bytes of the thumbnail of the original with the hash
    /// `hex_hash`, or `None` when it has no thumbnail.
    pub fn read_thumbnail(&self, hex_hash: &str) -> BlobResult<Option<Vec<u8>>> {
        if let Some(path) = self.find_thumbnail_path(hex_hash)? {
            return Ok(Some(fs::read(&path)?));
        }
        Ok(None)
    }

    /// Deletes the thumbnail of the original with the hash `hex_hash`, both
    /// the `.jpg` and the `.png`.
    pub fn delete_thumbnail(&self, hex_hash: &str) -> BlobResult<()> {
        remove_file_if_exists(self.thumbnail_path_with_ext(hex_hash, "jpg")?)?;
        remove_file_if_exists(self.thumbnail_path_with_ext(hex_hash, "png")?)?;
        Ok(())
    }

    /// Writes `jpeg` as the first-frame picture of the video with the hash
    /// `hex_hash`. A first-frame picture that already exists is replaced,
    /// so that the picture can be made again.
    pub fn write_first_frame(&self, hex_hash: &str, jpeg: &[u8]) -> BlobResult<()> {
        let path = self.first_frame_path(hex_hash)?;
        // The picture can be made again, so it is not flushed to the disk.
        self.write_atomic(&path, jpeg, false, true)
    }

    /// Returns the path of the first-frame picture of the video with the
    /// hash `hex_hash`, or `None` when that file does not exist.
    pub fn find_first_frame_path(&self, hex_hash: &str) -> BlobResult<Option<PathBuf>> {
        let path = self.first_frame_path(hex_hash)?;
        Ok(path.exists().then_some(path))
    }

    /// Deletes every original, every thumbnail and every first-frame
    /// picture, and leaves the folders `f` and `t` empty.
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

    /// Returns the hashes in the store that are not in `referenced` and
    /// whose files were all last changed at least `min_age` ago, each with
    /// the number of its files and their size together. The files counted
    /// are the original, the thumbnail and the first-frame picture. Nothing
    /// is deleted here. The library deletes a file only inside the database
    /// write that has checked that no item uses the file
    /// (`Library::clean_pending_blobs`).
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

    /// Deletes every file the store holds for the hash `hex_hash`: the
    /// original, the thumbnail and the first-frame picture.
    pub(crate) fn delete(&self, hex_hash: &str) -> BlobResult<()> {
        // The original's extension is not known here, so every file named
        // `<hash>.<extension>` in the hash's folder under `f` is deleted.
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
        remove_file_if_exists(self.first_frame_path(hex_hash)?)?;
        Ok(())
    }

    /// Returns the path of the original with the hash `hex_hash` and the
    /// extension `ext`: `blobs/f/<ab>/<cd>/<hash>.<ext>`.
    pub fn original_path_with_ext(&self, hex_hash: &str, ext: Option<&str>) -> BlobResult<PathBuf> {
        let (ab, cd) = shard_prefix(hex_hash)?;
        let e = match ext {
            Some(e) if !e.is_empty() => e,
            _ => return Err(BlobError::MissingExtension(hex_hash.to_string())),
        };
        let filename = format!("{}.{}", hex_hash, e);
        Ok(self.root.join("f").join(ab).join(cd).join(filename))
    }

    /// Returns the path of the thumbnail with the extension `ext` of the
    /// original with the hash `hex_hash`: `blobs/t/<ab>/<cd>/<hash>.<ext>`.
    pub fn thumbnail_path_with_ext(&self, hex_hash: &str, ext: &str) -> BlobResult<PathBuf> {
        let (ab, cd) = shard_prefix(hex_hash)?;
        Ok(self
            .root
            .join("t")
            .join(ab)
            .join(cd)
            .join(format!("{}.{}", hex_hash, ext)))
    }

    /// Returns the path of the first-frame picture of the video with the
    /// hash `hex_hash`: `blobs/t/<ab>/<cd>/<hash>.first-frame.jpg`. It is
    /// in the same folder as the video's thumbnail.
    pub fn first_frame_path(&self, hex_hash: &str) -> BlobResult<PathBuf> {
        let (ab, cd) = shard_prefix(hex_hash)?;
        Ok(self
            .root
            .join("t")
            .join(ab)
            .join(cd)
            .join(format!("{hex_hash}.first-frame.jpg")))
    }

    /// Returns two sizes in bytes: of the folder `blobs/f/`, which holds
    /// the originals, and of the folder `blobs/t/`, which holds the
    /// thumbnails and the first-frame pictures.
    pub fn disk_usage(&self) -> (u64, u64) {
        let originals = dir_size(&self.root.join("f"));
        let thumbnails = dir_size(&self.root.join("t"));
        (originals, thumbnails)
    }

    /// Returns the path of the thumbnail of the original with the hash
    /// `hex_hash`: the `.jpg` when it exists, otherwise the `.png` when
    /// that exists, otherwise `None`.
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

/// Returns the path at which the library in the folder `library_root` stores
/// the original with the hash `hex_hash` and the extension `ext`:
/// `blobs/f/<ab>/<cd>/<hash>.<ext>`. Nothing is read or created, so this is
/// how a file of a library that is not open is found.
pub fn library_original_path(
    library_root: &Path,
    hex_hash: &str,
    ext: &str,
) -> BlobResult<PathBuf> {
    let (ab, cd) = shard_prefix(hex_hash)?;
    Ok(library_root
        .join("blobs")
        .join("f")
        .join(ab)
        .join(cd)
        .join(format!("{hex_hash}.{ext}")))
}

/// Returns whether the library in the folder `library_root` stores an
/// original with the hash `hex_hash`, with any extension. Nothing is created,
/// so this works for a library that is not open.
pub fn library_has_original(library_root: &Path, hex_hash: &str) -> BlobResult<bool> {
    let (ab, cd) = shard_prefix(hex_hash)?;
    let folder = library_root.join("blobs").join("f").join(ab).join(cd);
    let entries = match fs::read_dir(&folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let prefix = format!("{hex_hash}.");
    for entry in entries {
        if entry?.file_name().to_string_lossy().starts_with(&prefix) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Returns the thumbnail that belongs to `original` when `original` is the
/// file with the hash `hex_hash` in the blob store of a library
/// (`<library>/blobs/f/<ab>/<cd>/<hash>.<ext>`) and that library holds a
/// thumbnail for it (`<library>/blobs/t/<ab>/<cd>/<hash>.jpg` or `.png`).
/// Returns `None` for any other file.
pub fn thumbnail_beside_original(original: &Path, hex_hash: &str) -> Option<PathBuf> {
    let (ab, cd) = shard_prefix(hex_hash).ok()?;
    let name = original.file_name()?.to_str()?;
    if !name.starts_with(&format!("{hex_hash}.")) {
        return None;
    }
    let mut folders = original.ancestors().skip(1);
    let named = |folder: Option<&Path>, name: &str| {
        folder.and_then(Path::file_name).and_then(|value| value.to_str()) == Some(name)
    };
    if !named(folders.next(), cd) || !named(folders.next(), ab) || !named(folders.next(), "f") {
        return None;
    }
    let thumbnails = folders.next()?.join("t").join(ab).join(cd);
    ["jpg", "png"]
        .into_iter()
        .map(|extension| thumbnails.join(format!("{hex_hash}.{extension}")))
        .find(|path| path.is_file())
}

/// Returns whether the directory entry `entry` is a folder. A symbolic link
/// counts as a folder when it leads to one, as with `Path::is_dir`. The kind
/// of an entry is taken from the directory listing, so the file system is
/// asked again only for a symbolic link.
fn entry_is_dir(entry: &fs::DirEntry) -> bool {
    match entry.file_type() {
        Ok(kind) if !kind.is_symlink() => kind.is_dir(),
        _ => entry.path().is_dir(),
    }
}

/// Returns the size in bytes of all files in the folder `path` and in the
/// folders inside it.
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

/// Returns the first two and the next two characters of the hash
/// `hex_hash`, which name the two folders its files are stored in:
/// `("ab", "cd")` for `"abcd..."`.
///
/// The hash must be exactly 64 hexadecimal characters. Any other text is
/// refused with `BlobError::InvalidHash`, so that a text such as `../..`
/// cannot make a path that leads outside the `blobs` folder.
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

    /// Checks the two listings that the recovery pass reads
    /// (`list_originals`, `list_thumbnails`) against a look at each file.
    /// An original in a folder that is reached through a symbolic link is
    /// listed. Every thumbnail that is listed is one that
    /// `find_thumbnail_path` finds. A first-frame picture is not listed as
    /// a thumbnail.
    #[cfg(unix)]
    #[test]
    fn listings_agree_with_looking_at_each_file() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = |n: u8| format!("ab{:02x}{}", n, "0".repeat(60));
        // There are three originals. Two are in their own folders, and the
        // third is in a folder that is reached through a symbolic link.
        store.write_original(&hash(1), b"one", Some("png")).unwrap();
        store.write_original(&hash(2), b"two", Some("jpg")).unwrap();
        let elsewhere = dir.path().join("elsewhere");
        fs::create_dir_all(&elsewhere).unwrap();
        fs::write(elsewhere.join(format!("{}.gif", hash(3))), b"three").unwrap();
        std::os::unix::fs::symlink(&elsewhere, store.root.join("f/ab/03")).unwrap();
        let mut originals = store.list_originals();
        originals.sort();
        assert_eq!(
            originals,
            vec![
                (hash(1), "png".to_string()),
                (hash(2), "jpg".to_string()),
                (hash(3), "gif".to_string()),
            ]
        );
        // There are five thumbnails: a `.jpg`, a `.png`, one in the folder of
        // another hash, one that is a symbolic link to nothing, and one that
        // is a symbolic link to a file. The seventh hash has a first-frame
        // picture and no thumbnail.
        store.write_first_frame(&hash(7), b"f").unwrap();
        store.write_thumbnail(&hash(1), b"t", "jpg").unwrap();
        store.write_thumbnail(&hash(2), b"t", "png").unwrap();
        let wrong = store.root.join("t/ab/01");
        fs::write(wrong.join(format!("{}.jpg", hash(4))), b"t").unwrap();
        let linked = store.thumbnail_path_with_ext(&hash(5), "jpg").unwrap();
        fs::create_dir_all(linked.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(dir.path().join("nothing"), &linked).unwrap();
        let target = dir.path().join("thumbnail.jpg");
        fs::write(&target, b"t").unwrap();
        let alive = store.thumbnail_path_with_ext(&hash(6), "jpg").unwrap();
        fs::create_dir_all(alive.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &alive).unwrap();
        let listed = store.list_thumbnails();
        for n in 1..=7 {
            let found = store.find_thumbnail_path(&hash(n)).unwrap().is_some();
            // Every listed thumbnail is found. A symbolic link is not
            // listed, whether `find_thumbnail_path` finds it or not.
            assert!(!listed.contains(&hash(n)) || found, "{n}");
        }
        assert!(store.find_thumbnail_path(&hash(7)).unwrap().is_none());
        let mut listed = listed.into_iter().collect::<Vec<_>>();
        listed.sort();
        assert_eq!(listed, vec![hash(1), hash(2)]);
    }

    #[test]
    fn test_invalid_hash() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();

        // A hash of two characters is too short.
        assert!(store.original_path_with_ext("ab", Some("jpg")).is_err());
    }

    /// A video's first-frame picture is stored in the folder of the video's
    /// thumbnail, under its own name, so the two do not replace each other.
    /// Writing the first-frame picture again replaces it.
    #[test]
    fn a_first_frame_picture_is_kept_beside_the_thumbnail() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();
        assert!(store.find_first_frame_path(&hash).unwrap().is_none());

        store.write_thumbnail(&hash, b"thumbnail", "jpg").unwrap();
        store.write_first_frame(&hash, b"first frame").unwrap();

        let thumbnail = store.find_thumbnail_path(&hash).unwrap().unwrap();
        let first_frame = store.find_first_frame_path(&hash).unwrap().unwrap();
        assert_eq!(
            first_frame,
            dir.path()
                .join("blobs/t/ab/cd")
                .join(format!("{hash}.first-frame.jpg"))
        );
        assert_eq!(first_frame.parent(), thumbnail.parent());
        assert_eq!(fs::read(&thumbnail).unwrap(), b"thumbnail");
        assert_eq!(fs::read(&first_frame).unwrap(), b"first frame");

        store.write_first_frame(&hash, b"made again").unwrap();
        assert_eq!(fs::read(&first_frame).unwrap(), b"made again");
    }

    /// Deleting a hash removes its original, its thumbnail and its
    /// first-frame picture. The files of another hash in the same folder
    /// stay.
    #[test]
    fn delete_removes_the_original_and_both_pictures() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();
        let other = format!("abcd{}", "0".repeat(60));
        for hash in [&hash, &other] {
            store.write_original(hash, b"video", Some("mp4")).unwrap();
            store.write_thumbnail(hash, b"thumbnail", "jpg").unwrap();
            store.write_first_frame(hash, b"first frame").unwrap();
        }

        store.delete(&hash).unwrap();

        assert!(store.find_original(&hash, Some("mp4")).unwrap().is_none());
        assert!(store.find_thumbnail_path(&hash).unwrap().is_none());
        assert!(store.find_first_frame_path(&hash).unwrap().is_none());
        assert!(store.find_original(&other, Some("mp4")).unwrap().is_some());
        assert!(store.find_thumbnail_path(&other).unwrap().is_some());
        assert!(store.find_first_frame_path(&other).unwrap().is_some());
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
    fn write_recreates_a_removed_staging_directory() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let hash = test_hash();
        fs::remove_dir_all(&store.staging).unwrap();

        store.write_original(&hash, b"data", Some("png")).unwrap();

        assert_eq!(store.read_original(&hash, Some("png")).unwrap(), b"data");
    }

    #[test]
    fn write_original_from_path_streams_and_verifies_content() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let source = dir.path().join("source.bin");
        let bytes = b"streamed original";
        fs::write(&source, bytes).unwrap();
        let hash = hex::encode(Sha256::digest(bytes));

        store
            .write_original_from_path(&hash, &source, Some("bin"))
            .unwrap();

        assert_eq!(store.read_original(&hash, Some("bin")).unwrap(), bytes);
    }

    #[test]
    fn write_original_from_path_rejects_changed_content() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let source = dir.path().join("source.bin");
        fs::write(&source, b"changed").unwrap();
        let expected = hex::encode(Sha256::digest(b"expected"));

        let error = store
            .write_original_from_path(&expected, &source, Some("bin"))
            .unwrap_err();

        assert!(matches!(error, BlobError::HashMismatch { .. }));
        assert!(!store
            .original_path_with_ext(&expected, Some("bin"))
            .unwrap()
            .exists());
    }

    #[test]
    fn promote_verified_original_moves_owned_staging_without_copying() {
        let dir = TempDir::new().unwrap();
        let store = BlobStore::open(dir.path()).unwrap();
        let source = dir.path().join("verified.bin");
        let bytes = b"already verified";
        fs::write(&source, bytes).unwrap();
        let hash = hex::encode(Sha256::digest(bytes));

        let original = store
            .promote_verified_original(&hash, &source, Some("bin"))
            .unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read(original).unwrap(), bytes);
    }

    #[test]
    fn test_open_clears_dead_process_staging_only() {
        let dir = TempDir::new().unwrap();
        let tmp = dir.path().join("blobs").join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        // This staging folder was left by another process, which no longer
        // runs.
        let dead = tmp.join("999999999-deadbeef");
        fs::create_dir_all(&dead).unwrap();
        fs::write(dead.join("partial.png"), b"partial").unwrap();
        // This staging folder belongs to an earlier `BlobStore` of this
        // process, which is still writing a file in it.
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
