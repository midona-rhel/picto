//! Roaring bitmap store for fast set operations on file_ids.
//!
//! Bitmaps are the core acceleration structure — status checks, tag membership,
//! folder membership, and smart folder compilation all reduce to bitmap ops.
//!
//! Persisted as per-category files under `db/bitmaps/`:
//!   - `status.bin`  + `status.wal`   (Status, Tagged)
//!   - `tags.bin`    + `tags.wal`     (Tag, ImpliedTag, EffectiveTag)
//!   - `folders.bin` + `folders.wal`  (Folder, SmartFolder)
//!
//! Each category has its own WAL and compaction cycle, so flushing tags
//! (the largest category) doesn't rewrite status or folder bitmaps.
//!
//! Migration from single-file format (`bitmaps.bin`) is automatic on first open.

use roaring::RoaringBitmap;
use serde_json::json;
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
    /// Entities that are members of a collection (parent_collection_id IS NOT NULL).
    /// Used to exclude members from sidebar counts.
    CollectionMember,
}

/// Compaction threshold — when the WAL exceeds this size, the next flush
/// triggers a full snapshot instead of another WAL append.
const WAL_COMPACT_THRESHOLD: u64 = 2 * 1024 * 1024; // 2 MB

/// Which category a bitmap key belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum BitmapCategory {
    Status,
    Tags,
    Folders,
}

impl BitmapCategory {
    const ALL: [BitmapCategory; 3] = [
        BitmapCategory::Status,
        BitmapCategory::Tags,
        BitmapCategory::Folders,
    ];

    fn file_stem(self) -> &'static str {
        match self {
            BitmapCategory::Status => "status",
            BitmapCategory::Tags => "tags",
            BitmapCategory::Folders => "folders",
        }
    }
}

fn category_of(key: &BitmapKey) -> BitmapCategory {
    match key {
        BitmapKey::Status(_) | BitmapKey::Tagged => BitmapCategory::Status,
        BitmapKey::Tag(_) | BitmapKey::ImpliedTag(_) | BitmapKey::EffectiveTag(_) => {
            BitmapCategory::Tags
        }
        BitmapKey::Folder(_) | BitmapKey::SmartFolder(_) => BitmapCategory::Folders,
        BitmapKey::CollectionMember => BitmapCategory::Status,
    }
}

/// Per-category active file names, parsed from manifest payload.
struct CategoryFiles {
    status: String,
    tags: String,
    folders: String,
}

impl CategoryFiles {
    fn for_category(&self, cat: BitmapCategory) -> &str {
        match cat {
            BitmapCategory::Status => &self.status,
            BitmapCategory::Tags => &self.tags,
            BitmapCategory::Folders => &self.folders,
        }
    }

    /// Collect all filenames into a set for pruning.
    fn all_files(&self) -> HashSet<String> {
        [&self.status, &self.tags, &self.folders]
            .into_iter()
            .map(|s| s.clone())
            .collect()
    }
}

/// Parse a manifest payload JSON into per-category file names.
///
/// Handles both formats:
/// - New: `{"format":"per_category","status":"status.v3.bin","tags":"tags.v7.bin","folders":"folders.v5.bin"}`
/// - Legacy: `{"active_file":"bitmaps.v5.bin"}` → derives per-category names from shared version
fn parse_bitmap_payload(payload: &str) -> Option<CategoryFiles> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;

    if v.get("format").and_then(|f| f.as_str()) == Some("per_category") {
        Some(CategoryFiles {
            status: v.get("status").and_then(|s| s.as_str())?.to_string(),
            tags: v.get("tags").and_then(|s| s.as_str())?.to_string(),
            folders: v.get("folders").and_then(|s| s.as_str())?.to_string(),
        })
    } else if let Some(active_file) = v.get("active_file").and_then(|s| s.as_str()) {
        // Legacy shared-version format
        let version = parse_version_from_active_file(active_file);
        Some(CategoryFiles {
            status: category_snapshot_filename(BitmapCategory::Status, version),
            tags: category_snapshot_filename(BitmapCategory::Tags, version),
            folders: category_snapshot_filename(BitmapCategory::Folders, version),
        })
    } else {
        None
    }
}

/// Extract the legacy active_file name from a manifest payload JSON.
fn parse_legacy_active_file(payload: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(payload).ok()?;
    v.get("active_file")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string())
}

