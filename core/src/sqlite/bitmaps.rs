//! Roaring bitmap store for fast set operations on file_ids.
//!
//! Bitmaps are the core acceleration structure — status checks, tag membership,
//! folder membership, and smart folder compilation all reduce to bitmap ops.
//!
//! Persisted via a snapshot file (`bitmaps.bin`) plus an append-only WAL
//! (`bitmaps.wal`) that captures only dirty-key deltas between snapshots.
//! On flush, only changed bitmaps are appended to the WAL. Compaction writes
//! a fresh full snapshot and removes the WAL.

use roaring::RoaringBitmap;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read as _, Seek as _, Write as _};
use std::path::{Path, PathBuf};
use std::sync::RwLock;

/// Key identifying a specific bitmap in the store.
///
/// Design notes:
/// - `Tag` vs `ImpliedTag` vs `EffectiveTag`: three bitmaps per tag enables efficient updates.
///   When a file is directly tagged, only `Tag(id)` changes. When parent relationships change,
///   only `ImpliedTag(id)` is recomputed. `EffectiveTag(id)` is the union and is what queries use.
/// - `Tagged` exists as a precomputed union of all tagged file_ids to make the "untagged" view
///   a simple `Status(1) - Tagged` bitmap operation instead of a full-table scan.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BitmapKey {
    /// Files with a given status (0=inbox, 1=active, 2=trash)
    Status(i64),
    /// No longer written by new code paths. Kept for backward-compatible
    /// deserialization of legacy bitmaps.bin files (tag byte = 1).
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

/// Compaction threshold — when the WAL exceeds this size, the next flush
/// triggers a full snapshot instead of another WAL append.
const WAL_COMPACT_THRESHOLD: u64 = 2 * 1024 * 1024; // 2 MB

pub struct BitmapStore {
    bitmaps: RwLock<HashMap<BitmapKey, RoaringBitmap>>,
    dirty_keys: RwLock<HashSet<BitmapKey>>,
    /// true when `clear()` was called — next flush must write a full snapshot
    /// because the WAL cannot represent "delete all keys".
    full_rewrite_needed: RwLock<bool>,
    dir: PathBuf,
    path: RwLock<PathBuf>,
}

impl BitmapStore {
    #[cfg(test)]
    fn open(dir: &Path) -> Self {
        Self::open_with_active_file(dir, None)
    }

