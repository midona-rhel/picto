//! Canonical read path for library item scopes.
//!
//! The query starts from `library_root` and projects collection members into
//! their owning collection. Every scope, filter, count, and page therefore
//! uses the same root set.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::app::{
    FileHash, ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemTarget, Lifecycle,
};
use crate::store::Store;

const DEFAULT_PAGE_LIMIT: i64 = 100;
const MAX_PAGE_LIMIT: i64 = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemPageRequest {
    #[ts(type = "number")]
    pub offset: i64,
    #[ts(type = "number")]
    pub limit: i64,
}

impl Default for ItemPageRequest {
    fn default() -> Self {
        Self {
            offset: 0,
            limit: DEFAULT_PAGE_LIMIT,
        }
    }
}

impl ItemPageRequest {
    pub fn new(offset: i64, limit: i64) -> Self {
        Self { offset, limit }
    }

    fn normalized(self) -> Self {
        Self {
            offset: self.offset.max(0),
            limit: self.limit.clamp(1, MAX_PAGE_LIMIT),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemSummary {
    pub item_id: ItemId,
    pub kind: ItemKind,
    pub lifecycle: Lifecycle,
    pub label: Option<String>,
    pub name: Option<String>,
    pub display_media_item_id: ItemId,
    pub display_file_hash: FileHash,
    pub display_mime_type: String,
    #[ts(type = "number | null")]
    pub pixel_width: Option<i64>,
    #[ts(type = "number | null")]
    pub pixel_height: Option<i64>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub dominant_color_hex: Option<String>,
    #[ts(type = "number")]
    pub size_bytes: i64,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
    pub captured_at: Option<String>,
    pub imported_at: Option<String>,
    #[ts(type = "number")]
    pub media_count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemPage {
    pub items: Vec<ItemSummary>,
    #[ts(type = "number")]
    pub revision: u64,
    #[ts(type = "number")]
    pub visible_item_count: i64,
    #[ts(type = "number")]
    pub visible_media_count: i64,
    #[ts(type = "number")]
    pub total_size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MediaDetails {
    pub media_item_id: ItemId,
    pub file_hash: FileHash,
    pub mime_type: String,
    pub dominant_color_hex: Option<String>,
    pub dominant_colors: Vec<String>,
    #[ts(type = "number")]
    pub size_bytes: i64,
    #[ts(type = "number | null")]
    pub pixel_width: Option<i64>,
    #[ts(type = "number | null")]
    pub pixel_height: Option<i64>,
    #[ts(type = "number | null")]
    pub duration_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub name: Option<String>,
    pub notes: Option<String>,
    #[ts(type = "number | null")]
    pub rating: Option<i64>,
    pub source_urls: Vec<String>,
    pub captured_at: Option<String>,
    pub imported_at: String,
    #[ts(type = "number")]
    pub position: i64,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemDetails {
    pub item_id: ItemId,
    pub kind: ItemKind,
    pub lifecycle: Lifecycle,
    pub label: Option<String>,
    pub cover_media_item_id: Option<ItemId>,
    #[ts(type = "number[]")]
    pub folder_ids: Vec<i64>,
    pub media: Vec<MediaDetails>,
    pub aggregate_tags: Vec<String>,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionTagCount {
    pub tag: String,
    #[ts(type = "number")]
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionFolderInfo {
    #[ts(type = "number")]
    pub folder_id: i64,
    pub name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionRatingStats {
    #[ts(type = "number | null")]
    pub min: Option<i64>,
    #[ts(type = "number | null")]
    pub max: Option<i64>,
    #[ts(type = "number | null")]
    pub shared: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionSummaryStats {
    #[ts(type = "number | null")]
    pub total_size_bytes: Option<i64>,
    #[ts(type = "Record<string, number>")]
    pub mime_counts: BTreeMap<String, i64>,
    pub rating_stats: SelectionRatingStats,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SelectionSummary {
    #[ts(type = "number")]
    pub total_count: i64,
    #[ts(type = "number")]
    pub selected_count: i64,
    pub sample_hashes: Vec<FileHash>,
    pub shared_tags: Vec<SelectionTagCount>,
    pub top_tags: Vec<SelectionTagCount>,
    pub shared_folders: Vec<SelectionFolderInfo>,
    pub stats: SelectionSummaryStats,
    #[ts(type = "number")]
    pub revision: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct RatingAccumulator {
    min: Option<i64>,
    max: Option<i64>,
    shared: Option<i64>,
    saw_unrated: bool,
}

impl RatingAccumulator {
    fn add(&mut self, rating: Option<i64>) {
        let Some(rating) = rating else {
            self.saw_unrated = true;
            self.shared = None;
            return;
        };
        self.min = Some(self.min.map_or(rating, |value| value.min(rating)));
        self.max = Some(self.max.map_or(rating, |value| value.max(rating)));
        if !self.saw_unrated && self.shared.is_none() {
            self.shared = Some(rating);
        } else if self.shared != Some(rating) {
            self.shared = None;
        }
    }

    fn finish(self) -> SelectionRatingStats {
        SelectionRatingStats {
            min: self.min,
            max: self.max,
            shared: self.shared,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ScopeCount {
    #[ts(type = "number")]
    pub id: i64,
    #[ts(type = "number")]
    pub count: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct SidebarCounts {
    #[ts(type = "number")]
    pub all: i64,
    #[ts(type = "number")]
    pub inbox: i64,
    #[ts(type = "number")]
    pub trash: i64,
    #[ts(type = "number")]
    pub recently_viewed: i64,
    #[ts(type = "number")]
    pub untagged: i64,
    #[ts(type = "number")]
    pub uncategorized: i64,
    #[ts(type = "number")]
    pub duplicates: i64,
    pub folders: Vec<ScopeCount>,
    pub smart_folders: Vec<ScopeCount>,
    #[ts(type = "number")]
    pub revision: u64,
}

/// Resolve one canonical page from the replacement store.
pub fn query(
    store: &Store,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> Result<ItemPage, String> {
    let page = page.normalized();
    store.read(|connection| resolve_connection(connection, item_query, page))
}

pub fn details(store: &Store, item_id: ItemId) -> Result<ItemDetails, String> {
    store.read(|connection| details_connection(connection, item_id.0))
}

pub fn selection_summary(store: &Store, target: &ItemTarget) -> Result<SelectionSummary, String> {
    store.read(|connection| {
        let item_ids = resolve_target_ids(connection, target)?;
        let selected_count = item_ids.len() as i64;
        let mut mime_counts = BTreeMap::new();
        let mut tag_root_counts = BTreeMap::<String, i64>::new();
        let mut folder_root_counts = BTreeMap::<i64, (String, i64)>::new();
        let mut total_size_bytes = 0_i64;
        let mut ratings = RatingAccumulator::default();

        for chunk in item_ids.chunks(400) {
            let placeholders = std::iter::repeat_n("?", chunk.len())
                .collect::<Vec<_>>()
                .join(", ");
            let root_media = format!(
                "WITH root_media(root_item_id, media_item_id) AS (
                     SELECT lr.item_id, lr.item_id
                     FROM library_root lr JOIN media_asset ma ON ma.item_id = lr.item_id
                     WHERE lr.item_id IN ({placeholders})
                     UNION ALL
                     SELECT cm.collection_id, cm.media_item_id
                     FROM collection_member cm
                     WHERE cm.collection_id IN ({placeholders})
                 )"
            );
            let mut root_values = Vec::with_capacity(chunk.len() * 2);
            root_values.extend(chunk.iter().copied());
            root_values.extend(chunk.iter().copied());

            let media_sql = format!(
                "{root_media}
                 SELECT mf.mime_type, mf.size_bytes, ma.rating
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 JOIN media_file mf ON mf.file_id = ma.file_id"
            );
            for row in connection.prepare(&media_sql)?.query_map(
                rusqlite::params_from_iter(root_values.iter()),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )? {
                let (mime_type, size_bytes, rating) = row?;
                *mime_counts.entry(mime_type).or_insert(0) += 1;
                total_size_bytes += size_bytes;
                ratings.add(rating);
            }

            let tag_sql = format!(
                "{root_media}
                 SELECT DISTINCT rm.root_item_id,
                    CASE WHEN t.namespace IN ('', 'general') THEN t.subtag
                         ELSE t.namespace || ':' || t.subtag END
                 FROM root_media rm
                 JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                 JOIN tag t ON t.tag_id = mt.tag_id
                 ORDER BY rm.root_item_id, 2"
            );
            for row in connection
                .prepare(&tag_sql)?
                .query_map(rusqlite::params_from_iter(root_values.iter()), |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })?
            {
                let (_, tag) = row?;
                *tag_root_counts.entry(tag).or_insert(0) += 1;
            }

            let folder_sql = format!(
                "SELECT fi.item_id, f.folder_id, f.name
                 FROM folder_item fi JOIN folder f ON f.folder_id = fi.folder_id
                 WHERE fi.item_id IN ({placeholders})
                 ORDER BY f.folder_id"
            );
            for row in connection.prepare(&folder_sql)?.query_map(
                rusqlite::params_from_iter(chunk.iter()),
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )? {
                let (_, folder_id, name) = row?;
                let entry = folder_root_counts.entry(folder_id).or_insert((name, 0));
                entry.1 += 1;
            }
        }

        let mut sample_hashes = Vec::new();
        for item_id in item_ids.iter().take(3) {
            if let Some(hash) = selection_display_hash(connection, *item_id)? {
                sample_hashes.push(FileHash(hash));
            }
        }

        let shared_tags = tag_root_counts
            .iter()
            .filter(|(_, count)| **count == selected_count)
            .map(|(tag, count)| SelectionTagCount {
                tag: tag.clone(),
                count: *count,
            })
            .collect::<Vec<_>>();
        let mut top_tags = tag_root_counts
            .iter()
            .map(|(tag, count)| SelectionTagCount {
                tag: tag.clone(),
                count: *count,
            })
            .collect::<Vec<_>>();
        top_tags.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.tag.cmp(&right.tag))
        });
        top_tags.truncate(20);

        let shared_folders = folder_root_counts
            .iter()
            .filter(|(_, (_, count))| *count == selected_count)
            .map(|(folder_id, (name, _))| SelectionFolderInfo {
                folder_id: *folder_id,
                name: name.clone(),
            })
            .collect::<Vec<_>>();

        Ok(SelectionSummary {
            total_count: selected_count,
            selected_count,
            sample_hashes,
            shared_tags,
            top_tags,
            shared_folders,
            stats: SelectionSummaryStats {
                total_size_bytes: Some(total_size_bytes),
                mime_counts,
                rating_stats: ratings.finish(),
            },
            revision: crate::store::schema::revision(connection)?,
        })
    })
}

fn selection_display_hash(
    connection: &Connection,
    item_id: i64,
) -> rusqlite::Result<Option<String>> {
    connection
        .query_row(
            "SELECT mf.file_hash
             FROM library_item li
             LEFT JOIN media_asset direct ON direct.item_id = li.item_id
             LEFT JOIN media_asset cover ON cover.item_id = li.cover_media_item_id
             LEFT JOIN media_asset first_member ON first_member.item_id = (
                 SELECT cm.media_item_id FROM collection_member cm
                 WHERE cm.collection_id = li.item_id
                 ORDER BY cm.position_rank, cm.media_item_id LIMIT 1
             )
             LEFT JOIN media_file mf ON mf.file_id = COALESCE(
                 direct.file_id, cover.file_id, first_member.file_id
             )
             WHERE li.item_id = ?1",
            [item_id],
            |row| row.get(0),
        )
        .optional()
}

pub fn sidebar_counts(store: &Store) -> Result<SidebarCounts, String> {
    store.read(|connection| {
        let mut result = connection.query_row(
            "WITH visible_roots(item_id, lifecycle) AS (
                 SELECT lr.item_id, lr.lifecycle FROM library_root lr
                 WHERE NOT EXISTS (
                     SELECT 1 FROM collection_member cm WHERE cm.media_item_id = lr.item_id
                 )
             ),
             root_media(root_item_id, media_item_id) AS (
                 SELECT vr.item_id, vr.item_id
                 FROM visible_roots vr JOIN media_asset ma ON ma.item_id = vr.item_id
                 UNION ALL
                 SELECT cm.collection_id, cm.media_item_id FROM collection_member cm
             )
             SELECT
                 COUNT(*) FILTER (WHERE vr.lifecycle = 'active'),
                 COUNT(*) FILTER (WHERE vr.lifecycle = 'inbox'),
                 COUNT(*) FILTER (WHERE vr.lifecycle = 'trash'),
                 COUNT(*) FILTER (
                     WHERE vr.lifecycle = 'active' AND mv.item_id IS NOT NULL
                 ),
                 COUNT(*) FILTER (
                     WHERE vr.lifecycle = 'active' AND NOT EXISTS (
                         SELECT 1 FROM root_media rm
                         JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                         WHERE rm.root_item_id = vr.item_id
                     )
                 ),
                 COUNT(*) FILTER (
                     WHERE vr.lifecycle = 'active' AND NOT EXISTS (
                         SELECT 1 FROM folder_item fi WHERE fi.item_id = vr.item_id
                     )
                 )
             FROM visible_roots vr
             LEFT JOIN media_view mv ON mv.item_id = vr.item_id",
            [],
            |row| {
                Ok(SidebarCounts {
                    all: row.get(0)?,
                    inbox: row.get(1)?,
                    trash: row.get(2)?,
                    recently_viewed: row.get(3)?,
                    untagged: row.get(4)?,
                    uncategorized: row.get(5)?,
                    ..SidebarCounts::default()
                })
            },
        )?;
        result.duplicates = connection.query_row(
            "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
            [],
            |row| row.get(0),
        )?;
        result.folders = connection
            .prepare(
                "SELECT f.folder_id, COUNT(lr.item_id)
                 FROM folder f
                 LEFT JOIN folder_item fi ON fi.folder_id = f.folder_id
                 LEFT JOIN library_root lr ON lr.item_id = fi.item_id
                   AND lr.lifecycle = 'active'
                   AND NOT EXISTS (
                       SELECT 1 FROM collection_member cm WHERE cm.media_item_id = lr.item_id
                   )
                 GROUP BY f.folder_id ORDER BY f.folder_id",
            )?
            .query_map([], |row| {
                Ok(ScopeCount {
                    id: row.get(0)?,
                    count: row.get(1)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        let smart_ids = connection
            .prepare("SELECT smart_folder_id FROM smart_folder ORDER BY smart_folder_id")?
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for smart_folder_id in smart_ids {
            let page = resolve_connection(
                connection,
                &ItemQuery {
                    scope: ItemScope::SmartFolder { smart_folder_id },
                    filters: ItemFilters::default(),
                    sort: crate::app::ItemSort::default(),
                },
                ItemPageRequest::new(0, 1),
            )?;
            result.smart_folders.push(ScopeCount {
                id: smart_folder_id,
                count: page.visible_item_count,
            });
        }
        result.revision = crate::store::schema::revision(connection)?;
        Ok(result)
    })
}

fn details_connection(connection: &Connection, item_id: i64) -> rusqlite::Result<ItemDetails> {
    let (kind, lifecycle, label, cover_media_item_id): (
        String,
        String,
        Option<String>,
        Option<i64>,
    ) = connection
        .query_row(
            "SELECT li.kind, lr.lifecycle, li.label, li.cover_media_item_id
             FROM library_root lr JOIN library_item li ON li.item_id = lr.item_id
             WHERE lr.item_id = ?1",
            [item_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()?
        .ok_or_else(|| invalid_target(format!("Item {item_id} is not a library root")))?;
    let kind = parse_kind(&kind)?;
    let lifecycle = parse_lifecycle(&lifecycle)?;
    let folder_ids = connection
        .prepare("SELECT folder_id FROM folder_item WHERE item_id = ?1 ORDER BY folder_id")?
        .query_map([item_id], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut media = connection
        .prepare(
            "WITH root_media(media_item_id, position) AS (
                 SELECT ?1, 0 WHERE ?2 = 'media'
                 UNION ALL
                 SELECT media_item_id, position_rank FROM collection_member
                 WHERE collection_id = ?1 AND ?2 = 'collection'
             )
             SELECT ma.item_id, mf.file_hash, mf.mime_type, mf.dominant_color_hex,
                    mf.dominant_palette_blob, mf.size_bytes, mf.pixel_width, mf.pixel_height,
                    mf.duration_ms, mf.frame_count, mf.has_audio, ma.name, ma.notes, ma.rating,
                    ma.source_urls_json, ma.captured_at, ma.imported_at, rm.position
             FROM root_media rm
             JOIN media_asset ma ON ma.item_id = rm.media_item_id
             JOIN media_file mf ON mf.file_id = ma.file_id
             ORDER BY rm.position, ma.item_id",
        )?
        .query_map(params![item_id, kind_string(kind)], |row| {
            let dominant_color_hex: Option<String> = row.get(3)?;
            let palette_blob: Option<Vec<u8>> = row.get(4)?;
            let mut dominant_colors: Vec<String> = palette_blob
                .as_deref()
                .and_then(|blob| {
                    crate::media_processing::colors::deserialize_dominant_palette_blob(blob).ok()
                })
                .map(|palette| palette.into_iter().map(|color| color.hex).collect())
                .unwrap_or_default();
            if dominant_colors.is_empty() {
                dominant_colors.extend(dominant_color_hex.iter().cloned());
            }
            Ok(MediaDetails {
                media_item_id: ItemId(row.get(0)?),
                file_hash: FileHash(row.get(1)?),
                mime_type: row.get(2)?,
                dominant_color_hex,
                dominant_colors,
                size_bytes: row.get(5)?,
                pixel_width: row.get(6)?,
                pixel_height: row.get(7)?,
                duration_ms: row.get(8)?,
                frame_count: row.get(9)?,
                has_audio: row.get(10)?,
                name: row.get(11)?,
                notes: row.get(12)?,
                rating: row.get(13)?,
                source_urls: {
                    let source_urls_json: Option<String> = row.get(14)?;
                    source_urls_json
                        .as_deref()
                        .and_then(|json| serde_json::from_str(json).ok())
                        .unwrap_or_default()
                },
                captured_at: row.get(15)?,
                imported_at: row.get(16)?,
                position: row.get(17)?,
                tags: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let media_indexes = media
        .iter()
        .enumerate()
        .map(|(index, media)| (media.media_item_id.0, index))
        .collect::<BTreeMap<_, _>>();
    let mut aggregate_tags = BTreeSet::new();
    if !media.is_empty() {
        let placeholders = std::iter::repeat_n("?", media.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT mt.media_item_id, t.namespace, t.subtag
             FROM media_tag mt JOIN tag t ON t.tag_id = mt.tag_id
             WHERE mt.media_item_id IN ({placeholders})
             ORDER BY mt.media_item_id, t.namespace, t.subtag"
        );
        let ids = media.iter().map(|item| item.media_item_id.0);
        let rows = connection
            .prepare(&sql)?
            .query_map(rusqlite::params_from_iter(ids), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (media_item_id, namespace, subtag) in rows {
            let name = if namespace == "general" {
                subtag
            } else {
                format!("{namespace}:{subtag}")
            };
            aggregate_tags.insert(name.clone());
            if let Some(index) = media_indexes.get(&media_item_id) {
                media[*index].tags.push(name);
            }
        }
    }
    Ok(ItemDetails {
        item_id: ItemId(item_id),
        kind,
        lifecycle,
        label,
        cover_media_item_id: cover_media_item_id.map(ItemId),
        folder_ids,
        media,
        aggregate_tags: aggregate_tags.into_iter().collect(),
        revision: crate::store::schema::revision(connection)?,
    })
}

fn parse_kind(value: &str) -> rusqlite::Result<ItemKind> {
    match value {
        "media" => Ok(ItemKind::Media),
        "collection" => Ok(ItemKind::Collection),
        _ => Err(invalid_target(format!("Unknown item kind: {value}"))),
    }
}

fn parse_lifecycle(value: &str) -> rusqlite::Result<Lifecycle> {
    match value {
        "inbox" => Ok(Lifecycle::Inbox),
        "active" => Ok(Lifecycle::Active),
        "trash" => Ok(Lifecycle::Trash),
        _ => Err(invalid_target(format!("Unknown lifecycle: {value}"))),
    }
}

fn kind_string(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Media => "media",
        ItemKind::Collection => "collection",
    }
}

/// Resolve a mutation target inside the caller's transaction. Query targets
/// use the same scope and filter compiler as grid pages and counts.
pub(crate) fn resolve_target_ids(
    connection: &Connection,
    target: &ItemTarget,
) -> rusqlite::Result<Vec<i64>> {
    match target {
        ItemTarget::Explicit { item_ids } => {
            let unique_ids = item_ids
                .iter()
                .map(|item_id| item_id.0)
                .collect::<std::collections::HashSet<_>>();
            if unique_ids.is_empty() || unique_ids.len() != item_ids.len() {
                return Err(invalid_target(
                    "An explicit target must contain unique library root IDs",
                ));
            }
            let encoded = serde_json::to_string(
                &item_ids.iter().map(|item_id| item_id.0).collect::<Vec<_>>(),
            )
            .map_err(|error| invalid_target(format!("Could not encode item target: {error}")))?;
            let mut statement = connection.prepare(
                "SELECT lr.item_id
                 FROM json_each(?1) target
                 JOIN library_root lr ON lr.item_id = CAST(target.value AS INTEGER)
                 ORDER BY CAST(target.key AS INTEGER)",
            )?;
            let resolved = statement
                .query_map([encoded], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            if resolved.len() != unique_ids.len() {
                return Err(invalid_target("A targeted item is not a library root"));
            }
            Ok(resolved)
        }
        ItemTarget::Query {
            query,
            excluded_item_ids,
        } => {
            let mut arguments: Vec<Box<dyn ToSql>> = vec![Box::new(match &query.scope {
                ItemScope::Folder { folder_id } => *folder_id,
                _ => -1,
            })];
            let mut predicates = vec![scope_predicate(connection, &query.scope, &mut arguments)?];
            apply_filters(&query.filters, &mut predicates, &mut arguments);
            if !excluded_item_ids.is_empty() {
                let excluded = excluded_item_ids
                    .iter()
                    .map(|item_id| {
                        let index = push_argument(&mut arguments, item_id.0);
                        format!("?{index}")
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                predicates.push(format!("ri.item_id NOT IN ({excluded})"));
            }
            let sql = format!(
                "WITH
                 root_items AS (
                     SELECT lr.item_id, lr.lifecycle, li.kind, li.label,
                            li.cover_media_item_id, li.created_at, li.updated_at,
                            fi.position_rank AS folder_position, mv.viewed_at
                     FROM library_root lr
                     JOIN library_item li ON li.item_id = lr.item_id
                     LEFT JOIN folder_item fi
                       ON fi.item_id = lr.item_id AND fi.folder_id = ?1
                     LEFT JOIN media_view mv ON mv.item_id = lr.item_id
                     WHERE li.kind = 'collection'
                        OR NOT EXISTS (
                            SELECT 1 FROM collection_member member_root
                            WHERE member_root.media_item_id = lr.item_id
                        )
                 ),
                 root_media AS (
                     SELECT ri.item_id AS root_item_id, ri.item_id AS media_item_id
                     FROM root_items ri WHERE ri.kind = 'media'
                     UNION ALL
                     SELECT ri.item_id, cm.media_item_id
                     FROM root_items ri
                     JOIN collection_member cm ON cm.collection_id = ri.item_id
                     WHERE ri.kind = 'collection'
                 )
                 SELECT ri.item_id FROM root_items ri
                 WHERE {}
                 ORDER BY ri.item_id",
                predicates.join(" AND ")
            );
            let references: Vec<&dyn ToSql> =
                arguments.iter().map(|value| value.as_ref()).collect();
            let mut statement = connection.prepare(&sql)?;
            let resolved = statement
                .query_map(references.as_slice(), |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(resolved)
        }
    }
}

fn invalid_target(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName(message.into())
}

fn resolve_connection(
    connection: &Connection,
    item_query: &ItemQuery,
    page: ItemPageRequest,
) -> rusqlite::Result<ItemPage> {
    let mut arguments: Vec<Box<dyn ToSql>> = Vec::new();

    // Parameter 1 is always present so the root CTE can expose folder order
    // without requiring a separate SQL shape for folder queries.
    arguments.push(Box::new(match &item_query.scope {
        ItemScope::Folder { folder_id } => *folder_id,
        _ => -1,
    }));

    let mut predicates = vec![scope_predicate(
        connection,
        &item_query.scope,
        &mut arguments,
    )?];
    apply_filters(&item_query.filters, &mut predicates, &mut arguments);
    let where_clause = predicates.join(" AND ");

    let order_by = order_expression(item_query, &mut arguments);
    let limit_index = arguments.len() + 1;
    arguments.push(Box::new(page.limit));
    let offset_index = arguments.len() + 1;
    arguments.push(Box::new(page.offset));

    let sql = format!(
        "WITH
         root_items AS (
             SELECT
                 lr.item_id,
                 lr.lifecycle,
                 li.kind,
                 li.label,
                 li.cover_media_item_id,
                 li.created_at,
                 li.updated_at,
                 fi.position_rank AS folder_position,
                 mv.viewed_at
             FROM library_root lr
             JOIN library_item li ON li.item_id = lr.item_id
             LEFT JOIN folder_item fi
               ON fi.item_id = lr.item_id AND fi.folder_id = ?1
             LEFT JOIN media_view mv ON mv.item_id = lr.item_id
             WHERE li.kind = 'collection'
                OR NOT EXISTS (
                    SELECT 1
                    FROM collection_member member_root
                    WHERE member_root.media_item_id = lr.item_id
                )
         ),
         root_media AS (
             SELECT ri.item_id AS root_item_id, ri.item_id AS media_item_id
             FROM root_items ri
             WHERE ri.kind = 'media'
             UNION ALL
             SELECT ri.item_id, cm.media_item_id
             FROM root_items ri
             JOIN collection_member cm ON cm.collection_id = ri.item_id
             WHERE ri.kind = 'collection'
         ),
         filtered_roots AS (
             SELECT
                 ri.item_id,
                 ri.lifecycle,
                 ri.kind,
                 ri.label,
                 ri.cover_media_item_id,
                 ri.created_at,
                 ri.updated_at,
                 ri.folder_position,
                 ri.viewed_at,
                 (
                     SELECT COUNT(*)
                     FROM root_media rm
                     WHERE rm.root_item_id = ri.item_id
                 ) AS collection_member_count,
                 COALESCE((
                     SELECT SUM(mf.size_bytes)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     JOIN media_file mf ON mf.file_id = ma.file_id
                     WHERE rm.root_item_id = ri.item_id
                 ), 0) AS total_size_bytes,
                 COALESCE((
                     SELECT MAX(ma.imported_at)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                 ), ri.created_at) AS imported_at,
                 (
                     SELECT MAX(ma.captured_at)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                 ) AS captured_at,
                 COALESCE(ri.label, (
                     SELECT ma.name
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                     ORDER BY rm.media_item_id ASC
                     LIMIT 1
                 )) AS sort_name,
                 (
                     SELECT MAX(ma.rating)
                     FROM root_media rm
                     JOIN media_asset ma ON ma.item_id = rm.media_item_id
                     WHERE rm.root_item_id = ri.item_id
                 ) AS sort_rating,
                 CASE
                     WHEN ri.kind = 'collection' THEN COALESCE(
                         ri.cover_media_item_id,
                         (
                             SELECT cm.media_item_id
                             FROM collection_member cm
                             WHERE cm.collection_id = ri.item_id
                             ORDER BY cm.position_rank ASC, cm.media_item_id ASC
                             LIMIT 1
                         )
                     )
                     ELSE ri.item_id
                 END AS resolved_cover_media_item_id
             FROM root_items ri
             WHERE {where_clause}
         ),
         metrics AS (
             SELECT
                 (SELECT revision FROM library_meta WHERE singleton = 1) AS revision,
                 COUNT(*) AS visible_item_count,
                 COALESCE(SUM(collection_member_count), 0) AS visible_media_count,
                 COALESCE(SUM(total_size_bytes), 0) AS total_size_bytes
             FROM filtered_roots
         ),
         paged AS (
             SELECT
                 fr.*,
                 display_asset.name AS display_name,
                 display_file.mime_type AS display_mime_type,
                 display_file.file_hash AS display_file_hash,
                 display_file.pixel_width,
                 display_file.pixel_height,
                 display_file.duration_ms,
                 display_file.frame_count,
                 display_file.has_audio,
                 display_file.dominant_color_hex
             FROM filtered_roots fr
             JOIN media_asset display_asset
               ON display_asset.item_id = fr.resolved_cover_media_item_id
             JOIN media_file display_file ON display_file.file_id = display_asset.file_id
             ORDER BY {order_by}, fr.item_id ASC
             LIMIT ?{limit_index} OFFSET ?{offset_index}
         )
         SELECT
             metrics.revision,
             metrics.visible_item_count,
             metrics.visible_media_count,
             metrics.total_size_bytes,
             paged.item_id,
             paged.kind,
             paged.lifecycle,
             paged.label,
             COALESCE(paged.label, paged.display_name),
             paged.resolved_cover_media_item_id,
             paged.display_file_hash,
             paged.display_mime_type,
             paged.pixel_width,
             paged.pixel_height,
             paged.duration_ms,
             paged.frame_count,
             paged.has_audio,
             paged.dominant_color_hex,
             paged.total_size_bytes,
             paged.sort_rating,
             paged.captured_at,
             paged.imported_at,
             paged.collection_member_count
         FROM metrics
         LEFT JOIN paged ON TRUE"
    );

    let references: Vec<&dyn ToSql> = arguments.iter().map(|value| value.as_ref()).collect();
    let mut statement = connection.prepare(&sql)?;
    let mut rows = statement.query(references.as_slice())?;

    let mut page_result: Option<ItemPage> = None;
    while let Some(row) = rows.next()? {
        let revision: u64 = row.get(0)?;
        let visible_item_count: i64 = row.get(1)?;
        let visible_media_count: i64 = row.get(2)?;
        let total_size_bytes: i64 = row.get(3)?;
        let item_id: Option<i64> = row.get(4)?;
        let items = if let Some(item_id) = item_id {
            vec![read_summary(row, item_id)?]
        } else {
            Vec::new()
        };

        if let Some(result) = &mut page_result {
            result.items.extend(items);
        } else {
            page_result = Some(ItemPage {
                items,
                revision,
                visible_item_count,
                visible_media_count,
                total_size_bytes,
            });
        }
    }

    Ok(page_result.unwrap_or(ItemPage {
        items: Vec::new(),
        revision: 0,
        visible_item_count: 0,
        visible_media_count: 0,
        total_size_bytes: 0,
    }))
}

fn read_summary(row: &rusqlite::Row<'_>, item_id: i64) -> rusqlite::Result<ItemSummary> {
    let kind = match row.get::<_, String>(5)?.as_str() {
        "media" => ItemKind::Media,
        "collection" => ItemKind::Collection,
        value => {
            return Err(rusqlite::Error::InvalidColumnType(
                5,
                value.into(),
                rusqlite::types::Type::Text,
            ))
        }
    };
    let lifecycle = match row.get::<_, String>(6)?.as_str() {
        "inbox" => Lifecycle::Inbox,
        "active" => Lifecycle::Active,
        "trash" => Lifecycle::Trash,
        value => {
            return Err(rusqlite::Error::InvalidColumnType(
                6,
                value.into(),
                rusqlite::types::Type::Text,
            ))
        }
    };

    Ok(ItemSummary {
        item_id: ItemId(item_id),
        kind,
        lifecycle,
        label: row.get(7)?,
        name: row.get(8)?,
        display_media_item_id: ItemId(row.get(9)?),
        display_file_hash: FileHash(row.get(10)?),
        display_mime_type: row.get(11)?,
        pixel_width: row.get(12)?,
        pixel_height: row.get(13)?,
        duration_ms: row.get(14)?,
        frame_count: row.get(15)?,
        has_audio: row.get(16)?,
        dominant_color_hex: row.get(17)?,
        size_bytes: row.get(18)?,
        rating: row.get(19)?,
        captured_at: row.get(20)?,
        imported_at: row.get(21)?,
        media_count: row.get(22)?,
    })
}

fn scope_predicate(
    connection: &Connection,
    scope: &ItemScope,
    arguments: &mut Vec<Box<dyn ToSql>>,
) -> rusqlite::Result<String> {
    let predicate = match scope {
        ItemScope::All => "ri.lifecycle = 'active'".to_string(),
        ItemScope::Inbox => "ri.lifecycle = 'inbox'".to_string(),
        ItemScope::Trash => "ri.lifecycle = 'trash'".to_string(),
        ItemScope::RecentlyViewed => {
            "ri.lifecycle = 'active' AND ri.viewed_at IS NOT NULL".to_string()
        }
        ItemScope::Untagged => "ri.lifecycle = 'active' AND NOT EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_tag mt ON mt.media_item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::Uncategorized => "ri.lifecycle = 'active' AND NOT EXISTS (
                 SELECT 1 FROM folder_item categorized_folder
                 WHERE categorized_folder.item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::Folder { .. } => "ri.lifecycle = 'active' AND EXISTS (
                 SELECT 1 FROM folder_item scoped_folder
                 WHERE scoped_folder.folder_id = ?1
                   AND scoped_folder.item_id = ri.item_id
             )"
        .to_string(),
        ItemScope::SmartFolder { smart_folder_id } => {
            let (sql, values) = crate::smart_v2::compile_smart_folder_sql(
                connection,
                *smart_folder_id,
                arguments.len(),
            )?;
            arguments.extend(
                values
                    .into_iter()
                    .map(|value| Box::new(value) as Box<dyn ToSql>),
            );
            format!("ri.lifecycle = 'active' AND ri.item_id IN ({sql})")
        }
    };
    Ok(predicate)
}

fn apply_filters(
    filters: &ItemFilters,
    predicates: &mut Vec<String>,
    arguments: &mut Vec<Box<dyn ToSql>>,
) {
    if let Some(text) = filters.text.as_deref().filter(|text| !text.is_empty()) {
        let index = push_argument(arguments, format!("%{text}%"));
        predicates.push(format!(
            "(ri.label LIKE ?{index} OR EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id
                   AND (ma.name LIKE ?{index}
                        OR ma.notes LIKE ?{index}
                        OR ma.source_urls_json LIKE ?{index})
             ))"
        ));
    }

    if let Some(rating) = filters.minimum_rating {
        let index = push_argument(arguments, rating);
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 WHERE rm.root_item_id = ri.item_id AND ma.rating >= ?{index}
             )"
        ));
    }

    if let Some(mime_prefix) = filters
        .mime_prefix
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let index = push_argument(arguments, format!("{mime_prefix}%"));
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 JOIN media_asset ma ON ma.item_id = rm.media_item_id
                 JOIN media_file mf ON mf.file_id = ma.file_id
                 WHERE rm.root_item_id = ri.item_id AND mf.mime_type LIKE ?{index}
             )"
        ));
    }

    for tag in &filters.include_tags {
        let (namespace, subtag) = split_tag(tag);
        let namespace_index = push_argument(arguments, namespace);
        let subtag_index = push_argument(arguments, subtag);
        let effective_match = crate::tags_v2::effective_tag_exists_sql(
            "rm.media_item_id",
            namespace_index,
            subtag_index,
        );
        predicates.push(format!(
            "EXISTS (
                 SELECT 1
                 FROM root_media rm
                 WHERE rm.root_item_id = ri.item_id
                   AND {effective_match}
             )"
        ));
    }

    for tag in &filters.exclude_tags {
        let (namespace, subtag) = split_tag(tag);
        let namespace_index = push_argument(arguments, namespace);
        let subtag_index = push_argument(arguments, subtag);
        let effective_match = crate::tags_v2::effective_tag_exists_sql(
            "rm.media_item_id",
            namespace_index,
            subtag_index,
        );
        predicates.push(format!(
            "NOT EXISTS (
                 SELECT 1
                 FROM root_media rm
                 WHERE rm.root_item_id = ri.item_id
                   AND {effective_match}
             )"
        ));
    }
}

fn split_tag(value: &str) -> (String, String) {
    value
        .split_once(':')
        .map(|(namespace, subtag)| {
            (
                namespace.trim().to_lowercase(),
                subtag.trim().to_lowercase(),
            )
        })
        .unwrap_or_else(|| ("general".to_string(), value.trim().to_lowercase()))
}

fn push_argument<T: ToSql + 'static>(arguments: &mut Vec<Box<dyn ToSql>>, value: T) -> usize {
    arguments.push(Box::new(value));
    arguments.len()
}

fn order_expression(item_query: &ItemQuery, arguments: &mut Vec<Box<dyn ToSql>>) -> String {
    use crate::app::{ItemSortField, SortDirection};

    if matches!(item_query.scope, ItemScope::RecentlyViewed)
        && matches!(item_query.sort.field, ItemSortField::ImportedAt)
    {
        return "fr.viewed_at DESC".to_string();
    }

    let expression = match item_query.sort.field {
        ItemSortField::ImportedAt => "fr.imported_at",
        ItemSortField::CapturedAt => "fr.captured_at",
        ItemSortField::Name => "fr.sort_name",
        ItemSortField::Rating => "COALESCE(fr.sort_rating, -1)",
        ItemSortField::Size => "fr.total_size_bytes",
        ItemSortField::Random => {
            let seed = stable_seed(item_query.sort.random_seed.as_deref().unwrap_or_default());
            let index = push_argument(arguments, seed);
            return format!(
                "((fr.item_id * 1103515245 + ?{index}) & 2147483647) {}",
                match item_query.sort.direction {
                    SortDirection::Ascending => "ASC",
                    SortDirection::Descending => "DESC",
                }
            );
        }
        ItemSortField::FolderOrder => "COALESCE(fr.folder_position, 9223372036854775807)",
    };

    let direction = match item_query.sort.direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    };
    format!("{expression} {direction}")
}

fn stable_seed(value: &str) -> i64 {
    value.bytes().fold(2_166_136_261_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(16_777_619)
    }) as i64
}

#[cfg(test)]
mod tests {
    use super::{
        details, query, resolve_target_ids, selection_summary, sidebar_counts, ItemPageRequest,
    };
    use crate::app::{ItemFilters, ItemId, ItemKind, ItemQuery, ItemScope, ItemSort, ItemTarget};
    use crate::store::Store;

    fn query_for(scope: ItemScope) -> ItemQuery {
        ItemQuery {
            scope,
            filters: ItemFilters::default(),
            sort: ItemSort::default(),
        }
    }

    fn seed_store() -> (tempfile::TempDir, Store) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        store
            .transaction(|tx| {
                insert_media(tx, 1, "standalone", "active", "one.jpg", "image/jpeg", 10, Some(2));
                insert_media(tx, 2, "inbox", "inbox", "inbox.jpg", "image/jpeg", 20, Some(1));
                insert_media(tx, 3, "trash", "trash", "trash.mp4", "video/mp4", 30, Some(4));
                insert_collection(tx, 10, "collection", "active", "Album");
                insert_media(tx, 11, "member-a", "active", "member-a.jpg", "image/jpeg", 40, Some(3));
                insert_media(tx, 12, "member-b", "active", "member-b.mp4", "video/mp4", 50, Some(5));
                tx.execute(
                    "INSERT INTO collection_member (collection_id, media_item_id, position_rank)
                     VALUES (10, 11, 0), (10, 12, 1)",
                    [],
                )?;
                tx.execute("INSERT INTO folder (folder_id, folder_key, name, created_at, updated_at) VALUES (7, 'folder', 'Folder', 'now', 'now')", [])?;
                tx.execute("INSERT INTO folder_item (folder_id, item_id, position_rank) VALUES (7, 10, 0), (7, 1, 1)", [])?;
                tx.execute("INSERT INTO tag (tag_id, namespace, subtag) VALUES (1, 'general', 'member-tag')", [])?;
                tx.execute("INSERT INTO media_tag (media_item_id, tag_id) VALUES (11, 1)", [])?;
                tx.execute(
                    "UPDATE media_file
                     SET dominant_color_hex = '#123456',
                         dominant_palette_blob = CAST(
                             '[{\"hex\":\"#123456\",\"l\":12.0,\"a\":1.0,\"b\":2.0},{\"hex\":\"#abcdef\",\"l\":80.0,\"a\":3.0,\"b\":4.0}]'
                             AS BLOB
                         )
                     WHERE file_id = 11",
                    [],
                )?;
                tx.execute("INSERT INTO media_view (item_id, viewed_at) VALUES (1, '2026-02-01')", [])?;
                Ok(())
            })
            .unwrap();
        (directory, store)
    }

    fn insert_collection(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        lifecycle: &str,
        label: &str,
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, label, created_at, updated_at)
             VALUES (?1, ?2, 'collection', ?3, '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key, label],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
    }

    fn insert_media(
        tx: &rusqlite::Transaction<'_>,
        item_id: i64,
        item_key: &str,
        lifecycle: &str,
        name: &str,
        mime_type: &str,
        size_bytes: i64,
        rating: Option<i64>,
    ) {
        tx.execute(
            "INSERT INTO library_item (item_id, item_key, kind, created_at, updated_at)
             VALUES (?1, ?2, 'media', '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, item_key],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, ?2)",
            rusqlite::params![item_id, lifecycle],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_file (file_id, file_hash, mime_type, size_bytes, created_at)
             VALUES (?1, ?2, ?3, ?4, '2026-01-01')",
            rusqlite::params![item_id, format!("hash-{item_id}"), mime_type, size_bytes],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO media_asset (item_id, file_id, name, rating, imported_at, updated_at)
             VALUES (?1, ?1, ?2, ?3, '2026-01-01', '2026-01-01')",
            rusqlite::params![item_id, name, rating],
        )
        .unwrap();
    }

    #[test]
    fn all_is_active_and_collapses_collection_members() {
        let (_directory, store) = seed_store();
        let page = query(
            &store,
            &query_for(ItemScope::All),
            ItemPageRequest::default(),
        )
        .unwrap();

        assert_eq!(page.items.len(), 2);
        let collection = page
            .items
            .iter()
            .find(|item| item.item_id == ItemId(10))
            .unwrap();
        assert_eq!(collection.kind, ItemKind::Collection);
        assert_eq!(collection.media_count, 2);
        assert_eq!(collection.size_bytes, 90);
        assert_eq!(page.visible_item_count, 2);
        assert_eq!(page.visible_media_count, 3);
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(11)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(12)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(2)));
        assert!(!page.items.iter().any(|item| item.item_id == ItemId(3)));
    }

    #[test]
    fn inbox_and_trash_are_separate_root_scopes() {
        let (_directory, store) = seed_store();
        let inbox = query(
            &store,
            &query_for(ItemScope::Inbox),
            ItemPageRequest::default(),
        )
        .unwrap();
        let trash = query(
            &store,
            &query_for(ItemScope::Trash),
            ItemPageRequest::default(),
        )
        .unwrap();

        assert_eq!(
            inbox
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(2)]
        );
        assert_eq!(
            trash
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(3)]
        );
        assert_eq!(inbox.visible_item_count, 1);
        assert_eq!(trash.visible_media_count, 1);
    }

    #[test]
    fn folder_scope_is_active_and_root_only() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::Folder { folder_id: 7 });
        item_query.sort.field = crate::app::ItemSortField::FolderOrder;
        item_query.sort.direction = crate::app::SortDirection::Ascending;
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10), ItemId(1)]
        );
        assert_eq!(page.visible_item_count, 2);
        assert_eq!(page.visible_media_count, 3);
    }

    #[test]
    fn media_filters_project_member_matches_to_collection_root() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.include_tags = vec!["member-tag".to_string()];
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(10)]
        );
        assert_eq!(page.visible_item_count, 1);
        assert_eq!(page.visible_media_count, 2);
    }

    #[test]
    fn revision_and_counts_survive_empty_pages() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.text = Some("does-not-exist".to_string());
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert!(page.items.is_empty());
        assert_eq!(page.visible_item_count, 0);
        assert_eq!(page.visible_media_count, 0);
        assert_eq!(page.revision, 1);
    }

    #[test]
    fn active_only_derived_scopes_use_the_same_root_set() {
        let (_directory, store) = seed_store();

        let recent = query(
            &store,
            &query_for(ItemScope::RecentlyViewed),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(recent.items[0].item_id, ItemId(1));

        let untagged = query(
            &store,
            &query_for(ItemScope::Untagged),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(
            untagged
                .items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );

        let uncategorized = query(
            &store,
            &query_for(ItemScope::Uncategorized),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert!(uncategorized.items.is_empty());
    }

    #[test]
    fn excluded_member_tag_excludes_the_collection_root() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.filters.exclude_tags = vec!["member-tag".to_string()];
        let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();

        assert_eq!(
            page.items
                .iter()
                .map(|item| item.item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(1)]
        );
    }

    #[test]
    fn tag_filters_use_aliases_and_transitive_implications() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO tag (tag_id, namespace, subtag) VALUES
                         (2, 'general', 'alias-tag'),
                         (3, 'general', 'parent-tag'),
                         (4, 'general', 'grandparent-tag')",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO tag_alias (from_tag_id, to_tag_id) VALUES (1, 2)",
                    [],
                )?;
                transaction.execute(
                    "INSERT INTO tag_implication (child_tag_id, parent_tag_id) VALUES
                         (1, 3), (3, 4)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        for tag in ["alias-tag", "parent-tag", "grandparent-tag"] {
            let mut item_query = query_for(ItemScope::All);
            item_query.filters.include_tags = vec![tag.to_string()];
            let page = query(&store, &item_query, ItemPageRequest::default()).unwrap();
            assert_eq!(page.items.len(), 1, "effective tag {tag}");
            assert_eq!(page.items[0].item_id, ItemId(10));
        }
    }

    #[test]
    fn seeded_random_order_is_stable() {
        let (_directory, store) = seed_store();
        let mut item_query = query_for(ItemScope::All);
        item_query.sort.field = crate::app::ItemSortField::Random;
        item_query.sort.random_seed = Some("stable".to_string());

        let first = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        let second = query(&store, &item_query, ItemPageRequest::default()).unwrap();
        assert_eq!(first.items, second.items);
    }

    #[test]
    fn smart_folder_scope_uses_the_same_page_and_count_query() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (9, 'smart:9', 'Tagged', ?1, 'now', 'now')",
                    [serde_json::json!({
                        "groups": [{
                            "match_mode": "all",
                            "negate": false,
                            "rules": [{
                                "field": "tags",
                                "op": "include",
                                "values": ["general:member-tag"]
                            }]
                        }]
                    })
                    .to_string()],
                )?;
                Ok(())
            })
            .unwrap();

        let page = query(
            &store,
            &query_for(ItemScope::SmartFolder { smart_folder_id: 9 }),
            ItemPageRequest::default(),
        )
        .unwrap();
        assert_eq!(page.visible_item_count, 1);
        assert_eq!(page.visible_media_count, 2);
        assert_eq!(page.items[0].item_id, ItemId(10));
    }

    #[test]
    fn query_mutation_target_matches_grid_and_applies_exclusions() {
        let (_directory, store) = seed_store();
        let target = ItemTarget::Query {
            query: query_for(ItemScope::All),
            excluded_item_ids: vec![ItemId(1)],
        };
        let ids = store
            .read(|connection| resolve_target_ids(connection, &target))
            .unwrap();
        assert_eq!(ids, vec![10]);
    }

    #[test]
    fn explicit_target_preserves_order_without_sql_parameter_expansion() {
        let (_directory, store) = seed_store();
        let target = ItemTarget::Explicit {
            item_ids: vec![ItemId(10), ItemId(1), ItemId(3), ItemId(2)],
        };
        let ids = store
            .read(|connection| resolve_target_ids(connection, &target))
            .unwrap();
        assert_eq!(ids, vec![10, 1, 3, 2]);

        let duplicate = ItemTarget::Explicit {
            item_ids: vec![ItemId(1), ItemId(1)],
        };
        assert!(store
            .read(|connection| resolve_target_ids(connection, &duplicate))
            .is_err());
    }

    #[test]
    fn collection_details_are_ordered_and_aggregate_member_tags() {
        let (_directory, store) = seed_store();
        let details = details(&store, ItemId(10)).unwrap();

        assert_eq!(details.kind, ItemKind::Collection);
        assert_eq!(details.folder_ids, vec![7]);
        assert_eq!(
            details
                .media
                .iter()
                .map(|media| media.media_item_id)
                .collect::<Vec<_>>(),
            vec![ItemId(11), ItemId(12)]
        );
        assert_eq!(details.aggregate_tags, vec!["member-tag"]);
        assert_eq!(
            details.media[0].dominant_color_hex.as_deref(),
            Some("#123456")
        );
        assert_eq!(details.media[0].dominant_colors, vec!["#123456", "#abcdef"]);
        assert_eq!(details.media[0].tags, vec!["member-tag"]);
        assert!(details.media[1].tags.is_empty());
    }

    #[test]
    fn selection_summary_uses_the_same_root_and_media_counts_as_grid() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_tag (media_item_id, tag_id) VALUES (1, 1)",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let target = ItemTarget::Query {
            query: query_for(ItemScope::All),
            excluded_item_ids: Vec::new(),
        };
        let summary = selection_summary(&store, &target).unwrap();

        assert_eq!(summary.selected_count, 2);
        assert_eq!(summary.stats.mime_counts["image/jpeg"], 2);
        assert_eq!(summary.stats.mime_counts["video/mp4"], 1);
        assert_eq!(summary.stats.total_size_bytes, Some(100));
        assert_eq!(summary.shared_tags[0].tag, "member-tag");
        assert_eq!(summary.shared_tags[0].count, 2);
        assert_eq!(summary.stats.rating_stats.min, Some(2));
        assert_eq!(summary.stats.rating_stats.max, Some(5));
        assert_eq!(summary.shared_folders.len(), 1);
        assert_eq!(summary.revision, 2);
    }

    #[test]
    fn sidebar_counts_match_canonical_active_root_scopes() {
        let (_directory, store) = seed_store();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO smart_folder (
                         smart_folder_id, smart_folder_key, name, predicate_json,
                         created_at, updated_at
                     ) VALUES (9, 'smart:9', 'Tagged', ?1, 'now', 'now')",
                    [serde_json::json!({
                        "groups": [{
                            "match_mode": "all",
                            "negate": false,
                            "rules": [{
                                "field": "tags",
                                "op": "include",
                                "values": ["general:member-tag"]
                            }]
                        }]
                    })
                    .to_string()],
                )?;
                Ok(())
            })
            .unwrap();

        let counts = sidebar_counts(&store).unwrap();
        assert_eq!(counts.all, 2);
        assert_eq!(counts.inbox, 1);
        assert_eq!(counts.trash, 1);
        assert_eq!(counts.recently_viewed, 1);
        assert_eq!(counts.untagged, 1);
        assert_eq!(counts.uncategorized, 0);
        assert_eq!(counts.folders[0].count, 2);
        assert_eq!(counts.smart_folders[0].count, 1);
        assert_eq!(counts.revision, 2);
    }
}
