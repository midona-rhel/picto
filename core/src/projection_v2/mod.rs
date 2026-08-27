//! In-memory projections for the replacement library schema.
//!
//! SQLite remains authoritative. This module only keeps root-scoped Roaring
//! bitmaps and the small amount of canonical state needed to update them.

mod checkpoint;

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use roaring::RoaringBitmap;
use rusqlite::{Connection, Transaction};

pub use crate::app::{ItemKind, Lifecycle};
use crate::bit_sliced::{BitSlicedU64, FilteredAggregate, OptionalU8};

/// A rebuildable projection of root, folder, membership, and tag state.
pub struct ProjectionStore {
    state: RcuCell<State>,
    writer: Mutex<()>,
}

/// A projection snapshot that has already completed every fallible update and
/// invariant check. Publishing it is a single infallible pointer swap.
pub(crate) struct PreparedProjection {
    state: Arc<State>,
}

/// An atomically published immutable `Arc` snapshot.
///
/// Readers never acquire the projection writer lock. The short reader count
/// protects the raw `Arc` pointer only while its strong count is acquired;
/// returned snapshots then keep their own normal `Arc` reference. Retired
/// publication references are reclaimed once no reader can still be loading
/// the old pointer.
struct RcuCell<T> {
    current: AtomicPtr<T>,
    readers: AtomicUsize,
    retired: Mutex<Vec<usize>>,
}

impl<T> RcuCell<T> {
    fn new(value: Arc<T>) -> Self {
        Self {
            current: AtomicPtr::new(Arc::into_raw(value).cast_mut()),
            readers: AtomicUsize::new(0),
            retired: Mutex::new(Vec::new()),
        }
    }

    fn load(&self) -> Arc<T> {
        self.readers.fetch_add(1, Ordering::SeqCst);
        let pointer = self.current.load(Ordering::SeqCst);
        // SAFETY: the current publication owns one strong reference. A
        // swapped-out publication is retained until `readers` reaches zero,
        // so `pointer` cannot be freed while this reader acquires its ref.
        unsafe { Arc::increment_strong_count(pointer) };
        let was_last_reader = self.readers.fetch_sub(1, Ordering::SeqCst) == 1;
        if was_last_reader {
            self.try_reclaim_if_quiescent();
        }
        // SAFETY: the increment above created the reference returned here.
        unsafe { Arc::from_raw(pointer) }
    }

    fn store(&self, value: Arc<T>) {
        let next = Arc::into_raw(value).cast_mut();
        let previous = self.current.swap(next, Ordering::SeqCst);
        self.retired.lock().unwrap().push(previous as usize);
        self.reclaim_if_quiescent();
    }

    fn reclaim_if_quiescent(&self) {
        if self.readers.load(Ordering::SeqCst) != 0 {
            return;
        }
        let retired = std::mem::take(&mut *self.retired.lock().unwrap());
        for pointer in retired {
            // SAFETY: each retired raw pointer is the unique publication
            // reference formerly owned by `current` and is reclaimed once.
            unsafe { drop(Arc::from_raw(pointer as *const T)) };
        }
    }

    fn try_reclaim_if_quiescent(&self) {
        if self.readers.load(Ordering::SeqCst) != 0 {
            return;
        }
        let Ok(mut retired) = self.retired.try_lock() else {
            return;
        };
        for pointer in std::mem::take(&mut *retired) {
            // SAFETY: identical ownership rule to `reclaim_if_quiescent`;
            // `try_lock` keeps the reader path non-blocking.
            unsafe { drop(Arc::from_raw(pointer as *const T)) };
        }
    }
}

impl<T> Drop for RcuCell<T> {
    fn drop(&mut self) {
        let current = *self.current.get_mut();
        // SAFETY: exclusive access proves no load or publication is active;
        // this is the publication reference installed by `new` or `store`.
        unsafe { drop(Arc::from_raw(current)) };
        for pointer in self.retired.get_mut().unwrap().drain(..) {
            // SAFETY: exclusive access and one retained publication reference.
            unsafe { drop(Arc::from_raw(pointer as *const T)) };
        }
    }
}

/// Cheaply cloned projection component with copy-on-write mutation.
#[derive(Clone)]
struct Shared<T: Clone>(Arc<T>);

impl<T: Clone + Default> Default for Shared<T> {
    fn default() -> Self {
        Self(Arc::new(T::default()))
    }
}

impl<T: Clone> Deref for Shared<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Clone> DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: Clone> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self(Arc::new(value))
    }
}

impl<'a, T> IntoIterator for &'a Shared<T>
where
    T: Clone,
    &'a T: IntoIterator,
{
    type Item = <&'a T as IntoIterator>::Item;
    type IntoIter = <&'a T as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        (&**self).into_iter()
    }
}

impl<T> IntoIterator for Shared<T>
where
    T: Clone + IntoIterator,
{
    type Item = T::Item;
    type IntoIter = T::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        Arc::try_unwrap(self.0)
            .unwrap_or_else(|shared| (*shared).clone())
            .into_iter()
    }
}

impl std::ops::BitOrAssign<&RoaringBitmap> for Shared<RoaringBitmap> {
    fn bitor_assign(&mut self, rhs: &RoaringBitmap) {
        **self |= rhs;
    }
}

impl std::ops::SubAssign<&RoaringBitmap> for Shared<RoaringBitmap> {
    fn sub_assign(&mut self, rhs: &RoaringBitmap) {
        **self -= rhs;
    }
}

const PROJECTION_SHARDS: usize = 64;

trait ProjectionKey {
    fn projection_shard(&self) -> usize;
}

impl ProjectionKey for i64 {
    fn projection_shard(&self) -> usize {
        (*self as u64 as usize) & (PROJECTION_SHARDS - 1)
    }
}

impl ProjectionKey for (i64, i64) {
    fn projection_shard(&self) -> usize {
        self.0.projection_shard()
    }
}

/// A map split into independently copy-on-write components. A point mutation
/// clones at most one shard, not a million-entry global map.
#[derive(Clone)]
struct ShardedMap<K: Clone, V: Clone> {
    shards: [Shared<HashMap<K, V>>; PROJECTION_SHARDS],
}

impl<K, V> Default for ShardedMap<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    fn default() -> Self {
        Self {
            shards: std::array::from_fn(|_| Shared::default()),
        }
    }
}

impl<K, V> ShardedMap<K, V>
where
    K: Clone + Eq + Hash + ProjectionKey,
    V: Clone,
{
    fn shard(&self, key: &K) -> &HashMap<K, V> {
        &self.shards[key.projection_shard()]
    }

    fn shard_mut(&mut self, key: &K) -> &mut HashMap<K, V> {
        &mut self.shards[key.projection_shard()]
    }

    fn get(&self, key: &K) -> Option<&V> {
        self.shard(key).get(key)
    }

    fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.shard_mut(key).get_mut(key)
    }

    fn contains_key(&self, key: &K) -> bool {
        self.shard(key).contains_key(key)
    }

    fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.shard_mut(&key).insert(key, value)
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        self.shard_mut(key).remove(key)
    }

    fn entry(&mut self, key: K) -> Entry<'_, K, V> {
        self.shard_mut(&key).entry(key)
    }

    fn clear(&mut self) {
        self.shards = std::array::from_fn(|_| Shared::default());
    }

    fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.shards.iter().flat_map(|shard| shard.iter())
    }

    fn keys(&self) -> impl Iterator<Item = &K> {
        self.iter().map(|(key, _)| key)
    }

    fn values(&self) -> impl Iterator<Item = &V> {
        self.iter().map(|(_, value)| value)
    }

    fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.shards.iter_mut().flat_map(|shard| shard.values_mut())
    }

    fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        for shard in &mut self.shards {
            shard.retain(|key, value| keep(key, value));
        }
    }
}

struct ProjectionWrite<'a> {
    cell: &'a RcuCell<State>,
    _guard: MutexGuard<'a, ()>,
    next: Option<State>,
}

impl Deref for ProjectionWrite<'_> {
    type Target = State;

    fn deref(&self) -> &Self::Target {
        self.next.as_ref().unwrap()
    }
}

impl DerefMut for ProjectionWrite<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.next.as_mut().unwrap()
    }
}

impl ProjectionWrite<'_> {
    fn abort(mut self) {
        self.next = None;
    }
}

