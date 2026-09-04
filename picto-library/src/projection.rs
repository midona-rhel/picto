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

    pub(crate) fn iter(&self) -> impl Iterator<Item = (u32, &V)> {
        self.shards.iter().flat_map(|(high, shard)| {
            shard
                .iter()
                .map(move |(low, value)| ((high << ID_MAP_SHARD_SHIFT) | u32::from(*low), value))
        })
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

    fn estimated_bytes(&self) -> usize {
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct NumericShard {
    present: RoaringBitmap,
    slices: Vec<RoaringBitmap>,
}

impl NumericIndex {
    pub fn insert(&mut self, id: u32, value: u64) {
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
            result |= shard.between(minimum, maximum);
        }
        result
    }

    pub fn sum(&self, selection: &RoaringBitmap) -> u128 {
        self.shards.values().map(|shard| shard.sum(selection)).sum()
    }

    fn estimated_bytes(&self) -> usize {
        self.shards
            .values()
            .map(|shard| shard.estimated_bytes())
            .sum::<usize>()
            + self.shards.len()
                * (std::mem::size_of::<u16>() + std::mem::size_of::<Arc<NumericShard>>())
    }
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

    fn between(&self, minimum: Option<u64>, maximum: Option<u64>) -> RoaringBitmap {
        let mut result = self.present.clone();
        if let Some(minimum) = minimum {
            result -= &self.less_than(minimum);
        }
        if let Some(maximum) = maximum {
            result &= self.less_than(maximum.saturating_add(1));
        }
        result
    }

    fn sum(&self, selection: &RoaringBitmap) -> u128 {
        self.slices
            .iter()
            .enumerate()
            .map(|(bit, slice)| (slice & selection).len() as u128 * (1u128 << bit))
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

    fn less_than(&self, value: u64) -> RoaringBitmap {
        if value == 0 {
            return RoaringBitmap::new();
        }
        let mut equal = self.present.clone();
        let mut less = RoaringBitmap::new();
        let highest = self.slices.len().max(64 - value.leading_zeros() as usize);
        for bit in (0..highest).rev() {
            let ones = self
                .slices
                .get(bit)
                .cloned()
                .unwrap_or_else(RoaringBitmap::new);
            let mut zeros = equal.clone();
            zeros -= &ones;
            if bit < 64 && value & (1u64 << bit) != 0 {
                less |= &zeros;
                equal &= &ones;
            } else {
                equal = zeros;
            }
        }
        less
    }
}

#[derive(Debug, Clone)]
pub struct ProjectionSnapshot {
    pub revision: u64,
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
    pub imported_at: Arc<NumericIndex>,
    pub captured_at: Arc<NumericIndex>,
    pub modified_at: Arc<NumericIndex>,
    pub notes_present: Arc<RoaringBitmap>,
    pub urls_present: Arc<RoaringBitmap>,
    /// Each smart folder's own rules evaluated over active roots.
    pub smart_local_results: Arc<HashMap<u32, SharedBitmap>>,
    /// Local results intersected with every ancestor result.
    pub smart_results: Arc<HashMap<u32, SharedBitmap>>,
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
            &self.imported_at,
            &self.captured_at,
            &self.modified_at,
        ]
        .into_iter()
        .map(|index| index.estimated_bytes())
        .sum::<usize>();
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
        bytes
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
}

impl ProjectionStore {
    pub fn load(database: &LibraryDatabase) -> Result<Self> {
        let (snapshot, recovered_partitions) = database.read(
            crate::database::WorkPriority::CorrectnessRecovery,
            |connection| {
                let revision = crate::schema::validate(connection)?;
                if let Some(payload) = crate::checkpoint::read(connection, revision)? {
                    let snapshot = crate::checkpoint::decode(&payload, revision)?;
                    if crate::checkpoint::tag_projection_matches(connection, &snapshot)?
                        && crate::smart::projection_matches(connection, &snapshot)?
                        && snapshot_partitions_match(&snapshot)
                    {
                        Ok((snapshot, false))
                    } else {
                        // A copied or interrupted checkpoint may have a valid checksum and outer
                        // revision while carrying stale derived state. Rebuild from canonical data.
                        load(connection, Some(&snapshot))
                    }
                } else {
                    load(connection, None)
                }
            },
        )?;
        if recovered_partitions {
            persist_recovered_partitions(database, &snapshot)?;
        }
        Ok(Self {
            snapshot: ArcSwap::from_pointee(snapshot),
        })
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
    let mut orders = connection
        .prepare("SELECT owner_kind, owner_id, checksum, payload FROM ordered_membership")?;
    let rows = orders.query_map([], |row| {
        Ok((
            row.get::<_, u8>(0)?,
            row.get::<_, u32>(1)?,
            row.get::<_, Vec<u8>>(2)?,
            row.get::<_, Vec<u8>>(3)?,
        ))
    })?;
    for row in rows {
        let (kind, owner_id, checksum, payload) = row?;
        let values = ordering::decode(&payload, &checksum)?;
        match kind {
            value if value == OrderOwnerKind::Collection as u8 => {
                collection_orders.insert(
                    RootId(owner_id),
                    Arc::new(values.into_iter().map(MediaId).collect::<Vec<_>>()),
                );
            }
            value if value == OrderOwnerKind::Folder as u8 => {
                let bitmap = values.iter().copied().collect::<RoaringBitmap>();
                folders.insert(FolderId(owner_id), bitmap.into());
                folder_orders.insert(
                    FolderId(owner_id),
                    Arc::new(values.into_iter().map(RootId).collect()),
                );
            }
            _ => {
                return Err(crate::LibraryError::InvalidState(format!(
                    "unknown ordered membership kind {kind}"
                )))
            }
        }
    }

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
    let mut imported_at = NumericIndex::default();
    let mut captured_at = NumericIndex::default();
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
    for row in rows {
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
        imported_at.insert(root_id, imported.max(0) as u64);
        if let Some(value) = captured {
            captured_at.insert(root_id, value.max(0) as u64);
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
    for row in media_rows {
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
        smart_queries: Arc::new(HashMap::new()),
        smart_effective_queries: Arc::new(HashMap::new()),
    };
    crate::smart::load(connection, &mut snapshot)?;
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
