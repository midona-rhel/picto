use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct DetectedProviderRoot {
    pub provider: String,
    pub account_label: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteObject {
    pub path: String,
    pub size_bytes: u64,
    pub checksum: Option<String>,
    pub revision: Option<String>,
}

pub trait CloudProvider: Send + Sync {
    fn connectivity(&self) -> ProviderFuture<'_, bool>;
    fn exists(&self, path: &str) -> ProviderFuture<'_, bool>;
    fn list(&self, prefix: &str) -> ProviderFuture<'_, Vec<RemoteObject>>;
    /// Returns the names of the folders directly inside `prefix`, without
    /// reading what they hold. A `prefix` that does not exist has none.
    fn children(&self, prefix: &str) -> ProviderFuture<'_, Vec<String>>;
    fn download(&self, path: &str) -> ProviderFuture<'_, Vec<u8>>;
    fn download_file(
        &self,
        path: &str,
        destination: PathBuf,
        checksum: &str,
    ) -> ProviderFuture<'_, ()>;
    fn upload(
        &self,
        path: &str,
        bytes: Vec<u8>,
        checksum: &str,
    ) -> ProviderFuture<'_, RemoteObject>;
    fn upload_file(
        &self,
        path: &str,
        source: PathBuf,
        checksum: &str,
    ) -> ProviderFuture<'_, RemoteObject>;
    fn upload_if_revision(
        &self,
        path: &str,
        bytes: Vec<u8>,
        checksum: &str,
        expected_revision: Option<&str>,
    ) -> ProviderFuture<'_, RemoteObject>;
    fn delete(&self, path: &str) -> ProviderFuture<'_, ()>;
}

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, String>> + Send + 'a>>;

/// The transport of cloud sync: a folder that an installed Google Drive or
/// Dropbox desktop app syncs. Picto reads and writes only its own files
/// below `picto/` in that folder. The desktop app signs in, uses the network
/// and carries the files to the other devices.
pub struct DirectoryProvider {
    root: PathBuf,
}

/// What [`DirectoryProvider::remove_library`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudDataRemoval {
    /// The number of files that were removed.
    pub removed_files: u64,
    /// Whether the library's folder is gone from the cloud folder.
    pub folder_removed: bool,
    /// The number of entries that were left: files and folders that the
    /// sync did not write, and files of the sync that could not be removed.
    pub kept_entries: u64,
    /// The first of those entries, as paths inside the library's folder.
    pub kept: Vec<String>,
}

impl CloudDataRemoval {
    /// At most this many kept entries are named in `kept`.
    const NAMED: usize = 12;

    fn keep(&mut self, path: &str) {
        self.kept_entries += 1;
        if self.kept.len() < Self::NAMED {
            self.kept.push(path.to_owned());
        }
    }
}

/// Which entries of one folder of a library's cloud data the sync wrote.
enum Known {
    /// Files whose names the function accepts.
    Files(fn(&str) -> bool),
    /// Folders whose names the function accepts. The entries of such a
    /// folder are checked against the next level.
    Folders(fn(&str) -> bool),
}

