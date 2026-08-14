//! Filesystem backend: a directory as the remote. Pointing this at a folder
//! synced by Google Drive for Desktop (or Dropbox, or any file-sync client)
//! gives real multi-device sync without any provider API — the sync client
//! is just transport for immutable files.

use std::fs;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

use super::backend::{BackendError, SyncBackend};

pub struct FsBackend {
    root: PathBuf,
    #[cfg(any(unix, windows))]
    root_identity: (u64, u64),
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<(), BackendError> {
    fs::File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| BackendError::Io(error.to_string()))
}

#[cfg(not(unix))]
fn sync_directory(_path: &Path) -> Result<(), BackendError> {
    Ok(())
}

impl FsBackend {
    pub fn open(root: &Path) -> Result<Self, BackendError> {
        fs::create_dir_all(root).map_err(|e| BackendError::Io(e.to_string()))?;
        let metadata = fs::symlink_metadata(root).map_err(|e| BackendError::Io(e.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(BackendError::Io(format!(
                "sync root must be a regular directory: {}",
                root.display()
            )));
        }
        let root = fs::canonicalize(root).map_err(|e| BackendError::Io(e.to_string()))?;
        Self::from_resolved_root(root)
    }

    /// Open an existing directory without ever recreating it, rejecting a
    /// symlinked root and requiring its resolved parent to match `parent`.
    pub fn open_existing_contained(root: &Path, parent: &Path) -> Result<Self, BackendError> {
        let metadata =
            fs::symlink_metadata(root).map_err(|error| BackendError::Io(error.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(BackendError::Io(format!(
                "sync root must be an existing regular directory: {}",
                root.display()
            )));
        }
        let resolved_parent =
            fs::canonicalize(parent).map_err(|error| BackendError::Io(error.to_string()))?;
        let resolved_root =
            fs::canonicalize(root).map_err(|error| BackendError::Io(error.to_string()))?;
        if resolved_root.parent() != Some(resolved_parent.as_path()) {
            return Err(BackendError::Io(format!(
                "sync root is not directly contained by {}",
                parent.display()
            )));
        }
        let backend = Self::from_resolved_root(resolved_root)?;
        backend.validate_root()?;
        Ok(backend)
    }

    fn from_resolved_root(root: PathBuf) -> Result<Self, BackendError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            let metadata =
                fs::metadata(&root).map_err(|error| BackendError::Io(error.to_string()))?;
            Ok(Self {
                root,
                root_identity: (metadata.dev(), metadata.ino()),
            })
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            let metadata =
                fs::metadata(&root).map_err(|error| BackendError::Io(error.to_string()))?;
            let volume = metadata.volume_serial_number().ok_or_else(|| {
                BackendError::Io(format!(
                    "sync root has no volume identity: {}",
                    root.display()
                ))
            })?;
            let index = metadata.file_index().ok_or_else(|| {
                BackendError::Io(format!(
                    "sync root has no file identity: {}",
                    root.display()
                ))
            })?;
            Ok(Self {
                root,
                root_identity: (u64::from(volume), index),
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Ok(Self { root })
        }
    }

    fn validate_root(&self) -> Result<(), BackendError> {
        let metadata = fs::symlink_metadata(&self.root)
            .map_err(|error| BackendError::Io(error.to_string()))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(BackendError::Io(format!(
                "sync root was replaced or removed: {}",
                self.root.display()
            )));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt as _;
            if (metadata.dev(), metadata.ino()) != self.root_identity {
                return Err(BackendError::Io(format!(
                    "sync root was replaced after it was opened: {}",
                    self.root.display()
                )));
            }
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt as _;
            let current_identity = metadata
                .volume_serial_number()
                .map(u64::from)
                .zip(metadata.file_index());
            if current_identity != Some(self.root_identity) {
                return Err(BackendError::Io(format!(
                    "sync root was replaced after it was opened: {}",
                    self.root.display()
                )));
            }
        }
        let current =
            fs::canonicalize(&self.root).map_err(|error| BackendError::Io(error.to_string()))?;
        if current != self.root {
            return Err(BackendError::Io(format!(
                "sync root no longer resolves to its opened directory: {}",
                self.root.display()
            )));
        }
        Ok(())
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, BackendError> {
        self.validate_root()?;
        // Keys are forward-slash object names; reject traversal.
        if key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains('\\'))
        {
            return Err(BackendError::Io(format!("invalid object key: {key}")));
        }
        Ok(self.root.join(key))
    }

    fn directory_for_prefix(&self, prefix: &str) -> Result<PathBuf, BackendError> {
        validate_list_prefix(prefix)?;
        let trimmed = prefix.trim_end_matches('/');
        if trimmed.is_empty() {
            Ok(self.root.clone())
        } else {
            self.path_for(trimmed)
        }
    }

    /// Return whether a path exists without following any symlink component.
    /// Missing ancestors make the object absent; an existing symlink is never
    /// accepted because it could be replaced or redirected between calls.
    fn existing_path(&self, path: &Path) -> Result<bool, BackendError> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| BackendError::Io("path escaped sync root".to_string()))?;
        let mut current = self.root.clone();
        let components = relative.components().collect::<Vec<_>>();

