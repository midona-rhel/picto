use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;
use std::iter::FromIterator;
use std::ops::{BitAnd, BitAndAssign, BitOrAssign, BitXor, Deref, SubAssign};
use std::sync::Arc;

use arc_swap::ArcSwap;
use roaring::RoaringBitmap;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::model::{FolderId, LabColor, Lifecycle, MediaId, Rating, RootId, RootKind, TagId};
use crate::ordering::{self, OrderOwnerKind};
use crate::predicate::ViewQuerySpec;
use crate::query_request::{LoadStage, OpenStage};
use crate::{LibraryDatabase, Result};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SharedBitmap(Arc<RoaringBitmap>);

impl SharedBitmap {
    pub(crate) fn allocation_id(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    pub(crate) fn insert(&mut self, value: u32) -> bool {
        Arc::make_mut(&mut self.0).insert(value)
    }

    pub(crate) fn remove(&mut self, value: u32) -> bool {
        Arc::make_mut(&mut self.0).remove(value)
    }

    pub(crate) fn subtract(&mut self, values: &RoaringBitmap) {
        *Arc::make_mut(&mut self.0) -= values;
    }

    pub(crate) fn union(&mut self, values: &RoaringBitmap) {
        *Arc::make_mut(&mut self.0) |= values;
    }

    pub(crate) fn to_bitmap(&self) -> RoaringBitmap {
        (*self.0).clone()
    }
}

impl From<RoaringBitmap> for SharedBitmap {
    fn from(value: RoaringBitmap) -> Self {
        Self(Arc::new(value))
    }
}

impl FromIterator<u32> for SharedBitmap {
    fn from_iter<T: IntoIterator<Item = u32>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<RoaringBitmap>())
    }
}

impl BitOrAssign<&RoaringBitmap> for SharedBitmap {
    fn bitor_assign(&mut self, rhs: &RoaringBitmap) {
        self.union(rhs);
    }
}

impl BitOrAssign<RoaringBitmap> for SharedBitmap {
    fn bitor_assign(&mut self, rhs: RoaringBitmap) {
        self.union(&rhs);
    }
}

impl BitOrAssign<&SharedBitmap> for SharedBitmap {
    fn bitor_assign(&mut self, rhs: &SharedBitmap) {
        self.union(rhs);
    }
}

impl SubAssign<&RoaringBitmap> for SharedBitmap {
    fn sub_assign(&mut self, rhs: &RoaringBitmap) {
        self.subtract(rhs);
    }
}

impl BitAndAssign<&RoaringBitmap> for SharedBitmap {
    fn bitand_assign(&mut self, rhs: &RoaringBitmap) {
        *Arc::make_mut(&mut self.0) &= rhs;
    }
}

impl<'a> BitAnd<&'a RoaringBitmap> for &'a SharedBitmap {
    type Output = RoaringBitmap;

    fn bitand(self, rhs: &'a RoaringBitmap) -> Self::Output {
        self.deref() & rhs
    }
}

impl<'a> BitXor<&'a SharedBitmap> for &'a SharedBitmap {
    type Output = RoaringBitmap;

    fn bitxor(self, rhs: &'a SharedBitmap) -> Self::Output {
        self.deref() ^ rhs.deref()
    }
}

impl<'a> IntoIterator for &'a SharedBitmap {
    type Item = u32;
    type IntoIter = roaring::bitmap::Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl BitOrAssign<&SharedBitmap> for RoaringBitmap {
    fn bitor_assign(&mut self, rhs: &SharedBitmap) {
        *self |= rhs.deref();
    }
}

impl Deref for SharedBitmap {
    type Target = RoaringBitmap;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

const ID_MAP_SHARD_SHIFT: u32 = 10;
const ID_MAP_SHARD_MASK: u32 = (1 << ID_MAP_SHARD_SHIFT) - 1;

/// Signed canonical timestamps, ordered without losing pre-epoch values.
/// Date filters deliberately retain their existing nonnegative/clamped semantics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DateIndex(NumericIndex);

impl DateIndex {
    fn key(value: i64) -> u64 {
        (value as u64) ^ (1u64 << 63)
    }

    pub fn insert(&mut self, id: u32, value: i64) {
        self.0.insert(id, Self::key(value));
    }

    pub fn remove(&mut self, id: u32) {
        self.0.remove(id);
    }

    /// The canonical signed timestamp, for cursor keys.
    pub fn signed_value(&self, id: u32) -> Option<i64> {
        self.0.value(id).map(|key| (key ^ (1u64 << 63)) as i64)
    }

    /// Existing nonnegative scalar consumers retain their pre-epoch clamping.
    pub fn value(&self, id: u32) -> Option<u64> {
        self.0
            .value(id)
            .map(|key| ((key ^ (1u64 << 63)) as i64).max(0) as u64)
    }

    pub fn between(&self, minimum: Option<u64>, maximum: Option<u64>) -> RoaringBitmap {
        if minimum.is_some_and(|value| value > i64::MAX as u64) {
            return RoaringBitmap::new();
        }
        let minimum = minimum
            .filter(|&value| value > 0)
            .map(|value| Self::key(value as i64));
        let maximum = maximum.map(|value| Self::key(value.min(i64::MAX as u64) as i64));
        self.0.between(minimum, maximum)
    }

    pub fn between_within(
        &self,
        minimum: Option<u64>,
        maximum: Option<u64>,
        within: &RoaringBitmap,
    ) -> RoaringBitmap {
        if minimum.is_some_and(|value| value > i64::MAX as u64) {
            return RoaringBitmap::new();
        }
        let minimum = minimum
            .filter(|&value| value > 0)
            .map(|value| Self::key(value as i64));
        let maximum = maximum.map(|value| Self::key(value.min(i64::MAX as u64) as i64));
        self.0.between_within(minimum, maximum, within)
    }

    pub(crate) fn ordered_window(
        &self,
        selection: &RoaringBitmap,
        start: usize,
        limit: usize,
        descending: bool,
        missing: bool,
    ) -> Result<Option<Vec<u32>>> {
        let index = &self.0.merged_order()?.index;
        let absent = selection - &index.present;
        if absent.is_empty() || !missing {
            return self.0.ordered_window(selection, start, limit, descending);
        }
        // SQL's COALESCE(date, -1) ties missing dates with actual -1 values.
        // Select three disjoint ranges; never materialize a full matching order.
        let below = index.between(None, Some(Self::key(-2)), selection & &index.present);
        let equal = index.between(
            Some(Self::key(-1)),
            Some(Self::key(-1)),
            selection & &index.present,
        ) | absent;
        let above = index.between(Some(Self::key(0)), None, selection & &index.present);
        let mut groups = [(below, false), (equal, true), (above, false)];
        if descending {
            groups.reverse();
        }
        let mut skip = start;
        let mut output = Vec::with_capacity(limit.min(selection.len() as usize));
        for (ids, equal) in groups {
            crate::query_request::check()?;
            if skip >= ids.len() as usize {
                skip -= ids.len() as usize;
                continue;
            }
            select_numeric_window(
                if equal { &[] } else { &index.slices },
                &ids,
                skip,
                limit - output.len(),
                descending,
                &mut output,
            )?;
            skip = 0;
            if output.len() == limit {
                break;
            }
        }
        Ok(Some(output))
    }

