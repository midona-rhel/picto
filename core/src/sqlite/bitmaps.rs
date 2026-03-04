//! Roaring bitmap store for fast set operations on file_ids.
//!
//! Bitmaps are the core acceleration structure — status checks, tag membership,
//! folder membership, and smart folder compilation all reduce to bitmap ops.
//!
//! Persisted to a sidecar file (`bitmaps.bin`), fully rebuildable from SQL.

use roaring::RoaringBitmap;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

/// Key identifying a specific bitmap in the store.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BitmapKey {
    /// Files with a given status (0=inbox, 1=active, 2=trash)
    Status(i64),
    /// Union of Status(0) | Status(1) — all non-trash files
    AllActive,
    /// Files directly tagged with tag_id
    Tag(i64),
    /// Files with tag_id via parent inheritance
    ImpliedTag(i64),
    /// Tag(id) | ImpliedTag(id) — effective tag membership
    EffectiveTag(i64),
    /// Files in a folder
    Folder(i64),
    /// Compiled smart folder result
    SmartFolder(i64),
    /// Union of all tagged file_ids — files that have at least one effective tag
    Tagged,
}

pub struct BitmapStore {
    bitmaps: RwLock<HashMap<BitmapKey, RoaringBitmap>>,
    dirty: AtomicBool,
    dir: PathBuf,
    path: RwLock<PathBuf>,
}

impl BitmapStore {
    pub fn open(dir: &Path) -> Self {
        Self::open_with_active_file(dir, None)
    }

    pub fn open_with_active_file(dir: &Path, active_file: Option<&str>) -> Self {
        let requested_path = active_file
            .map(|name| dir.join(name))
            .unwrap_or_else(|| dir.join("bitmaps.bin"));
        let bitmaps = if requested_path.exists() {
            match Self::load_from_file(&requested_path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        "Failed to load bitmaps from {:?}: {}, starting fresh",
                        requested_path,
                        e
                    );
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };

        Self {
            bitmaps: RwLock::new(bitmaps),
            dirty: AtomicBool::new(false),
            dir: dir.to_path_buf(),
            path: RwLock::new(requested_path),
        }
    }

    pub fn get(&self, key: &BitmapKey) -> RoaringBitmap {
        self.bitmaps
            .read()
            .unwrap()
            .get(key)
            .cloned()
            .unwrap_or_default()
    }

    pub fn len(&self, key: &BitmapKey) -> u64 {
        self.bitmaps
            .read()
            .unwrap()
            .get(key)
            .map(|b| b.len())
            .unwrap_or(0)
    }

    pub fn contains(&self, key: &BitmapKey, file_id: u32) -> bool {
        self.bitmaps
            .read()
            .unwrap()
            .get(key)
            .map(|b| b.contains(file_id))
            .unwrap_or(false)
    }