impl DirectoryProvider {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        std::fs::create_dir_all(&root)
            .map_err(|error| format!("Failed to create cloud sync root: {error}"))?;
        Ok(Self { root })
    }

    pub fn open_existing(root: impl Into<PathBuf>) -> Result<Self, String> {
        let root = root.into();
        if !root.is_dir() {
            return Err(format!(
                "Cloud sync folder is unavailable: {}",
                root.display()
            ));
        }
        Ok(Self { root })
    }

    pub fn open_provider_root(_provider: &str, root: impl Into<PathBuf>) -> Result<Self, String> {
        Self::open_existing(root)
    }

    pub fn verify_writable(&self) -> Result<(), String> {
        let picto_root = self.resolve("picto")?;
        std::fs::create_dir_all(&picto_root)
            .map_err(|error| format!("Cloud sync folder is not writable: {error}"))?;
        let probe = picto_root.join(format!(".write-test-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut file = std::fs::File::create(&probe)?;
            std::io::Write::write_all(&mut file, b"picto")?;
            file.sync_all()
        })();
        let _ = std::fs::remove_file(&probe);
        result.map_err(|error| format!("Cloud sync folder is not writable: {error}"))
    }

    /// Resolves an object key inside the provider root. Keys are provider-owned
    /// identifiers, not host paths: `/`-separated segments of ordinary names.
    /// Anything a host could read as rooted, drive-relative, device or parent
    /// (`\\`, `:`, `..`, empty segments) is rejected on every platform, and a
    /// link inside the root never leads out of it.
    fn resolve(&self, key: &str) -> Result<PathBuf, String> {
        let escape = || Err("Cloud object path escapes its provider root".to_string());
        if key.is_empty() {
            return escape();
        }
        let segments = key.split('/').collect::<Vec<_>>();
        let ordinary = |segment: &&str| {
            !segment.is_empty()
                && *segment != "."
                && *segment != ".."
                && !segment.chars().any(|c| matches!(c, '\\' | ':') || c.is_control())
                && !windows_device_name(segment)
        };
        if !segments.iter().all(ordinary) {
            return escape();
        }
        let mut path = self.root.clone();
        for (index, segment) in segments.iter().enumerate() {
            path.push(segment);
            match std::fs::symlink_metadata(&path) {
                // Junctions and other reparse points report as links too.
                Ok(metadata) if metadata.file_type().is_symlink() => return escape(),
                Ok(_) => {}
                // Nothing further down exists yet, so nothing can be a link.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    path.extend(&segments[index + 1..]);
                    break;
                }
                Err(error) => return Err(format!("Failed to inspect cloud object path: {error}")),
            }
        }
        Ok(path)
    }

    /// Removes the cloud data of the library `library_id`: the files that the
    /// sync wrote below `picto/<library id>/`, and each folder that is empty
    /// afterwards.
    ///
    /// The folder is touched only when it is a real folder (not a link) and
    /// its `library.json`, if it has one, names this library. A file is
    /// removed only when its name and its place are those of a file that
    /// the sync writes: `library.json`, a snapshot and its manifest in
    /// `snapshots/`, an original in `blobs/f/<aa>/<bb>/`, a deletion file in
    /// `deletions/<device id>/`, and a file that an interrupted upload left
    /// beside one of them. Everything else is left where it is and counted in
    /// the result, and a folder that still holds something is not removed.
    /// Links are never followed and never removed.
    ///
    /// `library.json` is removed first, so that another device sees at once
    /// that the cloud data is being removed.
    pub fn remove_library(&self, library_id: &str) -> Result<CloudDataRemoval, String> {
        if uuid::Uuid::parse_str(library_id).is_err() {
            return Err("The library has no valid cloud identity".to_string());
        }
        let folder = self.resolve(&format!("picto/{library_id}"))?;
        let mut removal = CloudDataRemoval::default();
        match std::fs::symlink_metadata(&folder) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => return Err("The library's cloud folder is not a folder".to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                removal.folder_removed = true;
                return Ok(removal);
            }
            Err(error) => return Err(format!("Failed to inspect the cloud folder: {error}")),
        }
        let manifest = folder.join("library.json");
        match std::fs::symlink_metadata(&manifest) {
            Ok(metadata) if metadata.file_type().is_file() => {
                let named = std::fs::read(&manifest)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                    .and_then(|value| value.get("library_id")?.as_str().map(str::to_owned));
                if named.as_deref() != Some(library_id) {
                    return Err(
                        "The cloud folder does not hold this library's data; nothing was removed"
                            .to_string(),
                    );
                }
                remove_file(&manifest, "library.json", &mut removal);
            }
            Ok(_) => removal.keep("library.json"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("Failed to inspect the cloud folder: {error}")),
        }
        let trees: [(&str, &[Known]); 3] = [
            ("snapshots", &[Known::Files(is_snapshot_file)]),
            (
                "deletions",
                &[Known::Folders(is_uuid), Known::Files(is_deletion_file)],
            ),
            (
                "blobs",
                &[
                    Known::Folders(|name| name == "f"),
                    Known::Folders(is_hex_pair),
                    Known::Folders(is_hex_pair),
                    Known::Files(is_blob_file),
                ],
            ),
        ];
        for entry in std::fs::read_dir(&folder)
            .map_err(|error| format!("Failed to list the cloud folder: {error}"))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let kind = entry.file_type().map_err(|error| error.to_string())?;
            let tree = trees.iter().find(|(known, _)| *known == name);
            match tree {
                Some((_, levels)) if kind.is_dir() => {
                    remove_known(&entry.path(), levels, &name, &mut removal)?;
                }
                _ if kind.is_file() && is_upload_remnant(&name) => {
                    remove_file(&entry.path(), &name, &mut removal);
                }
                // `library.json` was handled above.
                _ if name == "library.json" => {}
                _ => removal.keep(&name),
            }
        }
        removal.folder_removed = std::fs::remove_dir(&folder).is_ok();
        Ok(removal)
    }

    /// Returns the `library.json` of every library folder directly below
    /// `picto/`. It does not look into the folders below a library's folder,
    /// which can hold millions of files.
    pub fn library_manifests(&self) -> Result<Vec<PathBuf>, String> {
        let picto_root = self.resolve("picto")?;
        let entries = match std::fs::read_dir(&picto_root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(format!("Failed to list Picto libraries: {error}")),
        };
        let mut manifests = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|error| error.to_string())?;
            if !entry
                .file_type()
                .map_err(|error| error.to_string())?
                .is_dir()
            {
                continue;
            }
            let manifest = entry.path().join("library.json");
            if manifest.is_file() {
                manifests.push(manifest);
            }
        }
        manifests.sort();
        Ok(manifests)
    }

    pub fn read_local(&self, path: &Path) -> Result<Vec<u8>, String> {
        if !path.starts_with(&self.root) {
            return Err("Cloud object path escapes its provider root".to_string());
        }
        std::fs::read(path)
            .map_err(|error| format!("Failed to read cloud object {}: {error}", path.display()))
    }
}