    fn estimated_bytes(&self) -> usize {
        self.0.estimated_bytes()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShardedIdMap<V> {
    shards: BTreeMap<u32, Arc<HashMap<u16, V>>>,
}

impl<V> Default for ShardedIdMap<V> {
    fn default() -> Self {
        Self {
            shards: BTreeMap::new(),
        }
    }
}

impl<V: Clone> ShardedIdMap<V> {
    pub fn get(&self, id: u32) -> Option<&V> {
        self.shards
            .get(&(id >> ID_MAP_SHARD_SHIFT))
            .and_then(|shard| shard.get(&((id & ID_MAP_SHARD_MASK) as u16)))
    }

    pub(crate) fn insert(&mut self, id: u32, value: V) -> Option<V> {
        Arc::make_mut(
            self.shards
                .entry(id >> ID_MAP_SHARD_SHIFT)
                .or_insert_with(|| Arc::new(HashMap::new())),
        )
        .insert((id & ID_MAP_SHARD_MASK) as u16, value)
    }

    pub(crate) fn remove(&mut self, id: u32) -> Option<V> {
        let high = id >> ID_MAP_SHARD_SHIFT;
        let shard = self.shards.get_mut(&high)?;
        let result = Arc::make_mut(shard).remove(&((id & ID_MAP_SHARD_MASK) as u16));
        if shard.is_empty() {
            self.shards.remove(&high);
        }
        result
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.shards
            .values()
            .map(|shard| {
                shard.capacity()
                    * (std::mem::size_of::<u16>()
                        + std::mem::size_of::<V>()
                        + 2 * std::mem::size_of::<usize>())
            })
            .sum::<usize>()
            + self.shards.len()
                * (std::mem::size_of::<u32>() + std::mem::size_of::<Arc<HashMap<u16, V>>>())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NumericIndex {
    shards: BTreeMap<u16, Arc<NumericShard>>,
    #[serde(skip)]
    order_slices: std::sync::OnceLock<Arc<NumericOrderIndex>>,
}

#[derive(Debug)]
struct NumericOrderIndex {
    index: NumericShard,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct NumericShard {
    present: RoaringBitmap,
    slices: Vec<RoaringBitmap>,
}

impl NumericIndex {
    pub fn insert(&mut self, id: u32, value: u64) {
        self.order_slices.take();
        let shard = Arc::make_mut(
            self.shards
                .entry((id >> 16) as u16)
                .or_insert_with(|| Arc::new(NumericShard::default())),
        );
        shard.remove(id);
        shard.present.insert(id);
        let bits = (64 - value.leading_zeros()).max(1) as usize;
        if shard.slices.len() < bits {
            shard.slices.resize_with(bits, RoaringBitmap::new);
        }
        for (bit, slice) in shard.slices.iter_mut().enumerate() {
            if value & (1u64 << bit) != 0 {
                slice.insert(id);
            }
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.order_slices.take();
        let high = (id >> 16) as u16;
        let Some(shard) = self.shards.get_mut(&high) else {
            return;
        };
        let shard = Arc::make_mut(shard);
        shard.remove(id);
        if shard.present.is_empty() {
            self.shards.remove(&high);
        }
    }

    pub fn value(&self, id: u32) -> Option<u64> {
        self.shards
            .get(&((id >> 16) as u16))
            .and_then(|shard| shard.value(id))
    }

    pub fn between(&self, minimum: Option<u64>, maximum: Option<u64>) -> RoaringBitmap {
        let mut result = RoaringBitmap::new();
        for shard in self.shards.values() {
            result |= shard.between(minimum, maximum, shard.present.clone());
        }
        result
    }

    /// `between` restricted to `within`. Work is proportional to the overlap,
    /// so evaluating a filter over a few changed roots stays cheap.
    pub fn between_within(
        &self,
        minimum: Option<u64>,
        maximum: Option<u64>,
        within: &RoaringBitmap,
    ) -> RoaringBitmap {
        let mut result = RoaringBitmap::new();
        if within.is_empty() {
            return result;
        }
        for shard in self.shards.values() {
            let base = &shard.present & within;
            if !base.is_empty() {
                result |= shard.between(minimum, maximum, base);
            }
        }
        result
    }

    pub fn sum(&self, selection: &RoaringBitmap) -> u128 {
        self.shards.values().map(|shard| shard.sum(selection)).sum()
    }

    /// Radix-select an ordinal window without materializing the matching order.
    /// Equal values use ID order, reversed together with values for descending.
    pub(crate) fn ordered_window(
        &self,
        selection: &RoaringBitmap,
        start: usize,
        limit: usize,
        descending: bool,
    ) -> Result<Option<Vec<u32>>> {
        crate::query_request::check()?;
        if selection.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let merged = &self.merged_order()?.index;
        if !selection.is_subset(&merged.present) {
            return Ok(None);
        }
        let mut output = Vec::with_capacity(limit.min(selection.len() as usize));
        select_numeric_window(
            &merged.slices,
            selection,
            start,
            limit,
            descending,
            &mut output,
        )?;
        Ok(Some(output))
    }

    /// An ordinal window over every selected root. Roots the index holds no
    /// value for sort as the smallest value, in ID order, so a selection wider
    /// than the index still has one total order.
    pub(crate) fn ordered_window_total(
        &self,
        selection: &RoaringBitmap,
        start: usize,
        limit: usize,
        descending: bool,
    ) -> Result<Vec<u32>> {
        if let Some(ids) = self.ordered_window(selection, start, limit, descending)? {
            return Ok(ids);
        }
        let merged = &self.merged_order()?.index;
        let present = selection & &merged.present;
        let absent = selection - &present;
        let mut groups = [(absent, true), (present, false)];
        if descending {
            groups.reverse();
        }
        let mut skip = start;
        let mut output = Vec::with_capacity(limit.min(selection.len() as usize));
        for (ids, plain) in groups {
            crate::query_request::check()?;
            if skip >= ids.len() as usize {
                skip -= ids.len() as usize;
                continue;
            }
            select_numeric_window(
                if plain { &[] } else { &merged.slices },
                &ids,
                skip,
                limit - output.len(),
                descending,
                &mut output,
            )?;
            skip = 0;
            if output.len() == limit {
                break;
            }
        }
        Ok(output)
    }

    fn merged_order(&self) -> Result<&NumericOrderIndex> {
        // Shared across queries, not across writes. This bounded bit-slice view
        // avoids re-merging the same projection on every scroll window.
        if self.order_slices.get().is_none() {
            let mut merged = NumericShard::default();
            for shard in self.shards.values() {
                crate::query_request::check()?;
                merged.present |= &shard.present;
                merged.slices.resize_with(
                    merged.slices.len().max(shard.slices.len()),
                    RoaringBitmap::new,
                );
                for (target, source) in merged.slices.iter_mut().zip(&shard.slices) {
                    crate::query_request::check()?;
                    *target |= source;
                }
            }
            crate::query_request::check()?;
            let _ = self
                .order_slices
                .set(Arc::new(NumericOrderIndex { index: merged }));
        }
        Ok(self
            .order_slices
            .get()
            .expect("complete numeric ordering index"))
    }

    fn estimated_bytes(&self) -> usize {
        self.shards
            .values()
            .map(|shard| shard.estimated_bytes())
            .sum::<usize>()
            + self
                .order_slices
                .get()
                .map_or(0, |index| index.index.estimated_bytes())
            + self.shards.len()
                * (std::mem::size_of::<u16>() + std::mem::size_of::<Arc<NumericShard>>())
    }
}

fn select_numeric_window(
    slices: &[RoaringBitmap],
    matches: &RoaringBitmap,
    start: usize,
    limit: usize,
    descending: bool,
    output: &mut Vec<u32>,
) -> Result<()> {
    crate::query_request::check()?;
    if start >= matches.len() as usize || limit == 0 {
        return Ok(());
    }
    let Some((slice, lower)) = slices.split_last() else {
        if descending {
            let last = matches
                .select((matches.len() as usize - 1 - start) as u32)
                .unwrap();
            output.extend(matches.range(..=last).rev().take(limit));
        } else {
            let first = matches.select(start as u32).unwrap();
            output.extend(matches.range(first..).take(limit));
        }
        return Ok(());
    };
    if matches.len() == 1 {
        output.extend(matches);
        return Ok(());
    }
    // Finish small radix buckets directly. Splitting down to individual IDs
    // would allocate thousands of tiny bitmaps for a normal 1,500-item window.
    if matches.len() <= 256 && slices.len() <= 16 {
        let mut values = matches
            .iter()
            .map(|id| {
                let value = slices.iter().enumerate().fold(0u64, |value, (bit, slice)| {
                    value | (u64::from(slice.contains(id)) << bit)
                });
                (value, id)
            })
            .collect::<Vec<_>>();
        values.sort_unstable();
        if descending {
            output.extend(
                values
                    .into_iter()
                    .rev()
                    .skip(start)
                    .take(limit)
                    .map(|(_, id)| id),
            );
        } else {
            output.extend(values.into_iter().skip(start).take(limit).map(|(_, id)| id));
        }
        return Ok(());
    }
    let ones_len = matches.intersection_len(slice);
    if ones_len == 0 || ones_len == matches.len() {
        return select_numeric_window(lower, matches, start, limit, descending, output);
    }
    let ones = matches & slice;
    let zeros = matches - &ones;
    let (first, second) = if descending {
        (ones, zeros)
    } else {
        (zeros, ones)
    };
    let first_len = first.len() as usize;
    let before = output.len();
    if start < first_len {
        select_numeric_window(lower, &first, start, limit, descending, output)?;
    }
    let remaining = limit - (output.len() - before);
    if remaining > 0 {
        select_numeric_window(
            lower,
            &second,
            start.saturating_sub(first_len),
            remaining,
            descending,
            output,
        )?;
    }
    Ok(())
}

impl NumericShard {
    fn remove(&mut self, id: u32) {
        self.present.remove(id);
        for slice in &mut self.slices {
            slice.remove(id);
        }
    }

    fn value(&self, id: u32) -> Option<u64> {
        self.present.contains(id).then(|| {
            self.slices
                .iter()
                .enumerate()
                .fold(0, |value, (bit, slice)| {
                    value | (u64::from(slice.contains(id)) << bit)
                })
        })
    }

    fn between(
        &self,
        minimum: Option<u64>,
        maximum: Option<u64>,
        base: RoaringBitmap,
    ) -> RoaringBitmap {
        let mut result = base;
        if let Some(minimum) = minimum {
            result -= &self.less_than(minimum, &result);
        }
        if let Some(exclusive) = maximum.and_then(|value| value.checked_add(1)) {
            result &= self.less_than(exclusive, &result);
        }
        result
    }

    fn sum(&self, selection: &RoaringBitmap) -> u128 {
        self.slices
            .iter()
            .enumerate()
            .map(|(bit, slice)| slice.intersection_len(selection) as u128 * (1u128 << bit))
            .sum()
    }

    fn estimated_bytes(&self) -> usize {
        self.present.serialized_size()
            + self
                .slices
                .iter()
                .map(RoaringBitmap::serialized_size)
                .sum::<usize>()
            + self.slices.capacity() * std::mem::size_of::<RoaringBitmap>()
    }

    /// Members of `base` (a subset of `present`) whose value is below `value`.
    fn less_than(&self, value: u64, base: &RoaringBitmap) -> RoaringBitmap {
        if value == 0 {
            return RoaringBitmap::new();
        }
        // Walk the bits from the top, keeping the members still equal to
        // `value` so far; no slice is copied, and the walk stops as soon as
        // nothing is left equal.
        let empty = RoaringBitmap::new();
        let mut equal = base.clone();
        let mut less = RoaringBitmap::new();
        let highest = self.slices.len().max(64 - value.leading_zeros() as usize);
        for bit in (0..highest).rev() {
            if equal.is_empty() {
                break;
            }
            let ones = self.slices.get(bit).unwrap_or(&empty);
            if bit < 64 && value & (1u64 << bit) != 0 {
                less |= &equal - ones;
                equal &= ones;
            } else {
                equal -= ones;
            }
        }
        less
    }
}

/// One immutable recent-view projection: display order and candidate membership
/// are built together from the same canonical rows.
#[derive(Debug, Clone, Default)]
pub struct RecentViews {
    ordered: Vec<(RootId, i64)>,
    pub(crate) members: RoaringBitmap,
}

impl From<Vec<(RootId, i64)>> for RecentViews {
    fn from(ordered: Vec<(RootId, i64)>) -> Self {
        let members = ordered.iter().map(|(id, _)| id.0).collect();
        Self { ordered, members }
    }
}

impl Deref for RecentViews {
    type Target = [(RootId, i64)];

    fn deref(&self) -> &Self::Target {
        &self.ordered
    }
}

#[derive(Debug, Clone)]
pub struct ProjectionSnapshot {
    pub revision: u64,
    pub subscription_counts: Arc<crate::subscription_counts::SubscriptionCounts>,
    pub text_index: Arc<crate::text_index::TextIndex>,
    pub name_index: Arc<crate::name_index::NameIndex>,
    /// Drawing facts of every root, hydrated for grid summaries without SQLite.
    pub drawing: Arc<crate::browsing::DrawingIndex>,
    /// Recently viewed roots, most recent first; ties break by descending ID.
    pub recent_views: Arc<RecentViews>,
    /// Parent of every folder, for folder-tree scopes.
    pub folder_parents: Arc<HashMap<FolderId, Option<FolderId>>>,
    pub(crate) query_versions: crate::query_dependencies::QueryVersions,
    pub lifecycle: Arc<HashMap<Lifecycle, SharedBitmap>>,
    pub ratings: Arc<HashMap<Rating, SharedBitmap>>,
    pub tags: Arc<HashMap<TagId, SharedBitmap>>,
    pub tag_ids_by_name: Arc<HashMap<String, TagId>>,
    pub folder_orders: Arc<HashMap<FolderId, Arc<Vec<RootId>>>>,
    pub folders: Arc<HashMap<FolderId, SharedBitmap>>,
    pub collection_orders: Arc<HashMap<RootId, Arc<Vec<MediaId>>>>,
    pub media_owner: Arc<ShardedIdMap<RootId>>,
    pub image_media: Arc<RoaringBitmap>,
    pub roots_with_images: Arc<RoaringBitmap>,
    pub root_kinds: Arc<HashMap<RootKind, SharedBitmap>>,
    pub mime: Arc<HashMap<String, SharedBitmap>>,
    pub mime_family: Arc<HashMap<String, SharedBitmap>>,
    pub color_cells: Arc<HashMap<u32, SharedBitmap>>,
    pub cover_palettes: Arc<ShardedIdMap<Arc<Vec<LabColor>>>>,
    pub tag_count: Arc<NumericIndex>,
    pub folder_count: Arc<NumericIndex>,
    pub total_bytes: Arc<NumericIndex>,
    pub media_count: Arc<NumericIndex>,
    pub width: Arc<NumericIndex>,
    pub height: Arc<NumericIndex>,
    pub duration: Arc<NumericIndex>,
    pub imported_at: Arc<DateIndex>,
    pub captured_at: Arc<DateIndex>,
    pub modified_at: Arc<NumericIndex>,
    pub notes_present: Arc<RoaringBitmap>,
    pub urls_present: Arc<RoaringBitmap>,
    /// Each smart folder's own rules evaluated over active roots.
    pub smart_local_results: Arc<HashMap<u32, SharedBitmap>>,
    /// Local results intersected with every ancestor result.
    pub smart_results: Arc<HashMap<u32, SharedBitmap>>,
    /// Session-local cache version of each smart folder's result. It moves only
    /// when that folder's membership changes, so other folders stay cached.
    pub smart_result_versions: Arc<HashMap<u32, u64>>,
    /// Definitions exactly as saved by each smart folder.
    pub smart_queries: Arc<HashMap<u32, ViewQuerySpec>>,
    /// Local definitions composed with every ancestor definition.
    pub smart_effective_queries: Arc<HashMap<u32, ViewQuerySpec>>,
}

impl ProjectionSnapshot {
    pub fn active(&self) -> &RoaringBitmap {
        self.lifecycle
            .get(&Lifecycle::Active)
            .expect("every snapshot contains the active partition")
    }

    pub fn lifecycle(&self, lifecycle: Lifecycle) -> &RoaringBitmap {
        self.lifecycle
            .get(&lifecycle)
            .expect("every snapshot contains every lifecycle partition")
    }

    pub fn rating(&self, rating: Rating) -> &RoaringBitmap {
        self.ratings
            .get(&rating)
            .expect("every snapshot contains every rating partition")
    }

    pub fn estimated_bytes(&self) -> usize {
        let mut bytes = bitmap_map_estimated_bytes(&self.lifecycle)
            + bitmap_map_estimated_bytes(&self.ratings)
            + bitmap_map_estimated_bytes(&self.tags)
            + bitmap_map_estimated_bytes(&self.folders)
            + bitmap_map_estimated_bytes(&self.root_kinds)
            + bitmap_map_estimated_bytes(&self.mime)
            + bitmap_map_estimated_bytes(&self.mime_family)
            + bitmap_map_estimated_bytes(&self.color_cells)
            + shared_bitmap_pair_estimated_bytes(&self.smart_local_results, &self.smart_results)
            + self.notes_present.serialized_size()
            + self.urls_present.serialized_size()
            + self.image_media.serialized_size()
            + self.roots_with_images.serialized_size()
            + self.media_owner.estimated_bytes();
        bytes += self
            .tag_ids_by_name
            .keys()
            .map(|name| name.capacity())
            .sum::<usize>();
        bytes += self
            .folder_orders
            .values()
            .map(|values| values.capacity() * std::mem::size_of::<RootId>())
            .sum::<usize>();
        bytes += self
            .collection_orders
            .values()
            .map(|values| values.capacity() * std::mem::size_of::<MediaId>())
            .sum::<usize>();
        bytes += self.cover_palettes.estimated_bytes();
        bytes += [
            &self.tag_count,
            &self.folder_count,
            &self.total_bytes,
            &self.media_count,
            &self.width,
            &self.height,
            &self.duration,
            &self.modified_at,
        ]
        .into_iter()
        .map(|index| index.estimated_bytes())
        .sum::<usize>();
        bytes += self.imported_at.estimated_bytes() + self.captured_at.estimated_bytes();
        bytes += self
            .smart_queries
            .values()
            .map(|query| serde_json::to_vec(query).map_or(0, |value| value.len()))
            .sum::<usize>();
        bytes += self
            .smart_effective_queries
            .values()
            .map(|query| serde_json::to_vec(query).map_or(0, |value| value.len()))
            .sum::<usize>();
        bytes += self.recent_views.ordered.capacity() * std::mem::size_of::<(RootId, i64)>()
            + self.recent_views.members.serialized_size();
        bytes += self.folder_parents.capacity()
            * (std::mem::size_of::<FolderId>()
                + std::mem::size_of::<Option<FolderId>>()
                + 2 * std::mem::size_of::<usize>());
        bytes
            + self.text_index.estimated_bytes()
            + self.name_index.estimated_bytes()
            + self.drawing.estimated_bytes()
    }
}

fn bitmap_map_estimated_bytes<K>(values: &HashMap<K, SharedBitmap>) -> usize {
    values
        .values()
        .map(|bitmap| bitmap.serialized_size())
        .sum::<usize>()
        + values.capacity()
            * (std::mem::size_of::<SharedBitmap>() + 2 * std::mem::size_of::<usize>())
}

fn shared_bitmap_pair_estimated_bytes(
    first: &HashMap<u32, SharedBitmap>,
    second: &HashMap<u32, SharedBitmap>,
) -> usize {
    let mut allocations = std::collections::HashSet::new();
    let payload_bytes = first
        .values()
        .chain(second.values())
        .filter(|bitmap| allocations.insert(bitmap.allocation_id()))
        .map(|bitmap| bitmap.serialized_size())
        .sum::<usize>();
    payload_bytes
        + (first.capacity() + second.capacity())
            * (std::mem::size_of::<SharedBitmap>() + 2 * std::mem::size_of::<usize>())
}

pub struct ProjectionStore {
    snapshot: ArcSwap<ProjectionSnapshot>,
    load_statistics: ProjectionLoadStatistics,
}

/// Non-overlapping startup stages in microseconds. The resident text, name,
/// drawing and navigation state is rebuilt from canonical rows on every open;
/// only the bitmap and index checkpoint is reused, and recovery from a missing
/// or stale checkpoint is reported separately.
#[derive(Debug, Default, Clone, Serialize)]
pub struct ProjectionLoadStatistics {
    pub total_us: u64,
    pub schema_validation_us: u64,
    pub checkpoint_read_us: u64,
    pub checkpoint_decode_us: u64,
    pub checkpoint_validation_us: u64,
    pub text_load_us: u64,
    pub name_load_us: u64,
    pub drawing_load_us: u64,
    pub navigation_load_us: u64,
    pub canonical_recovery_us: u64,
    pub recovery_persistence_us: u64,
    pub checkpoint_payload_bytes: usize,
    pub checkpoint_reused: bool,
}

/// Resident state rebuilt from canonical rows on every open and never
/// checkpointed: one streaming scan each is simpler than another persisted
/// derived format with its own journal and repair lifecycle.
struct ResidentState {
    text_index: Arc<crate::text_index::TextIndex>,
    name_index: Arc<crate::name_index::NameIndex>,
    drawing: Arc<crate::browsing::DrawingIndex>,
    recent_views: Arc<RecentViews>,
    folder_parents: Arc<HashMap<FolderId, Option<FolderId>>>,
}

impl ResidentState {
    fn load(
        connection: &Connection,
        statistics: &mut ProjectionLoadStatistics,
        progress: &mut dyn FnMut(OpenStage),
    ) -> Result<Self> {
        progress(OpenStage::Loading(LoadStage::Text));
        let stage = std::time::Instant::now();
        let text_index = Arc::new(crate::text_index::TextIndex::load(connection)?);
        statistics.text_load_us = stage.elapsed().as_micros() as u64;
        progress(OpenStage::Loading(LoadStage::Names));
        let stage = std::time::Instant::now();
        let name_index = Arc::new(crate::name_index::NameIndex::load(connection)?);
        statistics.name_load_us = stage.elapsed().as_micros() as u64;
        progress(OpenStage::Loading(LoadStage::Drawing));
        let stage = std::time::Instant::now();
        let drawing = Arc::new(crate::browsing::DrawingIndex::load(connection)?);
        statistics.drawing_load_us = stage.elapsed().as_micros() as u64;
        progress(OpenStage::Loading(LoadStage::Navigation));
        let stage = std::time::Instant::now();
        let recent_views = Arc::new(load_recent_views(connection)?.into());
        let folder_parents = Arc::new(load_folder_parents(connection)?);
        statistics.navigation_load_us = stage.elapsed().as_micros() as u64;
        Ok(Self {
            text_index,
            name_index,
            drawing,
            recent_views,
            folder_parents,
        })
    }

    fn attach(self, snapshot: &mut ProjectionSnapshot) {
        snapshot.text_index = self.text_index;
        snapshot.name_index = self.name_index;
        snapshot.drawing = self.drawing;
        snapshot.recent_views = self.recent_views;
        snapshot.folder_parents = self.folder_parents;
    }
}

/// Recently viewed roots in display order: most recent first, ties by
/// descending ID, exactly the canonical `recent_view` order.
pub(crate) fn load_recent_views(connection: &Connection) -> Result<Vec<(RootId, i64)>> {
    let mut statement = connection.prepare_cached(
        "SELECT root_id, viewed_at_ms FROM recent_view
         ORDER BY viewed_at_ms DESC, root_id DESC",
    )?;
    let values = statement
        .query_map([], |row| Ok((RootId(row.get(0)?), row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

pub(crate) fn load_folder_parents(
    connection: &Connection,
) -> Result<HashMap<FolderId, Option<FolderId>>> {
    let mut statement =
        connection.prepare_cached("SELECT folder_id, parent_id FROM folder_definition")?;
    let values = statement
        .query_map([], |row| {
            Ok((
                FolderId(row.get(0)?),
                row.get::<_, Option<u32>>(1)?.map(FolderId),
            ))
        })?
        .collect::<std::result::Result<HashMap<_, _>, rusqlite::Error>>()?;
    Ok(values)
}

impl ProjectionStore {
    pub fn load(database: &LibraryDatabase) -> Result<Self> {
        Self::load_with(database, &mut |_| {})
    }

    /// Load, reporting each stage. Cancellation is the caller's scoped open
    /// token: the resident scans and the canonical rebuild check it as they
    /// run, and the atomic steps (checkpoint read and decode, the recovery
    /// write) are checked immediately before and after.
    pub fn load_with(
        database: &LibraryDatabase,
        progress: &mut dyn FnMut(OpenStage),
    ) -> Result<Self> {
        let started = std::time::Instant::now();
        let mut statistics = ProjectionLoadStatistics::default();
        let (mut snapshot, recovered_partitions) = database.read(
            crate::database::WorkPriority::CorrectnessRecovery,
            |connection| {
                let stage = std::time::Instant::now();
                let revision = crate::schema::validate(connection)?;
                statistics.schema_validation_us = stage.elapsed().as_micros() as u64;
                progress(OpenStage::Loading(LoadStage::Checkpoint));
                crate::query_request::check()?;
                let stage = std::time::Instant::now();
                let payload = crate::checkpoint::read(connection, revision)?;
                statistics.checkpoint_read_us = stage.elapsed().as_micros() as u64;
                let resident = ResidentState::load(connection, &mut statistics, progress)?;
                if let Some(payload) = payload {
                    statistics.checkpoint_payload_bytes = payload.len();
                    progress(OpenStage::Loading(LoadStage::Validation));
                    crate::query_request::check()?;
                    let stage = std::time::Instant::now();
                    let mut snapshot = crate::checkpoint::decode(&payload, revision)?;
                    statistics.checkpoint_decode_us = stage.elapsed().as_micros() as u64;
                    let stage = std::time::Instant::now();
                    let valid = crate::checkpoint::tag_projection_matches(connection, &snapshot)?
                        && crate::smart::projection_matches(connection, &snapshot)?
                        && snapshot_partitions_match(&snapshot);
                    statistics.checkpoint_validation_us = stage.elapsed().as_micros() as u64;
                    crate::query_request::check()?;
                    if valid {
                        statistics.checkpoint_reused = true;
                        resident.attach(&mut snapshot);
                        return Ok((snapshot, false));
                    }
                    // A copied or interrupted checkpoint may have a valid checksum and outer
                    // revision while carrying stale derived state. Rebuild from canonical data.
                    progress(OpenStage::Loading(LoadStage::Canonical));
                    let stage = std::time::Instant::now();
                    let recovered = load(connection, Some(&snapshot), resident, progress)?;
                    statistics.canonical_recovery_us = stage.elapsed().as_micros() as u64;
                    Ok(recovered)
                } else {
                    progress(OpenStage::Loading(LoadStage::Canonical));
                    let stage = std::time::Instant::now();
                    let recovered = load(connection, None, resident, progress)?;
                    statistics.canonical_recovery_us = stage.elapsed().as_micros() as u64;
                    Ok(recovered)
                }
            },
        )?;
        if recovered_partitions {
            progress(OpenStage::Loading(LoadStage::Recovery));
            crate::query_request::check()?;
            let stage = std::time::Instant::now();
            persist_recovered_partitions(database, &snapshot)?;
            statistics.recovery_persistence_us = stage.elapsed().as_micros() as u64;
        }
        snapshot.subscription_counts = Arc::new(database.subscription_counts()?);
        statistics.total_us = started.elapsed().as_micros() as u64;
        Ok(Self {
            snapshot: ArcSwap::from_pointee(snapshot),
            load_statistics: statistics,
        })
    }

    pub fn load_statistics(&self) -> &ProjectionLoadStatistics {
        &self.load_statistics
    }

    pub fn snapshot(&self) -> Arc<ProjectionSnapshot> {
        self.snapshot.load_full()
    }

    pub(crate) fn publish(&self, mut snapshot: ProjectionSnapshot) {
        snapshot.query_versions =
            crate::query_dependencies::QueryVersions::advance(&self.snapshot.load(), &snapshot);
        self.snapshot.store(Arc::new(snapshot));
    }
}

fn load(
    connection: &Connection,
    checkpoint_fallback: Option<&ProjectionSnapshot>,
    resident: ResidentState,
    progress: &mut dyn FnMut(OpenStage),
) -> Result<(ProjectionSnapshot, bool)> {
    let revision = crate::schema::validate(connection)?;
    let canonical = bitmap::load_all(connection)?;

    let mut lifecycle = HashMap::new();
    for value in Lifecycle::ALL {
        lifecycle.insert(
            value,
            canonical
                .get(&BitmapKey {
                    domain: BitmapDomain::Lifecycle,
                    key_id: value.bitmap_key(),
                })
                .cloned()
                .unwrap_or_default()
                .into(),
        );
    }
    let mut ratings = HashMap::new();
    for value in Rating::ALL {
        ratings.insert(
            value,
            canonical
                .get(&BitmapKey {
                    domain: BitmapDomain::Rating,
                    key_id: value.bitmap_key(),
                })
                .cloned()
                .unwrap_or_default()
                .into(),
        );
    }
    validate_partitions("lifecycle", lifecycle.values())?;
    validate_partitions("rating", ratings.values())?;

    let tags: HashMap<TagId, SharedBitmap> = canonical
        .iter()
        .filter(|(key, _)| key.domain == BitmapDomain::Tag)
        .map(|(key, bitmap)| (TagId(key.key_id), bitmap.clone().into()))
        .collect::<HashMap<_, _>>();
    let mut tag_ids_by_name = HashMap::new();
    let mut tag_statement = connection.prepare(
        "SELECT tag.tag_id, namespace.display_name, tag.subname
         FROM tag_definition tag
         JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id",
    )?;
    let tag_rows = tag_statement.query_map([], |row| {
        Ok((
            TagId(row.get::<_, u32>(0)?),
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in tag_rows {
        let (tag_id, namespace, subname) = row?;
        let name = if namespace.is_empty() {
            subname
        } else {
            format!("{namespace}:{subname}")
        };
        tag_ids_by_name.insert(name, tag_id);
    }

    let mut folder_orders = HashMap::new();
    let mut folders = HashMap::new();
    let mut collection_orders = HashMap::new();
    ordering::load_all(connection, |kind, owner_id, values| {
        match kind {
            OrderOwnerKind::Collection => {
                collection_orders.insert(
                    RootId(owner_id),
                    Arc::new(values.into_iter().map(MediaId).collect::<Vec<_>>()),
                );
            }
            OrderOwnerKind::Folder => {
                let bitmap = values.iter().copied().collect::<RoaringBitmap>();
                folders.insert(FolderId(owner_id), bitmap.into());
                folder_orders.insert(
                    FolderId(owner_id),
                    Arc::new(values.into_iter().map(RootId).collect()),
                );
            }
        }
        Ok(())
    })?;

    let mut root_kinds: HashMap<RootKind, SharedBitmap> = [
        (RootKind::Media, SharedBitmap::default()),
        (RootKind::Collection, SharedBitmap::default()),
    ]
    .into_iter()
    .collect();
    let mut mime: HashMap<String, SharedBitmap> = HashMap::new();
    let mut mime_family: HashMap<String, SharedBitmap> = HashMap::new();
    let mut color_cells: HashMap<u32, SharedBitmap> = HashMap::new();
    let mut cover_palettes = ShardedIdMap::default();
    let mut total_bytes = NumericIndex::default();
    let mut media_count = NumericIndex::default();
    let mut width = NumericIndex::default();
    let mut height = NumericIndex::default();
    let mut duration = NumericIndex::default();
    let mut imported_at = DateIndex::default();
    let mut captured_at = DateIndex::default();
    let mut modified_at = NumericIndex::default();
    let mut notes_present = RoaringBitmap::new();
    let mut urls_present = RoaringBitmap::new();
    let mut roots = connection.prepare(
        "SELECT root.root_id, item.item_kind, root.total_size_bytes, root.media_count,
                root.imported_at_ms, root.captured_at_ms, root.modified_at_ms,
                root.notes, root.source_urls_json,
                file.width, file.height, file.duration_ms, file.palette_json
         FROM library_root root
         JOIN library_item item ON item.local_id = root.root_id
         JOIN media_item media ON media.media_id = root.cover_media_id
         JOIN media_file file ON file.file_id = media.file_id",
    )?;
    let rows = roots.query_map([], |row| {
        Ok((
            row.get::<_, u32>(0)?,
            row.get::<_, u8>(1)?,
            row.get::<_, i64>(2)? as u64,
            row.get::<_, u32>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, Option<String>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, Option<u32>>(9)?,
            row.get::<_, Option<u32>>(10)?,
            row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
            row.get::<_, String>(12)?,
        ))
    })?;
    for (index, row) in rows.enumerate() {
        if index % 1024 == 0 {
            crate::query_request::check()?;
        }
        let (
            root_id,
            kind,
            bytes,
            count,
            imported,
            captured,
            modified,
            notes,
            urls,
            root_width,
            root_height,
            root_duration,
            palette,
        ) = row?;
        let kind = match kind {
            1 => RootKind::Media,
            2 => RootKind::Collection,
            value => {
                return Err(crate::LibraryError::InvalidState(format!(
                    "unknown root kind {value}"
                )))
            }
        };
        root_kinds.entry(kind).or_default().insert(root_id);
        total_bytes.insert(root_id, bytes);
        media_count.insert(root_id, count as u64);
        imported_at.insert(root_id, imported);
        if let Some(value) = captured {
            captured_at.insert(root_id, value);
        }
        modified_at.insert(root_id, modified.max(0) as u64);
        if let Some(value) = root_width {
            width.insert(root_id, value as u64);
        }
        if let Some(value) = root_height {
            height.insert(root_id, value as u64);
        }
        if let Some(value) = root_duration {
            duration.insert(root_id, value);
        }
        if notes.as_deref().is_some_and(|value| !value.is_empty()) {
            notes_present.insert(root_id);
        }
        if urls != "[]" {
            urls_present.insert(root_id);
        }
        let root_palette = serde_json::from_str::<Vec<LabColor>>(&palette).unwrap_or_default();
        for color in &root_palette {
            color_cells
                .entry(color_cell(color))
                .or_default()
                .insert(root_id);
        }
        cover_palettes.insert(root_id, Arc::new(root_palette));
    }

    let mut media_owner = ShardedIdMap::default();
    for media_id in root_kinds
        .get(&RootKind::Media)
        .into_iter()
        .flat_map(|bitmap| bitmap.iter())
    {
        media_owner.insert(media_id, RootId(media_id));
    }
    for (root_id, members) in &collection_orders {
        for media_id in members.iter() {
            if media_owner.insert(media_id.0, *root_id).is_some() {
                return Err(crate::LibraryError::InvalidState(format!(
                    "media {} has multiple owning roots",
                    media_id.0
                )));
            }
        }
    }

    let mut image_media = RoaringBitmap::new();
    let mut media_statement = connection.prepare(
        "SELECT media.media_id, file.mime
         FROM media_item media
         JOIN media_file file ON file.file_id = media.file_id",
    )?;
    let media_rows = media_statement.query_map([], |row| {
        Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
    })?;
    for (index, row) in media_rows.enumerate() {
        if index % 1024 == 0 {
            crate::query_request::check()?;
        }
        let (media_id, media_mime) = row?;
        let owner = media_owner.get(media_id).ok_or_else(|| {
            crate::LibraryError::InvalidState(format!("media {media_id} has no owning root"))
        })?;
        mime.entry(media_mime.clone()).or_default().insert(owner.0);
        mime_family
            .entry(mime_family_name(&media_mime).to_owned())
            .or_default()
            .insert(owner.0);
        if media_mime.starts_with("image/") {
            image_media.insert(media_id);
        }
    }
    let roots_with_images = image_media
        .iter()
        .filter_map(|media_id| media_owner.get(media_id).map(|root_id| root_id.0))
        .collect::<RoaringBitmap>();

    let mut tag_counts: HashMap<u32, u64> = HashMap::new();
    for members in tags.values() {
        for root_id in members.iter() {
            *tag_counts.entry(root_id).or_default() += 1;
        }
    }
    let mut folder_counts: HashMap<u32, u64> = HashMap::new();
    for members in folders.values() {
        for root_id in members {
            *folder_counts.entry(root_id).or_default() += 1;
        }
    }
    let mut tag_count = NumericIndex::default();
    let mut folder_count = NumericIndex::default();
    for root_id in all_roots(&root_kinds) {
        tag_count.insert(root_id, tag_counts.get(&root_id).copied().unwrap_or(0));
        folder_count.insert(root_id, folder_counts.get(&root_id).copied().unwrap_or(0));
    }

    let every_root = all_roots(&root_kinds);
    let recovered_lifecycle = recover_partition(
        "lifecycle",
        &mut lifecycle,
        checkpoint_fallback.map(|snapshot| snapshot.lifecycle.as_ref()),
        &every_root,
        Lifecycle::Active,
    )?;
    let recovered_rating = recover_partition(
        "rating",
        &mut ratings,
        checkpoint_fallback.map(|snapshot| snapshot.ratings.as_ref()),
        &every_root,
        Rating::Unrated,
    )?;
    validate_partition_coverage("lifecycle", lifecycle.values(), &every_root)?;
    validate_partition_coverage("rating", ratings.values(), &every_root)?;

    let mut snapshot = ProjectionSnapshot {
        revision,
        subscription_counts: Arc::default(),
        text_index: resident.text_index,
        name_index: resident.name_index,
        drawing: resident.drawing,
        recent_views: resident.recent_views,
        folder_parents: resident.folder_parents,
        query_versions: crate::query_dependencies::QueryVersions::new(revision),
        lifecycle: Arc::new(lifecycle),
        ratings: Arc::new(ratings),
        tags: Arc::new(tags),
        tag_ids_by_name: Arc::new(tag_ids_by_name),
        folder_orders: Arc::new(folder_orders),
        folders: Arc::new(folders),
        collection_orders: Arc::new(collection_orders),
        media_owner: Arc::new(media_owner),
        image_media: Arc::new(image_media),
        roots_with_images: Arc::new(roots_with_images),
        root_kinds: Arc::new(root_kinds),
        mime: Arc::new(mime),
        mime_family: Arc::new(mime_family),
        color_cells: Arc::new(color_cells),
        cover_palettes: Arc::new(cover_palettes),
        tag_count: Arc::new(tag_count),
        folder_count: Arc::new(folder_count),
        total_bytes: Arc::new(total_bytes),
        media_count: Arc::new(media_count),
        width: Arc::new(width),
        height: Arc::new(height),
        duration: Arc::new(duration),
        imported_at: Arc::new(imported_at),
        captured_at: Arc::new(captured_at),
        modified_at: Arc::new(modified_at),
        notes_present: Arc::new(notes_present),
        urls_present: Arc::new(urls_present),
        smart_local_results: Arc::new(HashMap::new()),
        smart_results: Arc::new(HashMap::new()),
        smart_result_versions: Arc::new(HashMap::new()),
        smart_queries: Arc::new(HashMap::new()),
        smart_effective_queries: Arc::new(HashMap::new()),
    };
    // Smart-folder evaluation may run on a worker pool that cannot see the
    // open token, so it is bracketed by checks rather than checked inside.
    progress(OpenStage::Loading(LoadStage::Smart));
    crate::query_request::check()?;
    crate::smart::load(connection, &mut snapshot)?;
    crate::query_request::check()?;
    Ok((snapshot, recovered_lifecycle || recovered_rating))
}

fn snapshot_partitions_match(snapshot: &ProjectionSnapshot) -> bool {
    let every_root = all_roots(&snapshot.root_kinds);
    partitions_match(snapshot.lifecycle.values(), &every_root)
        && partitions_match(snapshot.ratings.values(), &every_root)
}

fn partitions_match<'a>(
    partitions: impl Iterator<Item = &'a SharedBitmap> + Clone,
    expected: &RoaringBitmap,
) -> bool {
    validate_partitions("projection", partitions.clone()).is_ok()
        && partitions.fold(RoaringBitmap::new(), |mut result, values| {
            result |= values;
            result
        }) == *expected
}

fn recover_partition<K: Copy + Eq + Hash>(
    name: &str,
    current: &mut HashMap<K, SharedBitmap>,
    fallback: Option<&HashMap<K, SharedBitmap>>,
    expected: &RoaringBitmap,
    default_key: K,
) -> Result<bool> {
    if partitions_match(current.values(), expected) {
        return Ok(false);
    }
    if let Some(fallback) = fallback.filter(|values| partitions_match(values.values(), expected)) {
        *current = fallback.clone();
        return Ok(true);
    }
    validate_partitions(name, current.values())?;
    for partition in current.values_mut() {
        let unexpected = partition.to_bitmap() - expected;
        partition.subtract(&unexpected);
    }
    let present = current
        .values()
        .fold(RoaringBitmap::new(), |mut result, values| {
            result |= values;
            result
        });
    let missing = expected - &present;
    current.entry(default_key).or_default().union(&missing);
    Ok(true)
}

fn persist_recovered_partitions(
    database: &LibraryDatabase,
    snapshot: &ProjectionSnapshot,
) -> Result<()> {
    database.maintenance_write(
        crate::database::WorkPriority::CorrectnessRecovery,
        |transaction| {
            let mut changed = 0;
            for value in Lifecycle::ALL {
                changed += bitmap::replace(
                    transaction,
                    snapshot.revision,
                    BitmapKey {
                        domain: BitmapDomain::Lifecycle,
                        key_id: value.bitmap_key(),
                    },
                    snapshot.lifecycle(value),
                )?;
            }
            for value in Rating::ALL {
                changed += bitmap::replace(
                    transaction,
                    snapshot.revision,
                    BitmapKey {
                        domain: BitmapDomain::Rating,
                        key_id: value.bitmap_key(),
                    },
                    snapshot.rating(value),
                )?;
            }
            if changed != 0 {
                transaction.execute("DELETE FROM projection_checkpoint WHERE singleton = 1", [])?;
            }
            Ok(())
        },
    )
}

fn validate_partitions<'a>(
    name: &str,
    partitions: impl Iterator<Item = &'a SharedBitmap>,
) -> Result<()> {
    let mut seen = RoaringBitmap::new();
    for partition in partitions {
        if !(&seen & partition.deref()).is_empty() {
            return Err(crate::LibraryError::InvalidState(format!(
                "{name} partitions overlap"
            )));
        }
        seen |= partition;
    }
    Ok(())
}

fn validate_partition_coverage<'a>(
    name: &str,
    partitions: impl Iterator<Item = &'a SharedBitmap>,
    expected: &RoaringBitmap,
) -> Result<()> {
    let actual = partitions.fold(RoaringBitmap::new(), |mut result, values| {
        result |= values;
        result
    });
    if &actual != expected {
        return Err(crate::LibraryError::InvalidState(format!(
            "{name} partitions do not cover every root"
        )));
    }
    Ok(())
}

fn all_roots(root_kinds: &HashMap<RootKind, SharedBitmap>) -> RoaringBitmap {
    root_kinds
        .values()
        .fold(RoaringBitmap::new(), |mut result, values| {
            result |= values;
            result
        })
}

fn mime_family_name(mime: &str) -> &str {
    mime.split_once('/').map_or(mime, |(family, _)| family)
}

pub fn color_cell(color: &LabColor) -> u32 {
    let l = (color.l.clamp(0.0, 100.0) / 8.0).floor() as u32;
    let a = ((color.a.clamp(-128.0, 127.0) + 128.0) / 8.0).floor() as u32;
    let b = ((color.b.clamp(-128.0, 127.0) + 128.0) / 8.0).floor() as u32;
    (l << 10) | (a << 5) | b
}

pub fn cell_center(cell: u32) -> (f32, f32, f32) {
    let l = ((cell >> 10) & 0x1f) as f32 * 8.0 + 4.0;
    let a = ((cell >> 5) & 0x1f) as f32 * 8.0 - 124.0;
    let b = (cell & 0x1f) as f32 * 8.0 - 124.0;
    (l.min(100.0), a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_date_windows_match_sql_with_nulls_ties_and_retained_snapshots() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("CREATE TABLE dates(id INTEGER PRIMARY KEY, value INTEGER)")
            .unwrap();
        let mut index = DateIndex::default();
        for n in 0..600u32 {
            let id = n * 131 + 1;
            let value = match n % 9 {
                0 => None,
                1 => Some(i64::MIN),
                2 => Some(-100),
                3 => Some(-1),
                4 => Some(0),
                5 => Some(i64::MAX),
                _ => Some(i64::from(n % 17)),
            };
            connection
                .execute(
                    "INSERT INTO dates VALUES (?1,?2)",
                    rusqlite::params![id, value],
                )
                .unwrap();
            if let Some(value) = value {
                index.insert(id, value);
            }
        }
        let selection: RoaringBitmap = (0..600u32)
            .filter(|n| n % 7 != 0)
            .map(|n| n * 131 + 1)
            .collect();
        // Warm a derived order, then retain the snapshot across an update/removal.
        index.ordered_window(&selection, 0, 4, false, true).unwrap();
        let previous = index.clone();
        index.insert(132, 1234);
        index.remove(263);
        connection.execute_batch("UPDATE dates SET value=1234 WHERE id=132; UPDATE dates SET value=NULL WHERE id=263;").unwrap();
        assert_eq!(previous.0.value(132), Some(DateIndex::key(i64::MIN)));
        assert_eq!(previous.0.value(263), Some(DateIndex::key(-100)));
        let persisted: DateIndex =
            bincode::deserialize(&bincode::serialize(&index).unwrap()).unwrap();
        for index in [&index, &persisted] {
            for descending in [false, true] {
                let direction = if descending { "DESC" } else { "ASC" };
                let expected = connection.prepare(&format!("SELECT id FROM dates ORDER BY COALESCE(value,-1) {direction}, id {direction}"))
                    .unwrap().query_map([], |row| row.get::<_,u32>(0)).unwrap()
                    .map(|row| row.unwrap()).filter(|id| selection.contains(*id)).collect::<Vec<_>>();
                for start in [0, 1, 55, 120, 250, 500, 515] {
                    for limit in [0, 1, 4, 137, 1500] {
                        assert_eq!(
                            index
                                .ordered_window(&selection, start, limit, descending, true)
                                .unwrap()
                                .unwrap(),
                            expected
                                .iter()
                                .skip(start)
                                .take(limit)
                                .copied()
                                .collect::<Vec<_>>(),
                            "{start} {limit} {descending}"
                        );
                    }
                }
            }
            for minimum in [None, Some(0), Some(1), Some(17), Some(u64::MAX)] {
                for maximum in [None, Some(0), Some(7), Some(u64::MAX)] {
                    let expected = connection
                        .prepare("SELECT id,value FROM dates WHERE value IS NOT NULL")
                        .unwrap()
                        .query_map([], |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?)))
                        .unwrap()
                        .map(|row| row.unwrap())
                        .filter(|(_, value)| {
                            let value = (*value).max(0) as u64;
                            minimum.is_none_or(|min| value >= min)
                                && maximum.is_none_or(|max| value <= max)
                        })
                        .map(|(id, _)| id)
                        .collect::<RoaringBitmap>();
                    assert_eq!(index.between(minimum, maximum), expected);
                }
            }
        }
    }

    #[test]
    fn numeric_ordinal_windows_match_value_and_id_sort_after_writes() {
        let mut index = NumericIndex::default();
        let mut values = Vec::new();
        for i in 0u32..3000 {
            let id = i * 67;
            let value = match i % 7 {
                0 => 0,
                1 => u64::MAX,
                2 => 42,
                _ => (u64::from(i) * 7919) % 1201,
            };
            index.insert(id, value);
            if i % 3 != 0 {
                values.push((value, id));
            }
        }
        let all_ids = values.iter().map(|(_, id)| *id).collect();
        index
            .ordered_window(&all_ids, 0, 1, false)
            .unwrap()
            .unwrap();
        let previous = index.clone();
        index.insert(67, 17);
        values.iter_mut().find(|(_, id)| *id == 67).unwrap().0 = 17;
        index.remove(134);
        values.retain(|(_, id)| *id != 134);
        assert_eq!(previous.value(67), Some(u64::MAX));
        let matches = values.iter().map(|(_, id)| *id).collect();
        for descending in [false, true] {
            values.sort_unstable();
            if descending {
                values.reverse();
            }
            for start in [0, 1, 17, 700, 1500, 1997, 2000] {
                for limit in [0, 1, 7, 500, 1500] {
                    assert_eq!(
                        index
                            .ordered_window(&matches, start, limit, descending)
                            .unwrap()
                            .unwrap(),
                        values
                            .iter()
                            .skip(start)
                            .take(limit)
                            .map(|(_, id)| *id)
                            .collect::<Vec<_>>()
                    );
                }
            }
        }
        assert!(index
            .ordered_window(&[u32::MAX].into_iter().collect(), 0, 1, false)
            .unwrap()
            .is_none());
    }

    #[test]
    fn numeric_ranges_and_sums_are_exact() {
        let mut index = NumericIndex::default();
        index.insert(1, 10);
        index.insert(2, 20);
        index.insert(3, 30);
        assert_eq!(
            index.between(Some(11), Some(30)).iter().collect::<Vec<_>>(),
            vec![2, 3]
        );
        assert_eq!(index.sum(&[1, 3].into_iter().collect()), 40);
    }

    #[test]
    fn color_cells_are_stable() {
        let color = LabColor {
            l: 52.0,
            a: -3.0,
            b: 44.0,
            weight: 1.0,
        };
        assert_eq!(color_cell(&color), color_cell(&color));
    }
}