/// Build the per-category manifest payload JSON from current category state.
fn build_per_category_payload(cats: &HashMap<BitmapCategory, CategoryStore>) -> String {
    let get_filename = |cat: BitmapCategory| -> &str {
        cats.get(&cat)
            .and_then(|s| s.snapshot_path.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.bin")
    };

    json!({
        "format": "per_category",
        "status": get_filename(BitmapCategory::Status),
        "tags": get_filename(BitmapCategory::Tags),
        "folders": get_filename(BitmapCategory::Folders),
    })
    .to_string()
}

/// Build a snapshot filename for a category with optional version.
fn category_snapshot_filename(cat: BitmapCategory, version: Option<u64>) -> String {
    match version {
        Some(v) => format!("{}.v{v}.bin", cat.file_stem()),
        None => format!("{}.bin", cat.file_stem()),
    }
}

/// Per-category storage: bitmaps, dirty tracking, and file paths.
struct CategoryStore {
    bitmaps: HashMap<BitmapKey, RoaringBitmap>,
    dirty_keys: HashSet<BitmapKey>,
    full_rewrite_needed: bool,
    snapshot_path: PathBuf,
}

impl CategoryStore {
    fn new(snapshot_path: PathBuf) -> Self {
        Self {
            bitmaps: HashMap::new(),
            dirty_keys: HashSet::new(),
            full_rewrite_needed: false,
            snapshot_path,
        }
    }

    fn is_dirty(&self) -> bool {
        !self.dirty_keys.is_empty() || self.full_rewrite_needed
    }
}

pub struct BitmapStore {
    categories: RwLock<HashMap<BitmapCategory, CategoryStore>>,
    /// Parent directory for per-category files (e.g. `db/bitmaps/`).
    dir: PathBuf,
}

impl BitmapStore {
    #[cfg(test)]
    fn open(dir: &Path) -> Self {
        Self::open_with_active_file(dir, None)
    }

    /// Open the bitmap store.
    ///
    /// `payload` is the raw manifest payload JSON for the "bitmaps" artifact.
    /// It can be:
    /// - `None` → fresh library or test usage
    /// - Per-category JSON: `{"format":"per_category","status":"status.v3.bin",...}`
    /// - Legacy JSON: `{"active_file":"bitmaps.v5.bin"}`
    /// - Bare filename: `"bitmaps.v5.bin"` (backward compat for tests)
    pub fn open_with_active_file(dir: &Path, payload: Option<&str>) -> Self {
        let bitmaps_dir = dir.join("bitmaps");

        // Parse payload into per-category file names
        let cat_files = payload.and_then(parse_bitmap_payload);

        // If per-category directory exists, load from it
        if bitmaps_dir.is_dir() {
            return Self::open_per_category(&bitmaps_dir, cat_files.as_ref());
        }

        // Check for legacy single-file format
        let legacy_filename = payload.and_then(|p| {
            parse_legacy_active_file(p).or_else(|| {
                // Bare filename (tests, backward compat)
                if p.ends_with(".bin") {
                    Some(p.to_string())
                } else {
                    None
                }
            })
        });
        let legacy_path = legacy_filename
            .as_ref()
            .map(|name| dir.join(name))
            .unwrap_or_else(|| dir.join("bitmaps.bin"));

        let legacy_wal = wal_path_for_snapshot(&legacy_path);
        let has_legacy = legacy_path.exists() || legacy_wal.exists();

        if has_legacy {
            return Self::migrate_from_legacy(dir, &legacy_path, &bitmaps_dir);
        }

        // Fresh library — create empty per-category stores
        Self::open_fresh(&bitmaps_dir)
    }

    fn open_fresh(bitmaps_dir: &Path) -> Self {
        let _ = fs::create_dir_all(bitmaps_dir);
        let mut categories = HashMap::new();
        for cat in BitmapCategory::ALL {
            let path = bitmaps_dir.join(format!("{}.bin", cat.file_stem()));
            categories.insert(cat, CategoryStore::new(path));
        }
        Self {
            categories: RwLock::new(categories),
            dir: bitmaps_dir.to_path_buf(),
        }
    }

    fn open_per_category(bitmaps_dir: &Path, cat_files: Option<&CategoryFiles>) -> Self {
        let mut categories = HashMap::new();
        for cat in BitmapCategory::ALL {
            let snapshot_path = match cat_files {
                Some(files) => bitmaps_dir.join(files.for_category(cat)),
                None => bitmaps_dir.join(format!("{}.bin", cat.file_stem())),
            };
            let mut bitmaps = if snapshot_path.exists() {
                match load_from_file(&snapshot_path) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(
                            "Failed to load {} bitmaps from {:?}: {}, starting fresh",
                            cat.file_stem(),
                            snapshot_path,
                            e
                        );
                        HashMap::new()
                    }
                }
            } else {
                HashMap::new()
            };

            // Replay WAL
            let wal = wal_path_for_snapshot(&snapshot_path);
            if wal.exists() {
                match replay_wal(&wal) {
                    Ok(entries) => {
                        let count = entries.len();
                        for (key, bitmap) in entries {
                            bitmaps.insert(key, bitmap);
                        }
                        if count > 0 {
                            tracing::info!(
                                "Replayed {count} WAL entries for {} from {:?}",
                                cat.file_stem(),
                                wal
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "Failed to replay {} WAL {:?}: {}, ignoring",
                            cat.file_stem(),
                            wal,
                            e
                        );
                    }
                }
            }

            categories.insert(
                cat,
                CategoryStore {
                    bitmaps,
                    dirty_keys: HashSet::new(),
                    full_rewrite_needed: false,
                    snapshot_path,
                },
            );
        }

        Self {
            categories: RwLock::new(categories),
            dir: bitmaps_dir.to_path_buf(),
        }
    }

    fn migrate_from_legacy(db_dir: &Path, legacy_path: &Path, bitmaps_dir: &Path) -> Self {
        tracing::info!("Migrating single-file bitmaps to per-category format");

        // Load all bitmaps from legacy file
        let mut all_bitmaps = if legacy_path.exists() {
            match load_from_file(legacy_path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(
                        "Failed to load legacy bitmaps from {:?}: {}, starting fresh",
                        legacy_path,
                        e
                    );
                    HashMap::new()
                }
            }
        } else {
            HashMap::new()
        };

        // Replay legacy WAL
        let legacy_wal = wal_path_for_snapshot(legacy_path);
        if legacy_wal.exists() {
            match replay_wal(&legacy_wal) {
                Ok(entries) => {
                    for (key, bitmap) in entries {
                        all_bitmaps.insert(key, bitmap);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to replay legacy WAL: {}, ignoring", e);
                }
            }
        }

        // Split by category
        let _ = fs::create_dir_all(bitmaps_dir);
        let mut categories = HashMap::new();
        for cat in BitmapCategory::ALL {
            let path = bitmaps_dir.join(format!("{}.bin", cat.file_stem()));
            categories.insert(cat, CategoryStore::new(path));
        }

        for (key, bitmap) in all_bitmaps {
            let cat = category_of(&key);
            if let Some(store) = categories.get_mut(&cat) {
                store.bitmaps.insert(key, bitmap);
            }
        }

        // Save each category to disk
        for cat in BitmapCategory::ALL {
            if let Some(store) = categories.get(&cat) {
                if !store.bitmaps.is_empty() {
                    if let Err(e) = save_to_file(&store.bitmaps, &store.snapshot_path) {
                        tracing::warn!(
                            "Failed to write {} category during migration: {}",
                            cat.file_stem(),
                            e
                        );
                    }
                }
            }
        }

        // Clean up legacy files
        let _ = fs::remove_file(legacy_path);
        let _ = fs::remove_file(&legacy_wal);

        // Also clean up any versioned legacy files in the db_dir
        if let Ok(entries) = fs::read_dir(db_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if is_legacy_bitmap_artifact(name) {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
        }

        tracing::info!("Migration to per-category bitmaps complete");

        Self {
            categories: RwLock::new(categories),
            dir: bitmaps_dir.to_path_buf(),
        }
    }

    // ── public API (unchanged signatures) ──────────────────────────────

    pub fn get(&self, key: &BitmapKey) -> RoaringBitmap {
        let cat = category_of(key);
        let cats = crate::poison::read_or_recover(&self.categories, "bitmaps::get");
        cats.get(&cat)
            .and_then(|s| s.bitmaps.get(key))
            .cloned()
            .unwrap_or_default()
    }

    pub fn len(&self, key: &BitmapKey) -> u64 {
        let cat = category_of(key);
        let cats = crate::poison::read_or_recover(&self.categories, "bitmaps::len");
        cats.get(&cat)
            .and_then(|s| s.bitmaps.get(key))
            .map(|b| b.len())
            .unwrap_or(0)
    }

    pub fn contains(&self, key: &BitmapKey, file_id: u32) -> bool {
        let cat = category_of(key);
        let cats = crate::poison::read_or_recover(&self.categories, "bitmaps::contains");
        cats.get(&cat)
            .and_then(|s| s.bitmaps.get(key))
            .map(|b| b.contains(file_id))
            .unwrap_or(false)
    }

    pub fn set(&self, key: BitmapKey, bitmap: RoaringBitmap) {
        let cat = category_of(&key);
        let mut cats = crate::poison::write_or_recover(&self.categories, "bitmaps::set");
        if let Some(store) = cats.get_mut(&cat) {
            store.dirty_keys.insert(key.clone());
            store.bitmaps.insert(key, bitmap);
        }
    }

    pub fn insert(&self, key: &BitmapKey, file_id: u32) {
        let cat = category_of(key);
        let mut cats = crate::poison::write_or_recover(&self.categories, "bitmaps::insert");
        if let Some(store) = cats.get_mut(&cat) {
            store
                .bitmaps
                .entry(key.clone())
                .or_default()
                .insert(file_id);
            store.dirty_keys.insert(key.clone());
        }
    }

    pub fn remove(&self, key: &BitmapKey, file_id: u32) {
        let cat = category_of(key);
        let mut cats = crate::poison::write_or_recover(&self.categories, "bitmaps::remove");
        if let Some(store) = cats.get_mut(&cat) {
            if let Some(bm) = store.bitmaps.get_mut(key) {
                bm.remove(file_id);
                store.dirty_keys.insert(key.clone());
            }
        }
    }

    pub fn clear(&self) {
        let mut cats = crate::poison::write_or_recover(&self.categories, "bitmaps::clear");
        for store in cats.values_mut() {
            store.bitmaps.clear();
            store.dirty_keys.clear();
            store.full_rewrite_needed = true;
        }
    }

    pub fn remove_key(&self, key: &BitmapKey) {
        let cat = category_of(key);
        let mut cats = crate::poison::write_or_recover(&self.categories, "bitmaps::remove_key");
        if let Some(store) = cats.get_mut(&cat) {
            store.bitmaps.remove(key);
            store.dirty_keys.insert(key.clone());
        }
    }

    pub fn is_dirty(&self) -> bool {
        let cats = crate::poison::read_or_recover(&self.categories, "bitmaps::is_dirty");
        cats.values().any(|s| s.is_dirty())
    }

    /// Flush dirty bitmaps. Each category is flushed independently —
    /// only categories with dirty keys are written.
    pub fn flush(&self) -> io::Result<()> {
        let mut cats = crate::poison::write_or_recover(&self.categories, "bitmaps::flush");

        for cat in BitmapCategory::ALL {
            let store = match cats.get_mut(&cat) {
                Some(s) => s,
                None => continue,
            };

            let needs_full = store.full_rewrite_needed;
            let dirty: HashSet<BitmapKey> = std::mem::take(&mut store.dirty_keys);

            if dirty.is_empty() && !needs_full {
                continue;
            }

            if needs_full {
                compact_category(store)?;
                continue;
            }

            // Check WAL size for this category
            let wal = wal_path_for_snapshot(&store.snapshot_path);
            let wal_size = fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);

            if wal_size >= WAL_COMPACT_THRESHOLD {
                compact_category(store)?;
            } else {
                append_wal_for_category(store, &dirty)?;
            }
        }

        Ok(())
    }

    /// Flush as versioned snapshots. Only compacts categories that have
    /// dirty keys or an existing WAL — clean categories keep their current
    /// snapshot file untouched.
    ///
    /// Returns a JSON payload string describing the per-category active files,
    /// suitable for storing directly in the manifest.
    pub fn flush_versioned(&self, artifact_version: u64) -> io::Result<String> {
        let mut cats = crate::poison::write_or_recover(&self.categories, "bitmaps::fv");

        for cat in BitmapCategory::ALL {
            let store = match cats.get_mut(&cat) {
                Some(s) => s,
                None => continue,
            };

            let has_wal = wal_path_for_snapshot(&store.snapshot_path).exists();
            let needs_write = store.is_dirty() || has_wal;

            if !needs_write {
                continue;
            }

            // Clean up old WAL before switching paths
            let old_wal = wal_path_for_snapshot(&store.snapshot_path);
            let _ = fs::remove_file(&old_wal);

            // Update snapshot path to versioned name
            let new_path = self
                .dir
                .join(format!("{}.v{artifact_version}.bin", cat.file_stem()));
            store.snapshot_path = new_path;

            // Drain dirty keys and compact
            store.dirty_keys.clear();
            compact_category(store)?;
        }

        Ok(build_per_category_payload(&cats))
    }

    /// Remove stale bitmap artifact files from disk.
    ///
    /// `keep_payloads` contains manifest payload JSON strings (per-category
    /// or legacy format). The exact filenames referenced are kept; everything
    /// else in the bitmaps directory is deleted.
    ///
    /// Also cleans up any legacy single-file artifacts from the parent db/ dir.
    pub fn prune_artifacts(&self, keep_payloads: &[String]) -> io::Result<usize> {
        // Extract all filenames to keep from the payload strings
        let mut keep_files: HashSet<String> = HashSet::new();
        for payload in keep_payloads {
            if let Some(cat_files) = parse_bitmap_payload(payload) {
                keep_files.extend(cat_files.all_files());
            }
        }

        let mut deleted = 0usize;
        let mut first_error: Option<io::Error> = None;

        // Prune per-category directory
        if self.dir.is_dir() {
            if let Ok(entries) = fs::read_dir(&self.dir) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let Some(name) = file_name.to_str() else {
                        continue;
                    };
                    if !is_category_artifact_file(name) || keep_files.contains(name) {
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
            }
        }

        // Also clean up legacy single-file artifacts from parent dir
        if let Some(parent) = self.dir.parent() {
            if let Ok(entries) = fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name();
                    let Some(name) = file_name.to_str() else {
                        continue;
                    };
                    if is_legacy_bitmap_artifact(name) {
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
                }
            }
        }

        if let Some(err) = first_error {
            return Err(err);
        }
        Ok(deleted)
    }
}