impl Drop for ProjectionWrite<'_> {
    fn drop(&mut self) {
        if let Some(next) = self.next.take() {
            self.cell.store(Arc::new(next));
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ItemProjectionChange {
    pub item_id: i64,
    pub kind: ItemKind,
    pub present: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct RootProjectionChange {
    pub item_id: i64,
    pub lifecycle: Option<Lifecycle>,
}

/// Exact numeric fields mirrored from one `root_summary` row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootSummaryProjectionChange {
    pub item_id: i64,
    pub total_size_bytes: u64,
    pub media_count: u64,
    pub rating: Option<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct MembershipProjectionChange {
    pub collection_id: i64,
    pub media_id: i64,
    pub present: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct MediaClassificationProjectionChange {
    pub media_id: i64,
    pub is_image: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct FolderProjectionChange {
    pub folder_id: i64,
    pub item_id: i64,
    pub present: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TagProjectionChange {
    pub media_id: i64,
    pub tag_id: i64,
    pub present: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct TagIdentityProjectionChange {
    pub source_tag_id: i64,
    pub target_tag_id: Option<i64>,
    pub remove_tag: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TagGraphProjectionDelta {
    pub identities: Vec<TagIdentityProjectionChange>,
}

#[derive(Debug, Clone, Default)]
pub struct StructureProjectionDelta {
    pub items: Vec<ItemProjectionChange>,
    pub media_classifications: Vec<MediaClassificationProjectionChange>,
    pub roots: Vec<RootProjectionChange>,
    pub memberships: Vec<MembershipProjectionChange>,
    pub folders: Vec<FolderProjectionChange>,
    pub tags: Vec<TagProjectionChange>,
}

#[derive(Clone, Default)]
struct NumericIndexes {
    total_size_bytes: BitSlicedU64,
    media_count: BitSlicedU64,
    rating: OptionalU8,
}

#[derive(Clone, Default)]
struct State {
    lifecycle_bitmaps: Arc<[RoaringBitmap; 3]>,
    numeric: Arc<NumericIndexes>,
    media_ids: Shared<HashSet<i64>>,
    image_media_ids: Shared<RoaringBitmap>,
    all_image_roots: Shared<RoaringBitmap>,
    collection_ids: Shared<HashSet<i64>>,
    collection_members: ShardedMap<i64, Shared<HashSet<i64>>>,
    media_to_root: ShardedMap<i64, i64>,
    folder_members: ShardedMap<i64, Shared<RoaringBitmap>>,
    folder_bitmaps: ShardedMap<i64, Shared<RoaringBitmap>>,
    root_folder_counts: ShardedMap<i64, u32>,
    categorized_roots: Shared<RoaringBitmap>,
    root_owned_tags: ShardedMap<i64, Shared<Vec<i64>>>,
    direct_tag_bitmaps: ShardedMap<i64, Shared<RoaringBitmap>>,
    tagged_roots: Shared<RoaringBitmap>,
}

#[derive(Debug, Clone, Default)]
pub struct ProjectionSidebarSnapshot {
    pub all: i64,
    pub inbox: i64,
    pub trash: i64,
    pub untagged: i64,
    pub uncategorized: i64,
    pub folders: Vec<(i64, i64)>,
}

/// Exact numeric aggregates for roots selected by one bitmap.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProjectionNumericAggregates {
    pub selected_root_count: u64,
    pub active_root_count: u64,
    pub total_size_bytes: FilteredAggregate,
    pub media_count: FilteredAggregate,
    pub rating: FilteredAggregate,
    pub rating_min: Option<u8>,
    pub rating_max: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LifecycleFolderSummaryDelta {
    pub folder_id: i64,
    pub root_count: i64,
    pub media_count: i64,
    pub total_size_bytes: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct LifecycleSummaryDelta {
    pub folders: Vec<LifecycleFolderSummaryDelta>,
    pub tags: Vec<(i64, i64)>,
}

impl LifecycleSummaryDelta {
    pub(crate) fn stage(&self, transaction: &Transaction<'_>) -> rusqlite::Result<()> {
        transaction.execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS picto_prepared_lifecycle_delta (
                 singleton INTEGER PRIMARY KEY CHECK (singleton = 1)
             ) WITHOUT ROWID;
             CREATE TEMP TABLE IF NOT EXISTS picto_lifecycle_folder_delta (
                 folder_id INTEGER PRIMARY KEY,
                 root_count INTEGER NOT NULL,
                 media_count INTEGER NOT NULL,
                 total_size_bytes INTEGER NOT NULL
             ) WITHOUT ROWID;
             CREATE TEMP TABLE IF NOT EXISTS picto_lifecycle_tag_delta (
                 tag_id INTEGER PRIMARY KEY,
                 visible_root_count INTEGER NOT NULL
             ) WITHOUT ROWID;
             DELETE FROM picto_prepared_lifecycle_delta;
             DELETE FROM picto_lifecycle_folder_delta;
             DELETE FROM picto_lifecycle_tag_delta;
             INSERT INTO picto_prepared_lifecycle_delta(singleton) VALUES (1);",
        )?;
        let folders = serde_json::to_string(
            &self
                .folders
                .iter()
                .map(|delta| {
                    (
                        delta.folder_id,
                        delta.root_count,
                        delta.media_count,
                        delta.total_size_bytes,
                    )
                })
                .collect::<Vec<_>>(),
        )
        .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "INSERT INTO picto_lifecycle_folder_delta(
                 folder_id, root_count, media_count, total_size_bytes
             )
             SELECT CAST(json_extract(value, '$[0]') AS INTEGER),
                    CAST(json_extract(value, '$[1]') AS INTEGER),
                    CAST(json_extract(value, '$[2]') AS INTEGER),
                    CAST(json_extract(value, '$[3]') AS INTEGER)
             FROM json_each(?1)",
            [folders],
        )?;
        let tags = serde_json::to_string(&self.tags)
            .map_err(|error| rusqlite::Error::ToSqlConversionFailure(Box::new(error)))?;
        transaction.execute(
            "INSERT INTO picto_lifecycle_tag_delta(tag_id, visible_root_count)
             SELECT CAST(json_extract(value, '$[0]') AS INTEGER),
                    CAST(json_extract(value, '$[1]') AS INTEGER)
             FROM json_each(?1)",
            [tags],
        )?;
        Ok(())
    }
}

/// Immutable projection view captured alongside one pinned SQLite revision.
/// Selection reads may keep this snapshot after the publication gate is
/// released without observing a later projection publication.
#[derive(Clone)]
pub(crate) struct ProjectionSelectionSnapshot {
    state: Arc<State>,
}

impl ProjectionSelectionSnapshot {
    pub(crate) fn numeric_aggregates(&self, roots: &RoaringBitmap) -> ProjectionNumericAggregates {
        let rating_range = self.state.numeric.rating.filtered_min_max(roots);
        ProjectionNumericAggregates {
            selected_root_count: roots.len(),
            active_root_count: self.state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Active)]
                .intersection_len(roots),
            total_size_bytes: self
                .state
                .numeric
                .total_size_bytes
                .filtered_aggregate(roots),
            media_count: self.state.numeric.media_count.filtered_aggregate(roots),
            rating: self.state.numeric.rating.filtered_aggregate(roots),
            rating_min: rating_range.map(|range| range.0),
            rating_max: rating_range.map(|range| range.1),
        }
    }

    pub(crate) fn direct_tag_counts(&self, roots: &RoaringBitmap) -> Vec<(i64, i64)> {
        let mut counts = HashMap::<i64, i64>::new();
        for root_id in roots.iter().map(i64::from) {
            if let Some(tags) = self.state.root_owned_tags.get(&root_id) {
                for tag_id in tags.iter().copied() {
                    *counts.entry(tag_id).or_default() += 1;
                }
            }
        }
        counts.into_iter().collect()
    }

    pub(crate) fn all_media_are_images(&self, roots: &RoaringBitmap) -> bool {
        !roots.is_empty() && (roots - &*self.state.all_image_roots).is_empty()
    }

    pub(crate) fn lifecycle_summary_delta(
        &self,
        roots: &RoaringBitmap,
        target: Lifecycle,
    ) -> Result<LifecycleSummaryDelta, String> {
        let active_after = if target == Lifecycle::Active {
            roots.clone()
        } else {
            RoaringBitmap::new()
        };
        self.lifecycle_summary_delta_for_active(roots, &active_after)
    }

    pub(crate) fn lifecycle_summary_delta_for_active(
        &self,
        roots: &RoaringBitmap,
        active_after: &RoaringBitmap,
    ) -> Result<LifecycleSummaryDelta, String> {
        let active = &self.state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Active)];
        let active_before = roots & active;

        let mut folders = Vec::new();
        for (folder_id, members) in self.state.folder_bitmaps.iter() {
            let before = &active_before & &**members;
            let after = active_after & &**members;
            let root_count = i64::try_from(after.len())
                .and_then(|after| i64::try_from(before.len()).map(|before| after - before))
                .map_err(|_| "folder lifecycle count exceeds SQLite range".to_string())?;
            let before_media = self.state.numeric.media_count.filtered_sum(&before);
            let after_media = self.state.numeric.media_count.filtered_sum(&after);
            let before_bytes = self.state.numeric.total_size_bytes.filtered_sum(&before);
            let after_bytes = self.state.numeric.total_size_bytes.filtered_sum(&after);
            let media_count = signed_u128_delta(after_media, before_media, "folder media count")?;
            let total_size_bytes =
                signed_u128_delta(after_bytes, before_bytes, "folder byte count")?;
            if root_count != 0 || media_count != 0 || total_size_bytes != 0 {
                folders.push(LifecycleFolderSummaryDelta {
                    folder_id: *folder_id,
                    root_count,
                    media_count,
                    total_size_bytes,
                });
            }
        }

        let mut tags = self
            .state
            .direct_tag_bitmaps
            .iter()
            .filter_map(|(tag_id, members)| {
                let before = members.intersection_len(&active_before);
                let after = members.intersection_len(&active_after);
                let delta = i64::try_from(after)
                    .and_then(|after| i64::try_from(before).map(|before| after - before))
                    .ok()?;
                (delta != 0).then_some((*tag_id, delta))
            })
            .collect::<Vec<_>>();
        folders.sort_unstable_by_key(|delta| delta.folder_id);
        tags.sort_unstable_by_key(|(tag_id, _)| *tag_id);
        Ok(LifecycleSummaryDelta { folders, tags })
    }
}

fn signed_u128_delta(after: u128, before: u128, field: &str) -> Result<i64, String> {
    if after >= before {
        i64::try_from(after - before).map_err(|_| format!("{field} exceeds SQLite range"))
    } else {
        i64::try_from(before - after)
            .map(|value| -value)
            .map_err(|_| format!("{field} exceeds SQLite range"))
    }
}

#[derive(Clone)]
struct NumericProjectionSnapshot {
    lifecycle_bitmaps: Arc<[RoaringBitmap; 3]>,
    numeric: Arc<NumericIndexes>,
}

impl ProjectionStore {
    pub fn new() -> Self {
        Self {
            state: RcuCell::new(Arc::new(State::default())),
            writer: Mutex::new(()),
        }
    }

    fn write_state(&self) -> ProjectionWrite<'_> {
        let guard = self.writer.lock().unwrap();
        let current = self.state.load();
        ProjectionWrite {
            cell: &self.state,
            _guard: guard,
            next: Some((*current).clone()),
        }
    }

    /// Prepare a complete incremental settlement without changing the
    /// currently published projection. The store writer serializes canonical
    /// mutations, so callers can build this after their SQL changes and before
    /// committing the transaction.
    pub(crate) fn prepare(
        &self,
        update: impl FnOnce(&ProjectionStore) -> Result<(), String>,
    ) -> Result<PreparedProjection, String> {
        let candidate = Self {
            state: RcuCell::new(self.state.load()),
            writer: Mutex::new(()),
        };
        update(&candidate)?;
        let state = candidate.state.load();
        validate_bitmap_ids(&state)?;
        Ok(PreparedProjection { state })
    }

    /// Publish a previously validated settlement. No allocation, validation,
    /// SQL, or other fallible work is permitted at this boundary.
    pub(crate) fn publish_prepared(&self, prepared: PreparedProjection) {
        let _guard = self
            .writer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.state.store(prepared.state);
    }

    /// Build every projection from the replacement schema in one read pass.
    pub fn from_connection(connection: &Connection) -> Result<Self, String> {
        let mut state = State::default();

        {
            let mut statement = connection
                .prepare(
                    "SELECT item.item_id, item.kind,
                            COALESCE(file.mime_type LIKE 'image/%', 0)
                     FROM library_item item
                     LEFT JOIN media_asset asset ON asset.item_id = item.item_id
                     LEFT JOIN media_file file ON file.file_id = asset.file_id",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, bool>(2)?,
                    ))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (item_id, kind, is_image) = row.map_err(|error| error.to_string())?;
                match kind.as_str() {
                    "media" => {
                        state.media_ids.insert(item_id);
                        if is_image {
                            state.image_media_ids.insert(bitmap_id(item_id)?);
                        }
                    }
                    "collection" => {
                        state.collection_ids.insert(item_id);
                    }
                    other => return Err(format!("invalid library item kind: {other}")),
                }
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT item_id, lifecycle FROM library_root")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (item_id, lifecycle) = row.map_err(|error| error.to_string())?;
                let item_id = bitmap_id(item_id)?;
                Arc::make_mut(&mut state.lifecycle_bitmaps)
                    [lifecycle_index(parse_lifecycle(&lifecycle)?)]
                .insert(item_id);
            }
        }

        {
            let mut statement = connection
                .prepare(
                    "SELECT root_item_id, total_size_bytes,
                            media_count, sort_rating
                     FROM root_summary",
                )
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                })
                .map_err(|error| error.to_string())?;
            let numeric = Arc::make_mut(&mut state.numeric);
            for row in rows {
                let (item_id, total_size_bytes, media_count, rating) =
                    row.map_err(|error| error.to_string())?;
                let item_id = bitmap_id(item_id)?;
                numeric.total_size_bytes.set(
                    item_id,
                    u64::try_from(total_size_bytes)
                        .map_err(|_| "root_summary contains a negative total size".to_string())?,
                );
                numeric.media_count.set(
                    item_id,
                    u64::try_from(media_count)
                        .map_err(|_| "root_summary contains a negative media count".to_string())?,
                );
                if let Some(rating) = rating {
                    numeric.rating.set(
                        item_id,
                        u8::try_from(rating)
                            .map_err(|_| "root_summary contains an invalid rating".to_string())?,
                    );
                }
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT collection_id, media_item_id FROM collection_member")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (collection_id, media_id) = row.map_err(|error| error.to_string())?;
                state
                    .collection_members
                    .entry(collection_id)
                    .or_default()
                    .insert(media_id);
                state.media_to_root.insert(media_id, collection_id);
            }
        }

        for media_id in state.media_ids.iter().copied() {
            if !state.media_to_root.contains_key(&media_id) && has_root(&state, media_id) {
                state.media_to_root.insert(media_id, media_id);
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT folder_id, item_id FROM folder_item")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (folder_id, item_id) = row.map_err(|error| error.to_string())?;
                validate_id(item_id)?;
                state
                    .folder_members
                    .entry(folder_id)
                    .or_default()
                    .insert(item_id as u32);
            }
        }

        {
            let mut statement = connection
                .prepare("SELECT root_item_id, tag_id FROM root_tag")
                .map_err(|error| error.to_string())?;
            let rows = statement
                .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
                .map_err(|error| error.to_string())?;
            for row in rows {
                let (root_id, tag_id) = row.map_err(|error| error.to_string())?;
                validate_id(root_id)?;
                insert_sorted_tag(state.root_owned_tags.entry(root_id).or_default(), tag_id);
            }
        }

        validate_bitmap_ids(&state)?;
        rebuild_all_derived(&mut state);
        Ok(Self {
            state: RcuCell::new(Arc::new(state)),
            writer: Mutex::new(()),
        })
    }

    pub fn initialize(connection: &Connection) -> Result<Self, String> {
        if let Ok(Some(state)) = checkpoint::load(connection) {
            return Ok(Self {
                state: RcuCell::new(Arc::new(state)),
                writer: Mutex::new(()),
            });
        }
        Self::from_connection(connection)
    }

    /// Persist the current immutable projection snapshot for a later startup.
    /// Runtime wiring should call this only after the corresponding SQLite
    /// revision and projection settlement have both completed.
    pub fn write_checkpoint(&self, connection: &Connection) -> Result<(), String> {
        let _guard = self
            .writer
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let state = self.state.load();
        checkpoint::write(connection, &state)
    }

    /// Replace all derived state from SQLite. This is the recovery path when
    /// an incremental settlement fails after the authoritative commit.
    pub fn reload(&self, connection: &Connection) -> Result<(), String> {
        let rebuilt = Self::from_connection(connection)?;
        self.replace_with(rebuilt);
        Ok(())
    }

    /// Publish an already-built projection without performing fallible work.
    pub fn replace_with(&self, rebuilt: ProjectionStore) {
        let _guard = self.writer.lock().unwrap();
        self.state.store(rebuilt.state.load());
    }

    pub fn lifecycle_bitmap(&self, lifecycle: Lifecycle) -> RoaringBitmap {
        let lifecycle_bitmaps = Arc::clone(&self.state.load().lifecycle_bitmaps);
        lifecycle_bitmaps[lifecycle_index(lifecycle)].clone()
    }

    pub fn active_bitmap(&self) -> RoaringBitmap {
        self.lifecycle_bitmap(Lifecycle::Active)
    }

    pub fn inbox_bitmap(&self) -> RoaringBitmap {
        self.lifecycle_bitmap(Lifecycle::Inbox)
    }

    pub fn trash_bitmap(&self) -> RoaringBitmap {
        self.lifecycle_bitmap(Lifecycle::Trash)
    }

    /// Aggregate numeric root fields without retaining the global projection
    /// lock while Roaring performs its intersections.
    pub fn numeric_aggregates(&self, roots: &RoaringBitmap) -> ProjectionNumericAggregates {
        let snapshot = self.numeric_snapshot();
        let rating_range = snapshot.numeric.rating.filtered_min_max(roots);
        ProjectionNumericAggregates {
            selected_root_count: roots.len(),
            active_root_count: snapshot.lifecycle_bitmaps[lifecycle_index(Lifecycle::Active)]
                .intersection_len(roots),
            total_size_bytes: snapshot.numeric.total_size_bytes.filtered_aggregate(roots),
            media_count: snapshot.numeric.media_count.filtered_aggregate(roots),
            rating: snapshot.numeric.rating.filtered_aggregate(roots),
            rating_min: rating_range.map(|range| range.0),
            rating_max: rating_range.map(|range| range.1),
        }
    }

    pub(crate) fn selection_snapshot(&self) -> ProjectionSelectionSnapshot {
        ProjectionSelectionSnapshot {
            state: self.state.load(),
        }
    }

    pub fn total_size_aggregate(&self, roots: &RoaringBitmap) -> FilteredAggregate {
        let numeric = Arc::clone(&self.state.load().numeric);
        numeric.total_size_bytes.filtered_aggregate(roots)
    }

    pub fn media_count_aggregate(&self, roots: &RoaringBitmap) -> FilteredAggregate {
        let numeric = Arc::clone(&self.state.load().numeric);
        numeric.media_count.filtered_aggregate(roots)
    }

    pub fn rating_aggregate(&self, roots: &RoaringBitmap) -> FilteredAggregate {
        let numeric = Arc::clone(&self.state.load().numeric);
        numeric.rating.filtered_aggregate(roots)
    }

    /// Restrict a selection to one lifecycle using the same immutable
    /// lifecycle snapshot as the numeric fields.
    pub fn numeric_aggregates_for_lifecycle(
        &self,
        roots: &RoaringBitmap,
        lifecycle: Lifecycle,
    ) -> ProjectionNumericAggregates {
        let snapshot = self.numeric_snapshot();
        let roots = roots & &snapshot.lifecycle_bitmaps[lifecycle_index(lifecycle)];
        ProjectionNumericAggregates {
            selected_root_count: roots.len(),
            active_root_count: roots.len(),
            total_size_bytes: snapshot.numeric.total_size_bytes.filtered_aggregate(&roots),
            media_count: snapshot.numeric.media_count.filtered_aggregate(&roots),
            rating: snapshot.numeric.rating.filtered_aggregate(&roots),
            rating_min: snapshot
                .numeric
                .rating
                .filtered_min_max(&roots)
                .map(|range| range.0),
            rating_max: snapshot
                .numeric
                .rating
                .filtered_min_max(&roots)
                .map(|range| range.1),
        }
    }

    fn numeric_snapshot(&self) -> NumericProjectionSnapshot {
        let state = self.state.load();
        NumericProjectionSnapshot {
            lifecycle_bitmaps: Arc::clone(&state.lifecycle_bitmaps),
            numeric: Arc::clone(&state.numeric),
        }
    }

    pub fn folder_bitmap(&self, folder_id: i64) -> RoaringBitmap {
        let state = self.state.load();
        let folder = state
            .folder_bitmaps
            .get(&folder_id)
            .map(|bitmap| (**bitmap).clone())
            .unwrap_or_default();
        folder & &state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Active)]
    }

    pub fn sidebar_snapshot(&self, folder_ids: &[i64]) -> ProjectionSidebarSnapshot {
        let state = self.state.load();
        let active = &state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Active)];
        ProjectionSidebarSnapshot {
            all: active.len() as i64,
            inbox: state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Inbox)].len() as i64,
            trash: state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Trash)].len() as i64,
            untagged: active
                .len()
                .saturating_sub(active.intersection_len(&state.tagged_roots))
                as i64,
            uncategorized: active
                .len()
                .saturating_sub(active.intersection_len(&state.categorized_roots))
                as i64,
            folders: folder_ids
                .iter()
                .map(|folder_id| {
                    let count = state
                        .folder_bitmaps
                        .get(folder_id)
                        .map(|bitmap| bitmap.intersection_len(active) as i64)
                        .unwrap_or_default();
                    (*folder_id, count)
                })
                .collect(),
        }
    }

    pub fn sidebar_snapshot_all(&self) -> ProjectionSidebarSnapshot {
        let state = self.state.load();
        let active = &state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Active)];
        let mut folders = state
            .folder_bitmaps
            .iter()
            .map(|(folder_id, bitmap)| (*folder_id, bitmap.intersection_len(active) as i64))
            .collect::<Vec<_>>();
        folders.sort_unstable_by_key(|(folder_id, _)| *folder_id);
        ProjectionSidebarSnapshot {
            all: active.len() as i64,
            inbox: state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Inbox)].len() as i64,
            trash: state.lifecycle_bitmaps[lifecycle_index(Lifecycle::Trash)].len() as i64,
            untagged: active
                .len()
                .saturating_sub(active.intersection_len(&state.tagged_roots))
                as i64,
            uncategorized: active
                .len()
                .saturating_sub(active.intersection_len(&state.categorized_roots))
                as i64,
            folders,
        }
    }

    pub fn direct_tag_bitmap(&self, tag_id: i64) -> RoaringBitmap {
        self.state
            .load()
            .direct_tag_bitmaps
            .get(&tag_id)
            .map(|bitmap| (**bitmap).clone())
            .unwrap_or_default()
    }

    pub fn root_for_media(&self, media_id: i64) -> Option<i64> {
        self.state.load().media_to_root.get(&media_id).copied()
    }

    /// Settle structural writes under one projection lock. Root tag matches
    /// use member multiplicities, so adding one collection member never
    /// rescans the collection's existing members.
    pub fn apply_structure_delta(&self, delta: StructureProjectionDelta) -> Result<(), String> {
        for change in &delta.items {
            validate_id(change.item_id)?;
        }
        for change in &delta.media_classifications {
            validate_id(change.media_id)?;
        }
        for change in &delta.roots {
            validate_id(change.item_id)?;
        }
        for change in &delta.memberships {
            validate_id(change.collection_id)?;
            validate_id(change.media_id)?;
        }
        for change in &delta.folders {
            validate_id(change.folder_id)?;
            validate_id(change.item_id)?;
        }
        for change in &delta.tags {
            validate_id(change.media_id)?;
            validate_id(change.tag_id)?;
        }
        let mut state = self.write_state();
        let mut touched_media = HashSet::new();
        let mut touched_roots = HashSet::new();

        for change in &delta.media_classifications {
            if let Some(root_id) = state.media_to_root.get(&change.media_id).copied() {
                touched_roots.insert(root_id);
            }
            if change.is_image {
                state.image_media_ids.insert(change.media_id as u32);
            } else {
                state.image_media_ids.remove(change.media_id as u32);
            }
            touched_media.insert(change.media_id);
        }

        for change in &delta.items {
            if change.present {
                match change.kind {
                    ItemKind::Media => {
                        state.media_ids.insert(change.item_id);
                        touched_media.insert(change.item_id);
                    }
                    ItemKind::Collection => {
                        state.collection_ids.insert(change.item_id);
                    }
                }
            } else {
                if let Some(root_id) = state.media_to_root.get(&change.item_id).copied() {
                    touched_roots.insert(root_id);
                }
                clear_root_tag_counts(&mut state, change.item_id);
                state.media_ids.remove(&change.item_id);
                state.image_media_ids.remove(change.item_id as u32);
                state.collection_ids.remove(&change.item_id);
                state.media_to_root.remove(&change.item_id);
                if let Some(members) = state.collection_members.remove(&change.item_id) {
                    touched_media.extend(members);
                }
            }
        }

        for change in &delta.roots {
            touched_roots.insert(change.item_id);
            if let Some(lifecycle) = change.lifecycle {
                set_lifecycle(&mut state, change.item_id, lifecycle);
            } else {
                remove_lifecycle(&mut state, change.item_id);
                remove_root_numeric_summary(&mut state, change.item_id as u32);
            }
            if state.media_ids.contains(&change.item_id) {
                touched_media.insert(change.item_id);
            }
        }

        for change in &delta.memberships {
            if change.present {
                if let Some(previous_root) = state.media_to_root.get(&change.media_id).copied() {
                    touched_roots.insert(previous_root);
                    if previous_root != change.collection_id
                        && state.collection_ids.contains(&previous_root)
                    {
                        if let Some(members) = state.collection_members.get_mut(&previous_root) {
                            members.remove(&change.media_id);
                        }
                    }
                }
                state
                    .collection_members
                    .entry(change.collection_id)
                    .or_default()
                    .insert(change.media_id);
                state
                    .media_to_root
                    .insert(change.media_id, change.collection_id);
                touched_roots.insert(change.collection_id);
                for bitmap in Arc::make_mut(&mut state.lifecycle_bitmaps) {
                    bitmap.remove(change.media_id as u32);
                }
            } else {
                touched_roots.insert(change.collection_id);
                if let Some(members) = state.collection_members.get_mut(&change.collection_id) {
                    members.remove(&change.media_id);
                }
                if state.media_to_root.get(&change.media_id) == Some(&change.collection_id) {
                    if has_root(&state, change.media_id)
                        && state.media_ids.contains(&change.media_id)
                    {
                        state.media_to_root.insert(change.media_id, change.media_id);
                        if let Some(lifecycle) = lifecycle_for(&state, change.media_id) {
                            set_lifecycle(&mut state, change.media_id, lifecycle);
                        }
                    } else {
                        state.media_to_root.remove(&change.media_id);
                    }
                }
            }
            touched_media.insert(change.media_id);
        }

        for media_id in touched_media {
            if !state.media_to_root.contains_key(&media_id)
                && has_root(&state, media_id)
                && state.media_ids.contains(&media_id)
            {
                state.media_to_root.insert(media_id, media_id);
            } else if !state.media_to_root.contains_key(&media_id) {
                state.media_to_root.remove(&media_id);
            }
            if state.media_to_root.get(&media_id) == Some(&media_id) {
                if let Some(lifecycle) = lifecycle_for(&state, media_id) {
                    set_lifecycle(&mut state, media_id, lifecycle);
                }
            }
            if let Some(root_id) = state.media_to_root.get(&media_id).copied() {
                touched_roots.insert(root_id);
            }
        }

        for root_id in touched_roots {
            sync_all_image_root(&mut state, root_id);
        }

        for change in delta.folders {
            let is_root = is_visible_root(&state, change.item_id);
            if change.present {
                let inserted = state
                    .folder_members
                    .entry(change.folder_id)
                    .or_default()
                    .insert(change.item_id as u32);
                if is_root && inserted {
                    state
                        .folder_bitmaps
                        .entry(change.folder_id)
                        .or_default()
                        .insert(change.item_id as u32);
                    increment_root_folder_count(&mut state, change.item_id);
                }
            } else {
                let removed = state
                    .folder_members
                    .get_mut(&change.folder_id)
                    .is_some_and(|members| members.remove(change.item_id as u32));
                if is_root && removed {
                    if let Some(bitmap) = state.folder_bitmaps.get_mut(&change.folder_id) {
                        bitmap.remove(change.item_id as u32);
                    }
                    decrement_root_folder_count(&mut state, change.item_id);
                } else if let Some(bitmap) = state.folder_bitmaps.get_mut(&change.folder_id) {
                    bitmap.remove(change.item_id as u32);
                }
            }
        }

        let mut tag_changes_by_root: HashMap<(i64, bool), RoaringBitmap> = HashMap::new();
        for change in delta.tags {
            tag_changes_by_root
                .entry((change.tag_id, change.present))
                .or_default()
                .insert(change.media_id as u32);
        }
        if let Some(root_id) = tag_changes_by_root
            .iter()
            .filter(|((_, present), _)| *present)
            .flat_map(|(_, roots)| roots.iter())
            .map(i64::from)
            .find(|root_id| !is_visible_root(&state, *root_id))
        {
            state.abort();
            return Err(format!("item {root_id} is not a projection root"));
        }
        for ((tag_id, present), roots) in tag_changes_by_root {
            apply_root_tag_changes_state(&mut state, tag_id, &roots, present)?;
        }
        Ok(())
    }

    /// Apply a lifecycle change to an existing root.
    pub fn apply_lifecycle_delta(&self, item_id: i64, lifecycle: Lifecycle) -> Result<(), String> {
        validate_id(item_id)?;
        let mut state = self.write_state();
        if !has_root(&state, item_id) {
            return Err(format!("item {item_id} is not a projection root"));
        }
        set_lifecycle(&mut state, item_id, lifecycle);
        Ok(())
    }

    /// Move an existing root set between lifecycle projections using bitmap
    /// algebra rather than one bitmap mutation per root.
    pub fn apply_lifecycle_bitmap(
        &self,
        item_ids: &RoaringBitmap,
        lifecycle: Lifecycle,
    ) -> Result<(), String> {
        if item_ids.is_empty() {
            return Ok(());
        }
        let mut state = self.write_state();
        let visible_roots = state
            .lifecycle_bitmaps
            .iter()
            .fold(RoaringBitmap::new(), |roots, bitmap| roots | bitmap);
        let invalid = item_ids - &visible_roots;
        if let Some(item_id) = invalid.min() {
            return Err(format!("item {item_id} is not a projection root"));
        }

        let target_index = lifecycle_index(lifecycle);
        let moving = item_ids - &state.lifecycle_bitmaps[target_index];
        if moving.is_empty() {
            return Ok(());
        }
        let bitmaps = Arc::make_mut(&mut state.lifecycle_bitmaps);
        for bitmap in bitmaps.iter_mut() {
            *bitmap -= &moving;
        }
        bitmaps[target_index] |= &moving;
        Ok(())
    }

    /// Apply exact `root_summary` numeric deltas under one component swap.
    pub fn apply_root_summary_changes(
        &self,
        changes: &[RootSummaryProjectionChange],
        removed: &RoaringBitmap,
    ) -> Result<(), String> {
        for change in changes {
            validate_id(change.item_id)?;
        }
        let mut state = self.write_state();
        if let Some(change) = changes
            .iter()
            .find(|change| !is_visible_root(&state, change.item_id))
        {
            return Err(format!("item {} is not a projection root", change.item_id));
        }
        let numeric = Arc::make_mut(&mut state.numeric);
        for item_id in removed {
            numeric.total_size_bytes.remove(item_id);
            numeric.media_count.remove(item_id);
            numeric.rating.remove(item_id);
        }
        for change in changes {
            let item_id = change.item_id as u32;
            numeric
                .total_size_bytes
                .set(item_id, change.total_size_bytes);
            numeric.media_count.set(item_id, change.media_count);
            match change.rating {
                Some(rating) => {
                    numeric.rating.set(item_id, rating);
                }
                None => {
                    numeric.rating.remove(item_id);
                }
            }
        }
        Ok(())
    }

    /// Update one rating across a broad root selection without constructing
    /// per-row projection changes.
    pub fn apply_rating_bitmap(
        &self,
        item_ids: &RoaringBitmap,
        rating: Option<u8>,
    ) -> Result<(), String> {
        let mut state = self.write_state();
        let visible_roots = state
            .lifecycle_bitmaps
            .iter()
            .fold(RoaringBitmap::new(), |roots, bitmap| roots | bitmap);
        if rating.is_some() {
            let invalid = item_ids - &visible_roots;
            if let Some(item_id) = invalid.min() {
                return Err(format!("item {item_id} is not a projection root"));
            }
        }
        let numeric = Arc::make_mut(&mut state.numeric);
        match rating {
            Some(rating) => numeric.rating.set_bitmap(item_ids, rating),
            None => numeric.rating.remove_bitmap(item_ids),
        }
        Ok(())
    }

    /// Add, update, or remove a root.
    pub fn apply_root_delta(
        &self,
        item_id: i64,
        kind: ItemKind,
        lifecycle: Option<Lifecycle>,
    ) -> Result<(), String> {
        validate_id(item_id)?;
        let mut state = self.write_state();
        match lifecycle {
            Some(lifecycle) => {
                match kind {
                    ItemKind::Media => {
                        state.media_ids.insert(item_id);
                    }
                    ItemKind::Collection => {
                        state.collection_ids.insert(item_id);
                    }
                }
                if matches!(kind, ItemKind::Media)
                    && !state
                        .collection_members
                        .values()
                        .any(|members| members.contains(&item_id))
                {
                    state.media_to_root.insert(item_id, item_id);
                }
                if matches!(kind, ItemKind::Collection) {
                    let members = state
                        .collection_members
                        .get(&item_id)
                        .cloned()
                        .unwrap_or_default();
                    for member_id in members {
                        state.media_to_root.insert(member_id, item_id);
                    }
                }
                set_lifecycle(&mut state, item_id, lifecycle);
                sync_all_image_root(&mut state, item_id);
            }
            None => {
                clear_root_tag_counts(&mut state, item_id);
                remove_lifecycle(&mut state, item_id);
                remove_root_numeric_summary(&mut state, item_id as u32);
                if state.media_ids.contains(&item_id)
                    && state.media_to_root.get(&item_id) == Some(&item_id)
                {
                    state.media_to_root.remove(&item_id);
                }
                if state.collection_ids.contains(&item_id) {
                    let members = state
                        .collection_members
                        .get(&item_id)
                        .cloned()
                        .unwrap_or_default();
                    for member_id in members {
                        if has_root(&state, member_id) {
                            state.media_to_root.insert(member_id, member_id);
                        } else {
                            state.media_to_root.remove(&member_id);
                        }
                    }
                }
                state.all_image_roots.remove(item_id as u32);
            }
        }
        Ok(())
    }

    pub fn apply_folder_delta(
        &self,
        folder_id: i64,
        item_id: i64,
        present: bool,
    ) -> Result<(), String> {
        validate_id(item_id)?;
        let mut state = self.write_state();
        let is_root = is_visible_root(&state, item_id);
        if present {
            let inserted = state
                .folder_members
                .entry(folder_id)
                .or_default()
                .insert(item_id as u32);
            if is_root && inserted {
                state
                    .folder_bitmaps
                    .entry(folder_id)
                    .or_default()
                    .insert(item_id as u32);
                increment_root_folder_count(&mut state, item_id);
            }
        } else {
            let removed = state
                .folder_members
                .get_mut(&folder_id)
                .is_some_and(|members| members.remove(item_id as u32));
            if is_root && removed {
                if let Some(bitmap) = state.folder_bitmaps.get_mut(&folder_id) {
                    bitmap.remove(item_id as u32);
                }
                decrement_root_folder_count(&mut state, item_id);
            } else if let Some(bitmap) = state.folder_bitmaps.get_mut(&folder_id) {
                bitmap.remove(item_id as u32);
            }
        }
        Ok(())
    }

    /// Apply one broad folder mutation as bitmap set algebra. The only
    /// per-root work left is the exact uncategorized multiplicity update.
    pub fn apply_folder_bitmap(
        &self,
        folder_id: i64,
        item_ids: &RoaringBitmap,
        present: bool,
    ) -> Result<(), String> {
        let mut state = self.write_state();
        let existing = state
            .folder_members
            .get(&folder_id)
            .map(|bitmap| (**bitmap).clone())
            .unwrap_or_default();
        let changed = if present {
            item_ids - &existing
        } else {
            &existing & item_ids
        };
        if changed.is_empty() {
            return Ok(());
        }

        let visible_roots = state
            .lifecycle_bitmaps
            .iter()
            .fold(RoaringBitmap::new(), |acc, bitmap| acc | bitmap);
        let visible_changed = &changed & &visible_roots;
        if present {
            *state.folder_members.entry(folder_id).or_default() |= item_ids;
            *state.folder_bitmaps.entry(folder_id).or_default() |= &visible_changed;
            for item_id in visible_changed.into_iter().map(i64::from) {
                increment_root_folder_count(&mut state, item_id);
            }
        } else {
            if let Some(members) = state.folder_members.get_mut(&folder_id) {
                *members -= item_ids;
            }
            if let Some(bitmap) = state.folder_bitmaps.get_mut(&folder_id) {
                *bitmap -= &visible_changed;
            }
            for item_id in visible_changed.into_iter().map(i64::from) {
                decrement_root_folder_count(&mut state, item_id);
            }
        }
        Ok(())
    }

    pub fn remove_folders(&self, folder_ids: &[i64]) -> Result<(), String> {
        for folder_id in folder_ids {
            validate_id(*folder_id)?;
        }
        let mut state = self.write_state();
        for folder_id in folder_ids {
            if let Some(members) = state.folder_members.remove(folder_id) {
                for item_id in members.into_iter().map(i64::from) {
                    if has_root(&state, item_id) {
                        decrement_root_folder_count(&mut state, item_id);
                    }
                }
            }
            state.folder_bitmaps.remove(folder_id);
        }
        Ok(())
    }

    /// Apply one root-owned tag to a broad root set without visiting media
    /// members. Graph-connected tags require a graph-aware delta because a
    /// second direct tag may preserve the same effective match on removal.
    pub fn apply_root_tag_bitmap(
        &self,
        tag_id: i64,
        root_ids: &RoaringBitmap,
        present: bool,
    ) -> Result<(), String> {
        validate_id(tag_id)?;
        if root_ids.is_empty() {
            return Ok(());
        }
        let mut state = self.write_state();
        let visible_roots = state
            .lifecycle_bitmaps
            .iter()
            .fold(RoaringBitmap::new(), |roots, bitmap| roots | bitmap);
        if present {
            let invalid = root_ids - &visible_roots;
            if let Some(root_id) = invalid.min() {
                return Err(format!("item {root_id} is not a projection root"));
            }
        }
        let existing = state
            .direct_tag_bitmaps
            .get(&tag_id)
            .map(|bitmap| (**bitmap).clone())
            .unwrap_or_default();
        let changed = if present {
            root_ids - &existing
        } else {
            &existing & root_ids
        };
        if changed.is_empty() {
            return Ok(());
        }

        if present {
            *state.direct_tag_bitmaps.entry(tag_id).or_default() |= &changed;
            for root_id in changed {
                let root_id = i64::from(root_id);
                insert_sorted_tag(state.root_owned_tags.entry(root_id).or_default(), tag_id);
                state.tagged_roots.insert(root_id as u32);
            }
        } else {
            if let Some(bitmap) = state.direct_tag_bitmaps.get_mut(&tag_id) {
                *bitmap -= &changed;
            }
            for root_id in changed {
                let root_id = i64::from(root_id);
                if let Some(tags) = state.root_owned_tags.get_mut(&root_id) {
                    remove_sorted_tag(tags, tag_id);
                    if tags.is_empty() {
                        state.root_owned_tags.remove(&root_id);
                    }
                }
                sync_root_tagged(&mut state, root_id);
            }
        }
        Ok(())
    }

    /// Compatibility entry point: canonical tags are owned by the visible
    /// root, so a member ID resolves to its root before publication.
    pub fn apply_tag_delta(&self, media_id: i64, tag_id: i64, present: bool) -> Result<(), String> {
        let root_id = self
            .root_for_media(media_id)
            .ok_or_else(|| format!("item {media_id} has no projection root"))?;
        self.apply_root_tag_bitmap(tag_id, &RoaringBitmap::from_iter([root_id as u32]), present)
    }

    /// Apply a batch under one lock and adjust only changed tag counts.
    pub fn apply_tag_changes(&self, changes: &[(i64, i64)], present: bool) -> Result<(), String> {
        let snapshot = self.state.load();
        let mut roots_by_tag: HashMap<i64, RoaringBitmap> = HashMap::new();
        for &(media_id, tag_id) in changes {
            validate_id(media_id)?;
            validate_id(tag_id)?;
            let root_id = snapshot
                .media_to_root
                .get(&media_id)
                .copied()
                .ok_or_else(|| format!("item {media_id} has no projection root"))?;
            roots_by_tag
                .entry(tag_id)
                .or_default()
                .insert(root_id as u32);
        }
        drop(snapshot);
        for (tag_id, roots) in roots_by_tag {
            self.apply_root_tag_bitmap(tag_id, &roots, present)?;
        }
        Ok(())
    }

    /// Apply one tag to a broad media set without allocating a tuple and
    /// grouping entry for every changed row.
    pub fn apply_tag_bitmap(
        &self,
        tag_id: i64,
        media_ids: &RoaringBitmap,
        present: bool,
    ) -> Result<(), String> {
        validate_id(tag_id)?;
        let snapshot = self.state.load();
        let mut roots = RoaringBitmap::new();
        for media_id in media_ids.iter().map(i64::from) {
            if let Some(root_id) = snapshot.media_to_root.get(&media_id) {
                roots.insert(*root_id as u32);
            }
        }
        drop(snapshot);
        self.apply_root_tag_bitmap(tag_id, &roots, present)
    }

    /// Apply a collection membership change and move tag matches between roots.
    pub fn apply_membership_delta(
        &self,
        collection_id: i64,
        media_id: i64,
        present: bool,
    ) -> Result<(), String> {
        validate_id(collection_id)?;
        validate_id(media_id)?;
        let mut state = self.write_state();
        let old_root = state.media_to_root.get(&media_id).copied();

        if present {
            if let Some(previous_root) = old_root {
                if previous_root != collection_id && state.collection_ids.contains(&previous_root) {
                    if let Some(members) = state.collection_members.get_mut(&previous_root) {
                        members.remove(&media_id);
                    }
                }
            }
            state
                .collection_members
                .entry(collection_id)
                .or_default()
                .insert(media_id);
            state.media_to_root.insert(media_id, collection_id);
        } else {
            if let Some(members) = state.collection_members.get_mut(&collection_id) {
                members.remove(&media_id);
            }
            match has_root(&state, media_id) {
                true => {
                    state.media_to_root.insert(media_id, media_id);
                }
                false => {
                    state.media_to_root.remove(&media_id);
                }
            }
        }
        if let Some(old_root) = old_root {
            sync_all_image_root(&mut state, old_root);
        }
        sync_all_image_root(&mut state, collection_id);
        if state.media_to_root.get(&media_id) == Some(&media_id) {
            sync_all_image_root(&mut state, media_id);
        }
        Ok(())
    }

    /// Apply tag identity changes without scanning unrelated media.
    pub fn apply_tag_graph_delta(&self, delta: TagGraphProjectionDelta) -> Result<(), String> {
        let mut state = self.write_state();
        for change in &delta.identities {
            replace_root_owned_tag(&mut state, change.source_tag_id, change.target_tag_id);
        }
        rebuild_all_tags(&mut state);
        Ok(())
    }
}

