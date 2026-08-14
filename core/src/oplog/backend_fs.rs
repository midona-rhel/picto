//! Filesystem backend: a directory as the remote. Pointing this at a folder
//! synced by Google Drive for Desktop (or Dropbox, or any file-sync client)
//! gives real multi-device sync without any provider API — the sync client
//! is just transport for immutable files.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::backend::{BackendError, SyncBackend};

pub struct FsBackend {
    root: PathBuf,
}

impl FsBackend {
    pub fn open(root: &Path) -> Result<Self, BackendError> {
        fs::create_dir_all(root).map_err(|e| BackendError::Io(e.to_string()))?;
        let root = fs::canonicalize(root).map_err(|e| BackendError::Io(e.to_string()))?;
        Ok(Self { root })
    }

    fn path_for(&self, key: &str) -> Result<PathBuf, BackendError> {
        // Keys are forward-slash object names; reject traversal.
        if key
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == ".." || part.contains('\\'))
        {
            return Err(BackendError::Io(format!("invalid object key: {key}")));
        }
        Ok(self.root.join(key))
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

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        validate_list_prefix(prefix)?;
        let mut keys = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
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
}