    pub fn open_with_active_file(dir: &Path, active_file: Option<&str>) -> Self {
        let requested_path = active_file
            .map(|name| dir.join(name))
            .unwrap_or_else(|| dir.join("bitmaps.bin"));

        // Load snapshot
        let mut bitmaps = if requested_path.exists() {
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

        // Replay WAL on top of the snapshot
        let wal_path = wal_path_for_snapshot(&requested_path);
        if wal_path.exists() {
            match Self::replay_wal(&wal_path) {
                Ok(wal_entries) => {
                    let count = wal_entries.len();
                    for (key, bitmap) in wal_entries {
                        bitmaps.insert(key, bitmap);
                    }
                    if count > 0 {
                        tracing::info!("Replayed {count} WAL entries from {:?}", wal_path);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to replay WAL {:?}: {}, ignoring", wal_path, e);
                }
            }
        }

        Self {
            bitmaps: RwLock::new(bitmaps),
            dirty_keys: RwLock::new(HashSet::new()),
            full_rewrite_needed: RwLock::new(false),
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
        self.mark_dirty(&key);
        crate::poison::write_or_recover(&self.bitmaps, "bitmaps::set").insert(key, bitmap);
    }

    pub fn insert(&self, key: &BitmapKey, file_id: u32) {
        let mut map = crate::poison::write_or_recover(&self.bitmaps, "bitmaps::insert");
        map.entry(key.clone()).or_default().insert(file_id);
        self.mark_dirty(key);
    }

    pub fn remove(&self, key: &BitmapKey, file_id: u32) {
        let mut map = crate::poison::write_or_recover(&self.bitmaps, "bitmaps::remove");
        if let Some(bm) = map.get_mut(key) {
            bm.remove(file_id);
            self.mark_dirty(key);
        }
    }

    pub fn clear(&self) {
        crate::poison::write_or_recover(&self.bitmaps, "bitmaps::clear").clear();
        crate::poison::write_or_recover(&self.dirty_keys, "bitmaps::dirty_keys").clear();
        *crate::poison::write_or_recover(&self.full_rewrite_needed, "bitmaps::full_rewrite")
            = true;
    }

    pub fn remove_key(&self, key: &BitmapKey) {
        crate::poison::write_or_recover(&self.bitmaps, "bitmaps::remove_key").remove(key);
        self.mark_dirty(key);
    }

    pub fn is_dirty(&self) -> bool {
        let has_dirty = !crate::poison::read_or_recover(&self.dirty_keys, "bitmaps::is_dirty")
            .is_empty();
        has_dirty || *crate::poison::read_or_recover(&self.full_rewrite_needed, "bitmaps::is_dirty_full")
    }

    /// Flush dirty bitmaps. Appends only changed keys to the WAL file,
    /// unless a compaction is warranted (WAL too large or full rewrite needed).
    pub fn flush(&self) -> io::Result<()> {
        let needs_full = *crate::poison::read_or_recover(
            &self.full_rewrite_needed,
            "bitmaps::flush_check",
        );

        let dirty: HashSet<BitmapKey> = {
            let mut dk = crate::poison::write_or_recover(&self.dirty_keys, "bitmaps::flush_drain");
            std::mem::take(&mut *dk)
        };

        if dirty.is_empty() && !needs_full {
            return Ok(());
        }

        if needs_full {
            self.compact_inner(&dirty)?;
            return Ok(());
        }

        // Check if WAL is too large → compact instead
        let snapshot_path: PathBuf =
            crate::poison::read_or_recover(&self.path, "bitmaps::flush_path").clone();
        let wal = wal_path_for_snapshot(&snapshot_path);
        let wal_size = fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);

        if wal_size >= WAL_COMPACT_THRESHOLD {
            self.compact_inner(&dirty)?;
        } else {
            self.append_wal(&dirty)?;
        }

        Ok(())
    }

    /// Flush as a versioned snapshot — always compacts (produces a clean
    /// snapshot for the manifest, no WAL).
    pub fn flush_versioned(&self, artifact_version: u64) -> io::Result<String> {
        let file_name = format!("bitmaps.v{artifact_version}.bin");
        let old_path: PathBuf =
            crate::poison::read_or_recover(&self.path, "bitmaps::fv_old").clone();

        let new_path = self.dir.join(&file_name);
        *crate::poison::write_or_recover(&self.path, "bitmaps::path") = new_path;

        // Drain dirty keys and compact
        let dirty: HashSet<BitmapKey> = {
            let mut dk =
                crate::poison::write_or_recover(&self.dirty_keys, "bitmaps::fv_drain");
            std::mem::take(&mut *dk)
        };
        self.compact_inner(&dirty)?;

        // Clean up the old snapshot's WAL if it exists
        let old_wal = wal_path_for_snapshot(&old_path);
        let _ = fs::remove_file(&old_wal);

        Ok(file_name)
    }

    /// Remove stale bitmap artifact files from disk.
    ///
    /// Keeps only explicitly-listed files (e.g. current + previous manifest file).
    /// Cleanup is best-effort: unreadable entries are skipped and individual delete
    /// failures are returned as an error to the caller.
    pub fn prune_artifacts(&self, keep_files: &[String]) -> io::Result<usize> {
        let keep: std::collections::HashSet<&str> =
            keep_files.iter().map(String::as_str).collect();
        let mut deleted = 0usize;
        let mut first_error: Option<io::Error> = None;

        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) => return Err(e),
        };

        for entry in entries {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if !is_bitmap_artifact_file(name) || keep.contains(name) {
                continue;
            }
            let path = entry.path();
            if path.is_file() {
                if let Err(e) = fs::remove_file(&path) {
                    if first_error.is_none() {
                        first_error = Some(e);
                    }
                } else {
                    deleted += 1;
                }
            }
        }

        if let Some(err) = first_error {
            return Err(err);
        }
        Ok(deleted)
    }