impl Default for ProjectionStore {
    fn default() -> Self {
        Self::new()
    }
}

fn apply_root_tag_changes_state(
    state: &mut State,
    tag_id: i64,
    root_ids: &RoaringBitmap,
    present: bool,
) -> Result<(), String> {
    for root_id in root_ids.iter().map(i64::from) {
        if present && !is_visible_root(state, root_id) {
            return Err(format!("item {root_id} is not a projection root"));
        }
        let tags = state.root_owned_tags.entry(root_id).or_default();
        let changed = if present {
            insert_sorted_tag(tags, tag_id)
        } else {
            remove_sorted_tag(tags, tag_id)
        };
        if !changed {
            continue;
        }
        if present {
            state
                .direct_tag_bitmaps
                .entry(tag_id)
                .or_default()
                .insert(root_id as u32);
        } else if let Some(bitmap) = state.direct_tag_bitmaps.get_mut(&tag_id) {
            bitmap.remove(root_id as u32);
        }
        if state
            .root_owned_tags
            .get(&root_id)
            .is_some_and(|tags| tags.is_empty())
        {
            state.root_owned_tags.remove(&root_id);
        }
        sync_root_tagged(state, root_id);
    }
    Ok(())
}

fn parse_lifecycle(value: &str) -> Result<Lifecycle, String> {
    match value {
        "inbox" => Ok(Lifecycle::Inbox),
        "active" => Ok(Lifecycle::Active),
        "trash" => Ok(Lifecycle::Trash),
        other => Err(format!("invalid library lifecycle: {other}")),
    }
}