    pub fn set(&self, key: BitmapKey, bitmap: RoaringBitmap) {
        crate::poison::write_or_recover(&self.bitmaps, "bitmaps::set").insert(key, bitmap);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn insert(&self, key: &BitmapKey, file_id: u32) {
        let mut map = crate::poison::write_or_recover(&self.bitmaps, "bitmaps::insert");
        map.entry(key.clone()).or_default().insert(file_id);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn remove(&self, key: &BitmapKey, file_id: u32) {
        let mut map = crate::poison::write_or_recover(&self.bitmaps, "bitmaps::remove");
        if let Some(bm) = map.get_mut(key) {
            bm.remove(file_id);
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    pub fn clear(&self) {
        crate::poison::write_or_recover(&self.bitmaps, "bitmaps::clear").clear();
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn remove_key(&self, key: &BitmapKey) {
        crate::poison::write_or_recover(&self.bitmaps, "bitmaps::remove_key").remove(key);
        self.dirty.store(true, Ordering::Relaxed);
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    pub fn flush(&self) -> io::Result<()> {
        if !self.dirty.load(Ordering::Relaxed) {
            return Ok(());
        }
        self.save_to_file()?;
        self.dirty.store(false, Ordering::Relaxed);
        Ok(())
    }

    pub fn flush_versioned(&self, artifact_version: u64) -> io::Result<String> {
        let file_name = format!("bitmaps.v{artifact_version}.bin");
        let new_path = self.dir.join(&file_name);
        *crate::poison::write_or_recover(&self.path, "bitmaps::path") = new_path;
        self.flush()?;
        Ok(file_name)
    }

    fn save_to_file(&self) -> io::Result<()> {
        let map = crate::poison::read_or_recover(&self.bitmaps, "bitmaps::save");
        let path: PathBuf = crate::poison::read_or_recover(&self.path, "bitmaps::path").clone();
        let mut buf = Vec::new();

        let count = map.len() as u64;
        buf.extend_from_slice(&count.to_le_bytes());

        for (key, bitmap) in map.iter() {
            let key_bytes = serialize_key(key);
            let key_len = key_bytes.len() as u32;
            buf.extend_from_slice(&key_len.to_le_bytes());
            buf.extend_from_slice(&key_bytes);

            let bm_size = bitmap.serialized_size();
            buf.extend_from_slice(&(bm_size as u64).to_le_bytes());
            let start = buf.len();
            buf.resize(start + bm_size, 0);
            bitmap.serialize_into(&mut buf[start..]).map_err(|e| {
                io::Error::new(io::ErrorKind::Other, format!("bitmap serialize: {e}"))
            })?;
        }

        let tmp_path = path.with_extension("bin.tmp");
        let mut file = fs::File::create(&tmp_path)?;
        file.write_all(&buf)?;
        file.sync_all()?;
        fs::rename(&tmp_path, &path)?;

        Ok(())
    }

    fn load_from_file(path: &Path) -> io::Result<HashMap<BitmapKey, RoaringBitmap>> {
        let data = fs::read(path)?;
        let mut pos = 0;

        if data.len() < 8 {
            return Ok(HashMap::new());
        }

        let count = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
        pos += 8;

        let mut map = HashMap::with_capacity(count as usize);

        for _ in 0..count {
            if pos + 4 > data.len() {
                break;
            }
            let key_len = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;

            if pos + key_len > data.len() {
                break;
            }
            let key = match deserialize_key(&data[pos..pos + key_len]) {
                Some(k) => k,
                None => {
                    pos += key_len;
                    continue;
                }
            };
            pos += key_len;

            if pos + 8 > data.len() {
                break;
            }
            let bm_size = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap()) as usize;
            pos += 8;

            if pos + bm_size > data.len() {
                break;
            }
            match RoaringBitmap::deserialize_from(&data[pos..pos + bm_size]) {
                Ok(bm) => {
                    map.insert(key, bm);
                }
                Err(e) => {
                    tracing::warn!("Skipping corrupt bitmap entry: {e}");
                }
            }
            pos += bm_size;
        }

        Ok(map)
    }
}

// Key serialization: tag byte + i64 payload (where applicable)
fn serialize_key(key: &BitmapKey) -> Vec<u8> {
    let mut buf = Vec::with_capacity(9);
    match key {
        BitmapKey::Status(v) => {
            buf.push(0);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::AllActive => {
            buf.push(1);
        }
        BitmapKey::Tag(v) => {
            buf.push(2);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::ImpliedTag(v) => {
            buf.push(3);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::EffectiveTag(v) => {
            buf.push(4);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::Folder(v) => {
            buf.push(5);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::SmartFolder(v) => {
            buf.push(6);
            buf.extend_from_slice(&v.to_le_bytes());
        }
        BitmapKey::Tagged => {
            buf.push(7);
        }
    }
    buf
}

fn deserialize_key(data: &[u8]) -> Option<BitmapKey> {
    if data.is_empty() {
        return None;
    }
    let tag = data[0];
    let read_i64 = |d: &[u8]| -> Option<i64> {
        if d.len() < 9 {
            None
        } else {
            Some(i64::from_le_bytes(d[1..9].try_into().unwrap()))
        }
    };
    match tag {
        0 => read_i64(data).map(BitmapKey::Status),
        1 => Some(BitmapKey::AllActive),
        2 => read_i64(data).map(BitmapKey::Tag),
        3 => read_i64(data).map(BitmapKey::ImpliedTag),
        4 => read_i64(data).map(BitmapKey::EffectiveTag),
        5 => read_i64(data).map(BitmapKey::Folder),
        6 => read_i64(data).map(BitmapKey::SmartFolder),
        7 => Some(BitmapKey::Tagged),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_round_trip() {
        let keys = vec![
            BitmapKey::Status(0),
            BitmapKey::Status(2),
            BitmapKey::AllActive,
            BitmapKey::Tag(42),
            BitmapKey::ImpliedTag(100),
            BitmapKey::EffectiveTag(200),
            BitmapKey::Folder(5),
            BitmapKey::SmartFolder(7),
            BitmapKey::Tagged,
        ];
        for key in keys {
            let serialized = serialize_key(&key);
            let deserialized = deserialize_key(&serialized).unwrap();
            assert_eq!(key, deserialized);
        }
    }

    #[test]
    fn bitmap_ops() {
        let dir = tempfile::tempdir().unwrap();
        let store = BitmapStore::open(dir.path());

        store.insert(&BitmapKey::Status(0), 1);
        store.insert(&BitmapKey::Status(0), 5);
        store.insert(&BitmapKey::Status(0), 10);

        assert_eq!(store.len(&BitmapKey::Status(0)), 3);
        assert!(store.contains(&BitmapKey::Status(0), 5));
        assert!(!store.contains(&BitmapKey::Status(0), 6));

        store.remove(&BitmapKey::Status(0), 5);
        assert_eq!(store.len(&BitmapKey::Status(0)), 2);
    }

    #[test]
    fn persistence_round_trip() {
        let dir = tempfile::tempdir().unwrap();

        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(42), 1);
            store.insert(&BitmapKey::Tag(42), 2);
            store.insert(&BitmapKey::Tag(42), 100);
            store.insert(&BitmapKey::Status(0), 50);
            store.flush().unwrap();
        }

        {
            let store = BitmapStore::open(dir.path());
            assert_eq!(store.len(&BitmapKey::Tag(42)), 3);
            assert!(store.contains(&BitmapKey::Tag(42), 100));
            assert_eq!(store.len(&BitmapKey::Status(0)), 1);
        }
    }
}
