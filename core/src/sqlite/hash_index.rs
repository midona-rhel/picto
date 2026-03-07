//! Bidirectional LRU cache mapping hex SHA256 hashes ↔ integer file_ids.
//!
//! All commands accept hex hashes at the API boundary.
//! Internally we use dense integer file_ids for joins and bitmaps.

use lru::LruCache;
use parking_lot::RwLock;
use std::num::NonZeroUsize;

const DEFAULT_CAPACITY: usize = 50_000;

struct HashIndexInner {
    forward: LruCache<String, i64>,
    reverse: LruCache<i64, String>,
}

pub struct HashIndex {
    inner: RwLock<HashIndexInner>,
}

impl HashIndex {
    pub fn new() -> Self {
        let cap = NonZeroUsize::new(DEFAULT_CAPACITY).unwrap();
        Self {
            inner: RwLock::new(HashIndexInner {
                forward: LruCache::new(cap),
                reverse: LruCache::new(cap),
            }),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        Self {
            inner: RwLock::new(HashIndexInner {
                forward: LruCache::new(cap),
                reverse: LruCache::new(cap),
            }),
        }
    }

    pub fn insert(&self, hash: String, file_id: i64) {
        let mut inner = self.inner.write();
        inner.forward.put(hash.clone(), file_id);
        inner.reverse.put(file_id, hash);
    }

    pub fn get_id(&self, hash: &str) -> Option<i64> {
        // LruCache::get() promotes the entry, requiring write access
        self.inner.write().forward.get(hash).copied()
    }

    pub fn get_hash(&self, file_id: i64) -> Option<String> {
        self.inner.write().reverse.get(&file_id).cloned()
    }

    pub fn remove_by_hash(&self, hash: &str) -> Option<i64> {
        let mut inner = self.inner.write();
        let file_id = inner.forward.pop(hash);
        if let Some(id) = file_id {
            inner.reverse.pop(&id);
        }
        file_id
    }

    pub fn clear(&self) {
        let mut inner = self.inner.write();
        inner.forward.clear();
        inner.reverse.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let idx = HashIndex::new();
        idx.insert("abc123".into(), 42);
        assert_eq!(idx.get_id("abc123"), Some(42));
        assert_eq!(idx.get_hash(42), Some("abc123".into()));
    }

    #[test]
    fn remove_by_hash() {
        let idx = HashIndex::new();
        idx.insert("abc123".into(), 42);
        assert_eq!(idx.remove_by_hash("abc123"), Some(42));
        assert_eq!(idx.get_id("abc123"), None);
        assert_eq!(idx.get_hash(42), None);
    }

    #[test]
    fn eviction() {
        let idx = HashIndex::with_capacity(2);
        idx.insert("a".into(), 1);
        idx.insert("b".into(), 2);
        idx.insert("c".into(), 3); // evicts "a"
        assert_eq!(idx.get_id("a"), None);
        assert_eq!(idx.get_id("b"), Some(2));
        assert_eq!(idx.get_id("c"), Some(3));
    }
}