fn validate_id(id: i64) -> Result<(), String> {
    bitmap_id(id).map(|_| ())
}

fn bitmap_id(id: i64) -> Result<u32, String> {
    u32::try_from(id).map_err(|_| format!("item id {id} cannot be represented by RoaringBitmap"))
}

fn validate_bitmap_ids(state: &State) -> Result<(), String> {
    for id in state.media_ids.iter().chain(state.collection_ids.iter()) {
        validate_id(*id)?;
    }
    Ok(())
}

fn set_lifecycle(state: &mut State, item_id: i64, lifecycle: Lifecycle) {
    let bitmaps = Arc::make_mut(&mut state.lifecycle_bitmaps);
    for bitmap in bitmaps.iter_mut() {
        bitmap.remove(item_id as u32);
    }
    bitmaps[lifecycle_index(lifecycle)].insert(item_id as u32);
}

fn remove_lifecycle(state: &mut State, item_id: i64) {
    for bitmap in Arc::make_mut(&mut state.lifecycle_bitmaps).iter_mut() {
        bitmap.remove(item_id as u32);
    }
}

fn remove_root_numeric_summary(state: &mut State, item_id: u32) {
    let numeric = Arc::make_mut(&mut state.numeric);
    numeric.total_size_bytes.remove(item_id);
    numeric.media_count.remove(item_id);
    numeric.rating.remove(item_id);
}

