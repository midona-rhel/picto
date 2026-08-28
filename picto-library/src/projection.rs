use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use roaring::RoaringBitmap;
use rusqlite::Connection;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::model::{FolderId, LabColor, Lifecycle, MediaId, Rating, RootId, RootKind, TagId};
use crate::ordering::{self, OrderOwnerKind};
use crate::predicate::ViewQuerySpec;
use crate::{LibraryDatabase, Result};

#[derive(Debug, Clone, Default)]
pub struct NumericIndex {
    present: RoaringBitmap,
    slices: Vec<RoaringBitmap>,
}

impl NumericIndex {
    pub fn insert(&mut self, id: u32, value: u64) {
        self.remove(id);
        self.present.insert(id);
        let bits = (64 - value.leading_zeros()).max(1) as usize;
        if self.slices.len() < bits {
            self.slices.resize_with(bits, RoaringBitmap::new);
        }
        for (bit, slice) in self.slices.iter_mut().enumerate() {
            if value & (1u64 << bit) != 0 {
                slice.insert(id);
            }
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.present.remove(id);
        for slice in &mut self.slices {
            slice.remove(id);
        }
    }

    pub fn value(&self, id: u32) -> Option<u64> {
        self.present.contains(id).then(|| {
            self.slices
                .iter()
                .enumerate()
                .fold(0, |value, (bit, slice)| {
                    value | (u64::from(slice.contains(id)) << bit)
                })
        })
    }

    pub fn present(&self) -> &RoaringBitmap {
        &self.present
    }

    pub fn between(&self, minimum: Option<u64>, maximum: Option<u64>) -> RoaringBitmap {
        let mut result = self.present.clone();
        if let Some(minimum) = minimum {
            result -= &self.less_than(minimum);
        }
        if let Some(maximum) = maximum {
            result &= self.less_than(maximum.saturating_add(1));
        }
        result
    }

    pub fn sum(&self, selection: &RoaringBitmap) -> u128 {
        self.slices
            .iter()
            .enumerate()
            .map(|(bit, slice)| (slice & selection).len() as u128 * (1u128 << bit))
            .sum()
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
    pub lifecycle: Arc<HashMap<Lifecycle, RoaringBitmap>>,
    pub ratings: Arc<HashMap<Rating, RoaringBitmap>>,
    pub tags: Arc<HashMap<TagId, RoaringBitmap>>,
    pub tag_ids_by_name: Arc<HashMap<String, TagId>>,
    pub folder_orders: Arc<HashMap<FolderId, Arc<Vec<RootId>>>>,
    pub folders: Arc<HashMap<FolderId, RoaringBitmap>>,
    pub collection_orders: Arc<HashMap<RootId, Arc<Vec<MediaId>>>>,
    pub media_owner: Arc<Vec<Option<RootId>>>,
    pub root_kinds: Arc<HashMap<RootKind, RoaringBitmap>>,
    pub mime: Arc<HashMap<String, RoaringBitmap>>,
    pub mime_family: Arc<HashMap<String, RoaringBitmap>>,
    pub color_cells: Arc<HashMap<u32, RoaringBitmap>>,
    pub cover_palettes: Arc<HashMap<RootId, Arc<Vec<LabColor>>>>,
    pub tag_count: Arc<NumericIndex>,
    pub folder_count: Arc<NumericIndex>,
    pub total_bytes: Arc<NumericIndex>,
    pub media_count: Arc<NumericIndex>,
    pub width: Arc<NumericIndex>,
    pub height: Arc<NumericIndex>,
    pub duration: Arc<NumericIndex>,
    pub imported_at: Arc<NumericIndex>,
    pub modified_at: Arc<NumericIndex>,
    pub notes_present: Arc<RoaringBitmap>,
    pub urls_present: Arc<RoaringBitmap>,
    pub smart_results: Arc<HashMap<u32, RoaringBitmap>>,
    pub smart_queries: Arc<HashMap<u32, ViewQuerySpec>>,
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
}

pub struct ProjectionStore {
    snapshot: ArcSwap<ProjectionSnapshot>,
}

impl ProjectionStore {
    pub fn load(database: &LibraryDatabase) -> Result<Self> {
        let snapshot = database.read(crate::database::WorkPriority::CorrectnessRecovery, load)?;
        Ok(Self {
            snapshot: ArcSwap::from_pointee(snapshot),
        })
    }

    pub fn snapshot(&self) -> Arc<ProjectionSnapshot> {
        self.snapshot.load_full()
    }

    pub fn publish(&self, snapshot: ProjectionSnapshot) {
        self.snapshot.store(Arc::new(snapshot));
    }
}

fn load(connection: &Connection) -> Result<ProjectionSnapshot> {
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
                .unwrap_or_default(),
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
                .unwrap_or_default(),
        );
    }
    validate_partitions("lifecycle", lifecycle.values())?;
    validate_partitions("rating", ratings.values())?;