    // ── internal ────────────────────────────────────────────────────────

    fn mark_dirty(&self, key: &BitmapKey) {
        crate::poison::write_or_recover(&self.dirty_keys, "bitmaps::mark_dirty")
            .insert(key.clone());
    }

    /// Remove empty bitmaps from the store to prevent unbounded growth.
    /// Called during compaction.
    fn prune_empty(&self) {
        let mut map = crate::poison::write_or_recover(&self.bitmaps, "bitmaps::prune_empty");
        map.retain(|_, bm| !bm.is_empty());
    }

    /// Append only the dirty keys' bitmaps to the WAL file.
    fn append_wal(&self, dirty: &HashSet<BitmapKey>) -> io::Result<()> {
        if dirty.is_empty() {
            return Ok(());
        }

        let snapshot_path: PathBuf =
            crate::poison::read_or_recover(&self.path, "bitmaps::wal_path").clone();
        let wal = wal_path_for_snapshot(&snapshot_path);

        let map = crate::poison::read_or_recover(&self.bitmaps, "bitmaps::wal_read");

        // Build the WAL entry buffer
        let mut buf = Vec::new();
        let mut entry_count = 0u32;

        for key in dirty {
            let key_bytes = serialize_key(key);
            let key_len = key_bytes.len() as u32;

            if let Some(bitmap) = map.get(key) {
                let bm_size = bitmap.serialized_size();
                buf.extend_from_slice(&key_len.to_le_bytes());
                buf.extend_from_slice(&key_bytes);
                buf.extend_from_slice(&(bm_size as u64).to_le_bytes());
                let start = buf.len();
                buf.resize(start + bm_size, 0);
                bitmap.serialize_into(&mut buf[start..]).map_err(|e| {
                    io::Error::new(io::ErrorKind::Other, format!("bitmap serialize: {e}"))
                })?;
            } else {
                // Key was removed — write an empty bitmap so replay overwrites
                let empty = RoaringBitmap::new();
                let bm_size = empty.serialized_size();
                buf.extend_from_slice(&key_len.to_le_bytes());
                buf.extend_from_slice(&key_bytes);
                buf.extend_from_slice(&(bm_size as u64).to_le_bytes());
                let start = buf.len();
                buf.resize(start + bm_size, 0);
                empty.serialize_into(&mut buf[start..]).map_err(|e| {
                    io::Error::new(io::ErrorKind::Other, format!("bitmap serialize: {e}"))
                })?;
            }
            entry_count += 1;
        }

        // Read existing entry count, append, update header
        let file_existed = wal.exists();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&wal)?;

        let existing_count = if file_existed {
            let mut header = [0u8; 4];
            if file.read_exact(&mut header).is_ok() {
                u32::from_le_bytes(header)
            } else {
                0
            }
        } else {
            // Write initial header
            file.write_all(&0u32.to_le_bytes())?;
            0
        };

        // Append entries at the end
        file.seek(io::SeekFrom::End(0))?;
        file.write_all(&buf)?;

        // Update entry count in the header
        let new_count = existing_count + entry_count;
        file.seek(io::SeekFrom::Start(0))?;
        file.write_all(&new_count.to_le_bytes())?;

        file.sync_all()?;