fn is_visible_root(state: &State, item_id: i64) -> bool {
    has_root(state, item_id)
        && (state.collection_ids.contains(&item_id)
            || state.media_to_root.get(&item_id) == Some(&item_id))
}

fn has_root(state: &State, item_id: i64) -> bool {
    let Ok(item_id) = u32::try_from(item_id) else {
        return false;
    };
    state
        .lifecycle_bitmaps
        .iter()
        .any(|bitmap| bitmap.contains(item_id))
}

fn lifecycle_for(state: &State, item_id: i64) -> Option<Lifecycle> {
    let item_id = u32::try_from(item_id).ok()?;
    [Lifecycle::Inbox, Lifecycle::Active, Lifecycle::Trash]
        .into_iter()
        .find(|lifecycle| state.lifecycle_bitmaps[lifecycle_index(*lifecycle)].contains(item_id))
}

fn lifecycle_index(lifecycle: Lifecycle) -> usize {
    match lifecycle {
        Lifecycle::Inbox => 0,
        Lifecycle::Active => 1,
        Lifecycle::Trash => 2,
    }
}

fn replace_root_owned_tag(state: &mut State, source_tag_id: i64, target_tag_id: Option<i64>) {
    for tags in state.root_owned_tags.values_mut() {
        if remove_sorted_tag(tags, source_tag_id) {
            if let Some(target_tag_id) = target_tag_id {
                insert_sorted_tag(tags, target_tag_id);
            }
        }
    }
    state.root_owned_tags.retain(|_, tags| !tags.is_empty());
}