/// Removes the entries of `folder` that `levels[0]` accepts, and then the
/// folder itself when nothing is left in it. `relative` is the folder's path
/// inside the library's cloud folder. An entry that is not accepted, a link,
/// and a file that cannot be removed are counted in `removal` and stay.
fn remove_known(
    folder: &Path,
    levels: &[Known],
    relative: &str,
    removal: &mut CloudDataRemoval,
) -> Result<(), String> {
    let entries = std::fs::read_dir(folder)
        .map_err(|error| format!("Failed to list the cloud folder: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        let path = format!("{relative}/{name}");
        match levels.first() {
            Some(Known::Folders(accepts)) if kind.is_dir() && accepts(&name) => {
                remove_known(&entry.path(), &levels[1..], &path, removal)?;
            }
            Some(Known::Files(accepts))
                if kind.is_file() && (accepts(&name) || is_upload_remnant(&name)) =>
            {
                remove_file(&entry.path(), &path, removal);
            }
            _ => removal.keep(&path),
        }
    }
    // `remove_dir` removes an empty folder only, so a folder that still
    // holds a kept entry stays.
    let _ = std::fs::remove_dir(folder);
    Ok(())
}

/// Removes one file of the sync. A file that cannot be removed, for example
/// because the provider's desktop app holds it open, is counted as kept.
fn remove_file(path: &Path, relative: &str, removal: &mut CloudDataRemoval) {
    match std::fs::remove_file(path) {
        Ok(()) => removal.removed_files += 1,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => removal.keep(relative),
    }
}

fn is_uuid(name: &str) -> bool {
    uuid::Uuid::parse_str(name).is_ok()
}

fn is_hex_pair(name: &str) -> bool {
    name.len() == 2 && name.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// A snapshot id is the time the snapshot was made, written as
/// `20261122T101500.123Z`, a hyphen, and a UUID.
fn is_snapshot_id(name: &str) -> bool {
    let Some((time, id)) = name.split_once('-') else {
        return false;
    };
    let digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
    time.len() == 20
        && digits(&time[..8])
        && &time[8..9] == "T"
        && digits(&time[9..15])
        && &time[15..16] == "."
        && digits(&time[16..19])
        && &time[19..] == "Z"
        && is_uuid(id)
}

/// A snapshot's manifest `<snapshot id>.json` or its database
/// `<snapshot id>.sqlite.zst`.
fn is_snapshot_file(name: &str) -> bool {
    name.strip_suffix(".json")
        .or_else(|| name.strip_suffix(".sqlite.zst"))
        .is_some_and(is_snapshot_id)
}

/// A deletion file: ten digits and `.jsonl.zst`.
fn is_deletion_file(name: &str) -> bool {
    name.strip_suffix(".jsonl.zst")
        .is_some_and(|number| number.len() == 10 && number.bytes().all(|b| b.is_ascii_digit()))
}

/// An original: its SHA-256 in hexadecimal, a dot, and its extension.
fn is_blob_file(name: &str) -> bool {
    name.split_once('.').is_some_and(|(hash, extension)| {
        hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !extension.is_empty()
            && extension.bytes().all(|byte| byte.is_ascii_alphanumeric())
    })
}

/// A file that `publish_file` writes beside its target while it uploads, and
/// that stays when the upload is interrupted: the target's name with its
/// last extension replaced by `picto-uploading-<uuid>`.
fn is_upload_remnant(name: &str) -> bool {
    name.rsplit_once(".picto-uploading-")
        .is_some_and(|(_, id)| is_uuid(id))
}

/// `CON`, `NUL`, `COM1`… name devices on Windows whatever their extension.
fn windows_device_name(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit())
}

fn replace_atomically(temporary: &Path, destination: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };

        let source = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let target = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();
        let succeeded = unsafe {
            MoveFileExW(
                source.as_ptr(),
                target.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if succeeded == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        std::fs::rename(temporary, destination).map_err(|error| error.to_string())
    }
}

fn file_checksum(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut HashWriter(&mut hasher)).map_err(|error| error.to_string())?;
    Ok(hex::encode(hasher.finalize()))
}