        Ok(())
    }

    /// Write a full snapshot and delete the WAL.
    /// Prunes empty bitmaps to prevent unbounded growth.
    fn compact_inner(&self, _remaining_dirty: &HashSet<BitmapKey>) -> io::Result<()> {
        self.prune_empty();
        self.save_to_file()?;

        // Clear full-rewrite flag
        *crate::poison::write_or_recover(
            &self.full_rewrite_needed,
            "bitmaps::compact_clear",
        ) = false;

        // Remove WAL
        let snapshot_path: PathBuf =
            crate::poison::read_or_recover(&self.path, "bitmaps::compact_wal").clone();
        let wal = wal_path_for_snapshot(&snapshot_path);
        let _ = fs::remove_file(&wal);

        Ok(())
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

    /// Parse WAL entries. Returns key→bitmap pairs in order; later entries
    /// for the same key naturally overwrite earlier ones when inserted into
    /// a HashMap.
    fn replay_wal(wal_path: &Path) -> io::Result<Vec<(BitmapKey, RoaringBitmap)>> {
        let data = fs::read(wal_path)?;

        if data.len() < 4 {
            return Ok(Vec::new());
        }

        let entry_count = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
        let mut pos = 4;
        let mut entries = Vec::with_capacity(entry_count);

        for _ in 0..entry_count {
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
                    // Skip bitmap data
                    if pos + 8 <= data.len() {
                        let bm_size =
                            u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap()) as usize;
                        pos += 8 + bm_size;
                    }
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
                Ok(bm) => entries.push((key, bm)),
                Err(e) => {
                    tracing::warn!("Skipping corrupt WAL entry: {e}");
                }
            }
            pos += bm_size;
        }

        Ok(entries)
    }
}

/// Derive the WAL path from a snapshot path: `bitmaps.bin` → `bitmaps.wal`,
/// `bitmaps.v5.bin` → `bitmaps.v5.wal`.
fn wal_path_for_snapshot(snapshot: &Path) -> PathBuf {
    let stem = snapshot
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("bitmaps");
    snapshot.with_file_name(format!("{stem}.wal"))
}