// ── Per-category flush helpers ─────────────────────────────────────────

/// Write a full snapshot for a single category and delete its WAL.
fn compact_category(store: &mut CategoryStore) -> io::Result<()> {
    // Prune empty bitmaps
    store.bitmaps.retain(|_, bm| !bm.is_empty());

    save_to_file(&store.bitmaps, &store.snapshot_path)?;

    store.full_rewrite_needed = false;

    // Remove WAL
    let wal = wal_path_for_snapshot(&store.snapshot_path);
    let _ = fs::remove_file(&wal);

    Ok(())
}

/// Append dirty keys to this category's WAL file.
fn append_wal_for_category(store: &CategoryStore, dirty: &HashSet<BitmapKey>) -> io::Result<()> {
    if dirty.is_empty() {
        return Ok(());
    }

    let wal = wal_path_for_snapshot(&store.snapshot_path);

    let mut buf = Vec::new();
    let mut entry_count = 0u32;

    for key in dirty {
        let key_bytes = serialize_key(key);
        let key_len = key_bytes.len() as u32;

        let bitmap = store.bitmaps.get(key).cloned().unwrap_or_default();

        let bm_size = bitmap.serialized_size();
        buf.extend_from_slice(&key_len.to_le_bytes());
        buf.extend_from_slice(&key_bytes);
        buf.extend_from_slice(&(bm_size as u64).to_le_bytes());
        let start = buf.len();
        buf.resize(start + bm_size, 0);
        bitmap
            .serialize_into(&mut buf[start..])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("bitmap serialize: {e}")))?;
        entry_count += 1;
    }

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
        file.write_all(&0u32.to_le_bytes())?;
        0
    };

    file.seek(io::SeekFrom::End(0))?;
    file.write_all(&buf)?;

    let new_count = existing_count + entry_count;
    file.seek(io::SeekFrom::Start(0))?;
    file.write_all(&new_count.to_le_bytes())?;

    file.sync_all()?;

    Ok(())
}