fn rebuild_all_derived(state: &mut State) {
    state.folder_bitmaps.clear();
    state.root_folder_counts.clear();
    state.categorized_roots.clear();
    for folder_id in state.folder_members.keys().copied().collect::<Vec<_>>() {
        rebuild_folder_bitmap(state, folder_id);
    }
    rebuild_all_tags(state);
    rebuild_all_image_roots(state);
}

fn rebuild_all_image_roots(state: &mut State) {
    state.all_image_roots.clear();
    let roots = state
        .lifecycle_bitmaps
        .iter()
        .fold(RoaringBitmap::new(), |roots, bitmap| roots | bitmap);
    for root_id in roots.into_iter().map(i64::from) {
        sync_all_image_root(state, root_id);
    }
}

fn sync_all_image_root(state: &mut State, root_id: i64) {
    state.all_image_roots.remove(root_id as u32);
    if !is_visible_root(state, root_id) {
        return;
    }
    let all_images = if state.media_ids.contains(&root_id) {
        state.image_media_ids.contains(root_id as u32)
    } else if state.collection_ids.contains(&root_id) {
        state
            .collection_members
            .get(&root_id)
            .is_some_and(|members| {
                !members.is_empty()
                    && members
                        .iter()
                        .all(|media_id| state.image_media_ids.contains(*media_id as u32))
            })
    } else {
        false
    };
    if all_images {
        state.all_image_roots.insert(root_id as u32);
    }
}

fn rebuild_folder_bitmap(state: &mut State, folder_id: i64) {
    let members = state
        .folder_members
        .get(&folder_id)
        .cloned()
        .unwrap_or_default();
    let mut bitmap = RoaringBitmap::new();
    for item_id in members.into_iter().map(i64::from) {
        if is_visible_root(state, item_id) {
            bitmap.insert(item_id as u32);
            increment_root_folder_count(state, item_id);
        }
    }
    state.folder_bitmaps.insert(folder_id, bitmap.into());
}

fn increment_root_folder_count(state: &mut State, root_id: i64) {
    let count = state.root_folder_counts.entry(root_id).or_default();
    if *count == 0 {
        state.categorized_roots.insert(root_id as u32);
    }
    *count = count.saturating_add(1);
}