fn is_bitmap_artifact_file(name: &str) -> bool {
    if name == "bitmaps.bin" {
        return true;
    }
    if name.starts_with("bitmaps.v") && name.ends_with(".bin") {
        return true;
    }
    if name.ends_with(".wal")
        && (name == "bitmaps.wal" || name.starts_with("bitmaps.v"))
    {
        return true;
    }
    false
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

    #[test]
    fn wal_append_and_replay() {
        let dir = tempfile::tempdir().unwrap();

        // First flush: creates snapshot via WAL
        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.insert(&BitmapKey::Tag(1), 20);
            store.insert(&BitmapKey::Status(1), 10);
            store.flush().unwrap();
        }

        // WAL should exist (first flush with small data goes to WAL)
        let wal = dir.path().join("bitmaps.wal");
        assert!(wal.exists(), "WAL file should exist after flush");

        // Second flush: appends more deltas to WAL
        {
            let store = BitmapStore::open(dir.path());
            // Verify replayed state
            assert_eq!(store.len(&BitmapKey::Tag(1)), 2);
            assert_eq!(store.len(&BitmapKey::Status(1)), 1);

            // Add more data
            store.insert(&BitmapKey::Tag(1), 30);
            store.insert(&BitmapKey::Tag(99), 5);
            store.flush().unwrap();
        }

        // Reopen and verify everything is there
        {
            let store = BitmapStore::open(dir.path());
            assert_eq!(store.len(&BitmapKey::Tag(1)), 3);
            assert!(store.contains(&BitmapKey::Tag(1), 30));
            assert_eq!(store.len(&BitmapKey::Tag(99)), 1);
            assert_eq!(store.len(&BitmapKey::Status(1)), 1);
        }
    }

    #[test]
    fn compaction_produces_clean_snapshot() {
        let dir = tempfile::tempdir().unwrap();

        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.insert(&BitmapKey::Folder(5), 20);
            store.flush().unwrap();

            // Force compaction via flush_versioned
            store.insert(&BitmapKey::Tag(1), 30);
            let name = store.flush_versioned(1).unwrap();
            assert_eq!(name, "bitmaps.v1.bin");
        }

        // The versioned snapshot should exist, WAL for it should not
        assert!(dir.path().join("bitmaps.v1.bin").exists());
        assert!(!dir.path().join("bitmaps.v1.wal").exists());

        // Reopen from the versioned snapshot
        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some("bitmaps.v1.bin"));
            assert_eq!(store.len(&BitmapKey::Tag(1)), 2);
            assert!(store.contains(&BitmapKey::Tag(1), 10));
            assert!(store.contains(&BitmapKey::Tag(1), 30));
            assert_eq!(store.len(&BitmapKey::Folder(5)), 1);
        }
    }

    #[test]
    fn clear_triggers_full_rewrite() {
        let dir = tempfile::tempdir().unwrap();

        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.flush().unwrap();
        }

        {
            let store = BitmapStore::open(dir.path());
            assert_eq!(store.len(&BitmapKey::Tag(1)), 1);

            store.clear();
            store.insert(&BitmapKey::Tag(2), 20);
            store.flush().unwrap();
        }

        // The old WAL should have been cleaned up by compaction
        let wal = dir.path().join("bitmaps.wal");
        assert!(!wal.exists(), "WAL should be removed after clear+flush compaction");

        {
            let store = BitmapStore::open(dir.path());
            assert_eq!(store.len(&BitmapKey::Tag(1)), 0);
            assert_eq!(store.len(&BitmapKey::Tag(2)), 1);
        }
    }

    #[test]
    fn removed_key_replays_as_empty() {
        let dir = tempfile::tempdir().unwrap();

        // Create initial state with a snapshot
        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.insert(&BitmapKey::Tag(1), 20);
            // Force a snapshot so we have baseline on disk
            *crate::poison::write_or_recover(&store.full_rewrite_needed, "test") = true;
            store.flush().unwrap();
        }

        // Remove the key, flush as WAL
        {
            let store = BitmapStore::open(dir.path());
            assert_eq!(store.len(&BitmapKey::Tag(1)), 2);
            store.remove_key(&BitmapKey::Tag(1));
            store.flush().unwrap();
        }

        // Reopen — WAL should replay the empty bitmap over the snapshot
        {
            let store = BitmapStore::open(dir.path());
            assert_eq!(store.len(&BitmapKey::Tag(1)), 0);
        }
    }

    #[test]
    fn compaction_prunes_empty_bitmaps() {
        let dir = tempfile::tempdir().unwrap();

        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.insert(&BitmapKey::Tag(2), 20);
            // Remove all values from Tag(2), leaving it empty
            store.remove(&BitmapKey::Tag(2), 20);
            // Force compaction via flush_versioned
            let _ = store.flush_versioned(1).unwrap();
        }

        // Reopen — empty bitmap should have been pruned
        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some("bitmaps.v1.bin"));
            assert_eq!(store.len(&BitmapKey::Tag(1)), 1);
            // Tag(2) should not exist at all (pruned during compaction)
            let map = crate::poison::read_or_recover(&store.bitmaps, "test");
            assert!(!map.contains_key(&BitmapKey::Tag(2)));
        }
    }

    #[test]
    fn prune_artifacts_keeps_only_requested_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bitmaps.bin"), b"legacy").unwrap();
        fs::write(dir.path().join("bitmaps.v1.bin"), b"v1").unwrap();
        fs::write(dir.path().join("bitmaps.v2.bin"), b"v2").unwrap();
        fs::write(dir.path().join("bitmaps.v3.bin"), b"v3").unwrap();
        fs::write(dir.path().join("bitmaps.v1.wal"), b"wal1").unwrap();
        fs::write(dir.path().join("not-bitmaps.txt"), b"keep").unwrap();

        let store = BitmapStore::open(dir.path());
        let deleted = store
            .prune_artifacts(&["bitmaps.v3.bin".to_string(), "bitmaps.v2.bin".to_string()])
            .unwrap();
        assert_eq!(deleted, 3); // bitmaps.bin + v1.bin + v1.wal
        assert!(dir.path().join("bitmaps.v3.bin").exists());
        assert!(dir.path().join("bitmaps.v2.bin").exists());
        assert!(!dir.path().join("bitmaps.v1.bin").exists());
        assert!(!dir.path().join("bitmaps.v1.wal").exists());
        assert!(!dir.path().join("bitmaps.bin").exists());
        assert!(dir.path().join("not-bitmaps.txt").exists());
    }

    #[test]
    fn wal_path_derivation() {
        assert_eq!(
            wal_path_for_snapshot(Path::new("/lib/bitmaps.bin")),
            PathBuf::from("/lib/bitmaps.wal")
        );
        assert_eq!(
            wal_path_for_snapshot(Path::new("/lib/bitmaps.v5.bin")),
            PathBuf::from("/lib/bitmaps.v5.wal")
        );
    }
}