        for (index, component) in components.iter().enumerate() {
            current.push(component.as_os_str());
            let metadata = match fs::symlink_metadata(&current) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
                Err(error) => return Err(BackendError::Io(error.to_string())),
            };
            if metadata.file_type().is_symlink() {
                return Err(BackendError::Io(format!(
                    "symlink component is not allowed: {}",
                    current.display()
                )));
            }
            if index + 1 < components.len() && !metadata.is_dir() {
                return Err(BackendError::Io(format!(
                    "path component is not a directory: {}",
                    current.display()
                )));
            }
        }

        Ok(true)
    }

    fn create_parent_dirs(&self, path: &Path) -> Result<(), BackendError> {
        let parent = path
            .parent()
            .ok_or_else(|| BackendError::Io("object has no parent directory".to_string()))?;
        let relative = parent
            .strip_prefix(&self.root)
            .map_err(|_| BackendError::Io("path escaped sync root".to_string()))?;
        let mut current = self.root.clone();

        for component in relative.components() {
            current.push(component.as_os_str());
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err(BackendError::Io(format!(
                        "symlink component is not allowed: {}",
                        current.display()
                    )));
                }
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => {
                    return Err(BackendError::Io(format!(
                        "path component is not a directory: {}",
                        current.display()
                    )));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current)
                        .map_err(|error| BackendError::Io(error.to_string()))?;
                    if let Some(parent) = current.parent() {
                        sync_directory(parent)?;
                    }
                    sync_directory(&current)?;
                }
                Err(error) => return Err(BackendError::Io(error.to_string())),
            }
        }
        Ok(())
    }
}