    let tags = canonical
        .iter()
        .filter(|(key, _)| key.domain == BitmapDomain::Tag)
        .map(|(key, bitmap)| (TagId(key.key_id), bitmap.clone()))
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
                let bitmap = values.iter().copied().collect();
                folders.insert(FolderId(owner_id), bitmap);
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

    let mut root_kinds: HashMap<RootKind, RoaringBitmap> = [
        (RootKind::Media, RoaringBitmap::new()),
        (RootKind::Collection, RoaringBitmap::new()),
    ]
    .into_iter()
    .collect();
    let mut mime: HashMap<String, RoaringBitmap> = HashMap::new();
    let mut mime_family: HashMap<String, RoaringBitmap> = HashMap::new();
    let mut color_cells: HashMap<u32, RoaringBitmap> = HashMap::new();
    let mut cover_palettes: HashMap<RootId, Arc<Vec<LabColor>>> = HashMap::new();
    let mut total_bytes = NumericIndex::default();
    let mut media_count = NumericIndex::default();
    let mut width = NumericIndex::default();
    let mut height = NumericIndex::default();
    let mut duration = NumericIndex::default();
    let mut imported_at = NumericIndex::default();
    let mut modified_at = NumericIndex::default();
    let mut notes_present = RoaringBitmap::new();
    let mut urls_present = RoaringBitmap::new();
    let mut roots = connection.prepare(
        "SELECT root.root_id, item.item_kind, root.total_size_bytes, root.media_count,
                root.imported_at_ms, root.modified_at_ms, root.notes, root.source_urls_json,
                file.mime, file.width, file.height, file.duration_ms, file.palette_json
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
            row.get::<_, i64>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, Option<u32>>(9)?,
            row.get::<_, Option<u32>>(10)?,
            row.get::<_, Option<i64>>(11)?.map(|value| value as u64),
            row.get::<_, String>(12)?,
        ))
    })?;
    let mut maximum_id = 0;
    for row in rows {
        let (
            root_id,
            kind,
            bytes,
            count,
            imported,
            modified,
            notes,
            urls,
            root_mime,
            root_width,
            root_height,
            root_duration,
            palette,
        ) = row?;
        maximum_id = maximum_id.max(root_id);
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
        mime.entry(root_mime.clone()).or_default().insert(root_id);
        mime_family
            .entry(mime_family_name(&root_mime).to_owned())
            .or_default()
            .insert(root_id);
        total_bytes.insert(root_id, bytes);
        media_count.insert(root_id, count as u64);
        imported_at.insert(root_id, imported.max(0) as u64);
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
        cover_palettes.insert(RootId(root_id), Arc::new(root_palette));
    }

    let mut media_owner = vec![None; maximum_id as usize + 1];
    for media_id in root_kinds
        .get(&RootKind::Media)
        .into_iter()
        .flat_map(|bitmap| bitmap.iter())
    {
        if media_owner.len() <= media_id as usize {
            media_owner.resize(media_id as usize + 1, None);
        }
        media_owner[media_id as usize] = Some(RootId(media_id));
    }
    for (root_id, members) in &collection_orders {
        for media_id in members.iter() {
            if media_owner.len() <= media_id.0 as usize {
                media_owner.resize(media_id.0 as usize + 1, None);
            }
            if media_owner[media_id.0 as usize].replace(*root_id).is_some() {
                return Err(crate::LibraryError::InvalidState(format!(
                    "media {} has multiple owning roots",
                    media_id.0
                )));
            }
        }
    }

    let mut tag_counts: HashMap<u32, u64> = HashMap::new();
    for members in tags.values() {
        for root_id in members {
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
    validate_partition_coverage("lifecycle", lifecycle.values(), &every_root)?;
    validate_partition_coverage("rating", ratings.values(), &every_root)?;

    let mut snapshot = ProjectionSnapshot {
        revision,
        lifecycle: Arc::new(lifecycle),
        ratings: Arc::new(ratings),
        tags: Arc::new(tags),
        tag_ids_by_name: Arc::new(tag_ids_by_name),
        folder_orders: Arc::new(folder_orders),
        folders: Arc::new(folders),
        collection_orders: Arc::new(collection_orders),
        media_owner: Arc::new(media_owner),
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
        modified_at: Arc::new(modified_at),
        notes_present: Arc::new(notes_present),
        urls_present: Arc::new(urls_present),
        smart_results: Arc::new(HashMap::new()),
        smart_queries: Arc::new(HashMap::new()),
    };
    crate::smart::load(connection, &mut snapshot)?;
    Ok(snapshot)
}

fn validate_partitions<'a>(
    name: &str,
    partitions: impl Iterator<Item = &'a RoaringBitmap>,
) -> Result<()> {
    let mut seen = RoaringBitmap::new();
    for partition in partitions {
        if !(&seen & partition).is_empty() {
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
    partitions: impl Iterator<Item = &'a RoaringBitmap>,
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

fn all_roots(root_kinds: &HashMap<RootKind, RoaringBitmap>) -> RoaringBitmap {
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