struct HashWriter<'a>(&'a mut Sha256);

impl std::io::Write for HashWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.0.update(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn publish_file(
    source: &Path,
    resolved: &Path,
    object_path: String,
    checksum: String,
    expected_revision: Option<String>,
) -> Result<RemoteObject, String> {
    if let Some(expected) = expected_revision {
        if !resolved.exists() {
            return Err("Cloud metadata was removed on another device".to_string());
        }
        if file_checksum(resolved)? != expected {
            return Err("Cloud metadata changed on another device".to_string());
        }
    }
    if file_checksum(source)? != checksum {
        return Err("Cloud upload checksum does not match its bytes".to_string());
    }
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Failed to create cloud object directory: {error}"))?;
    }
    let temporary = resolved.with_extension(format!("picto-uploading-{}", uuid::Uuid::new_v4()));
    std::fs::copy(source, &temporary)
        .map_err(|error| format!("Failed to stage cloud object: {error}"))?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|error| format!("Failed to flush staged cloud object: {error}"))?;
    replace_atomically(&temporary, resolved)
        .map_err(|error| format!("Failed to publish cloud object: {error}"))?;
    #[cfg(unix)]
    if let Some(parent) = resolved.parent() {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())?;
    }
    Ok(RemoteObject {
        path: object_path,
        size_bytes: std::fs::metadata(resolved)
            .map_err(|error| format!("Failed to inspect published cloud object: {error}"))?
            .len(),
        checksum: Some(checksum.clone()),
        revision: Some(checksum),
    })
}

/// Returns the folders that the variables `PICTO_GOOGLE_DRIVE_ROOT` and
/// `PICTO_DROPBOX_ROOT` name. Each stands in for the folder of that
/// provider's desktop app.
fn named_roots() -> Vec<DetectedProviderRoot> {
    let mut roots = Vec::new();
    if let Ok(path) = std::env::var("PICTO_GOOGLE_DRIVE_ROOT") {
        push_provider_directory(
            &mut roots,
            "google_drive",
            "Google Drive",
            PathBuf::from(path),
        );
    }
    if let Ok(path) = std::env::var("PICTO_DROPBOX_ROOT") {
        push_provider_directory(&mut roots, "dropbox", "Dropbox", PathBuf::from(path));
    }
    roots
}