impl SyncBackend for FsBackend {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        let path = self.path_for(key)?;
        if self.existing_path(&path)? {
            return Err(BackendError::AlreadyExists(key.to_string()));
        }
        self.create_parent_dirs(&path)?;
        // Recheck after creating parents so a symlink or competing object is
        // never accepted as the destination.
        if self.existing_path(&path)? {
            return Err(BackendError::AlreadyExists(key.to_string()));
        }
        // Stage-and-rename so a sync client never observes (or uploads) a
        // half-written object; create_new on rename target is approximated by
        // the exists() check above plus per-device key namespacing.
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(&self.root))
            .map_err(|e| BackendError::Io(e.to_string()))?;
        tmp.write_all(bytes)
            .and_then(|_| tmp.as_file().sync_all())
            .map_err(|e| BackendError::Io(e.to_string()))?;
        tmp.persist_noclobber(&path).map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                BackendError::AlreadyExists(key.to_string())
            } else {
                BackendError::Io(e.error.to_string())
            }
        })?;
        sync_directory(path.parent().unwrap_or(&self.root))?;
        Ok(())
    }

    fn put_replace(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        let path = self.path_for(key)?;
        self.create_parent_dirs(&path)?;
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(&self.root))
            .map_err(|error| BackendError::Io(error.to_string()))?;
        tmp.write_all(bytes)
            .and_then(|_| tmp.as_file().sync_all())
            .map_err(|error| BackendError::Io(error.to_string()))?;
        tmp.persist(&path)
            .map_err(|error| BackendError::Io(error.error.to_string()))?;
        sync_directory(path.parent().unwrap_or(&self.root))?;
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, BackendError> {
        let path = self.path_for(key)?;
        if !self.existing_path(&path)? {
            return Ok(None);
        }
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BackendError::Io(e.to_string())),
        }
    }

    fn get_limited(&self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, BackendError> {
        let path = self.path_for(key)?;
        if !self.existing_path(&path)? {
            return Ok(None);
        }
        let mut file =
            fs::File::open(&path).map_err(|error| BackendError::Io(error.to_string()))?;
        let mut bytes = Vec::new();
        std::io::Read::by_ref(&mut file)
            .take(max_bytes as u64)
            .read_to_end(&mut bytes)
            .map_err(|error| BackendError::Io(error.to_string()))?;
        let mut extra = [0_u8; 1];
        if file
            .read(&mut extra)
            .map_err(|error| BackendError::Io(error.to_string()))?
            > 0
        {
            return Err(BackendError::LimitExceeded(format!(
                "object {key} exceeds {max_bytes} bytes"
            )));
        }
        Ok(Some(bytes))
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        self.validate_root()?;
        validate_list_prefix(prefix)?;
        let mut keys = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let entries =
                fs::read_dir(&dir).map_err(|error| BackendError::Io(error.to_string()))?;
            for entry in entries {
                let entry = entry.map_err(|error| BackendError::Io(error.to_string()))?;
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .map_err(|error| BackendError::Io(error.to_string()))?;
                if file_type.is_symlink() {
                    continue;
                }
                if file_type.is_dir() {
                    stack.push(path);
                } else if file_type.is_file() {
                    if let Ok(rel) = path.strip_prefix(&self.root) {
                        let key = rel
                            .components()
                            .map(|c| c.as_os_str().to_string_lossy())
                            .collect::<Vec<_>>()
                            .join("/");
                        // Skip sync-client droppings and staged temp files.
                        if key.starts_with(prefix) && !key.starts_with('.') {
                            keys.push(key);
                        }
                    }
                }
            }
        }
        keys.sort();
        Ok(keys)
    }

    fn list_directories(
        &self,
        prefix: &str,
        max_results: usize,
    ) -> Result<Vec<String>, BackendError> {
        self.validate_root()?;
        let directory = self.directory_for_prefix(prefix)?;
        if !self.existing_path(&directory)? {
            return Ok(Vec::new());
        }
        let mut children = Vec::new();
        let entries =
            fs::read_dir(&directory).map_err(|error| BackendError::Io(error.to_string()))?;
        for entry in entries {
            let entry = entry.map_err(|error| BackendError::Io(error.to_string()))?;
            let file_type = entry
                .file_type()
                .map_err(|error| BackendError::Io(error.to_string()))?;
            if file_type.is_dir() && !file_type.is_symlink() {
                children.push(entry.file_name().to_string_lossy().into_owned());
                if children.len() > max_results {
                    return Err(BackendError::LimitExceeded(format!(
                        "more than {max_results} directories under {prefix}"
                    )));
                }
            }
        }
        children.sort();
        Ok(children)
    }

    fn delete(&self, key: &str) -> Result<(), BackendError> {
        let path = self.path_for(key)?;
        if !self.existing_path(&path)? {
            return Ok(());
        }
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(BackendError::Io(e.to_string())),
        }
    }
}

