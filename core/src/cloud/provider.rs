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

    pub fn open_provider_root(provider: &str, root: impl Into<PathBuf>) -> Result<Self, String> {
        Self::open_existing(canonical_provider_root(provider, root.into()))
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

    #[cfg(target_os = "macos")]
    {
        let cloud_storage = home.join("Library").join("CloudStorage");
        if let Ok(entries) = std::fs::read_dir(cloud_storage) {
            for entry in entries.flatten().filter(|entry| entry.path().is_dir()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(account) = name.strip_prefix("GoogleDrive-") {
                    push_provider_directory(&mut roots, "google_drive", account, entry.path());
                } else if name == "Dropbox" || name.starts_with("Dropbox-") {
                    let account = name.strip_prefix("Dropbox-").unwrap_or("Dropbox");
                    push_provider_directory(&mut roots, "dropbox", account, entry.path());
                }
            }
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        push_provider_directory(&mut roots, "dropbox", "Dropbox", home.join("Dropbox"));
        push_provider_directory(
            &mut roots,
            "google_drive",
            "Google Drive",
            home.join("Google Drive"),
        );
    }
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

pub fn canonical_provider_root(provider: &str, root: PathBuf) -> PathBuf {
    if provider == "google_drive" {
        let my_drive = root.join("My Drive");
        if my_drive.is_dir() {
            return my_drive;
        }
    }
    root
}

fn home_directory() -> Option<PathBuf> {
    std::env::var_os(if cfg!(target_os = "windows") {
        "USERPROFILE"
    } else {
        "HOME"
    })
    .map(PathBuf::from)
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
    push_if_directory(
        roots,
        provider,
        account_label,
        canonical_provider_root(provider, path),
    );
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
    fn google_drive_account_container_resolves_to_my_drive() {
        let account_root = tempfile::tempdir().unwrap();
        let my_drive = account_root.path().join("My Drive");
        std::fs::create_dir(&my_drive).unwrap();

        assert_eq!(
            canonical_provider_root("google_drive", account_root.path().to_path_buf()),
            my_drive
        );
        assert_eq!(
            canonical_provider_root("dropbox", account_root.path().to_path_buf()),
            account_root.path()
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
}
