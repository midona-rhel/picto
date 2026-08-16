//! The storage backend contract for immutable sync objects. Google Drive,
//! Dropbox, S3, WebDAV, or a local directory all fit;
//! nothing above this trait may depend on backend-specific behavior.

use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum BackendError {
    #[error("object already exists: {0}")]
    AlreadyExists(String),
    #[error("backend IO error: {0}")]
    Io(String),
    #[error("backend limit exceeded: {0}")]
    LimitExceeded(String),
}

pub trait SyncBackend: Send + Sync {
    /// Store an immutable object. Fails with `AlreadyExists` if the key is
    /// taken — remote objects are write-once by contract.
    fn put(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError>;
    /// Atomically replace device-local coordination metadata. Truth objects
    /// continue to use create-only `put`.
    fn put_replace(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError>;
    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, BackendError>;
    /// Read at most `max_bytes`; oversized objects fail without being fully
    /// materialized in memory.
    fn get_limited(&self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, BackendError>;
    /// List keys under a prefix, in lexicographic order.
    fn list(&self, prefix: &str) -> Result<Vec<String>, BackendError>;
    /// List immediate child directories under a prefix. This is the bounded
    /// discovery path used by periodic sync instead of listing every object.
    fn list_directories(
        &self,
        prefix: &str,
        max_results: usize,
    ) -> Result<Vec<String>, BackendError>;
    /// Remove an object. Only backend cleanup may call this.
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

    fn put_replace(&self, key: &str, bytes: &[u8]) -> Result<(), BackendError> {
        self.objects
            .lock()
            .unwrap()
            .insert(key.to_string(), bytes.to_vec());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Vec<u8>>, BackendError> {
        Ok(self.objects.lock().unwrap().get(key).cloned())
    }

    fn get_limited(&self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, BackendError> {
        let objects = self.objects.lock().unwrap();
        let Some(bytes) = objects.get(key) else {
            return Ok(None);
        };
        if bytes.len() > max_bytes {
            return Err(BackendError::LimitExceeded(format!(
                "object {key} exceeds {max_bytes} bytes"
            )));
        }
        Ok(Some(bytes.clone()))
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

    fn list_directories(
        &self,
        prefix: &str,
        max_results: usize,
    ) -> Result<Vec<String>, BackendError> {
        let objects = self.objects.lock().unwrap();
        let mut children = std::collections::BTreeSet::new();
        for key in objects.keys().filter(|key| key.starts_with(prefix)) {
            let rest = &key[prefix.len()..];
            let Some((child, _)) = rest.split_once('/') else {
                continue;
            };
            if child.is_empty() {
                continue;
            }
            children.insert(child.to_owned());
            if children.len() > max_results {
                return Err(BackendError::LimitExceeded(format!(
                    "more than {max_results} directories under {prefix}"
                )));
            }
        }
        Ok(children.into_iter().collect())
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
    fn coordination_metadata_can_be_replaced() {
        let backend = MemoryBackend::new();
        backend.put_replace("oplog/a/head", b"1").unwrap();
        backend.put_replace("oplog/a/head", b"2").unwrap();
        assert_eq!(backend.get("oplog/a/head").unwrap().unwrap(), b"2");
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
        assert_eq!(
            backend.list_directories("oplog/", 10).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn bounded_reads_and_directory_lists_reject_oversized_inputs() {
        let backend = MemoryBackend::new();
        backend.put("oplog/a/1.seg", b"too large").unwrap();
        assert!(matches!(
            backend.get_limited("oplog/a/1.seg", 3),
            Err(BackendError::LimitExceeded(_))
        ));
        assert!(matches!(
            backend.list_directories("oplog/", 0),
            Err(BackendError::LimitExceeded(_))
        ));
    }
}