fn validate_list_prefix(prefix: &str) -> Result<(), BackendError> {
    if prefix.is_empty() {
        return Ok(());
    }
    let parts = prefix.split('/').collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        let is_trailing_separator = index + 1 == parts.len() && part.is_empty();
        if is_trailing_separator && !prefix.is_empty() {
            continue;
        }
        if part.is_empty() || *part == "." || *part == ".." || part.contains('\\') {
            return Err(BackendError::Io(format!("invalid object prefix: {prefix}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn fs_backend_round_trip_and_create_only() {
        let dir = TempDir::new().unwrap();
        let backend = FsBackend::open(dir.path()).unwrap();
        backend.put("oplog/dev/0001.seg", b"abc").unwrap();
        assert!(matches!(
            backend.put("oplog/dev/0001.seg", b"xyz"),
            Err(BackendError::AlreadyExists(_))
        ));
        assert_eq!(backend.get("oplog/dev/0001.seg").unwrap().unwrap(), b"abc");
        assert_eq!(
            backend.list("oplog/").unwrap(),
            vec!["oplog/dev/0001.seg".to_string()]
        );
        assert!(backend.get("oplog/dev/0002.seg").unwrap().is_none());
        assert_eq!(
            backend.list_directories("oplog/", 10).unwrap(),
            vec!["dev".to_string()]
        );
        assert!(matches!(
            backend.get_limited("oplog/dev/0001.seg", 2),
            Err(BackendError::LimitExceeded(_))
        ));
        backend.put_replace("oplog/dev/head", b"1").unwrap();
        backend.put_replace("oplog/dev/head", b"2").unwrap();
        assert_eq!(backend.get("oplog/dev/head").unwrap().unwrap(), b"2");
    }

    #[test]
    fn traversal_keys_are_rejected() {
        let dir = TempDir::new().unwrap();
        let backend = FsBackend::open(dir.path()).unwrap();
        assert!(backend.put("../escape", b"x").is_err());
        assert!(backend.put("a//b", b"x").is_err());
        assert!(backend.get("../escape").is_err());
        assert!(backend.list("../").is_err());
        assert!(backend.delete("../escape").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_file_cannot_redirect_operations_outside_root() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("secret.seg");
        fs::write(&outside_file, b"secret").unwrap();
        symlink(&outside_file, root.path().join("redirect.seg")).unwrap();
        let backend = FsBackend::open(root.path()).unwrap();

        assert!(backend.get("redirect.seg").is_err());
        assert!(backend.put("redirect.seg", b"replacement").is_err());
        assert!(backend.delete("redirect.seg").is_err());
        assert!(backend.list("").unwrap().is_empty());
        assert_eq!(fs::read(&outside_file).unwrap(), b"secret");
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_cannot_redirect_operations_outside_root() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("secret.seg");
        fs::write(&outside_file, b"secret").unwrap();
        symlink(outside.path(), root.path().join("redirect")).unwrap();
        let backend = FsBackend::open(root.path()).unwrap();

        assert!(backend.get("redirect/secret.seg").is_err());
        assert!(backend.put("redirect/new.seg", b"replacement").is_err());
        assert!(backend.delete("redirect/secret.seg").is_err());
        assert!(backend.list("redirect/").unwrap().is_empty());
        assert_eq!(fs::read(&outside_file).unwrap(), b"secret");
    }

    #[cfg(unix)]
    #[test]
    fn replaced_root_cannot_redirect_operations() {
        use std::os::unix::fs::symlink;

        let parent = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let root = parent.path().join("remote");
        fs::create_dir(&root).unwrap();
        let backend = FsBackend::open(&root).unwrap();
        fs::rename(&root, parent.path().join("original")).unwrap();
        symlink(outside.path(), &root).unwrap();

        assert!(backend.put("escape.bin", b"no").is_err());
        assert!(backend.get("escape.bin").is_err());
        assert!(!outside.path().join("escape.bin").exists());
    }

    #[cfg(unix)]
    #[test]
    fn replaced_regular_root_is_rejected() {
        let parent = TempDir::new().unwrap();
        let root = parent.path().join("remote");
        fs::create_dir(&root).unwrap();
        let backend = FsBackend::open(&root).unwrap();
        fs::rename(&root, parent.path().join("original")).unwrap();
        fs::create_dir(&root).unwrap();

        assert!(backend.put("unexpected.bin", b"no").is_err());
        assert!(backend.get("unexpected.bin").is_err());
        assert!(backend.list("").is_err());
    }
}
