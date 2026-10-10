use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use parking_lot::Mutex;
use roaring::RoaringBitmap;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::model::{
    FileId, FolderId, ImmutableMediaFacts, LabColor, Lifecycle, MediaId, MediaRecord, Rating,
    RootDetails, RootId, RootKind, RootRecord, SmartFolderId,
};
use crate::ordering::OrderOwnerKind;
use crate::predicate::{self, FilterExpr, ItemSort, SortDirection, SortField, ViewQuerySpec};
use crate::projection::ProjectionSnapshot;
use crate::query_dependencies::QueryKey;
use crate::{LibraryError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ItemScope {
    All,
    Inbox,
    Trash,
    MediaMatches { item_id: u32 },
    RecentlyViewed,
    Untagged,
    Uncategorized,
    Folder { folder_id: FolderId },
    FolderTree { folder_id: FolderId },
    SmartFolder { smart_folder_id: SmartFolderId },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootQuery {
    pub scope: ItemScope,
    #[serde(default)]
    pub view: ViewQuerySpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PageRequest {
    pub limit: usize,
    pub cursor: Option<String>,
}

/// An exact ordinal window in the canonical filtered order, not a SQL OFFSET scan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowRequest {
    pub position: WindowPosition,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum WindowPosition {
    Ordinal(usize),
    Fraction(f64),
}

impl WindowRequest {
    pub fn at(start: usize, limit: usize) -> Self {
        Self {
            position: WindowPosition::Ordinal(start),
            limit,
        }
    }

    fn resolve(&self, total: usize, limit: usize) -> Result<usize> {
        let start = match self.position {
            WindowPosition::Ordinal(start) => start,
            WindowPosition::Fraction(fraction) => {
                if !fraction.is_finite() || !(0.0..=1.0).contains(&fraction) {
                    return Err(LibraryError::InvalidInput(
                        "scroll fraction must be between zero and one".into(),
                    ));
                }
                // Keep the existing 500-item alignment and one-page leading buffer.
                let index = (fraction * total as f64).floor() as usize;
                index.saturating_sub(500) / 500 * 500
            }
        };
        Ok(start.min(total.saturating_sub(limit)))
    }
}

#[cfg(test)]
mod position_tests {
    use super::*;

    #[test]
    fn fractions_preserve_alignment_and_clamp_to_the_actual_matching_set() {
        let at = |fraction| WindowRequest {
            position: WindowPosition::Fraction(fraction),
            limit: 1500,
        };
        assert_eq!(at(0.5).resolve(1_000_000, 1500).unwrap(), 499_500);
        assert_eq!(at(1.0).resolve(1_000_000, 1500).unwrap(), 998_500);
        assert_eq!(at(1.0).resolve(3, 1500).unwrap(), 0);
        assert_eq!(at(0.0).resolve(0, 1500).unwrap(), 0);
        for invalid in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
            assert!(at(invalid).resolve(1_000_000, 1500).is_err());
        }
        assert!(serde_json::from_str::<WindowRequest>(r#"{"start":0,"limit":500}"#).is_err());
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootWindow {
    #[serde(flatten)]
    pub page: RootPage,
    pub start: usize,
    pub head_items: Vec<RootSummary>,
}

struct OrderedResult {
    revision: u64,
    key: QueryKey,
    query: RootQuery,
    ids: Vec<RootId>,
}

/// Up to eight compact orders, capped at three million IDs (12 MB). This lets
/// several small smart folders coexist with a million-item order.
#[derive(Default)]
pub(crate) struct OrderCache(Mutex<Vec<Arc<OrderedResult>>>);

impl OrderCache {
    fn get(&self, key: &QueryKey, query: &RootQuery) -> Option<Arc<OrderedResult>> {
        let mut entries = self.0.lock();
        let index = entries
            .iter()
            .position(|entry| entry.key == *key && entry.query == *query)?;
        let entry = entries.remove(index);
        entries.push(entry.clone());
        Some(entry)
    }

    fn insert(&self, value: Arc<OrderedResult>) {
        let mut entries = self.0.lock();
        // A slower obsolete read must not evict the newer revision's order.
        if entries.iter().any(|entry| entry.revision > value.revision) {
            return;
        }
        entries.retain(|entry| entry.query != value.query);
        entries.push(value);
        while entries.len() > 8
            || (entries.len() > 1 && entries.iter().map(|e| e.ids.len()).sum::<usize>() > 3_000_000)
        {
            entries.remove(0);
        }
    }
}

/// Roots sharing a file with an inspected item. Duplicate inspection is the
/// one scope whose membership is not resident: file identities live only in
/// canonical tables, so the caller resolves it inside a consistent read before
/// the query runs on the captured snapshot. Every other scope ignores it.
#[derive(Debug, Default)]
pub struct MediaMatches(Option<RoaringBitmap>);

impl MediaMatches {
    pub fn required(scope: &ItemScope) -> bool {
        matches!(scope, ItemScope::MediaMatches { .. })
    }

    pub fn resolve(
        connection: &Connection,
        snapshot: &ProjectionSnapshot,
        scope: &ItemScope,
    ) -> Result<Self> {
        let ItemScope::MediaMatches { item_id } = scope else {
            return Ok(Self(None));
        };
        Ok(Self(Some(media_match_roots(
            connection, snapshot, *item_id,
        )?)))
    }

    fn roots(&self) -> Result<&RoaringBitmap> {
        self.0.as_ref().ok_or_else(|| {
            LibraryError::InvalidState("media matches were not resolved before the query".into())
        })
    }
}

pub(crate) fn window_cached(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &WindowRequest,
    media: &MediaMatches,
    matches_cache: &MatchCache,
    order_cache: &OrderCache,
) -> Result<RootWindow> {
    crate::query_request::check()?;
    let limit = request.limit.clamp(1, 1500);
    let matches = cached_matches(snapshot, query, media, Some(matches_cache))?;
    let start = request.resolve(matches.len() as usize, limit)?;
    let started = Instant::now();
    let ids = window_ids(
        snapshot,
        query,
        &matches,
        start,
        limit,
        order_cache,
        &matches_cache.metrics,
    )?;
    let head_ids = if start == 0 && ids.len() >= (matches.len() as usize).min(4) {
        ids.iter().take(4).copied().collect::<Vec<_>>()
    } else {
        window_ids(
            snapshot,
            query,
            &matches,
            0,
            4,
            order_cache,
            &matches_cache.metrics,
        )?
    };
    matches_cache
        .metrics
        .order_us
        .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    crate::query_request::check()?;
    // Hydrate each root once, even when the requested window overlaps its preview.
    let mut selected = ids.clone();
    // Only four preview IDs need lookup. Avoid building two extra hash tables
    // for every 1,500-item scroll window just to recover those four positions.
    selected.extend(head_ids.iter().copied().filter(|id| !ids.contains(id)));
    let mut items = measured_summaries(snapshot, &selected, Some(matches_cache))?;
    let head_items = head_ids
        .iter()
        .map(|id| {
            items[selected
                .iter()
                .position(|selected| selected == id)
                .expect("selected preview")]
            .clone()
        })
        .collect();
    items.truncate(ids.len());
    crate::query_request::check()?;
    Ok(RootWindow {
        start,
        head_items,
        page: RootPage {
            items,
            next_cursor: None,
            total: matches.len(),
            media_count: matches.media_count,
            total_size_bytes: matches.total_size_bytes,
            revision: snapshot.revision,
        },
    })
}

fn window_ids(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    matches: &RoaringBitmap,
    start: usize,
    limit: usize,
    order_cache: &OrderCache,
    metrics: &QueryMetrics,
) -> Result<Vec<RootId>> {
    crate::query_request::check()?;
    if matches.is_empty() {
        return Ok(Vec::new());
    }
    if let Some(ids) = direct_window(snapshot, query, matches, start, limit)? {
        return Ok(ids);
    }
    let order = cached_complete_order(snapshot, query, matches, Some((order_cache, metrics)))?;
    Ok(order.ids[start..(start + limit).min(order.ids.len())].to_vec())
}

/// Ordinal windows that seek the resident order directly, without
/// enumerating the matching set. Only the recently viewed order, a short
/// resident list in its own display order, is served by enumeration instead.
fn direct_window(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    matches: &RoaringBitmap,
    start: usize,
    limit: usize,
) -> Result<Option<Vec<RootId>>> {
    if matches!(query.scope, ItemScope::RecentlyViewed) {
        return Ok(None);
    }
    let descending = query.view.sort.direction == SortDirection::Descending;
    let ids = match query.view.sort.field {
        SortField::FolderOrder if matches!(query.scope, ItemScope::Folder { .. }) => {
            let ItemScope::Folder { folder_id } = query.scope else {
                unreachable!()
            };
            let mut output = Vec::with_capacity(limit);
            let mut remaining = start;
            if let Some(order) = snapshot.folder_orders.get(&folder_id) {
                for (index, id) in order.iter().enumerate() {
                    if index % 256 == 0 {
                        crate::query_request::check()?;
                    }
                    if !matches.contains(id.0) {
                        continue;
                    }
                    if remaining > 0 {
                        remaining -= 1;
                        continue;
                    }
                    output.push(*id);
                    if output.len() == limit {
                        break;
                    }
                }
            }
            output
        }
        SortField::Name => snapshot
            .name_index
            .window(matches, start, limit, descending)?
            .into_iter()
            .map(RootId)
            .collect(),
        SortField::Random => random_window_ids(matches, &query.view.sort, start, limit),
        // Folder trees have always used descending import order, not the
        // explicit order of any one constituent folder.
        SortField::FolderOrder => snapshot
            .imported_at
            .ordered_window(matches, start, limit, true, true)?
            .expect("date windows cover every root")
            .into_iter()
            .map(RootId)
            .collect(),
        SortField::ImportedAt | SortField::CapturedAt => {
            let captured = query.view.sort.field == SortField::CapturedAt;
            let index = if captured {
                &snapshot.captured_at
            } else {
                &snapshot.imported_at
            };
            index
                .ordered_window(matches, start, limit, descending, true)?
                .expect("date windows cover every root")
                .into_iter()
                .map(RootId)
                .collect()
        }
        SortField::TotalSize => snapshot
            .total_bytes
            .ordered_window_total(matches, start, limit, descending)?
            .into_iter()
            .map(RootId)
            .collect(),
        SortField::Rating => {
            let mut ratings = Rating::ALL;
            if descending {
                ratings.reverse();
            }
            let mut skip = start;
            let mut output = Vec::with_capacity(limit);
            for rating in ratings {
                let roots = snapshot.rating(rating) & matches;
                if skip >= roots.len() as usize {
                    skip -= roots.len() as usize;
                    continue;
                }
                for ordinal in skip..(roots.len() as usize).min(skip + limit - output.len()) {
                    let rank = if descending {
                        roots.len() as usize - 1 - ordinal
                    } else {
                        ordinal
                    };
                    if let Some(id) = roots.select(rank as u32) {
                        output.push(RootId(id));
                    }
                }
                skip = 0;
                if output.len() == limit {
                    break;
                }
            }
            output
        }
    };
    Ok(Some(ids))
}

/// Every matching root in display order: the recently viewed list filtered
/// to the matches, or one complete direct window for any other sort. Cursor
/// paging and range selection need positions, not ordinal windows.
fn complete_order(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    matches: &RoaringBitmap,
) -> Result<Vec<RootId>> {
    if matches!(query.scope, ItemScope::RecentlyViewed) {
        return Ok(snapshot
            .recent_views
            .iter()
            .filter(|(id, _)| matches.contains(id.0))
            .map(|(id, _)| *id)
            .collect());
    }
    Ok(
        direct_window(snapshot, query, matches, 0, matches.len() as usize)?
            .expect("every other sort has a direct window"),
    )
}

fn cached_complete_order(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    matches: &RoaringBitmap,
    caches: Option<(&OrderCache, &QueryMetrics)>,
) -> Result<Arc<OrderedResult>> {
    let key = QueryKey::ordered(snapshot, query);
    if let Some(ordered) = caches.and_then(|(cache, _)| cache.get(&key, query)) {
        return Ok(ordered);
    }
    let ids = complete_order(snapshot, query, matches)?;
    crate::query_request::check()?;
    let ordered = Arc::new(OrderedResult {
        revision: snapshot.revision,
        key,
        query: query.clone(),
        ids,
    });
    if let Some((cache, metrics)) = caches {
        metrics.order_builds.fetch_add(1, Ordering::Relaxed);
        cache.insert(ordered.clone());
    }
    Ok(ordered)
}

impl Default for PageRequest {
    fn default() -> Self {
        Self {
            limit: 200,
            cursor: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootSummary {
    pub root_id: RootId,
    pub kind: RootKind,
    pub lifecycle: Lifecycle,
    pub name: String,
    pub cover_media_id: crate::MediaId,
    pub content_hash: String,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
    pub frame_count: Option<u32>,
    pub palette: Vec<LabColor>,
    pub imported_at_ms: i64,
    pub captured_at_ms: Option<i64>,
    pub modified_at_ms: i64,
    pub media_count: u32,
    pub total_size_bytes: u64,
    pub rating: Rating,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootPage {
    pub items: Vec<RootSummary>,
    pub next_cursor: Option<String>,
    pub total: u64,
    pub media_count: u128,
    pub total_size_bytes: u128,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MimeFacet {
    pub mime: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MimeFacets {
    pub values: Vec<MimeFacet>,
    pub revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryCounts {
    pub all: u64,
    pub inbox: u64,
    pub trash: u64,
    pub tags: HashMap<crate::TagId, u64>,
    pub folders: HashMap<FolderId, u64>,
    pub smart_folders: HashMap<SmartFolderId, u64>,
    pub untagged: u64,
    pub uncategorized: u64,
    pub revision: u64,
}

pub fn counts(snapshot: &ProjectionSnapshot) -> LibraryCounts {
    let mut counts = scope_counts(snapshot, &crate::smart::SmartCountCache::default());
    let active = snapshot.active();
    counts.tags = snapshot
        .tags
        .iter()
        .map(|(tag_id, roots)| (*tag_id, roots.intersection_len(active)))
        .collect();
    counts
}

/// Counts for every scope except per-tag counts, which cost one intersection
/// per tag in the library and are not shown in the sidebar.
pub fn scope_counts(
    snapshot: &ProjectionSnapshot,
    smart_counts: &crate::smart::SmartCountCache,
) -> LibraryCounts {
    let active = snapshot.active();
    let tags = Default::default();
    let folders = snapshot
        .folders
        .iter()
        .map(|(folder_id, roots)| (*folder_id, roots.intersection_len(active)))
        .collect();
    let smart_folders = snapshot
        .smart_results
        .keys()
        .map(|id| (SmartFolderId(*id), smart_counts.active_count(snapshot, *id)))
        .collect();
    LibraryCounts {
        all: active.len(),
        inbox: snapshot.lifecycle(Lifecycle::Inbox).len(),
        trash: snapshot.lifecycle(Lifecycle::Trash).len(),
        tags,
        folders,
        smart_folders,
        untagged: snapshot.tag_count.between_within(Some(0), Some(0), active).len(),
        uncategorized: snapshot.folder_count.between_within(Some(0), Some(0), active).len(),
        revision: snapshot.revision,
    }
}

pub fn details(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    root_id: RootId,
) -> Result<RootDetails> {
    let root = connection
        .query_row(
            "SELECT item.stable_key, item.item_kind, root.name, root.notes,
                    root.source_urls_json, root.cover_media_id, root.imported_at_ms,
                    root.captured_at_ms, root.modified_at_ms, root.media_count,
                    root.total_size_bytes
             FROM library_root root
             JOIN library_item item ON item.local_id = root.root_id
             WHERE root.root_id = ?1",
            [root_id.0],
            |row| {
                let kind = match row.get::<_, i64>(1)? {
                    1 => RootKind::Media,
                    2 => RootKind::Collection,
                    value => {
                        return Err(rusqlite::Error::FromSqlConversionFailure(
                            1,
                            rusqlite::types::Type::Integer,
                            format!("invalid root kind {value}").into(),
                        ))
                    }
                };
                let source_urls_json = row.get::<_, String>(4)?;
                let source_urls = serde_json::from_str(&source_urls_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
                Ok(RootRecord {
                    root_id,
                    stable_key: row.get(0)?,
                    kind,
                    name: row.get(2)?,
                    notes: row.get(3)?,
                    source_urls,
                    cover_media_id: MediaId(row.get(5)?),
                    imported_at_ms: row.get(6)?,
                    captured_at_ms: row.get(7)?,
                    modified_at_ms: row.get(8)?,
                    media_count: row.get(9)?,
                    total_size_bytes: row.get::<_, i64>(10)? as u64,
                })
            },
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                LibraryError::NotFound(format!("root {root_id}"))
            }
            other => other.into(),
        })?;

    let lifecycle = Lifecycle::ALL
        .into_iter()
        .find(|value| snapshot.lifecycle(*value).contains(root_id.0))
        .ok_or_else(|| {
            LibraryError::InvalidState(format!("root {root_id} has no lifecycle partition"))
        })?;
    let rating = Rating::ALL
        .into_iter()
        .find(|value| snapshot.rating(*value).contains(root_id.0))
        .ok_or_else(|| {
            LibraryError::InvalidState(format!("root {root_id} has no rating partition"))
        })?;
    let mut folder_ids = snapshot
        .folders
        .iter()
        .filter_map(|(folder_id, roots)| roots.contains(root_id.0).then_some(*folder_id))
        .collect::<Vec<_>>();
    folder_ids.sort_unstable();
    let mut tag_ids = snapshot
        .tags
        .iter()
        .filter_map(|(tag_id, roots)| roots.contains(root_id.0).then_some(*tag_id))
        .collect::<Vec<_>>();
    tag_ids.sort_unstable();

    let media_ids = match root.kind {
        RootKind::Media => vec![MediaId(root_id.0)],
        RootKind::Collection => {
            crate::ordering::load(connection, OrderOwnerKind::Collection, root_id.0)?
                .ok_or_else(|| {
                    LibraryError::InvalidState(format!("collection {root_id} has no member order"))
                })?
                .into_iter()
                .map(MediaId)
                .collect()
        }
    };
    if media_ids.len() != root.media_count as usize {
        return Err(LibraryError::InvalidState(format!(
            "root {root_id} reports {} media but stores {} members",
            root.media_count,
            media_ids.len()
        )));
    }
    let media = load_media(connection, &media_ids)?;

    Ok(RootDetails {
        root,
        lifecycle,
        rating,
        folder_ids,
        tag_ids,
        media,
        revision: snapshot.revision,
    })
}

/// Media records in the given order, read with one query for all members.
fn load_media(connection: &Connection, media_ids: &[MediaId]) -> Result<Vec<MediaRecord>> {
    let mut statement = connection.prepare_cached(
        "SELECT media.media_id, media.media_name, media.media_notes, file.file_id,
                file.file_path, file.mime, file.size_bytes, file.width, file.height,
                file.duration_ms, file.frame_count, file.content_hash, file.perceptual_hash,
                file.palette_json
         FROM media_item media
         JOIN media_file file ON file.file_id = media.file_id
         WHERE media.media_id IN (SELECT value FROM json_each(?1))",
    )?;
    let ids = serde_json::to_string(&media_ids.iter().map(|id| id.0).collect::<Vec<_>>())?;
    let mut by_id = statement
        .query_map([ids], |row| {
            let palette_json = row.get::<_, String>(13)?;
            let palette =
                serde_json::from_str::<Vec<LabColor>>(&palette_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        13,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?;
            let media_id = MediaId(row.get(0)?);
            Ok((
                media_id,
                MediaRecord {
                    media_id,
                    media_name: row.get(1)?,
                    media_notes: row.get(2)?,
                    file_id: FileId(row.get(3)?),
                    file_path: row.get(4)?,
                    facts: ImmutableMediaFacts {
                        mime: row.get(5)?,
                        size_bytes: row.get::<_, i64>(6)? as u64,
                        width: row.get(7)?,
                        height: row.get(8)?,
                        duration_ms: row.get::<_, Option<i64>>(9)?.map(|value| value as u64),
                        frame_count: row.get(10)?,
                        content_hash: row.get(11)?,
                        perceptual_hash: row.get(12)?,
                        palette,
                    },
                },
            ))
        })?
        .collect::<std::result::Result<std::collections::HashMap<_, _>, _>>()?;
    media_ids
        .iter()
        .map(|media_id| {
            by_id
                .remove(media_id)
                .ok_or_else(|| LibraryError::InvalidState(format!("missing media {media_id}")))
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum Cursor {
    Integer { value: i64, root_id: u32 },
    Text { value: String, root_id: u32 },
    Rating { value: u8, root_id: u32 },
    Random { index: usize },
    Vector { index: usize },
}

/// Bounded match/aggregate cache owned by one open library. Dependency versions
/// belong to each consistent snapshot, so unrelated writes preserve reusable data.
pub struct MatchCache {
    entries: ArcSwap<Vec<MatchCacheEntry>>,
    writes: Mutex<()>,
    metrics: QueryMetrics,
}

#[derive(Default)]
struct QueryMetrics {
    match_builds: AtomicU64,
    match_us: AtomicU64,
    aggregate_us: AtomicU64,
    order_builds: AtomicU64,
    order_us: AtomicU64,
    summary_us: AtomicU64,
    summary_items: AtomicU64,
}

/// Cumulative stage timings for diagnostics and repeatable benchmark comparisons.
#[derive(Debug, Clone, Serialize)]
pub struct QueryStatistics {
    pub match_builds: u64,
    pub match_us: u64,
    pub aggregate_us: u64,
    pub order_builds: u64,
    pub order_us: u64,
    pub summary_us: u64,
    pub summary_items: u64,
}

#[derive(Clone)]
struct MatchCacheEntry {
    revision: u64,
    key: QueryKey,
    scope: ItemScope,
    filter: FilterExpr,
    matches: Arc<MatchedResult>,
}

struct MatchedResult {
    ids: Arc<RoaringBitmap>,
    aggregate_key: QueryKey,
    media_count: u128,
    total_size_bytes: u128,
}

impl std::ops::Deref for MatchedResult {
    type Target = RoaringBitmap;
    fn deref(&self) -> &Self::Target {
        &self.ids
    }
}

// Preserve the existing eight expensive text results even when users browse
// many cheap structured scopes. Both classes share the same query/cache path.
const MATCH_CACHE_CLASS_LIMIT: usize = 8;

impl Default for MatchCache {
    fn default() -> Self {
        Self {
            entries: ArcSwap::from_pointee(Vec::new()),
            writes: Mutex::new(()),
            metrics: QueryMetrics::default(),
        }
    }
}

impl MatchCache {
    pub fn statistics(&self) -> QueryStatistics {
        let m = &self.metrics;
        QueryStatistics {
            match_builds: m.match_builds.load(Ordering::Relaxed),
            match_us: m.match_us.load(Ordering::Relaxed),
            aggregate_us: m.aggregate_us.load(Ordering::Relaxed),
            order_builds: m.order_builds.load(Ordering::Relaxed),
            order_us: m.order_us.load(Ordering::Relaxed),
            summary_us: m.summary_us.load(Ordering::Relaxed),
            summary_items: m.summary_items.load(Ordering::Relaxed),
        }
    }

    fn get(
        &self,
        key: &QueryKey,
        scope: &ItemScope,
        filter: &FilterExpr,
    ) -> Option<Arc<MatchedResult>> {
        let entries = self.entries.load();
        entries
            .iter()
            .find(|entry| entry.key == *key && entry.scope == *scope && entry.filter == *filter)
            .map(|entry| entry.matches.clone())
    }

    fn insert(
        &self,
        revision: u64,
        key: QueryKey,
        scope: &ItemScope,
        filter: &FilterExpr,
        matches: Arc<MatchedResult>,
    ) {
        let _write = self.writes.lock();
        let mut entries = self.entries.load_full().as_ref().clone();
        if entries.iter().any(|entry| entry.revision > revision) {
            return;
        }
        if entries.iter().any(|entry| {
            entry.key == key
                && entry.scope == *scope
                && entry.filter == *filter
                && entry.matches.aggregate_key == matches.aggregate_key
        }) {
            return;
        }
        entries.retain(|entry| entry.scope != *scope || entry.filter != *filter);
        let is_text = predicate::contains_text(filter);
        let same_class =
            |entry: &MatchCacheEntry| predicate::contains_text(&entry.filter) == is_text;
        if entries.iter().filter(|entry| same_class(entry)).count() >= MATCH_CACHE_CLASS_LIMIT {
            if let Some(index) = entries.iter().position(same_class) {
                entries.remove(index);
            }
        }
        entries.push(MatchCacheEntry {
            revision,
            key,
            scope: scope.clone(),
            filter: filter.clone(),
            matches,
        });
        self.entries.store(Arc::new(entries));
    }
}

pub fn matching_roots(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    media: &MediaMatches,
) -> Result<RoaringBitmap> {
    let universe = scope_bitmap(snapshot, &query.scope, media)?;
    let mut text = |field, value: &str, candidates: &RoaringBitmap| {
        snapshot.text_index.search(field, value, candidates)
    };
    predicate::evaluate(&query.view.filter, &universe, snapshot, &mut text)
}

pub(crate) fn matching_roots_cached(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    media: &MediaMatches,
    cache: Option<&MatchCache>,
) -> Result<RoaringBitmap> {
    match cache {
        Some(cache) => Ok((*cached_matches(snapshot, query, media, Some(cache))?.ids).clone()),
        None => matching_roots(snapshot, query, media),
    }
}

/// Returns MIME values for the complete query result, independent of paging.
/// A collection contributes to every MIME represented by one of its members,
/// matching the semantics of the MIME filter projection itself.
pub fn mime_facets(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    media: &MediaMatches,
) -> Result<MimeFacets> {
    mime_facets_cached(snapshot, query, media, None)
}

/// Facets share the grid's match cache, so opening the type filter after a
/// page load costs one intersection count per MIME and no re-evaluation.
pub(crate) fn mime_facets_cached(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    media: &MediaMatches,
    cache: Option<&MatchCache>,
) -> Result<MimeFacets> {
    let matches = cached_matches(snapshot, query, media, cache)?;
    let matches: &RoaringBitmap = &matches.ids;
    let mut values = snapshot
        .mime
        .iter()
        .filter_map(|(mime, roots)| {
            let count = roots.intersection_len(matches);
            (count > 0).then(|| MimeFacet {
                mime: mime.clone(),
                count,
            })
        })
        .collect::<Vec<_>>();
    values.sort_by(|left, right| left.mime.cmp(&right.mime));
    Ok(MimeFacets {
        values,
        revision: snapshot.revision,
    })
}

pub(crate) fn page_cached(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &PageRequest,
    media: &MediaMatches,
    match_cache: &MatchCache,
    order_cache: &OrderCache,
) -> Result<RootPage> {
    page_inner(
        snapshot,
        query,
        request,
        media,
        Some((match_cache, order_cache)),
    )
}

fn page_inner(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &PageRequest,
    media: &MediaMatches,
    caches: Option<(&MatchCache, &OrderCache)>,
) -> Result<RootPage> {
    let limit = request.limit.clamp(1, 1000);
    let match_cache = caches.map(|(cache, _)| cache);
    let matches = cached_matches(snapshot, query, media, match_cache)?;
    if matches.is_empty() {
        return Ok(RootPage {
            items: Vec::new(),
            next_cursor: None,
            total: 0,
            media_count: 0,
            total_size_bytes: 0,
            revision: snapshot.revision,
        });
    }
    let cursor = request
        .cursor
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let started = Instant::now();
    let (ids, next_cursor) = ordered_page_ids(
        snapshot,
        query,
        &matches,
        limit,
        cursor,
        caches.map(|(match_cache, order_cache)| (order_cache, &match_cache.metrics)),
    )?;
    if let Some(cache) = match_cache {
        cache
            .metrics
            .order_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
    Ok(RootPage {
        items: measured_summaries(snapshot, &ids, match_cache)?,
        next_cursor,
        total: matches.len(),
        media_count: matches.media_count,
        total_size_bytes: matches.total_size_bytes,
        revision: snapshot.revision,
    })
}

fn cached_matches(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    media: &MediaMatches,
    cache: Option<&MatchCache>,
) -> Result<Arc<MatchedResult>> {
    let key = QueryKey::matches(snapshot, query);
    let aggregate_key = QueryKey::aggregates(snapshot);
    let hit = cache.and_then(|cache| cache.get(&key, &query.scope, &query.view.filter));
    if let Some(hit) = &hit {
        if hit.aggregate_key == aggregate_key {
            return Ok(hit.clone());
        }
    }
    let started = Instant::now();
    let ids = if let Some(hit) = &hit {
        hit.ids.clone()
    } else {
        Arc::new(matching_roots(snapshot, query, media)?)
    };
    crate::query_request::check()?;
    if let Some(cache) = cache.filter(|_| hit.is_none()) {
        cache.metrics.match_builds.fetch_add(1, Ordering::Relaxed);
        cache
            .metrics
            .match_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
    let started = Instant::now();
    let result = Arc::new(MatchedResult {
        aggregate_key,
        media_count: snapshot.media_count.sum(&ids),
        total_size_bytes: snapshot.total_bytes.sum(&ids),
        ids,
    });
    crate::query_request::check()?;
    if let Some(cache) = cache {
        cache
            .metrics
            .aggregate_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        cache.insert(
            snapshot.revision,
            key,
            &query.scope,
            &query.view.filter,
            result.clone(),
        );
    }
    Ok(result)
}

fn ordered_page_ids(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    matches: &RoaringBitmap,
    limit: usize,
    cursor: Option<Cursor>,
    caches: Option<(&OrderCache, &QueryMetrics)>,
) -> Result<(Vec<RootId>, Option<String>)> {
    if matches.is_empty() {
        return Ok((Vec::new(), None));
    }
    if !matches!(query.scope, ItemScope::RecentlyViewed) {
        match query.view.sort.field {
            SortField::FolderOrder if matches!(query.scope, ItemScope::Folder { .. }) => {
                return page_folder_order(snapshot, query, matches, limit, cursor)
            }
            SortField::Rating => {
                return page_rating(snapshot, matches, &query.view.sort, limit, cursor)
            }
            SortField::Random => return page_random(matches, &query.view.sort, limit, cursor),
            SortField::Name => {
                return page_name(
                    &snapshot.name_index,
                    matches,
                    &query.view.sort,
                    limit,
                    cursor,
                )
            }
            _ => {}
        }
    }
    let order = cached_complete_order(snapshot, query, matches, caches)?;
    page_by_integer_key(snapshot, query, &order.ids, limit, cursor)
}

/// The value half of the `(value, ID)` key that orders `id` under an integer
/// sort or the recently viewed order. Keys agree with the resident orders:
/// value first, ID second, both reversed together for descending sorts, and
/// a missing captured date ties with `-1` exactly as the canonical
/// `COALESCE(captured_at_ms, -1)` expression did.
fn integer_key(snapshot: &ProjectionSnapshot, query: &RootQuery, id: u32) -> i64 {
    if matches!(query.scope, ItemScope::RecentlyViewed) {
        return snapshot
            .recent_views
            .iter()
            .find(|(root, _)| root.0 == id)
            .map_or(i64::MIN, |(_, viewed_at_ms)| *viewed_at_ms);
    }
    match query.view.sort.field {
        SortField::CapturedAt => snapshot.captured_at.signed_value(id).unwrap_or(-1),
        SortField::TotalSize => snapshot
            .total_bytes
            .value(id)
            .map_or(i64::MIN, |value| value.min(i64::MAX as u64) as i64),
        _ => snapshot.imported_at.signed_value(id).unwrap_or(-1),
    }
}

fn page_by_integer_key(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    order: &[RootId],
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    // Recently viewed and folder-tree orders are always descending.
    let descending = matches!(query.scope, ItemScope::RecentlyViewed)
        || query.view.sort.field == SortField::FolderOrder
        || query.view.sort.direction == SortDirection::Descending;
    let start = match cursor {
        Some(Cursor::Integer { value, root_id }) => {
            let cursor = (value, root_id);
            order.partition_point(|id| {
                let key = (integer_key(snapshot, query, id.0), id.0);
                if descending {
                    key >= cursor
                } else {
                    key <= cursor
                }
            })
        }
        None => 0,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    crate::query_request::check()?;
    let ids = order[start..(start + limit).min(order.len())].to_vec();
    let next = (ids.len() == limit)
        .then(|| {
            ids.last().map(|id| {
                serde_json::to_string(&Cursor::Integer {
                    value: integer_key(snapshot, query, id.0),
                    root_id: id.0,
                })
            })
        })
        .flatten()
        .transpose()?;
    Ok((ids, next))
}

pub fn all_ordered_ids(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    media: &MediaMatches,
) -> Result<Vec<RootId>> {
    let matches = matching_roots(snapshot, query, media)?;
    complete_order(snapshot, query, &matches)
}

fn scope_bitmap(
    snapshot: &ProjectionSnapshot,
    scope: &ItemScope,
    media: &MediaMatches,
) -> Result<RoaringBitmap> {
    Ok(match scope {
        ItemScope::All => snapshot.active().clone(),
        ItemScope::Inbox => snapshot.lifecycle(Lifecycle::Inbox).clone(),
        ItemScope::Trash => snapshot.lifecycle(Lifecycle::Trash).clone(),
        ItemScope::MediaMatches { .. } => media.roots()?.clone(),
        ItemScope::Untagged => {
            let mut values = snapshot.active().clone();
            values &= snapshot.tag_count.between(Some(0), Some(0));
            values
        }
        ItemScope::Uncategorized => {
            let mut values = snapshot.active().clone();
            values &= snapshot.folder_count.between(Some(0), Some(0));
            values
        }
        ItemScope::Folder { folder_id } => {
            let mut values = snapshot
                .folders
                .get(folder_id)
                .map(|values| values.to_bitmap())
                .unwrap_or_default();
            values &= snapshot.active();
            values
        }
        ItemScope::FolderTree { folder_id } => {
            let mut values = RoaringBitmap::new();
            let mut visited = std::collections::HashSet::new();
            let mut pending = vec![*folder_id];
            while let Some(folder_id) = pending.pop() {
                if !visited.insert(folder_id) {
                    continue;
                }
                if let Some(members) = snapshot.folders.get(&folder_id) {
                    values |= members;
                }
                pending.extend(
                    snapshot
                        .folder_parents
                        .iter()
                        .filter(|(_, parent)| **parent == Some(folder_id))
                        .map(|(child, _)| *child),
                );
            }
            values &= snapshot.active();
            values
        }
        ItemScope::SmartFolder { smart_folder_id } => {
            let mut values = snapshot
                .smart_results
                .get(&smart_folder_id.0)
                .map(|values| values.to_bitmap())
                .unwrap_or_default();
            values &= snapshot.active();
            values
        }
        ItemScope::RecentlyViewed => {
            let mut values = snapshot
                .recent_views
                .iter()
                .map(|(id, _)| id.0)
                .collect::<RoaringBitmap>();
            values &= snapshot.active();
            values
        }
    })
}

fn media_match_roots(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    item_id: u32,
) -> Result<RoaringBitmap> {
    let media_ids = snapshot
        .collection_orders
        .get(&RootId(item_id))
        .map(|members| {
            members
                .iter()
                .map(|media_id| media_id.0)
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| vec![item_id]);
    let mut file_ids = RoaringBitmap::new();
    for chunk in media_ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT file_id FROM media_item WHERE media_id IN ({placeholders})");
        let mut statement = connection.prepare_cached(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk), |row| {
            row.get::<_, u32>(0)
        })?;
        file_ids.extend(rows.collect::<std::result::Result<Vec<_>, _>>()?);
    }

    let file_ids = file_ids.iter().collect::<Vec<_>>();
    let mut roots = RoaringBitmap::new();
    for chunk in file_ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!("SELECT media_id FROM media_item WHERE file_id IN ({placeholders})");
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(chunk), |row| {
            row.get::<_, u32>(0)
        })?;
        for media_id in rows {
            if let Some(root_id) = snapshot.media_owner.get(media_id?).copied() {
                roots.insert(root_id.0);
            }
        }
    }
    Ok(roots)
}

fn page_name(
    index: &crate::name_index::NameIndex,
    matches: &RoaringBitmap,
    sort: &ItemSort,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let descending = sort.direction == SortDirection::Descending;
    let start = match cursor {
        Some(Cursor::Text { value, root_id }) => index.after(matches, &value, root_id, descending),
        None => 0,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    let ids = index.window(matches, start, limit, descending)?;
    let next = if ids.len() == limit {
        ids.last()
            .map(|&root_id| {
                let value = index
                    .name(root_id)
                    .expect("name index returned its own root")
                    .to_owned();
                serde_json::to_string(&Cursor::Text { value, root_id })
            })
            .transpose()?
    } else {
        None
    };
    Ok((ids.into_iter().map(RootId).collect(), next))
}

fn page_rating(
    snapshot: &ProjectionSnapshot,
    matches: &RoaringBitmap,
    sort: &ItemSort,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let mut ratings = Rating::ALL;
    if sort.direction == SortDirection::Descending {
        ratings.reverse();
    }
    let (cursor_rating, cursor_root) = match cursor {
        Some(Cursor::Rating { value, root_id }) => (Some(value), Some(root_id)),
        None => (None, None),
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    let mut output = Vec::with_capacity(limit);
    let mut started = cursor_rating.is_none();
    for rating in ratings {
        let key = rating.bitmap_key() as u8;
        if !started {
            if cursor_rating == Some(key) {
                started = true;
            } else {
                continue;
            }
        }
        let mut roots = (snapshot.rating(rating) & matches)
            .iter()
            .collect::<Vec<_>>();
        if sort.direction == SortDirection::Descending {
            roots.reverse();
        }
        for root_id in roots {
            if cursor_rating == Some(key)
                && cursor_root.is_some_and(|cursor| match sort.direction {
                    SortDirection::Ascending => root_id <= cursor,
                    SortDirection::Descending => root_id >= cursor,
                })
            {
                continue;
            }
            output.push(RootId(root_id));
            if output.len() == limit {
                return Ok((
                    output,
                    Some(serde_json::to_string(&Cursor::Rating {
                        value: key,
                        root_id,
                    })?),
                ));
            }
        }
    }
    Ok((output, None))
}

fn page_random(
    matches: &RoaringBitmap,
    sort: &ItemSort,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let start = match cursor {
        Some(Cursor::Random { index }) => index,
        None => 0,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    let values = random_window_ids(matches, sort, start, limit);
    let next_index = start.saturating_add(values.len());
    let next = (next_index < matches.len() as usize)
        .then(|| serde_json::to_string(&Cursor::Random { index: next_index }))
        .transpose()?;
    Ok((values, next))
}

fn random_window_ids(
    matches: &RoaringBitmap,
    sort: &ItemSort,
    start: usize,
    limit: usize,
) -> Vec<RootId> {
    let total = matches.len() as usize;
    let end = start.saturating_add(limit).min(total);
    let seed = random_seed_key(sort.random_seed.as_deref().unwrap_or("picto"));
    (start..end)
        .filter_map(|ordinal| {
            let input = match sort.direction {
                SortDirection::Ascending => ordinal,
                SortDirection::Descending => total - 1 - ordinal,
            };
            let selected = random_permuted_index(input as u64, total as u64, seed);
            matches.select(selected as u32).map(RootId)
        })
        .collect()
}

fn random_seed_key(seed: &str) -> u64 {
    let digest = Sha256::digest(seed.as_bytes());
    u64::from_le_bytes(digest[..8].try_into().expect("eight bytes"))
}

/// A keyed format-preserving permutation of `[0, length)`. The underlying
/// operations are bijections modulo a power of two; cycle walking restricts
/// that permutation to the exact result length without modulo bias.
fn random_permuted_index(index: u64, length: u64, seed: u64) -> u64 {
    if length <= 1 {
        return 0;
    }
    let domain = length.next_power_of_two();
    let mask = domain - 1;
    let bits = domain.trailing_zeros();
    let shift_a = (bits / 3).max(1);
    let shift_b = (bits * 2 / 3).max(1);
    let multiplier = seed.rotate_left(17) | 1;
    let increment = seed.rotate_left(41);
    let mut value = index;
    loop {
        value = value.wrapping_add(seed) & mask;
        value ^= value >> shift_a;
        value = value.wrapping_mul(multiplier) & mask;
        value ^= (value << shift_b) & mask;
        value = value.wrapping_add(increment) & mask;
        value ^= value >> shift_b;
        if value < length {
            return value;
        }
    }
}

fn page_folder_order(
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    matches: &RoaringBitmap,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let ItemScope::Folder { folder_id } = query.scope else {
        return Err(LibraryError::InvalidInput(
            "folder ordering requires a folder scope".into(),
        ));
    };
    let start = match cursor {
        Some(Cursor::Vector { index }) => index,
        None => 0,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    let order = snapshot
        .folder_orders
        .get(&folder_id)
        .map(AsRef::as_ref)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut output = Vec::with_capacity(limit);
    let mut index = start;
    while index < order.len() && output.len() < limit {
        let root = order[index];
        index += 1;
        if matches.contains(root.0) {
            output.push(root);
        }
    }
    let next = (index < order.len())
        .then(|| serde_json::to_string(&Cursor::Vector { index }))
        .transpose()?;
    Ok((output, next))
}

fn measured_summaries(
    snapshot: &ProjectionSnapshot,
    ids: &[RootId],
    cache: Option<&MatchCache>,
) -> Result<Vec<RootSummary>> {
    let started = Instant::now();
    let result = load_summaries(snapshot, ids);
    if let Some(cache) = cache {
        cache
            .metrics
            .summary_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        cache
            .metrics
            .summary_items
            .fetch_add(ids.len() as u64, Ordering::Relaxed);
    }
    result
}

/// Grid summaries hydrated from the captured snapshot alone: drawing facts
/// from the resident records, names from the alphabetical index, kind,
/// lifecycle, rating and palette from the bitmaps and palette map.
fn load_summaries(snapshot: &ProjectionSnapshot, ids: &[RootId]) -> Result<Vec<RootSummary>> {
    let collections = snapshot.root_kinds.get(&RootKind::Collection);
    ids.iter()
        .enumerate()
        .map(|(index, root_id)| {
            if index % 256 == 0 {
                crate::query_request::check()?;
            }
            let id = root_id.0;
            let missing = || LibraryError::InvalidState(format!("missing root summary {root_id}"));
            let record = snapshot.drawing.record(id).ok_or_else(missing)?;
            let name = snapshot.name_index.name(id).ok_or_else(missing)?;
            Ok(RootSummary {
                root_id: *root_id,
                kind: if collections.is_some_and(|roots| roots.contains(id)) {
                    RootKind::Collection
                } else {
                    RootKind::Media
                },
                lifecycle: lifecycle_for(snapshot, id),
                name: name.into(),
                cover_media_id: crate::MediaId(record.row.cover),
                content_hash: record.content_hash.into(),
                mime: record.mime.into(),
                width: record.row.width,
                height: record.row.height,
                duration_ms: record.row.duration.map(|value| value as u64),
                frame_count: record.row.frames,
                palette: snapshot
                    .cover_palettes
                    .get(id)
                    .map(|palette| palette.as_ref().clone())
                    .unwrap_or_default(),
                imported_at_ms: record.row.imported,
                captured_at_ms: record.row.captured,
                modified_at_ms: record.row.modified,
                media_count: record.row.count,
                total_size_bytes: record.row.bytes,
                rating: rating_for(snapshot, id),
            })
        })
        .collect()
}

fn rating_for(snapshot: &ProjectionSnapshot, root_id: u32) -> Rating {
    Rating::ALL
        .into_iter()
        .find(|rating| snapshot.rating(*rating).contains(root_id))
        .unwrap_or(Rating::Unrated)
}

fn lifecycle_for(snapshot: &ProjectionSnapshot, root_id: u32) -> Lifecycle {
    Lifecycle::ALL
        .into_iter()
        .find(|lifecycle| snapshot.lifecycle(*lifecycle).contains(root_id))
        .unwrap_or(Lifecycle::Active)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_read_cannot_evict_a_newer_order_and_cache_is_bounded() {
        let cache = OrderCache::default();
        let query = RootQuery {
            scope: ItemScope::All,
            view: ViewQuerySpec::default(),
        };
        let entry = |revision, query: RootQuery| {
            Arc::new(OrderedResult {
                revision,
                key: QueryKey::test_key(revision),
                query,
                ids: vec![RootId(1)],
            })
        };
        cache.insert(entry(2, query.clone()));
        assert!(cache.get(&QueryKey::test_key(1), &query).is_none());
        cache.insert(entry(1, query.clone()));
        assert_eq!(
            cache.get(&QueryKey::test_key(2), &query).unwrap().revision,
            2
        );
        for scope in [ItemScope::Inbox, ItemScope::Trash, ItemScope::Untagged] {
            cache.insert(entry(
                2,
                RootQuery {
                    scope,
                    view: ViewQuerySpec::default(),
                },
            ));
        }
        assert_eq!(cache.0.lock().len(), 4);
        for id in 0..12 {
            cache.insert(entry(
                2,
                RootQuery {
                    scope: ItemScope::Folder {
                        folder_id: FolderId(id),
                    },
                    view: ViewQuerySpec::default(),
                },
            ));
        }
        assert_eq!(cache.0.lock().len(), 8);
        cache.insert(Arc::new(OrderedResult {
            revision: 3,
            key: QueryKey::test_key(3),
            query: query.clone(),
            ids: vec![RootId(1); 3_000_000],
        }));
        assert_eq!(cache.0.lock().len(), 1);
    }

    const NAMES: &[&str] = &[
        "Alpha", "alpha", "ALPHA", "beta-1", "Beta-2", "Gamma", "gamma", "Art", "art", "zeta",
    ];

    /// Canonical rows with ties, NOCASE-equal names and null captured dates:
    /// the SQL ORDER BY oracle every resident order is checked against.
    fn scan_fixture() -> Connection {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE library_root (
                     root_id INTEGER PRIMARY KEY,
                     name TEXT NOT NULL,
                     imported_at_ms INTEGER NOT NULL,
                     captured_at_ms INTEGER,
                     total_size_bytes INTEGER NOT NULL
                 );
                 CREATE TABLE recent_view (
                     root_id INTEGER PRIMARY KEY,
                     viewed_at_ms INTEGER NOT NULL
                 );",
            )
            .unwrap();
        for root_id in 1u32..=30 {
            connection
                .execute(
                    "INSERT INTO library_root VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        root_id,
                        NAMES[root_id as usize % NAMES.len()],
                        100 + (root_id % 7) as i64,
                        (root_id % 3 != 0).then_some(500 - root_id as i64),
                        ((root_id * 37) % 11) as i64,
                    ],
                )
                .unwrap();
        }
        connection
    }

    /// A snapshot whose resident indexes mirror the fixture rows exactly.
    fn resident_snapshot(connection: &Connection) -> ProjectionSnapshot {
        let directory = tempfile::tempdir().unwrap();
        let library = crate::Library::create(directory.path().join("library.sqlite")).unwrap();
        let mut snapshot = (*library.projections().snapshot()).clone();
        let mut statement = connection
            .prepare("SELECT root_id, imported_at_ms, captured_at_ms, total_size_bytes FROM library_root")
            .unwrap();
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .unwrap();
        for row in rows {
            let (id, imported, captured, bytes) = row.unwrap();
            Arc::make_mut(&mut snapshot.imported_at).insert(id, imported);
            if let Some(captured) = captured {
                Arc::make_mut(&mut snapshot.captured_at).insert(id, captured);
            }
            Arc::make_mut(&mut snapshot.total_bytes).insert(id, bytes as u64);
        }
        snapshot.name_index = Arc::new(crate::name_index::NameIndex::load(connection).unwrap());
        snapshot.recent_views = Arc::new(crate::projection::load_recent_views(connection).unwrap());
        snapshot
    }

    fn direction_sql(direction: SortDirection) -> &'static str {
        match direction {
            SortDirection::Ascending => "ASC",
            SortDirection::Descending => "DESC",
        }
    }

    fn oracle(connection: &Connection, sql: &str) -> Vec<RootId> {
        connection
            .prepare(sql)
            .unwrap()
            .query_map([], |row| row.get::<_, u32>(0).map(RootId))
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap()
    }

    fn sort(direction: SortDirection) -> ItemSort {
        ItemSort {
            field: SortField::ImportedAt,
            direction,
            random_seed: None,
        }
    }

    fn walk(
        mut fetch: impl FnMut(Option<Cursor>) -> (Vec<RootId>, Option<String>),
    ) -> Vec<(Vec<RootId>, Option<String>)> {
        let mut pages = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let decoded = cursor
                .as_deref()
                .map(|value| serde_json::from_str(value).unwrap());
            let (ids, next) = fetch(decoded);
            let done = next.is_none();
            pages.push((ids, next.clone()));
            cursor = next;
            if done {
                return pages;
            }
        }
    }

    #[test]
    fn resident_windows_cursor_pages_and_complete_orders_match_sql_with_ties_and_null_dates() {
        let connection = scan_fixture();
        connection
            .execute_batch(
                "WITH RECURSIVE n(id) AS (SELECT 31 UNION ALL SELECT id+1 FROM n WHERE id<4100)
            INSERT INTO library_root SELECT id, CASE WHEN id%2=0 THEN 'ALpha' ELSE 'alpha' END,
            id%11, CASE WHEN id%3=0 THEN NULL ELSE id%13 END, id%19 FROM n;
            INSERT INTO recent_view SELECT root_id, root_id%9 FROM library_root;",
            )
            .unwrap();
        let snapshot = resident_snapshot(&connection);
        let matches: RoaringBitmap = (1..=4100).filter(|id| id % 7 != 0).collect();
        for scope in [
            ItemScope::All,
            ItemScope::RecentlyViewed,
            ItemScope::FolderTree {
                folder_id: FolderId(1),
            },
        ] {
            for field in [
                SortField::ImportedAt,
                SortField::Name,
                SortField::CapturedAt,
                SortField::TotalSize,
                SortField::FolderOrder,
            ] {
                if field == SortField::FolderOrder && !matches!(scope, ItemScope::FolderTree { .. })
                {
                    continue;
                }
                for direction in [SortDirection::Ascending, SortDirection::Descending] {
                    let query = RootQuery {
                        scope: scope.clone(),
                        view: ViewQuerySpec {
                            sort: ItemSort {
                                field,
                                direction,
                                random_seed: None,
                            },
                            ..ViewQuerySpec::default()
                        },
                    };
                    let expected = if matches!(scope, ItemScope::RecentlyViewed) {
                        oracle(
                            &connection,
                            "SELECT root_id FROM recent_view WHERE root_id % 7 != 0
                             ORDER BY viewed_at_ms DESC, root_id DESC",
                        )
                    } else {
                        let (expression, direction) = match field {
                            SortField::ImportedAt => ("imported_at_ms", direction_sql(direction)),
                            SortField::Name => ("name COLLATE NOCASE", direction_sql(direction)),
                            SortField::CapturedAt => {
                                ("COALESCE(captured_at_ms, -1)", direction_sql(direction))
                            }
                            SortField::TotalSize => ("total_size_bytes", direction_sql(direction)),
                            SortField::FolderOrder => ("imported_at_ms", "DESC"),
                            SortField::Rating | SortField::Random => unreachable!(),
                        };
                        oracle(
                            &connection,
                            &format!(
                                "SELECT root_id FROM library_root WHERE root_id % 7 != 0
                                 ORDER BY {expression} {direction}, root_id {direction}"
                            ),
                        )
                    };
                    let label = format!("{scope:?} {field:?} {direction:?}");
                    assert_eq!(
                        complete_order(&snapshot, &query, &matches).unwrap(),
                        expected,
                        "{label}"
                    );
                    let pages = walk(|cursor| {
                        ordered_page_ids(&snapshot, &query, &matches, 137, cursor, None).unwrap()
                    });
                    let paged: Vec<_> = pages.into_iter().flat_map(|(ids, _)| ids).collect();
                    assert_eq!(paged, expected, "pages {label}");
                    let cache = OrderCache::default();
                    let metrics = QueryMetrics::default();
                    for (start, limit) in [
                        (0, 4),
                        (0, 1500),
                        (1, 7),
                        (2000, 137),
                        (expected.len() - 1, 1500),
                        (expected.len() - 3000, 1500),
                    ] {
                        assert_eq!(
                            window_ids(&snapshot, &query, &matches, start, limit, &cache, &metrics)
                                .unwrap(),
                            expected[start..(start + limit).min(expected.len())],
                            "window {label} {start} {limit}"
                        );
                    }
                }
            }
        }
        let empty = RoaringBitmap::new();
        let query = RootQuery {
            scope: ItemScope::All,
            view: ViewQuerySpec::default(),
        };
        assert_eq!(
            ordered_page_ids(&snapshot, &query, &empty, 7, None, None).unwrap(),
            (Vec::new(), None)
        );
    }

    #[test]
    fn folder_tree_scopes_walk_resident_parents_and_tolerate_cycles() {
        let directory = tempfile::tempdir().unwrap();
        let library = crate::Library::create(directory.path().join("library.sqlite")).unwrap();
        let mut snapshot = (*library.projections().snapshot()).clone();
        let folder = FolderId;
        snapshot.folder_parents = Arc::new(HashMap::from([
            (folder(1), None),
            (folder(2), Some(folder(1))),
            (folder(3), Some(folder(2))),
            (folder(4), None),
            (folder(5), Some(folder(6))),
            (folder(6), Some(folder(5))),
        ]));
        snapshot.folders = Arc::new(
            (1..=6)
                .map(|id| (folder(id), [id].into_iter().collect()))
                .collect(),
        );
        Arc::make_mut(&mut snapshot.lifecycle)
            .insert(Lifecycle::Active, (1..=5).collect::<RoaringBitmap>().into());
        let tree = |id| {
            scope_bitmap(
                &snapshot,
                &ItemScope::FolderTree {
                    folder_id: folder(id),
                },
                &MediaMatches::default(),
            )
            .unwrap()
            .iter()
            .collect::<Vec<_>>()
        };
        assert_eq!(tree(1), [1, 2, 3]);
        assert_eq!(tree(2), [2, 3]);
        assert_eq!(tree(4), [4]);
        assert_eq!(tree(5), [5]);
        assert!(tree(99).is_empty());
        assert!(matches!(
            scope_bitmap(
                &snapshot,
                &ItemScope::MediaMatches { item_id: 1 },
                &MediaMatches::default()
            ),
            Err(LibraryError::InvalidState(_))
        ));
    }

    #[test]
    fn seeded_random_indices_are_a_stable_bijection_for_arbitrary_lengths() {
        for length in [1u64, 2, 3, 7, 16, 31, 1_003, 10_001, 100_000] {
            let seed = random_seed_key("stable-random-test");
            let first = (0..length)
                .map(|index| random_permuted_index(index, length, seed))
                .collect::<Vec<_>>();
            let unique = first
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>();
            assert_eq!(unique.len(), length as usize, "length {length}");
            assert!(first.iter().all(|index| *index < length));
            assert_eq!(
                first,
                (0..length)
                    .map(|index| random_permuted_index(index, length, seed))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn random_pages_and_direct_windows_share_one_reversible_order() {
        let matches = [2u32, 4, 5, 9, 12, 13, 17, 18, 21, 26, 30]
            .into_iter()
            .collect::<RoaringBitmap>();
        let mut ascending_sort = sort(SortDirection::Ascending);
        ascending_sort.field = SortField::Random;
        ascending_sort.random_seed = Some("window-seed".into());
        let ascending = walk(|cursor| page_random(&matches, &ascending_sort, 3, cursor).unwrap())
            .into_iter()
            .flat_map(|(ids, _)| ids)
            .collect::<Vec<_>>();
        assert_eq!(
            ascending
                .iter()
                .copied()
                .collect::<std::collections::HashSet<_>>(),
            matches.iter().map(RootId).collect()
        );
        assert_eq!(
            random_window_ids(&matches, &ascending_sort, 3, 4),
            ascending[3..7]
        );

        let mut descending_sort = ascending_sort.clone();
        descending_sort.direction = SortDirection::Descending;
        let descending = walk(|cursor| page_random(&matches, &descending_sort, 4, cursor).unwrap())
            .into_iter()
            .flat_map(|(ids, _)| ids)
            .collect::<Vec<_>>();
        assert_eq!(
            descending,
            ascending.iter().rev().copied().collect::<Vec<_>>()
        );

        let mut other_seed = ascending_sort.clone();
        other_seed.random_seed = Some("other-seed".into());
        assert_ne!(
            random_window_ids(&matches, &ascending_sort, 0, matches.len() as usize),
            random_window_ids(&matches, &other_seed, 0, matches.len() as usize),
        );
    }

    #[test]
    fn name_pages_match_sqlite_in_both_directions() {
        let connection = scan_fixture();
        let index = crate::name_index::NameIndex::load(&connection).unwrap();
        let matches = (1..=30).filter(|id| id % 3 != 0).collect::<RoaringBitmap>();
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            let sql = format!("SELECT root_id FROM library_root WHERE root_id % 3 != 0 ORDER BY name COLLATE NOCASE {0}, root_id {0}", direction_sql(direction));
            let expected = oracle(&connection, &sql);
            for limit in [1, 3, 4, 10, 20] {
                let actual = walk(|cursor| {
                    page_name(&index, &matches, &sort(direction), limit, cursor).unwrap()
                })
                .into_iter()
                .flat_map(|(ids, _)| ids)
                .collect::<Vec<_>>();
                assert_eq!(actual, expected);
            }
        }
    }
}