// ── File format helpers ────────────────────────────────────────────────

fn save_to_file(map: &HashMap<BitmapKey, RoaringBitmap>, path: &Path) -> io::Result<()> {
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
        bitmap
            .serialize_into(&mut buf[start..])
            .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("bitmap serialize: {e}")))?;
    }

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let tmp_path = path.with_extension("bin.tmp");
    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(&buf)?;
    file.sync_all()?;
    fs::rename(&tmp_path, path)?;

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

// ── Path helpers ───────────────────────────────────────────────────────

/// Derive the WAL path from a snapshot path: `status.bin` → `status.wal`,
/// `tags.v5.bin` → `tags.v5.wal`.
fn wal_path_for_snapshot(snapshot: &Path) -> PathBuf {
    let stem = snapshot
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    snapshot.with_file_name(format!("{stem}.wal"))
}

/// Parse version number from the canonical active_file name.
/// e.g. "bitmaps.v5.bin" → Some(5)
fn parse_version_from_active_file(name: &str) -> Option<u64> {
    let name = name.strip_prefix("bitmaps.v")?;
    let name = name.strip_suffix(".bin")?;
    name.parse().ok()
}

/// Parse version from a per-category file name.
/// e.g. "tags.v5.bin" → Some(5), "status.bin" → None
#[cfg(test)]
fn parse_version_from_category_file(name: &str) -> Option<u64> {
    // Pattern: <stem>.v<N>.bin or <stem>.v<N>.wal
    let name = name
        .strip_suffix(".bin")
        .or_else(|| name.strip_suffix(".wal"))?;
    let dot_v = name.rfind(".v")?;
    let version_str = &name[dot_v + 2..];
    version_str.parse().ok()
}