pub fn detect_roots() -> Vec<DetectedProviderRoot> {
    let mut roots = named_roots();
    let Some(home) = home_directory() else {
        return roots;
    };

    detect_dropbox_roots(&mut roots, &home);

    #[cfg(target_os = "macos")]
    {
        let cloud_storage = home.join("Library").join("CloudStorage");
        if let Ok(entries) = std::fs::read_dir(cloud_storage) {
            for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(account) = name.strip_prefix("GoogleDrive-") {
                    push_google_content_roots(&mut roots, account, &entry.path());
                } else if name == "Dropbox" || name.starts_with("Dropbox-") {
                    let account = name.strip_prefix("Dropbox-").unwrap_or("Dropbox");
                    push_provider_directory(&mut roots, "dropbox", account, entry.path());
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Drive for desktop assigns each streamed account its own drive letter.
        // Enumerating the mounted roots also handles non-default letters and
        // multiple signed-in accounts without relying on a display name.
        for letter in b'A'..=b'Z' {
            let drive = PathBuf::from(format!("{}:\\", letter as char));
            if is_google_drive_volume(&drive) || drive.join("My Drive").is_dir() {
                push_google_content_roots(
                    &mut roots,
                    &format!("Google Drive ({})", letter as char),
                    &drive,
                );
            }
        }
    }

    // Google does not ship Drive for desktop on Linux. An explicitly mounted
    // location remains available there through PICTO_GOOGLE_DRIVE_ROOT.
    roots.sort_by(|left, right| {
        (&left.provider, &left.account_label, &left.path).cmp(&(
            &right.provider,
            &right.account_label,
            &right.path,
        ))
    });
    roots.dedup_by(|left, right| left.path == right.path);
    roots
}

/// Returns the provider folder that the user's choice `root` is, or an error
/// when it is not the folder of an installed desktop app of `provider`. A
/// folder that can be written to is not enough: Picto would then say that a
/// library syncs while it writes to a folder that no provider uploads.
///
/// A folder that `PICTO_GOOGLE_DRIVE_ROOT` or `PICTO_DROPBOX_ROOT` names is
/// accepted before the installed desktop apps are looked for, so a check
/// that uses such a stand-in folder does not read the home folder.
pub fn validate_root(
    provider: &str,
    root: impl Into<PathBuf>,
) -> Result<DetectedProviderRoot, String> {
    if !matches!(provider, "google_drive" | "dropbox") {
        return Err(format!("Unsupported cloud folder provider: {provider}"));
    }
    let selected = root.into();
    let selected_identity = path_identity(&selected);
    let chosen = |candidate: &DetectedProviderRoot| {
        candidate.provider == provider
            && path_identity(Path::new(&candidate.path)) == selected_identity
    };
    if let Some(named) = named_roots().into_iter().find(chosen) {
        return Ok(named);
    }
    if let Some(detected) = detect_roots().into_iter().find(chosen) {
        return Ok(detected);
    }
    #[cfg(target_os = "windows")]
    if provider == "google_drive" && is_google_folder_mount_selection(&selected) {
        return Ok(DetectedProviderRoot {
            provider: provider.to_string(),
            account_label: selected
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("Google Drive")
                .to_string(),
            path: selected.to_string_lossy().into_owned(),
        });
    }
    Err({
        let name = if provider == "google_drive" {
            "Google Drive"
        } else {
            "Dropbox"
        };
        format!("The selected folder is not the root of an installed {name} account")
    })
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
}

fn path_identity(path: &Path) -> String {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let value = resolved
        .to_string_lossy()
        .trim_end_matches(['/', '\\'])
        .to_string();
    if cfg!(target_os = "windows") {
        value.to_lowercase()
    } else {
        value
    }
}

fn detect_dropbox_roots(roots: &mut Vec<DetectedProviderRoot>, home: &Path) {
    #[cfg(target_os = "windows")]
    let info_files: Vec<_> = ["APPDATA", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(std::env::var_os)
        .map(|directory| PathBuf::from(directory).join("Dropbox").join("info.json"))
        .collect();
    #[cfg(not(target_os = "windows"))]
    let info_files = vec![home.join(".dropbox").join("info.json")];

    for info_file in info_files {
        let Ok(contents) = std::fs::read(&info_file) else {
            continue;
        };
        let Ok(accounts) =
            serde_json::from_slice::<serde_json::Map<String, serde_json::Value>>(&contents)
        else {
            continue;
        };
        for (kind, account) in accounts {
            let Some(path) = account.get("path").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let label = if kind == "business" {
                "Dropbox Business"
            } else {
                "Dropbox"
            };
            push_provider_directory(roots, "dropbox", label, PathBuf::from(path));
        }
    }

    // Older/simple installations may not expose info.json yet. This fallback
    // remains constrained to Dropbox's documented default folder.
    push_provider_directory(roots, "dropbox", "Dropbox", home.join("Dropbox"));
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
fn push_google_content_roots(
    roots: &mut Vec<DetectedProviderRoot>,
    account_label: &str,
    account_container: &Path,
) {
    let Ok(entries) = std::fs::read_dir(account_container) else {
        return;
    };
    for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        push_if_directory(
            roots,
            "google_drive",
            &format!("{account_label} · {name}"),
            entry.path(),
        );
    }
}

#[cfg(target_os = "windows")]
fn is_google_drive_volume(root: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;

    let path = root
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut volume_name = [0_u16; 128];
    let result = unsafe {
        GetVolumeInformationW(
            path.as_ptr(),
            volume_name.as_mut_ptr(),
            volume_name.len() as u32,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if result == 0 {
        return false;
    }
    let length = volume_name
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(volume_name.len());
    String::from_utf16_lossy(&volume_name[..length])
        .to_lowercase()
        .contains("google drive")
}

#[cfg(target_os = "windows")]
fn is_google_folder_mount_selection(selected: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, FILE_ATTRIBUTE_REPARSE_POINT, INVALID_FILE_ATTRIBUTES,
    };

    selected.is_dir()
        && [selected, selected.parent().unwrap_or(selected)]
            .into_iter()
            .any(|path| {
                let wide = path
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect::<Vec<_>>();
                let attributes = unsafe { GetFileAttributesW(wide.as_ptr()) };
                attributes != INVALID_FILE_ATTRIBUTES
                    && attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            })
}

fn push_if_directory(
    roots: &mut Vec<DetectedProviderRoot>,
    provider: &str,
    account_label: &str,
    path: PathBuf,
) {
    if path.is_dir() {
        roots.push(DetectedProviderRoot {
            provider: provider.to_string(),
            account_label: account_label.to_string(),
            path: path.to_string_lossy().into_owned(),
        });
    }
}

fn push_provider_directory(
    roots: &mut Vec<DetectedProviderRoot>,
    provider: &str,
    account_label: &str,
    path: PathBuf,
) {
    push_if_directory(roots, provider, account_label, path);
}

impl CloudProvider for DirectoryProvider {
    fn connectivity(&self) -> ProviderFuture<'_, bool> {
        let root = self.root.clone();
        Box::pin(async move { Ok(root.is_dir()) })
    }

    fn exists(&self, path: &str) -> ProviderFuture<'_, bool> {
        let resolved = self.resolve(path);
        Box::pin(async move { Ok(resolved?.is_file()) })
    }

    fn list(&self, prefix: &str) -> ProviderFuture<'_, Vec<RemoteObject>> {
        let root = self.root.clone();
        let base = self.resolve(prefix);
        Box::pin(async move {
            let base = base?;
            if !base.exists() {
                return Ok(Vec::new());
            }
            let mut pending = vec![base];
            let mut objects = Vec::new();
            while let Some(directory) = pending.pop() {
                for entry in std::fs::read_dir(&directory)
                    .map_err(|error| format!("Failed to list cloud objects: {error}"))?
                {
                    let entry = entry.map_err(|error| error.to_string())?;
                    let path = entry.path();
                    let kind = entry.file_type().map_err(|error| error.to_string())?;
                    if kind.is_symlink() {
                        // Links never lead a listing out of the provider root.
                        continue;
                    }
                    if kind.is_dir() {
                        pending.push(path);
                    } else {
                        let relative = path
                            .strip_prefix(&root)
                            .map_err(|error| error.to_string())?
                            .to_string_lossy()
                            .replace('\\', "/");
                        let metadata = entry.metadata().map_err(|error| error.to_string())?;
                        objects.push(RemoteObject {
                            path: relative,
                            size_bytes: metadata.len(),
                            checksum: None,
                            revision: None,
                        });
                    }
                }
            }
            objects.sort_by(|a, b| a.path.cmp(&b.path));
            Ok(objects)
        })
    }

    fn children(&self, prefix: &str) -> ProviderFuture<'_, Vec<String>> {
        let base = self.resolve(prefix);
        Box::pin(async move {
            let entries = match std::fs::read_dir(base?) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Vec::new())
                }
                Err(error) => return Err(format!("Failed to list cloud objects: {error}")),
            };
            let mut names = Vec::new();
            for entry in entries {
                let entry = entry.map_err(|error| error.to_string())?;
                // A link is not a folder of the sync.
                if entry.file_type().map_err(|error| error.to_string())?.is_dir() {
                    names.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
            Ok(names)
        })
    }

    fn download(&self, path: &str) -> ProviderFuture<'_, Vec<u8>> {
        let resolved = self.resolve(path);
        let path = path.to_string();
        Box::pin(async move {
            std::fs::read(resolved?)
                .map_err(|error| format!("Failed to download cloud object {path}: {error}"))
        })
    }

    fn download_file(
        &self,
        path: &str,
        destination: PathBuf,
        checksum: &str,
    ) -> ProviderFuture<'_, ()> {
        let source = self.resolve(path);
        let checksum = checksum.to_string();
        Box::pin(async move {
            let source = source?;
            tokio::task::spawn_blocking(move || {
                if file_checksum(&source)? != checksum {
                    return Err(
                        "Cloud original checksum does not match its content hash".to_string()
                    );
                }
                if let Some(parent) = destination.parent() {
                    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
                }
                std::fs::copy(&source, &destination).map_err(|error| error.to_string())?;
                Ok(())
            })
            .await
            .map_err(|error| format!("Cloud download worker failed: {error}"))?
        })
    }

    fn upload(
        &self,
        path: &str,
        bytes: Vec<u8>,
        _checksum: &str,
    ) -> ProviderFuture<'_, RemoteObject> {
        self.upload_if_revision(path, bytes, _checksum, None)
    }

    fn upload_file(
        &self,
        path: &str,
        source: PathBuf,
        checksum: &str,
    ) -> ProviderFuture<'_, RemoteObject> {
        let resolved = self.resolve(path);
        let object_path = path.to_string();
        let checksum = checksum.to_string();
        Box::pin(async move {
            let resolved = resolved?;
            tokio::task::spawn_blocking(move || {
                publish_file(&source, &resolved, object_path, checksum, None)
            })
            .await
            .map_err(|error| format!("Cloud upload worker failed: {error}"))?
        })
    }

    fn upload_if_revision(
        &self,
        path: &str,
        bytes: Vec<u8>,
        checksum: &str,
        expected_revision: Option<&str>,
    ) -> ProviderFuture<'_, RemoteObject> {
        let resolved = self.resolve(path);
        let path = path.to_string();
        let checksum = checksum.to_string();
        let expected_revision = expected_revision.map(str::to_string);
        Box::pin(async move {
            let resolved = resolved?;
            tokio::task::spawn_blocking(move || {
                let source = tempfile::NamedTempFile::new().map_err(|error| error.to_string())?;
                std::fs::write(source.path(), bytes).map_err(|error| error.to_string())?;
                publish_file(source.path(), &resolved, path, checksum, expected_revision)
            })
            .await
            .map_err(|error| format!("Cloud upload worker failed: {error}"))?
        })
    }

    fn delete(&self, path: &str) -> ProviderFuture<'_, ()> {
        let resolved = self.resolve(path);
        Box::pin(async move {
            let resolved = resolved?;
            if resolved.exists() {
                std::fs::remove_file(resolved).map_err(|error| error.to_string())?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    #[tokio::test]
    async fn conditional_upload_rejects_a_stale_revision() {
        let root = tempfile::tempdir().unwrap();
        let provider = DirectoryProvider::open(root.path()).unwrap();
        let first = b"first".to_vec();
        let first_hash = hex::encode(sha2::Sha256::digest(&first));
        provider
            .upload("library.json", first, &first_hash)
            .await
            .unwrap();
        let next = b"next".to_vec();
        let next_hash = hex::encode(sha2::Sha256::digest(&next));
        assert!(provider
            .upload_if_revision("library.json", next, &next_hash, Some("stale"))
            .await
            .unwrap_err()
            .contains("another device"));
    }

    #[tokio::test]
    async fn object_keys_cannot_reach_outside_the_provider_root() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("provider");
        std::fs::create_dir_all(root.join("picto")).unwrap();
        let marker = fixture.path().join("marker.txt");
        std::fs::write(&marker, b"outside").unwrap();
        let provider = DirectoryProvider::open(&root).unwrap();
        let hash = hex::encode(sha2::Sha256::digest(b"x"));
        let escapes = [
            "",
            "/marker.txt",
            "../marker.txt",
            "picto/../../marker.txt",
            "\\marker.txt",
            "\\\\server\\share\\marker.txt",
            "\\\\?\\C:\\marker.txt",
            "C:marker.txt",
            "C:/marker.txt",
            "picto\\..\\..\\marker.txt",
            "picto//marker.txt",
            "picto/./marker.txt",
            "picto/NUL",
            "picto/com1.txt",
            "picto/marker\u{0}.txt",
        ];
        for key in escapes {
            assert!(provider.exists(key).await.is_err(), "exists {key:?}");
            assert!(provider.download(key).await.is_err(), "download {key:?}");
            assert!(provider.list(key).await.is_err(), "list {key:?}");
            assert!(provider.delete(key).await.is_err(), "delete {key:?}");
            assert!(provider.upload(key, b"x".to_vec(), &hash).await.is_err(), "upload {key:?}");
        }
        assert_eq!(std::fs::read(&marker).unwrap(), b"outside");
        // Ordinary keys still work.
        provider.upload("picto/lib/library.json", b"x".to_vec(), &hash).await.unwrap();
        assert!(provider.exists("picto/lib/library.json").await.unwrap());
        assert_eq!(provider.list("picto").await.unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn links_inside_the_root_do_not_lead_out_of_it() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path().join("provider");
        let outside = fixture.path().join("outside");
        std::fs::create_dir_all(root.join("picto")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("marker.txt"), b"outside").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("picto/linked")).unwrap();
        std::os::unix::fs::symlink(outside.join("marker.txt"), root.join("picto/file")).unwrap();
        let provider = DirectoryProvider::open(&root).unwrap();

        assert!(provider.download("picto/linked/marker.txt").await.is_err());
        assert!(provider.download("picto/file").await.is_err());
        assert!(provider.delete("picto/linked/marker.txt").await.is_err());
        assert!(provider.list("picto").await.unwrap().is_empty());
        assert_eq!(std::fs::read(outside.join("marker.txt")).unwrap(), b"outside");
    }

    #[test]
    fn library_discovery_does_not_walk_artifact_trees() {
        let root = tempfile::tempdir().unwrap();
        let provider = DirectoryProvider::open(root.path()).unwrap();
        std::fs::create_dir_all(root.path().join("picto/library-a/blobs/f/aa")).unwrap();
        std::fs::write(root.path().join("picto/library-a/library.json"), b"{}").unwrap();
        std::fs::write(
            root.path().join("picto/library-a/blobs/f/aa/library.json"),
            b"not a manifest",
        )
        .unwrap();

        assert_eq!(
            provider.library_manifests().unwrap(),
            vec![root.path().join("picto/library-a/library.json")]
        );
    }

    #[test]
    fn writable_probe_does_not_leave_a_test_file() {
        let root = tempfile::tempdir().unwrap();
        let provider = DirectoryProvider::open_existing(root.path()).unwrap();

        provider.verify_writable().unwrap();

        assert_eq!(
            std::fs::read_dir(root.path().join("picto"))
                .unwrap()
                .count(),
            0
        );
    }

    #[test]
    fn google_content_roots_keep_provider_supplied_names() {
        let container = tempfile::tempdir().unwrap();
        for name in ["Min enhet", "Shared folders"] {
            std::fs::create_dir(container.path().join(name)).unwrap();
        }
        std::fs::create_dir(container.path().join(".internal")).unwrap();
        let mut roots = Vec::new();

        push_google_content_roots(&mut roots, "account", container.path());

        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|root| root.path.ends_with("Min enhet")));
        assert!(roots
            .iter()
            .any(|root| root.path.ends_with("Shared folders")));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn dropbox_uses_its_documented_account_metadata() {
        let home = tempfile::tempdir().unwrap();
        let dropbox = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".dropbox")).unwrap();
        std::fs::write(
            home.path().join(".dropbox/info.json"),
            serde_json::json!({ "personal": { "path": dropbox.path() } }).to_string(),
        )
        .unwrap();
        let mut roots = Vec::new();

        detect_dropbox_roots(&mut roots, home.path());

        assert_eq!(roots.len(), 1);
        assert_eq!(Path::new(&roots[0].path), dropbox.path());
    }
}
