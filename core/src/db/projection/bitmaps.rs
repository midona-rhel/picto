//! In-memory roaring bitmap store.
//!
//! Bitmaps are derived artifacts, not authoritative data. They are rebuilt
//! from authoritative tables on open and maintained incrementally by the
//! compilers while running. Nothing here is persisted.

use roaring::RoaringBitmap;
use std::collections::HashMap;
use std::sync::RwLock;

/// In-memory grouping of bitmap kinds (used to scope rebuilds).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BitmapCategory {
    Status,
    Tags,
    Folders,
}

/// Keys that identify a specific bitmap within a category.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BitmapKey {
    Status(i64),
    Tag(i64),
    ImpliedTag(i64),
    EffectiveTag(i64),
    Folder(i64),
    SmartFolder(i64),
    Tagged,
}

impl BitmapKey {
    pub fn category(&self) -> BitmapCategory {
        match self {
            BitmapKey::Status(_) | BitmapKey::Tagged => BitmapCategory::Status,
            BitmapKey::Tag(_) | BitmapKey::ImpliedTag(_) | BitmapKey::EffectiveTag(_) => {
                BitmapCategory::Tags
            }
            BitmapKey::Folder(_) | BitmapKey::SmartFolder(_) => BitmapCategory::Folders,
        }
    }
}

/// In-memory bitmap store. Fully derived: rebuilt from authoritative tables
/// on open and maintained incrementally by the compilers while running.
pub struct BitmapStore {
    bitmaps: RwLock<HashMap<BitmapKey, RoaringBitmap>>,
}

impl BitmapStore {
    pub fn new() -> Self {
        Self {
            bitmaps: RwLock::new(HashMap::new()),
        }
    }

    /// Get a bitmap by key. Returns empty bitmap if not found.
    pub fn get(&self, key: &BitmapKey) -> RoaringBitmap {
        let bitmaps = self.bitmaps.read().unwrap();
        bitmaps.get(key).cloned().unwrap_or_default()
    }

    /// Get the count of entries in a bitmap.
    pub fn len(&self, key: &BitmapKey) -> u64 {
        let bitmaps = self.bitmaps.read().unwrap();
        bitmaps.get(key).map(|b| b.len()).unwrap_or(0)
    }

    /// Replace a bitmap entirely (used by compilers during rebuild).
    pub fn set(&self, key: BitmapKey, bitmap: RoaringBitmap) {
        let mut bitmaps = self.bitmaps.write().unwrap();
        bitmaps.insert(key, bitmap);
    }

    /// Insert an entity_id into a bitmap.
    pub fn insert(&self, key: &BitmapKey, entity_id: u32) {
        let mut bitmaps = self.bitmaps.write().unwrap();
        bitmaps
            .entry(key.clone())
            .or_insert_with(RoaringBitmap::new)
            .insert(entity_id);
    }

    /// Remove an entity_id from a bitmap.
    pub fn remove(&self, key: &BitmapKey, entity_id: u32) {
        let mut bitmaps = self.bitmaps.write().unwrap();
        if let Some(bm) = bitmaps.get_mut(key) {
            bm.remove(entity_id);
        }
    }

    /// Remove deleted entities from every derived projection in one write lock.
    pub fn remove_entities(&self, entity_ids: &[i64]) {
        let removed = RoaringBitmap::from_iter(
            entity_ids
                .iter()
                .filter_map(|entity_id| u32::try_from(*entity_id).ok()),
        );
        if removed.is_empty() {
            return;
        }

        let mut bitmaps = self.bitmaps.write().unwrap();
        for bitmap in bitmaps.values_mut() {
            *bitmap -= &removed;
        }
    }

    /// Clear all bitmaps (for full rebuild).
    pub fn clear(&self) {
        let mut bitmaps = self.bitmaps.write().unwrap();
        bitmaps.clear();
    }
}

impl Default for BitmapStore {
    fn default() -> Self {
        Self::new()
    }
}