/// Check if a file in the per-category directory is a bitmap artifact.
fn is_category_artifact_file(name: &str) -> bool {
    for stem in ["status", "tags", "folders"] {
        if name.starts_with(stem)
            && (name.ends_with(".bin") || name.ends_with(".wal") || name.ends_with(".bin.tmp"))
        {
            return true;
        }
    }
    false
}

/// Check if a file in the parent db/ directory is a legacy bitmap artifact.
fn is_legacy_bitmap_artifact(name: &str) -> bool {
    if name == "bitmaps.bin" || name == "bitmaps.wal" || name == "bitmaps.bin.tmp" {
        return true;
    }
    if name.starts_with("bitmaps.v") && (name.ends_with(".bin") || name.ends_with(".wal")) {
        return true;
    }
    false
}

// ── Key serialization ──────────────────────────────────────────────────

fn serialize_key(key: &BitmapKey) -> Vec<u8> {
    let mut buf = Vec::with_capacity(9);
    match key {
        BitmapKey::Status(v) => {
            buf.push(0);
            buf.extend_from_slice(&v.to_le_bytes());
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
        BitmapKey::CollectionMember => {
            buf.push(8);
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
        1 => None, // Legacy AllActive — skip on read
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
    fn per_category_file_layout() {
        let dir = tempfile::tempdir().unwrap();

        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Status(1), 10);
            store.insert(&BitmapKey::Tag(1), 20);
            store.insert(&BitmapKey::Folder(1), 30);
            // Force compaction so we get .bin files
            store.flush_versioned(1).unwrap();
        }

        let bitmaps_dir = dir.path().join("bitmaps");
        assert!(bitmaps_dir.join("status.v1.bin").exists());
        assert!(bitmaps_dir.join("tags.v1.bin").exists());
        assert!(bitmaps_dir.join("folders.v1.bin").exists());

        // No WAL files after versioned flush
        assert!(!bitmaps_dir.join("status.v1.wal").exists());
        assert!(!bitmaps_dir.join("tags.v1.wal").exists());
        assert!(!bitmaps_dir.join("folders.v1.wal").exists());
    }

    #[test]
    fn wal_append_and_replay() {
        let dir = tempfile::tempdir().unwrap();

        // First flush: creates WAL
        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.insert(&BitmapKey::Tag(1), 20);
            store.insert(&BitmapKey::Status(1), 10);
            store.flush().unwrap();
        }

        // WAL should exist for tags category
        let bitmaps_dir = dir.path().join("bitmaps");
        assert!(
            bitmaps_dir.join("tags.wal").exists(),
            "Tags WAL should exist after flush"
        );

        // Second flush: appends more
        {
            let store = BitmapStore::open(dir.path());
            assert_eq!(store.len(&BitmapKey::Tag(1)), 2);
            assert_eq!(store.len(&BitmapKey::Status(1)), 1);

            store.insert(&BitmapKey::Tag(1), 30);
            store.insert(&BitmapKey::Tag(99), 5);
            store.flush().unwrap();
        }

        // Reopen and verify
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

        let payload;
        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.insert(&BitmapKey::Folder(5), 20);
            store.flush().unwrap();

            store.insert(&BitmapKey::Tag(1), 30);
            payload = store.flush_versioned(1).unwrap();
            // Should be per-category JSON
            let parsed: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert_eq!(parsed["format"], "per_category");
        }

        // Reopen from the versioned snapshot using the payload
        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some(&payload));
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

        // WAL should be cleaned up by compaction
        let bitmaps_dir = dir.path().join("bitmaps");
        assert!(
            !bitmaps_dir.join("tags.wal").exists(),
            "WAL should be removed after clear+flush compaction"
        );

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
            // Force a snapshot via clear + re-insert
            {
                let mut cats = crate::poison::write_or_recover(&store.categories, "test");
                if let Some(s) = cats.get_mut(&BitmapCategory::Tags) {
                    s.full_rewrite_needed = true;
                }
            }
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

        let payload;
        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Tag(1), 10);
            store.insert(&BitmapKey::Tag(2), 20);
            store.remove(&BitmapKey::Tag(2), 20);
            payload = store.flush_versioned(1).unwrap();
        }

        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some(&payload));
            assert_eq!(store.len(&BitmapKey::Tag(1)), 1);
            // Tag(2) should not exist at all (pruned during compaction)
            let cats = crate::poison::read_or_recover(&store.categories, "test");
            let tag_store = cats.get(&BitmapCategory::Tags).unwrap();
            assert!(!tag_store.bitmaps.contains_key(&BitmapKey::Tag(2)));
        }
    }

    #[test]
    fn prune_artifacts_cleans_old_versions() {
        let dir = tempfile::tempdir().unwrap();
        let bitmaps_dir = dir.path().join("bitmaps");
        fs::create_dir_all(&bitmaps_dir).unwrap();

        // Create various versioned files
        fs::write(bitmaps_dir.join("status.v1.bin"), b"v1").unwrap();
        fs::write(bitmaps_dir.join("tags.v1.bin"), b"v1").unwrap();
        fs::write(bitmaps_dir.join("folders.v1.bin"), b"v1").unwrap();
        fs::write(bitmaps_dir.join("tags.v1.wal"), b"wal").unwrap();
        fs::write(bitmaps_dir.join("status.v2.bin"), b"v2").unwrap();
        fs::write(bitmaps_dir.join("tags.v2.bin"), b"v2").unwrap();
        fs::write(bitmaps_dir.join("folders.v2.bin"), b"v2").unwrap();
        fs::write(bitmaps_dir.join("status.v3.bin"), b"v3").unwrap();
        fs::write(bitmaps_dir.join("tags.v3.bin"), b"v3").unwrap();
        fs::write(bitmaps_dir.join("folders.v3.bin"), b"v3").unwrap();
        fs::write(bitmaps_dir.join("not-bitmaps.txt"), b"keep").unwrap();

        let store = BitmapStore::open(dir.path());
        // Use per-category payloads — keep v3 and v2 (shared version)
        let keep_v3 = json!({
            "format": "per_category",
            "status": "status.v3.bin",
            "tags": "tags.v3.bin",
            "folders": "folders.v3.bin"
        })
        .to_string();
        let keep_v2 = json!({
            "format": "per_category",
            "status": "status.v2.bin",
            "tags": "tags.v2.bin",
            "folders": "folders.v2.bin"
        })
        .to_string();
        let deleted = store.prune_artifacts(&[keep_v3, keep_v2]).unwrap();

        // v1 files (3 .bin + 1 .wal = 4) should be deleted
        assert_eq!(deleted, 4);
        assert!(bitmaps_dir.join("status.v3.bin").exists());
        assert!(bitmaps_dir.join("tags.v3.bin").exists());
        assert!(bitmaps_dir.join("folders.v3.bin").exists());
        assert!(bitmaps_dir.join("status.v2.bin").exists());
        assert!(!bitmaps_dir.join("status.v1.bin").exists());
        assert!(!bitmaps_dir.join("tags.v1.wal").exists());
        assert!(bitmaps_dir.join("not-bitmaps.txt").exists());
    }

    #[test]
    fn migration_from_legacy_single_file() {
        let dir = tempfile::tempdir().unwrap();

        // Create a legacy single-file store
        {
            let mut bitmaps = HashMap::new();
            let mut status_bm = RoaringBitmap::new();
            status_bm.insert(1);
            status_bm.insert(2);
            bitmaps.insert(BitmapKey::Status(1), status_bm);

            let mut tag_bm = RoaringBitmap::new();
            tag_bm.insert(10);
            tag_bm.insert(20);
            bitmaps.insert(BitmapKey::Tag(42), tag_bm);

            let mut folder_bm = RoaringBitmap::new();
            folder_bm.insert(100);
            bitmaps.insert(BitmapKey::Folder(5), folder_bm);

            save_to_file(&bitmaps, &dir.path().join("bitmaps.bin")).unwrap();
        }

        assert!(dir.path().join("bitmaps.bin").exists());
        assert!(!dir.path().join("bitmaps").is_dir());

        // Open — should trigger migration
        let store = BitmapStore::open(dir.path());

        // Verify data survived migration
        assert_eq!(store.len(&BitmapKey::Status(1)), 2);
        assert!(store.contains(&BitmapKey::Status(1), 1));
        assert_eq!(store.len(&BitmapKey::Tag(42)), 2);
        assert!(store.contains(&BitmapKey::Tag(42), 10));
        assert_eq!(store.len(&BitmapKey::Folder(5)), 1);

        // Legacy file should be cleaned up
        assert!(!dir.path().join("bitmaps.bin").exists());

        // Per-category directory should exist
        let bitmaps_dir = dir.path().join("bitmaps");
        assert!(bitmaps_dir.is_dir());
        assert!(bitmaps_dir.join("status.bin").exists());
        assert!(bitmaps_dir.join("tags.bin").exists());
        assert!(bitmaps_dir.join("folders.bin").exists());

        // Verify round-trip: flush and reopen
        store.insert(&BitmapKey::Tag(42), 30);
        store.flush().unwrap();
        drop(store);

        let store2 = BitmapStore::open(dir.path());
        assert_eq!(store2.len(&BitmapKey::Tag(42)), 3);
        assert_eq!(store2.len(&BitmapKey::Status(1)), 2);
        assert_eq!(store2.len(&BitmapKey::Folder(5)), 1);
    }

    #[test]
    fn only_dirty_category_wal_written() {
        let dir = tempfile::tempdir().unwrap();
        let bitmaps_dir = dir.path().join("bitmaps");

        {
            let store = BitmapStore::open(dir.path());
            // Only dirty the tags category
            store.insert(&BitmapKey::Tag(1), 10);
            store.flush().unwrap();
        }

        // Only tags WAL should exist
        assert!(
            bitmaps_dir.join("tags.wal").exists(),
            "Tags WAL should exist"
        );
        assert!(
            !bitmaps_dir.join("status.wal").exists(),
            "Status WAL should NOT exist (not dirty)"
        );
        assert!(
            !bitmaps_dir.join("folders.wal").exists(),
            "Folders WAL should NOT exist (not dirty)"
        );
    }

    #[test]
    fn wal_path_derivation() {
        assert_eq!(
            wal_path_for_snapshot(Path::new("/lib/bitmaps/status.bin")),
            PathBuf::from("/lib/bitmaps/status.wal")
        );
        assert_eq!(
            wal_path_for_snapshot(Path::new("/lib/bitmaps/tags.v5.bin")),
            PathBuf::from("/lib/bitmaps/tags.v5.wal")
        );
    }

    #[test]
    fn version_parsing() {
        assert_eq!(parse_version_from_active_file("bitmaps.v5.bin"), Some(5));
        assert_eq!(
            parse_version_from_active_file("bitmaps.v123.bin"),
            Some(123)
        );
        assert_eq!(parse_version_from_active_file("bitmaps.bin"), None);
        assert_eq!(parse_version_from_active_file("other.v5.bin"), None);

        assert_eq!(parse_version_from_category_file("tags.v5.bin"), Some(5));
        assert_eq!(parse_version_from_category_file("status.v12.wal"), Some(12));
        assert_eq!(parse_version_from_category_file("tags.bin"), None);
    }

    #[test]
    fn payload_parsing() {
        // Per-category format
        let payload = json!({
            "format": "per_category",
            "status": "status.v3.bin",
            "tags": "tags.v7.bin",
            "folders": "folders.v5.bin"
        })
        .to_string();
        let cat = parse_bitmap_payload(&payload).unwrap();
        assert_eq!(cat.status, "status.v3.bin");
        assert_eq!(cat.tags, "tags.v7.bin");
        assert_eq!(cat.folders, "folders.v5.bin");

        // Legacy format
        let legacy = json!({"active_file": "bitmaps.v5.bin"}).to_string();
        let cat = parse_bitmap_payload(&legacy).unwrap();
        assert_eq!(cat.status, "status.v5.bin");
        assert_eq!(cat.tags, "tags.v5.bin");
        assert_eq!(cat.folders, "folders.v5.bin");

        // Legacy unversioned
        let legacy_default = json!({"active_file": "bitmaps.bin"}).to_string();
        let cat = parse_bitmap_payload(&legacy_default).unwrap();
        assert_eq!(cat.status, "status.bin");
        assert_eq!(cat.tags, "tags.bin");
        assert_eq!(cat.folders, "folders.bin");
    }

    #[test]
    fn independent_versioning_only_dirty_categories_written() {
        let dir = tempfile::tempdir().unwrap();
        let bitmaps_dir = dir.path().join("bitmaps");

        // Initial: all categories dirty
        let payload1;
        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Status(1), 10);
            store.insert(&BitmapKey::Tag(1), 20);
            store.insert(&BitmapKey::Folder(1), 30);
            payload1 = store.flush_versioned(1).unwrap();
        }

        // All three categories should be at v1
        assert!(bitmaps_dir.join("status.v1.bin").exists());
        assert!(bitmaps_dir.join("tags.v1.bin").exists());
        assert!(bitmaps_dir.join("folders.v1.bin").exists());

        // Next cycle: only tags dirty
        let payload2;
        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some(&payload1));
            store.insert(&BitmapKey::Tag(2), 40);
            payload2 = store.flush_versioned(2).unwrap();
        }

        // Tags should advance to v2, status and folders stay at v1
        assert!(bitmaps_dir.join("tags.v2.bin").exists());
        assert!(
            !bitmaps_dir.join("status.v2.bin").exists(),
            "Status should NOT be rewritten"
        );
        assert!(
            !bitmaps_dir.join("folders.v2.bin").exists(),
            "Folders should NOT be rewritten"
        );

        // Parse payload to verify per-category versions
        let parsed: serde_json::Value = serde_json::from_str(&payload2).unwrap();
        assert_eq!(parsed["status"], "status.v1.bin");
        assert_eq!(parsed["tags"], "tags.v2.bin");
        assert_eq!(parsed["folders"], "folders.v1.bin");

        // Reopen with the mixed-version payload
        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some(&payload2));
            assert_eq!(store.len(&BitmapKey::Status(1)), 1);
            assert_eq!(store.len(&BitmapKey::Tag(1)), 1);
            assert_eq!(store.len(&BitmapKey::Tag(2)), 1);
            assert_eq!(store.len(&BitmapKey::Folder(1)), 1);
        }
    }

    #[test]
    fn independent_versioning_prune_mixed_versions() {
        let dir = tempfile::tempdir().unwrap();
        let bitmaps_dir = dir.path().join("bitmaps");

        // Cycle 1: all categories
        let payload1;
        {
            let store = BitmapStore::open(dir.path());
            store.insert(&BitmapKey::Status(1), 1);
            store.insert(&BitmapKey::Tag(1), 1);
            store.insert(&BitmapKey::Folder(1), 1);
            payload1 = store.flush_versioned(1).unwrap();
        }

        // Cycle 2: only tags
        let payload2;
        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some(&payload1));
            store.insert(&BitmapKey::Tag(2), 2);
            payload2 = store.flush_versioned(2).unwrap();
        }

        // Cycle 3: only status
        let payload3;
        {
            let store = BitmapStore::open_with_active_file(dir.path(), Some(&payload2));
            store.insert(&BitmapKey::Status(0), 3);
            payload3 = store.flush_versioned(3).unwrap();
        }

        // State: status=v3, tags=v2, folders=v1
        // payload2 refs: status.v1, tags.v2, folders.v1
        // payload3 refs: status.v3, tags.v2, folders.v1
        // Keep payload3 (current) and payload2 (previous)
        let store = BitmapStore::open_with_active_file(dir.path(), Some(&payload3));
        let deleted = store
            .prune_artifacts(&[payload3.clone(), payload2.clone()])
            .unwrap();

        // Only tags.v1.bin should be deleted — everything else is referenced
        assert!(bitmaps_dir.join("status.v3.bin").exists());
        assert!(
            bitmaps_dir.join("status.v1.bin").exists(),
            "status.v1 kept by payload2"
        );
        assert!(bitmaps_dir.join("tags.v2.bin").exists());
        assert!(bitmaps_dir.join("folders.v1.bin").exists());
        assert!(
            !bitmaps_dir.join("tags.v1.bin").exists(),
            "tags.v1 not in any payload"
        );
        assert_eq!(deleted, 1);
    }

    #[test]
    fn legacy_payload_backward_compat() {
        let dir = tempfile::tempdir().unwrap();

        // Create per-category files at version 5 (simulating existing library)
        let bitmaps_dir = dir.path().join("bitmaps");
        fs::create_dir_all(&bitmaps_dir).unwrap();

        {
            let mut bitmaps = HashMap::new();
            let mut bm = RoaringBitmap::new();
            bm.insert(42);
            bitmaps.insert(BitmapKey::Tag(1), bm);
            save_to_file(&bitmaps, &bitmaps_dir.join("tags.v5.bin")).unwrap();

            let mut bitmaps = HashMap::new();
            let mut bm = RoaringBitmap::new();
            bm.insert(10);
            bitmaps.insert(BitmapKey::Status(1), bm);
            save_to_file(&bitmaps, &bitmaps_dir.join("status.v5.bin")).unwrap();

            let bitmaps = HashMap::new();
            save_to_file(&bitmaps, &bitmaps_dir.join("folders.v5.bin")).unwrap();
        }

        // Open with legacy-format payload — should derive per-category names
        let legacy_payload = json!({"active_file": "bitmaps.v5.bin"}).to_string();
        let store = BitmapStore::open_with_active_file(dir.path(), Some(&legacy_payload));

        assert_eq!(store.len(&BitmapKey::Tag(1)), 1);
        assert!(store.contains(&BitmapKey::Tag(1), 42));
        assert_eq!(store.len(&BitmapKey::Status(1)), 1);
    }
}
