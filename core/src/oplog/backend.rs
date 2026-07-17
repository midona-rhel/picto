//! The storage backend contract for sync: four operations over immutable
//! objects. Google Drive, Dropbox, S3, WebDAV, or a local directory all fit;
//! nothing above this trait may depend on backend-specific behavior.

use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum BackendError {
    #[error("object already exists: {0}")]
    AlreadyExists(String),
    #[error("backend IO error: {0}")]
    Io(String),
}

pub trait SyncBackend: Send + Sync {
    /// Store an immutable object. Fails with `AlreadyExists` if the key is
    /// taken — remote objects are write-once by contract.
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, BackendError>;
    /// List keys under a prefix, in lexicographic order.
    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError>;
    /// Remove an object. Only garbage collection may call this.
    fn delete(&self, key: &str) -> Result<(), BackendError>;
}

/// In-memory backend for tests and the local replay engine.
#[derive(Default)]
pub struct MemoryBackend {
    objects: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SyncBackend for MemoryBackend {
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        let mut objects = self.objects.lock().unwrap();
        if objects.contains_key(key) {
            return Err(BackendError::AlreadyExists(key.to_string()));
        }
        objects.insert(key.to_string(), bytes.to_vec());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, BackendError> {
        Ok(self.objects.lock().unwrap().get(key).cloned())
    }

    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError> {
        Ok(self
            .objects
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    fn delete(&self, key: &str) -> Result<(), BackendError> {
        self.objects.lock().unwrap().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_is_create_only() {
        let backend = MemoryBackend::new();
        backend.put("a/1", b"x").unwrap();
        assert!(matches!(
            backend.put("a/1", b"y"),
            Err(BackendError::AlreadyExists(_))
        ));
        assert_eq!(backend.get("a/1").unwrap().unwrap(), b"x");
    }

    #[test]
    fn list_is_prefix_scoped_and_ordered() {
        let backend = MemoryBackend::new();
        backend.put("oplog/b/2", b"").unwrap();
        backend.put("oplog/a/1", b"").unwrap();
        backend.put("blobs/x", b"").unwrap();
        assert_eq!(
            backend.list("oplog/").unwrap(),
            vec!["oplog/a/1".to_string(), "oplog/b/2".to_string()]
        );
    }
}