fn decrement_root_folder_count(state: &mut State, root_id: i64) {
    let remove_root = if let Some(count) = state.root_folder_counts.get_mut(&root_id) {
        *count = count.saturating_sub(1);
        *count == 0
    } else {
        false
    };
    if remove_root {
        state.root_folder_counts.remove(&root_id);
        state.categorized_roots.remove(root_id as u32);
    }
}

fn rebuild_all_tags(state: &mut State) {
    state.direct_tag_bitmaps.clear();
    state.tagged_roots.clear();

    let root_tags = state
        .root_owned_tags
        .iter()
        .map(|(root_id, tags)| (*root_id, tags.clone()))
        .collect::<Vec<_>>();
    for (root_id, direct_tags) in root_tags {
        if !is_visible_root(state, root_id) {
            continue;
        }
        for tag_id in &direct_tags {
            state
                .direct_tag_bitmaps
                .entry(*tag_id)
                .or_default()
                .insert(root_id as u32);
        }
        if !direct_tags.is_empty() {
            state.tagged_roots.insert(root_id as u32);
        }
    }
}

fn clear_root_tag_counts(state: &mut State, root_id: i64) {
    let direct = state.root_owned_tags.remove(&root_id).unwrap_or_default();
    for tag_id in direct {
        if let Some(bitmap) = state.direct_tag_bitmaps.get_mut(&tag_id) {
            bitmap.remove(root_id as u32);
        }
    }
    state.tagged_roots.remove(root_id as u32);
}

fn sync_root_tagged(state: &mut State, root_id: i64) {
    if state
        .root_owned_tags
        .get(&root_id)
        .is_some_and(|tags| !tags.is_empty())
    {
        state.tagged_roots.insert(root_id as u32);
    } else {
        state.tagged_roots.remove(root_id as u32);
    }
}

fn insert_sorted_tag(tags: &mut Vec<i64>, tag_id: i64) -> bool {
    match tags.binary_search(&tag_id) {
        Ok(_) => false,
        Err(index) => {
            tags.insert(index, tag_id);
            true
        }
    }
}

