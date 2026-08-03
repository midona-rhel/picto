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
        Ok(Self {
            root: root.to_path_buf(),
        })
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
}

impl SyncBackend for FsBackend {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        let path = self.path_for(key)?;
        if path.exists() {
            return Err(BackendError::AlreadyExists(key.to_string()));
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| BackendError::Io(e.to_string()))?;
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
        match fs::read(&path) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BackendError::Io(e.to_string())),
        }
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        let mut keys = Vec::new();
        let mut stack = vec![self.root.clone()];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.is_file() {
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
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(BackendError::Io(e.to_string())),
        }
    }
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
    }
}
