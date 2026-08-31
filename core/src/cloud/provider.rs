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

/// Filesystem transport backed by an installed Drive or Dropbox desktop app.
/// Picto owns only the checked artifacts inside this directory; the desktop
/// client owns authentication, networking, and cross-device transfer.
pub struct DirectoryProvider {
    root: PathBuf,
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

    fn resolve(&self, relative: &str) -> Result<PathBuf, String> {
        let path = Path::new(relative);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err("Cloud object path escapes its provider root".to_string());
        }
        Ok(self.root.join(path))
    }

    /// Finds only immediate Picto library manifests. Blob and epoch trees may
    /// contain millions of files and must never be traversed for discovery.
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
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = resolved.with_extension(format!("picto-uploading-{}", uuid::Uuid::new_v4()));
    std::fs::copy(source, &temporary).map_err(|error| error.to_string())?;
    std::fs::File::open(&temporary)
        .and_then(|file| file.sync_all())
        .map_err(|error| error.to_string())?;
    replace_atomically(&temporary, resolved)?;
    #[cfg(unix)]
    if let Some(parent) = resolved.parent() {
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())?;
    }
    Ok(RemoteObject {
        path: object_path,
        size_bytes: std::fs::metadata(resolved)
            .map_err(|error| error.to_string())?
            .len(),
        checksum: Some(checksum.clone()),
        revision: Some(checksum),
    })
}

pub fn detect_roots() -> Vec<DetectedProviderRoot> {
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

/// Resolve a user choice to an installed provider root. A writable arbitrary
/// directory is deliberately not enough: otherwise Picto can claim cloud sync
/// while writing to a location no provider uploads.
pub fn validate_root(
    provider: &str,
    root: impl Into<PathBuf>,
) -> Result<DetectedProviderRoot, String> {
    if !matches!(provider, "google_drive" | "dropbox") {
        return Err(format!("Unsupported cloud folder provider: {provider}"));
    }
    let selected = root.into();
    let selected_identity = path_identity(&selected);
    let detected = detect_roots().into_iter().find(|candidate| {
        candidate.provider == provider
            && path_identity(Path::new(&candidate.path)) == selected_identity
    });
    if let Some(detected) = detected {
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
                    if path.is_dir() {
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