fn remove_sorted_tag(tags: &mut Vec<i64>, tag_id: i64) -> bool {
    match tags.binary_search(&tag_id) {
        Ok(index) => {
            tags.remove(index);
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use roaring::RoaringBitmap;
    use rusqlite::Connection;

    use super::{
        rebuild_all_derived, set_lifecycle, ItemKind, Lifecycle,
        MediaClassificationProjectionChange, MembershipProjectionChange, ProjectionStore,
        RootProjectionChange, RootSummaryProjectionChange, StructureProjectionDelta,
    };

    fn fixture() -> (Connection, ProjectionStore) {
        let mut connection = Connection::open_in_memory().unwrap();
        crate::store::schema::create_canonical_v1(&mut connection).unwrap();
        connection
            .execute_batch(
                "
                INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
                    VALUES (1, 'a', 'image/png', 1, 'now'),
                           (2, 'b', 'image/png', 1, 'now'),
                           (3, 'c', 'image/png', 1, 'now');
                INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                    VALUES (10, 'media-a', 'media', 'now', 'now'),
                           (11, 'media-b', 'media', 'now', 'now'),
                           (12, 'media-c', 'media', 'now', 'now'),
                           (20, 'collection-a', 'collection', 'now', 'now');
                INSERT INTO media_asset (item_id, file_id, imported_at, updated_at)
                    VALUES (10, 1, 'now', 'now'), (11, 2, 'now', 'now'), (12, 3, 'now', 'now');
                INSERT INTO library_root (item_id, lifecycle)
                    VALUES (20, 'active'), (11, 'active'), (12, 'inbox');
                INSERT INTO root_metadata (root_item_id, name, updated_at)
                    VALUES (20, 'Collection A', 'now'),
                           (11, 'Media B', 'now'),
                           (12, 'Media C', 'now');
                INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                    VALUES (20, 10, 1);
                INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at)
                    VALUES (7, 'folder-a', 'A', 'now', 'now');
                INSERT INTO folder_item (folder_id, item_id) VALUES (7, 20);
                INSERT INTO tag (tag_id, subtag) VALUES (100, 'child'), (101, 'parent');
                INSERT INTO root_tag (root_item_id, tag_id) VALUES (20, 100);
                ",
            )
            .unwrap();
        let projection = ProjectionStore::from_connection(&connection).unwrap();
        (connection, projection)
    }

    #[test]
    fn full_initialization_uses_collection_and_standalone_root_ids() {
        let (_connection, projection) = fixture();

        assert_eq!(projection.root_for_media(10), Some(20));
        assert_eq!(projection.root_for_media(11), Some(11));
        assert_eq!(
            projection.active_bitmap(),
            RoaringBitmap::from_iter([11, 20])
        );
        assert_eq!(projection.inbox_bitmap(), RoaringBitmap::from_iter([12]));
        assert!(projection.trash_bitmap().is_empty());
        assert_eq!(projection.folder_bitmap(7), RoaringBitmap::from_iter([20]));
    }

    #[test]
    fn image_compatibility_tracks_standalone_and_collection_structure() {
        let (_connection, projection) = fixture();
        let all_roots = RoaringBitmap::from_iter([11, 12, 20]);
        assert!(projection
            .selection_snapshot()
            .all_media_are_images(&all_roots));

        projection
            .apply_structure_delta(StructureProjectionDelta {
                media_classifications: vec![MediaClassificationProjectionChange {
                    media_id: 11,
                    is_image: false,
                }],
                roots: vec![RootProjectionChange {
                    item_id: 11,
                    lifecycle: None,
                }],
                memberships: vec![MembershipProjectionChange {
                    collection_id: 20,
                    media_id: 11,
                    present: true,
                }],
                ..StructureProjectionDelta::default()
            })
            .unwrap();
        let snapshot = projection.selection_snapshot();
        assert!(!snapshot.all_media_are_images(&RoaringBitmap::from_iter([20])));

        projection
            .apply_structure_delta(StructureProjectionDelta {
                roots: vec![RootProjectionChange {
                    item_id: 11,
                    lifecycle: Some(Lifecycle::Active),
                }],
                memberships: vec![MembershipProjectionChange {
                    collection_id: 20,
                    media_id: 11,
                    present: false,
                }],
                ..StructureProjectionDelta::default()
            })
            .unwrap();
        let snapshot = projection.selection_snapshot();
        assert!(snapshot.all_media_are_images(&RoaringBitmap::from_iter([20])));
        assert!(!snapshot.all_media_are_images(&RoaringBitmap::from_iter([11])));
    }

    #[test]
    fn direct_tag_matches_project_to_collection_root() {
        let (_connection, projection) = fixture();

        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([20])
        );
        assert!(!projection.direct_tag_bitmap(100).contains(10));
    }

    #[test]
    fn root_owned_tags_are_the_only_canonical_tag_source() {
        let (_connection, projection) = fixture();

        projection
            .apply_root_tag_bitmap(200, &RoaringBitmap::from_iter([11]), true)
            .unwrap();

        let state = projection.state.load();
        assert_eq!(
            state.root_owned_tags.get(&11).map(|tags| tags.as_slice()),
            Some(&[200][..])
        );
    }

    #[test]
    fn incremental_deltas_move_root_scoped_matches() {
        let (_connection, projection) = fixture();

        projection
            .apply_lifecycle_delta(11, Lifecycle::Trash)
            .unwrap();
        assert_eq!(projection.trash_bitmap(), RoaringBitmap::from_iter([11]));

        projection
            .apply_root_tag_bitmap(100, &RoaringBitmap::from_iter([11]), true)
            .unwrap();
        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([11, 20])
        );

        projection
            .apply_root_delta(11, ItemKind::Media, None)
            .unwrap();
        projection.apply_membership_delta(20, 11, true).unwrap();
        assert_eq!(projection.root_for_media(11), Some(20));
        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([20])
        );
        assert!(!projection.active_bitmap().contains(10));
    }

    #[test]
    fn folder_projection_excludes_non_active_roots() {
        let (connection, projection) = fixture();
        connection
            .execute(
                "INSERT INTO folder_item (folder_id, item_id) VALUES (7, 12)",
                [],
            )
            .unwrap();
        projection.reload(&connection).unwrap();

        assert_eq!(projection.folder_bitmap(7), RoaringBitmap::from_iter([20]));
    }

    #[test]
    fn replace_with_publishes_an_already_built_projection() {
        let (connection, projection) = fixture();
        connection
            .execute(
                "UPDATE library_root SET lifecycle = 'trash' WHERE item_id = 11",
                [],
            )
            .unwrap();
        let rebuilt = ProjectionStore::from_connection(&connection).unwrap();

        projection.replace_with(rebuilt);

        assert!(projection.trash_bitmap().contains(11));
        assert!(!projection.active_bitmap().contains(11));
    }

    #[test]
    fn prepared_delta_is_invisible_until_publication() {
        let (_connection, projection) = fixture();
        let prepared = projection
            .prepare(|candidate| candidate.apply_lifecycle_delta(11, Lifecycle::Trash))
            .unwrap();

        assert!(projection.active_bitmap().contains(11));
        assert!(!projection.trash_bitmap().contains(11));

        projection.publish_prepared(prepared);
        assert!(!projection.active_bitmap().contains(11));
        assert!(projection.trash_bitmap().contains(11));
    }

    #[test]
    fn structure_delta_does_not_move_root_owned_tags_between_collections() {
        let (connection, projection) = fixture();
        connection
            .execute(
                "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
                 VALUES (21, 'collection-b', 'collection', 'now', 'now')",
                [],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO library_root (item_id, lifecycle) VALUES (21, 'active')",
                [],
            )
            .unwrap();
        projection.reload(&connection).unwrap();

        projection
            .apply_structure_delta(StructureProjectionDelta {
                memberships: vec![MembershipProjectionChange {
                    collection_id: 21,
                    media_id: 10,
                    present: true,
                }],
                ..StructureProjectionDelta::default()
            })
            .unwrap();

        assert_eq!(projection.root_for_media(10), Some(21));
        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([20])
        );
    }

    #[test]
    fn root_owned_tag_survives_member_changes() {
        let (_connection, projection) = fixture();

        projection.apply_membership_delta(20, 11, true).unwrap();
        projection.apply_membership_delta(20, 10, false).unwrap();

        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([20])
        );

        projection.apply_membership_delta(20, 11, false).unwrap();
        assert!(projection.direct_tag_bitmap(100).contains(20));
    }

    #[test]
    fn numeric_bit_slices_return_exact_filtered_aggregates() {
        let (_connection, projection) = fixture();
        projection
            .apply_root_summary_changes(
                &[
                    RootSummaryProjectionChange {
                        item_id: 11,
                        total_size_bytes: 100,
                        media_count: 1,
                        rating: Some(4),
                    },
                    RootSummaryProjectionChange {
                        item_id: 20,
                        total_size_bytes: 900,
                        media_count: 3,
                        rating: None,
                    },
                ],
                &RoaringBitmap::new(),
            )
            .unwrap();

        let selected = RoaringBitmap::from_iter([11, 20, 999]);
        let aggregate = projection.numeric_aggregates(&selected);
        assert_eq!(aggregate.total_size_bytes.count, 2);
        assert_eq!(aggregate.total_size_bytes.sum, 1_000);
        assert_eq!(aggregate.media_count.count, 2);
        assert_eq!(aggregate.media_count.sum, 4);
        assert_eq!(aggregate.rating.count, 1);
        assert_eq!(aggregate.rating.sum, 4);

        projection
            .apply_rating_bitmap(&RoaringBitmap::from_iter([11, 20]), Some(2))
            .unwrap();
        assert_eq!(projection.rating_aggregate(&selected).count, 2);
        assert_eq!(projection.rating_aggregate(&selected).sum, 4);
    }

    #[test]
    fn numeric_reader_snapshot_stays_immutable_during_publication() {
        let (_connection, projection) = fixture();
        let selected = RoaringBitmap::from_iter([11]);
        let before = projection.numeric_snapshot();

        projection
            .apply_root_summary_changes(
                &[RootSummaryProjectionChange {
                    item_id: 11,
                    total_size_bytes: 321,
                    media_count: 7,
                    rating: Some(5),
                }],
                &RoaringBitmap::new(),
            )
            .unwrap();

        assert_eq!(before.numeric.total_size_bytes.filtered_sum(&selected), 1);
        assert_eq!(projection.total_size_aggregate(&selected).sum, 321);
    }

    #[test]
    fn publication_retains_old_snapshot_and_reuses_unchanged_components() {
        let (_connection, projection) = fixture();
        let before = projection.state.load();
        let before_folders = Arc::clone(&before.folder_members.shards[0].0);
        let before_tags = Arc::clone(&before.direct_tag_bitmaps.shards[0].0);

        projection
            .apply_lifecycle_delta(11, Lifecycle::Trash)
            .unwrap();

        let after = projection.state.load();
        assert!(before.lifecycle_bitmaps[super::lifecycle_index(Lifecycle::Active)].contains(11));
        assert!(!before.lifecycle_bitmaps[super::lifecycle_index(Lifecycle::Trash)].contains(11));
        assert!(!after.lifecycle_bitmaps[super::lifecycle_index(Lifecycle::Active)].contains(11));
        assert!(after.lifecycle_bitmaps[super::lifecycle_index(Lifecycle::Trash)].contains(11));
        assert!(Arc::ptr_eq(
            &before_folders,
            &after.folder_members.shards[0].0
        ));
        assert!(Arc::ptr_eq(
            &before_tags,
            &after.direct_tag_bitmaps.shards[0].0
        ));
        assert!(!Arc::ptr_eq(
            &before.lifecycle_bitmaps,
            &after.lifecycle_bitmaps
        ));
    }

    #[test]
    fn retained_reader_snapshot_does_not_block_writer_publication() {
        let (_connection, projection) = fixture();
        let projection = Arc::new(projection);
        let retained = projection.state.load();
        let writer = Arc::clone(&projection);

        thread::spawn(move || writer.apply_lifecycle_delta(11, Lifecycle::Trash).unwrap())
            .join()
            .unwrap();

        assert!(retained.lifecycle_bitmaps[super::lifecycle_index(Lifecycle::Active)].contains(11));
        assert!(projection.trash_bitmap().contains(11));
    }

    #[test]
    fn bulk_lifecycle_publication_moves_one_hundred_thousand_roots() {
        let projection = ProjectionStore::new();
        let roots = RoaringBitmap::from_sorted_iter(1..=100_000).unwrap();
        {
            let mut state = projection.write_state();
            for item_id in &roots {
                let item_id = i64::from(item_id);
                state.media_ids.insert(item_id);
                state.media_to_root.insert(item_id, item_id);
                set_lifecycle(&mut state, item_id, Lifecycle::Active);
            }
            rebuild_all_derived(&mut state);
        }
        let reader_snapshot = projection.numeric_snapshot();

        let started = Instant::now();
        projection
            .apply_lifecycle_bitmap(&roots, Lifecycle::Trash)
            .unwrap();
        let elapsed = started.elapsed();
        eprintln!("100k lifecycle projection publication: {elapsed:?}");

        assert_eq!(projection.trash_bitmap(), roots);
        assert!(projection.active_bitmap().is_empty());
        assert_eq!(
            reader_snapshot.lifecycle_bitmaps[super::lifecycle_index(Lifecycle::Active)].len(),
            100_000
        );
        assert!(
            elapsed < Duration::from_millis(250),
            "100k lifecycle projection publication exceeded budget: {elapsed:?}"
        );
    }

    #[test]
    fn broad_numeric_aggregate_is_independent_of_selected_root_count() {
        let projection = ProjectionStore::new();
        let roots = RoaringBitmap::from_sorted_iter(1..=100_000).unwrap();
        {
            let mut state = projection.write_state();
            for item_id in &roots {
                let item_id = i64::from(item_id);
                state.media_ids.insert(item_id);
                state.media_to_root.insert(item_id, item_id);
                set_lifecycle(&mut state, item_id, Lifecycle::Active);
            }
            rebuild_all_derived(&mut state);
        }
        let changes = roots
            .iter()
            .map(|item_id| RootSummaryProjectionChange {
                item_id: i64::from(item_id),
                total_size_bytes: 10,
                media_count: 1,
                rating: Some(5),
            })
            .collect::<Vec<_>>();
        projection
            .apply_root_summary_changes(&changes, &RoaringBitmap::new())
            .unwrap();

        let started = Instant::now();
        let aggregate = projection.numeric_aggregates(&roots);
        let elapsed = started.elapsed();
        eprintln!("100k-root numeric aggregate: {elapsed:?}");

        assert_eq!(aggregate.total_size_bytes.sum, 1_000_000);
        assert_eq!(aggregate.media_count.sum, 100_000);
        assert_eq!(aggregate.rating.sum, 500_000);
        assert!(
            elapsed < Duration::from_millis(5),
            "100k-root aggregate exceeded budget: {elapsed:?}"
        );
    }

    #[test]
    fn canonical_root_tags_accumulate_without_member_tag_state() {
        let (connection, projection) = fixture();
        connection
            .execute(
                "INSERT INTO root_tag(root_item_id, tag_id)
                 VALUES (11, 100), (20, 101)",
                [],
            )
            .unwrap();
        projection.reload(&connection).unwrap();

        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([11, 20])
        );
        assert_eq!(
            projection.direct_tag_bitmap(101),
            RoaringBitmap::from_iter([20])
        );
    }

    #[test]
    fn root_tag_bitmap_preserves_direct_matches() {
        let (connection, projection) = fixture();
        connection
            .execute("INSERT INTO tag(tag_id, subtag) VALUES (200, 'plain')", [])
            .unwrap();
        connection
            .execute(
                "INSERT INTO root_tag(root_item_id, tag_id) VALUES (20, 101)",
                [],
            )
            .unwrap();
        projection.reload(&connection).unwrap();

        projection
            .apply_root_tag_bitmap(200, &RoaringBitmap::from_iter([11, 20]), true)
            .unwrap();
        assert_eq!(
            projection.direct_tag_bitmap(200),
            RoaringBitmap::from_iter([11, 20])
        );
        projection
            .apply_root_tag_bitmap(200, &RoaringBitmap::from_iter([11]), false)
            .unwrap();
        assert_eq!(
            projection.direct_tag_bitmap(200),
            RoaringBitmap::from_iter([20])
        );
        projection
            .apply_root_tag_bitmap(100, &RoaringBitmap::from_iter([20]), true)
            .unwrap();
        assert_eq!(
            projection.direct_tag_bitmap(100),
            RoaringBitmap::from_iter([20])
        );
        assert_eq!(
            projection.direct_tag_bitmap(101),
            RoaringBitmap::from_iter([20])
        );

        projection
            .apply_root_tag_bitmap(100, &RoaringBitmap::from_iter([20]), false)
            .unwrap();
        assert!(projection.direct_tag_bitmap(100).is_empty());
        assert_eq!(
            projection.direct_tag_bitmap(101),
            RoaringBitmap::from_iter([20])
        );
    }

    #[test]
    fn root_tag_bitmap_publishes_one_hundred_thousand_roots_without_media_walk() {
        let projection = ProjectionStore::new();
        let roots = RoaringBitmap::from_sorted_iter(1..=100_000).unwrap();
        {
            let mut state = projection.write_state();
            for item_id in &roots {
                let item_id = i64::from(item_id);
                state.media_ids.insert(item_id);
                state.media_to_root.insert(item_id, item_id);
                set_lifecycle(&mut state, item_id, Lifecycle::Active);
            }
            rebuild_all_derived(&mut state);
        }

        let started = Instant::now();
        projection.apply_root_tag_bitmap(900, &roots, true).unwrap();
        let elapsed = started.elapsed();
        eprintln!("100k root-tag projection publication: {elapsed:?}");

        assert_eq!(projection.direct_tag_bitmap(900), roots);
        assert!(
            elapsed < Duration::from_millis(250),
            "100k root-tag projection publication exceeded budget: {elapsed:?}"
        );
    }
}
