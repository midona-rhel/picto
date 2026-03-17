/// Cached snapshot of a filtered scope — avoids rebuilding temp id-sets on
/// consecutive page fetches for the same scope+filter+sort combination.
#[derive(Debug, Clone)]
pub struct ScopeSnapshot {
    pub ids: Vec<i64>,
    pub total_count: i64,
    pub created_at: std::time::Instant,
}

/// Key for the scope snapshot cache.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ScopeSnapshotKey {
    pub scope: String,
    pub predicate_hash: u64,
    pub sort_field: String,
    pub sort_dir: String,
}

use super::SqliteDatabase;

impl SqliteDatabase {
    const SCOPE_CACHE_MAX_ENTRIES: usize = 64;
    const SCOPE_CACHE_TTL: std::time::Duration = std::time::Duration::from_secs(30);

    pub fn scope_cache_get(&self, key: &ScopeSnapshotKey) -> Option<ScopeSnapshot> {
        let cache = crate::poison::read_or_recover(&self.scope_cache, "scope_cache::get");
        cache.get(key).and_then(|snap| {
            if snap.created_at.elapsed() < Self::SCOPE_CACHE_TTL {
                Some(snap.clone())
            } else {
                None
            }
        })
    }

    pub fn scope_cache_put(&self, key: ScopeSnapshotKey, snapshot: ScopeSnapshot) {
        let mut cache = crate::poison::write_or_recover(&self.scope_cache, "scope_cache::put");
        if cache.len() >= Self::SCOPE_CACHE_MAX_ENTRIES {
            cache.retain(|_, v| v.created_at.elapsed() < Self::SCOPE_CACHE_TTL);
        }
        if cache.len() >= Self::SCOPE_CACHE_MAX_ENTRIES {
            if let Some(oldest_key) = cache
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _): (&ScopeSnapshotKey, _)| k.clone())
            {
                cache.remove(&oldest_key);
            }
        }
        cache.insert(key, snapshot);
    }

    pub fn scope_cache_invalidate_all(&self) {
        let mut cache =
            crate::poison::write_or_recover(&self.scope_cache, "scope_cache::invalidate_all");
        cache.clear();
    }

    pub fn scope_cache_invalidate_scope(&self, scope_prefix: &str) {
        let mut cache =
            crate::poison::write_or_recover(&self.scope_cache, "scope_cache::invalidate_scope");
        cache.retain(|k, _| !k.scope.starts_with(scope_prefix));
    }
}
