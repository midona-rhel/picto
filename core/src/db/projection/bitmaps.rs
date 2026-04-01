//! Roaring bitmap store with delta log.
//!
//! Bitmaps are derived artifacts, not authoritative data. They can be
//! rebuilt from authoritative tables at any time. Runtime writes append
//! deltas instead of rewriting snapshots eagerly.

use roaring::RoaringBitmap;
use std::collections::HashMap;
use std::sync::RwLock;

/// Categories for independent snapshot/delta files.
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
    CollectionMember,
}

impl BitmapKey {
    pub fn category(&self) -> BitmapCategory {
        match self {
            BitmapKey::Status(_) | BitmapKey::Tagged | BitmapKey::CollectionMember => {
                BitmapCategory::Status
            }
            BitmapKey::Tag(_) | BitmapKey::ImpliedTag(_) | BitmapKey::EffectiveTag(_) => {
                BitmapCategory::Tags
            }
            BitmapKey::Folder(_) | BitmapKey::SmartFolder(_) => BitmapCategory::Folders,
        }
    }
}

/// A single delta entry: insert or remove an entity from a bitmap.
#[derive(Debug, Clone)]
pub struct BitmapDelta {
    pub key: BitmapKey,
    pub entity_id: u32,
    pub insert: bool,
}

/// In-memory bitmap store. Snapshots are loaded on startup, deltas are
/// applied in-memory and appended to the delta log for persistence.
pub struct BitmapStore {
    bitmaps: RwLock<HashMap<BitmapKey, RoaringBitmap>>,
    pending_deltas: RwLock<Vec<BitmapDelta>>,
}

impl BitmapStore {
    pub fn new() -> Self {
        Self {
            bitmaps: RwLock::new(HashMap::new()),
            pending_deltas: RwLock::new(Vec::new()),
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

    /// Insert an entity_id into a bitmap and record the delta.
    pub fn insert(&self, key: &BitmapKey, entity_id: u32) {
        {
            let mut bitmaps = self.bitmaps.write().unwrap();
            bitmaps
                .entry(key.clone())
                .or_insert_with(RoaringBitmap::new)
                .insert(entity_id);
        }
        self.pending_deltas.write().unwrap().push(BitmapDelta {
            key: key.clone(),
            entity_id,
            insert: true,
        });
    }

    /// Remove an entity_id from a bitmap and record the delta.
    pub fn remove(&self, key: &BitmapKey, entity_id: u32) {
        {
            let mut bitmaps = self.bitmaps.write().unwrap();
            if let Some(bm) = bitmaps.get_mut(key) {
                bm.remove(entity_id);
            }
        }
        self.pending_deltas.write().unwrap().push(BitmapDelta {
            key: key.clone(),
            entity_id,
            insert: false,
        });
    }

    /// Drain pending deltas (for persistence to delta log file).
    pub fn drain_deltas(&self) -> Vec<BitmapDelta> {
        let mut deltas = self.pending_deltas.write().unwrap();
        std::mem::take(&mut *deltas)
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
