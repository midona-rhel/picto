use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use arc_swap::ArcSwap;
use parking_lot::Mutex;
use roaring::RoaringBitmap;
use rusqlite::{params_from_iter, types::Value, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fts;
use crate::model::{
    FileId, FolderId, ImmutableMediaFacts, LabColor, Lifecycle, MediaId, MediaRecord, Rating,
    RootDetails, RootId, RootKind, RootRecord, SmartFolderId,
};
use crate::ordering::OrderOwnerKind;
use crate::predicate::{self, FilterExpr, ItemSort, SortDirection, SortField, ViewQuerySpec};
use crate::projection::ProjectionSnapshot;
use crate::query_dependencies::QueryKey;
use crate::{LibraryError, Result};

const SCAN_CHUNK: usize = 1024;

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
    pub start: usize,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootWindow {
    #[serde(flatten)]
    pub page: RootPage,
    pub start: usize,
}

struct OrderedResult {
    revision: u64,
    key: QueryKey,
    query: RootQuery,
    ids: Vec<RootId>,
    media_count: u128,
    total_size_bytes: u128,
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

pub(crate) fn window_cached(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &WindowRequest,
    matches_cache: &MatchCache,
    order_cache: &OrderCache,
) -> Result<RootWindow> {
    let limit = request.limit.clamp(1, 1500);
    if query.view.sort.field == SortField::Random
        && !matches!(&query.scope, ItemScope::RecentlyViewed)
    {
        let matches = cached_matches(connection, snapshot, query, Some(matches_cache))?;
        let total = matches.len() as usize;
        let start = request.start.min(total.saturating_sub(limit));
        let started = Instant::now();
        let ids = random_window_ids(&matches, &query.view.sort, start, limit);
        matches_cache
            .metrics
            .order_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        return Ok(RootWindow {
            start,
            page: RootPage {
                items: measured_summaries(connection, snapshot, &ids, Some(matches_cache))?,
                next_cursor: None,
                total: matches.len(),
                media_count: matches.media_count,
                total_size_bytes: matches.total_size_bytes,
                revision: snapshot.revision,
            },
        });
    }
    let key = QueryKey::ordered(snapshot, query);
    let cached = order_cache.get(&key, query);
    // Keep first paint cheap: constructing the complete order is only necessary
    // once navigation leaves the initial page. Subsequent jumps reuse its IDs.
    if request.start == 0 && cached.is_none() && limit <= 1000 {
        return Ok(RootWindow {
            page: page_cached(
                connection,
                snapshot,
                query,
                &PageRequest {
                    limit,
                    cursor: None,
                },
                matches_cache,
            )?,
            start: 0,
        });
    }
    let ordered = if let Some(ordered) = cached {
        ordered
    } else {
        let matches = cached_matches(connection, snapshot, query, Some(matches_cache))?;
        let started = Instant::now();
        let (ids, _) = ordered_page_ids(
            connection,
            snapshot,
            query,
            &matches,
            matches.len() as usize + 1,
            None,
        )?;
        matches_cache
            .metrics
            .order_builds
            .fetch_add(1, Ordering::Relaxed);
        matches_cache
            .metrics
            .order_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        let ordered = Arc::new(OrderedResult {
            revision: snapshot.revision,
            key,
            query: query.clone(),
            ids,
            media_count: matches.media_count,
            total_size_bytes: matches.total_size_bytes,
        });
        order_cache.insert(ordered.clone());
        ordered
    };
    let start = request.start.min(ordered.ids.len().saturating_sub(limit));
    let end = (start + limit).min(ordered.ids.len());
    Ok(RootWindow {
        start,
        page: RootPage {
            items: measured_summaries(
                connection,
                snapshot,
                &ordered.ids[start..end],
                Some(matches_cache),
            )?,
            next_cursor: None,
            total: ordered.ids.len() as u64,
            media_count: ordered.media_count,
            total_size_bytes: ordered.total_size_bytes,
            revision: snapshot.revision,
        },
    })
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
    let active = snapshot.active();
    let tags = snapshot
        .tags
        .iter()
        .map(|(tag_id, roots)| (*tag_id, (roots & active).len()))
        .collect();
    let folders = snapshot
        .folders
        .iter()
        .map(|(folder_id, roots)| (*folder_id, (roots & active).len()))
        .collect();
    let smart_folders = snapshot
        .smart_results
        .iter()
        .map(|(smart_folder_id, roots)| (SmartFolderId(*smart_folder_id), roots.len()))
        .collect();
    LibraryCounts {
        all: active.len(),
        inbox: snapshot.lifecycle(Lifecycle::Inbox).len(),
        trash: snapshot.lifecycle(Lifecycle::Trash).len(),
        tags,
        folders,
        smart_folders,
        untagged: (active & &snapshot.tag_count.between(Some(0), Some(0))).len(),
        uncategorized: (active & &snapshot.folder_count.between(Some(0), Some(0))).len(),
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

fn load_media(connection: &Connection, media_ids: &[MediaId]) -> Result<Vec<MediaRecord>> {
    let mut statement = connection.prepare_cached(
        "SELECT media.media_name, media.media_notes, file.file_id, file.file_path, file.mime,
                file.size_bytes, file.width, file.height, file.duration_ms,
                file.frame_count, file.content_hash, file.perceptual_hash,
                file.palette_json
         FROM media_item media
         JOIN media_file file ON file.file_id = media.file_id
         WHERE media.media_id = ?1",
    )?;
    media_ids
        .iter()
        .map(|media_id| {
            statement
                .query_row([media_id.0], |row| {
                    let palette_json = row.get::<_, String>(12)?;
                    let palette =
                        serde_json::from_str::<Vec<LabColor>>(&palette_json).map_err(|error| {
                            rusqlite::Error::FromSqlConversionFailure(
                                12,
                                rusqlite::types::Type::Text,
                                Box::new(error),
                            )
                        })?;
                    Ok(MediaRecord {
                        media_id: *media_id,
                        media_name: row.get(0)?,
                        media_notes: row.get(1)?,
                        file_id: FileId(row.get(2)?),
                        file_path: row.get(3)?,
                        facts: ImmutableMediaFacts {
                            mime: row.get(4)?,
                            size_bytes: row.get::<_, i64>(5)? as u64,
                            width: row.get(6)?,
                            height: row.get(7)?,
                            duration_ms: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                            frame_count: row.get(9)?,
                            content_hash: row.get(10)?,
                            perceptual_hash: row.get(11)?,
                            palette,
                        },
                    })
                })
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::InvalidState(format!("missing media {media_id}"))
                    }
                    other => other.into(),
                })
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
    ids: RoaringBitmap,
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
        if entries
            .iter()
            .any(|entry| entry.key == key && entry.scope == *scope && entry.filter == *filter)
        {
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
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
) -> Result<RoaringBitmap> {
    let universe = scope_bitmap(connection, snapshot, &query.scope)?;
    let mut text = |field, value: &str| fts::search(connection, field, value);
    predicate::evaluate(&query.view.filter, &universe, snapshot, &mut text)
}

/// Returns MIME values for the complete query result, independent of paging.
/// A collection contributes to every MIME represented by one of its members,
/// matching the semantics of the MIME filter projection itself.
pub fn mime_facets(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
) -> Result<MimeFacets> {
    let matches = matching_roots(connection, snapshot, query)?;
    let mut values = snapshot
        .mime
        .iter()
        .filter_map(|(mime, roots)| {
            let count = (roots & &matches).len();
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

pub fn page(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &PageRequest,
) -> Result<RootPage> {
    page_inner(connection, snapshot, query, request, None)
}

pub(crate) fn page_cached(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &PageRequest,
    cache: &MatchCache,
) -> Result<RootPage> {
    page_inner(connection, snapshot, query, request, Some(cache))
}

fn page_inner(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &PageRequest,
    cache: Option<&MatchCache>,
) -> Result<RootPage> {
    let limit = request.limit.clamp(1, 1000);
    let matches = cached_matches(connection, snapshot, query, cache)?;
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
    let (ids, next_cursor) =
        ordered_page_ids(connection, snapshot, query, &matches, limit, cursor)?;
    if let Some(cache) = cache {
        cache
            .metrics
            .order_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
    Ok(RootPage {
        items: measured_summaries(connection, snapshot, &ids, cache)?,
        next_cursor,
        total: matches.len(),
        media_count: matches.media_count,
        total_size_bytes: matches.total_size_bytes,
        revision: snapshot.revision,
    })
}

fn cached_matches(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    cache: Option<&MatchCache>,
) -> Result<Arc<MatchedResult>> {
    let key = QueryKey::matches(snapshot, query);
    if let Some(hit) = cache.and_then(|cache| cache.get(&key, &query.scope, &query.view.filter)) {
        return Ok(hit);
    }
    let started = Instant::now();
    let ids = matching_roots(connection, snapshot, query)?;
    if let Some(cache) = cache {
        cache.metrics.match_builds.fetch_add(1, Ordering::Relaxed);
        cache
            .metrics
            .match_us
            .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
    }
    let started = Instant::now();
    let result = Arc::new(MatchedResult {
        media_count: snapshot.media_count.sum(&ids),
        total_size_bytes: snapshot.total_bytes.sum(&ids),
        ids,
    });
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
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    matches: &RoaringBitmap,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    if matches.is_empty() {
        return Ok((Vec::new(), None));
    }
    if cursor.is_none() && limit > matches.len() as usize && matches.len() > SPARSE_SCAN_MAXIMUM {
        // Enumerate the compact order once. Repeating limited scans would be
        // particularly costly for expression sorts without a matching index.
        let recent = matches!(query.scope, ItemScope::RecentlyViewed);
        let expression = if recent {
            Some("root.viewed_at_ms")
        } else {
            match query.view.sort.field {
                SortField::Name => Some("root.name COLLATE NOCASE"),
                SortField::ImportedAt => Some("root.imported_at_ms"),
                SortField::CapturedAt => Some("COALESCE(root.captured_at_ms, -1)"),
                SortField::TotalSize => Some("root.total_size_bytes"),
                SortField::FolderOrder if matches!(query.scope, ItemScope::FolderTree { .. }) => {
                    Some("root.imported_at_ms")
                }
                _ => None,
            }
        };
        if let Some(expression) = expression {
            let direction = if recent || (query.view.sort.field == SortField::FolderOrder) {
                "DESC"
            } else {
                direction_sql(query.view.sort.direction)
            };
            let table = if recent {
                "recent_view"
            } else {
                "library_root"
            };
            let sql = format!("SELECT root.root_id FROM {table} root ORDER BY {expression} {direction}, root.root_id {direction}");
            let mut statement = connection.prepare(&sql)?;
            let mut rows = statement.query([])?;
            let mut ids = Vec::with_capacity(matches.len() as usize);
            while let Some(row) = rows.next()? {
                let id = row.get::<_, u32>(0)?;
                if matches.contains(id) {
                    ids.push(RootId(id));
                }
            }
            return Ok((ids, None));
        }
    }
    let sparse = matches.len() <= SPARSE_SCAN_MAXIMUM;
    let (ids, next_cursor) = match query.scope {
        ItemScope::RecentlyViewed => page_recent_order(connection, matches, limit, cursor)?,
        _ => match query.view.sort.field {
            SortField::FolderOrder if matches!(&query.scope, ItemScope::FolderTree { .. }) => {
                let sort = ItemSort {
                    field: SortField::ImportedAt,
                    direction: SortDirection::Descending,
                    random_seed: None,
                };
                scan_integer(
                    connection,
                    "root.imported_at_ms",
                    matches,
                    sparse,
                    &sort,
                    limit,
                    cursor,
                )?
            }
            SortField::FolderOrder => page_folder_order(snapshot, query, matches, limit, cursor)?,
            SortField::Rating => page_rating(snapshot, matches, &query.view.sort, limit, cursor)?,
            SortField::Random => page_random(matches, &query.view.sort, limit, cursor)?,
            SortField::Name => {
                scan_text(connection, matches, sparse, &query.view.sort, limit, cursor)?
            }
            SortField::ImportedAt => scan_integer(
                connection,
                "root.imported_at_ms",
                matches,
                sparse,
                &query.view.sort,
                limit,
                cursor,
            )?,
            SortField::CapturedAt => scan_integer(
                connection,
                "COALESCE(root.captured_at_ms, -1)",
                matches,
                sparse,
                &query.view.sort,
                limit,
                cursor,
            )?,
            SortField::TotalSize => scan_integer(
                connection,
                "root.total_size_bytes",
                matches,
                sparse,
                &query.view.sort,
                limit,
                cursor,
            )?,
        },
    };
    Ok((ids, next_cursor))
}

pub fn all_ordered_ids(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
) -> Result<Vec<RootId>> {
    let mut cursor = None;
    let mut ids = Vec::new();
    loop {
        let page = page(
            connection,
            snapshot,
            query,
            &PageRequest {
                limit: 1000,
                cursor,
            },
        )?;
        ids.extend(page.items.into_iter().map(|item| item.root_id));
        cursor = page.next_cursor;
        if cursor.is_none() {
            return Ok(ids);
        }
    }
}

fn scope_bitmap(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    scope: &ItemScope,
) -> Result<RoaringBitmap> {
    Ok(match scope {
        ItemScope::All => snapshot.active().clone(),
        ItemScope::Inbox => snapshot.lifecycle(Lifecycle::Inbox).clone(),
        ItemScope::Trash => snapshot.lifecycle(Lifecycle::Trash).clone(),
        ItemScope::MediaMatches { item_id } => media_match_roots(connection, snapshot, *item_id)?,
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
            for descendant_id in
                descendant_ids(connection, "folder_definition", "folder_id", folder_id.0)?
            {
                if let Some(descendant) = snapshot.folders.get(&FolderId(descendant_id)) {
                    values |= descendant;
                }
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
            let mut statement = connection.prepare_cached("SELECT root_id FROM recent_view")?;
            let rows = statement.query_map([], |row| row.get::<_, u32>(0))?;
            let mut values = rows.collect::<std::result::Result<RoaringBitmap, _>>()?;
            values &= snapshot.active();
            values
        }
    })
}

fn descendant_ids(
    connection: &Connection,
    table: &str,
    id_column: &str,
    root_id: u32,
) -> Result<Vec<u32>> {
    let sql = format!(
        "WITH RECURSIVE descendants(id) AS (
             SELECT ?1
             UNION ALL
             SELECT child.{id_column}
             FROM {table} child
             JOIN descendants parent ON child.parent_id = parent.id
         )
         SELECT id FROM descendants"
    );
    let mut statement = connection.prepare(&sql)?;
    let rows = statement.query_map([root_id], |row| row.get::<_, u32>(0))?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
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

/// Below this many matches, fetch sort keys for just the matching roots and
/// order them in memory instead of scanning the table in sort order. A scan
/// reads roughly `total / matches` rows per emitted item, which for a sparse
/// match set dwarfs the point lookups this path performs. The branch choice is
/// stable across the pages of one query because the match count is.
const SPARSE_SCAN_MAXIMUM: u64 = 2048;

fn sparse_scan_integer(
    connection: &Connection,
    expression: &str,
    matches: &RoaringBitmap,
    sort: &ItemSort,
    limit: usize,
    cursor: Option<(i64, u32)>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let ids = matches.iter().collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT root.root_id, {expression} AS sort_value
             FROM library_root root
             WHERE root.root_id IN ({placeholders})"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(chunk), |row| {
            Ok((row.get::<_, i64>(1)?, row.get::<_, u32>(0)?))
        })?;
        for row in rows {
            entries.push(row?);
        }
    }
    entries.sort_unstable();
    if sort.direction == SortDirection::Descending {
        entries.reverse();
    }
    let mut output = Vec::with_capacity(limit);
    let mut last = None;
    for (value, root_id) in entries {
        if let Some(cursor) = cursor {
            let after = match sort.direction {
                SortDirection::Ascending => (value, root_id) > cursor,
                SortDirection::Descending => (value, root_id) < cursor,
            };
            if !after {
                continue;
            }
        }
        if output.len() == limit {
            break;
        }
        output.push(RootId(root_id));
        last = Some((value, root_id));
    }
    let next = (output.len() == limit)
        .then(|| {
            last.map(|(value, root_id)| serde_json::to_string(&Cursor::Integer { value, root_id }))
        })
        .flatten()
        .transpose()?;
    Ok((output, next))
}

fn scan_integer(
    connection: &Connection,
    expression: &str,
    matches: &RoaringBitmap,
    sparse: bool,
    sort: &ItemSort,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let direction = direction_sql(sort.direction);
    let mut scan_cursor = match cursor {
        Some(Cursor::Integer { value, root_id }) => Some((value, root_id)),
        None => None,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    if sparse {
        return sparse_scan_integer(connection, expression, matches, sort, limit, scan_cursor);
    }
    let mut output = Vec::with_capacity(limit);
    loop {
        let (where_sql, mut values) = integer_cursor_sql(expression, direction, scan_cursor);
        values.push(Value::Integer(SCAN_CHUNK as i64));
        let sql = format!(
            "SELECT root.root_id, {expression} AS sort_value
             FROM library_root root
             {where_sql}
             ORDER BY {expression} {direction}, root.root_id {direction}
             LIMIT ?{}",
            values.len()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok((output, None));
        }
        let row_count = rows.len();
        for (root_id, value) in rows {
            scan_cursor = Some((value, root_id));
            if matches.contains(root_id) {
                output.push(RootId(root_id));
                if output.len() == limit {
                    return Ok((
                        output,
                        Some(serde_json::to_string(&Cursor::Integer { value, root_id })?),
                    ));
                }
            }
        }
        if row_count < SCAN_CHUNK {
            return Ok((output, None));
        }
    }
}

/// Mirrors SQLite's NOCASE collation, which folds only ASCII upper case
/// before a byte-wise comparison.
fn nocase_key(value: &str) -> Vec<u8> {
    value
        .bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .collect()
}

fn sparse_scan_text(
    connection: &Connection,
    matches: &RoaringBitmap,
    sort: &ItemSort,
    limit: usize,
    cursor: Option<(String, u32)>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let ids = matches.iter().collect::<Vec<_>>();
    let mut entries = Vec::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT root.root_id, root.name
             FROM library_root root
             WHERE root.root_id IN ({placeholders})"
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(chunk), |row| {
            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (root_id, name) = row?;
            entries.push((nocase_key(&name), root_id, name));
        }
    }
    entries.sort_unstable_by(|left, right| (&left.0, left.1).cmp(&(&right.0, right.1)));
    if sort.direction == SortDirection::Descending {
        entries.reverse();
    }
    let cursor = cursor.map(|(value, root_id)| (nocase_key(&value), root_id));
    let mut output = Vec::with_capacity(limit);
    let mut last = None;
    for (key, root_id, name) in entries {
        if let Some((cursor_key, cursor_root)) = &cursor {
            let after = match sort.direction {
                SortDirection::Ascending => (&key, root_id) > (cursor_key, *cursor_root),
                SortDirection::Descending => (&key, root_id) < (cursor_key, *cursor_root),
            };
            if !after {
                continue;
            }
        }
        if output.len() == limit {
            break;
        }
        output.push(RootId(root_id));
        last = Some((name, root_id));
    }
    let next = (output.len() == limit)
        .then(|| {
            last.map(|(value, root_id)| serde_json::to_string(&Cursor::Text { value, root_id }))
        })
        .flatten()
        .transpose()?;
    Ok((output, next))
}

fn scan_text(
    connection: &Connection,
    matches: &RoaringBitmap,
    sparse: bool,
    sort: &ItemSort,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let direction = direction_sql(sort.direction);
    let mut scan_cursor = match cursor {
        Some(Cursor::Text { value, root_id }) => Some((value, root_id)),
        None => None,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    if sparse {
        return sparse_scan_text(connection, matches, sort, limit, scan_cursor);
    }
    let mut output = Vec::with_capacity(limit);
    loop {
        let (where_sql, mut values) = text_cursor_sql(direction, scan_cursor.as_ref());
        values.push(Value::Integer(SCAN_CHUNK as i64));
        let sql = format!(
            "SELECT root.root_id, root.name
             FROM library_root root {where_sql}
             ORDER BY root.name COLLATE NOCASE {direction}, root.root_id {direction}
             LIMIT ?{}",
            values.len()
        );
        let mut statement = connection.prepare(&sql)?;
        let rows = statement
            .query_map(params_from_iter(values), |row| {
                Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if rows.is_empty() {
            return Ok((output, None));
        }
        let row_count = rows.len();
        for (root_id, value) in rows {
            scan_cursor = Some((value.clone(), root_id));
            if matches.contains(root_id) {
                output.push(RootId(root_id));
                if output.len() == limit {
                    return Ok((
                        output,
                        Some(serde_json::to_string(&Cursor::Text { value, root_id })?),
                    ));
                }
            }
        }
        if row_count < SCAN_CHUNK {
            return Ok((output, None));
        }
    }
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

fn page_recent_order(
    connection: &Connection,
    matches: &RoaringBitmap,
    limit: usize,
    cursor: Option<Cursor>,
) -> Result<(Vec<RootId>, Option<String>)> {
    let mut scan_cursor = match cursor {
        Some(Cursor::Integer { value, root_id }) => Some((value, root_id)),
        None => None,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match recently viewed order".into(),
            ))
        }
    };
    let mut output = Vec::with_capacity(limit);
    loop {
        let rows = if let Some((viewed_at_ms, root_id)) = scan_cursor {
            let mut statement = connection.prepare_cached(
                "SELECT root_id, viewed_at_ms FROM recent_view
                 WHERE (viewed_at_ms, root_id) < (?1, ?2)
                 ORDER BY viewed_at_ms DESC, root_id DESC LIMIT ?3",
            )?;
            let values = statement
                .query_map(
                    rusqlite::params![viewed_at_ms, root_id, SCAN_CHUNK as i64],
                    |row| Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?)),
                )?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            values
        } else {
            let mut statement = connection.prepare_cached(
                "SELECT root_id, viewed_at_ms FROM recent_view
                 ORDER BY viewed_at_ms DESC, root_id DESC LIMIT ?1",
            )?;
            let values = statement
                .query_map([SCAN_CHUNK as i64], |row| {
                    Ok((row.get::<_, u32>(0)?, row.get::<_, i64>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            values
        };
        if rows.is_empty() {
            return Ok((output, None));
        }
        for (root_id, viewed_at_ms) in &rows {
            scan_cursor = Some((*viewed_at_ms, *root_id));
            if matches.contains(*root_id) {
                output.push(RootId(*root_id));
                if output.len() == limit {
                    return Ok((
                        output,
                        Some(serde_json::to_string(&Cursor::Integer {
                            value: *viewed_at_ms,
                            root_id: *root_id,
                        })?),
                    ));
                }
            }
        }
        if rows.len() < SCAN_CHUNK {
            return Ok((output, None));
        }
    }
}

fn measured_summaries(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    ids: &[RootId],
    cache: Option<&MatchCache>,
) -> Result<Vec<RootSummary>> {
    let started = Instant::now();
    let result = load_summaries(connection, snapshot, ids);
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

fn load_summaries(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    ids: &[RootId],
) -> Result<Vec<RootSummary>> {
    let mut by_id = HashMap::with_capacity(ids.len());
    for chunk in ids.chunks(500) {
        let placeholders = std::iter::repeat_n("?", chunk.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT root.root_id, root.name, root.cover_media_id,
                file.content_hash, file.mime, file.width, file.height, file.duration_ms,
                file.frame_count, root.imported_at_ms,
                root.captured_at_ms, root.modified_at_ms, root.media_count,
                root.total_size_bytes
         FROM library_root root
         JOIN media_item media ON media.media_id = root.cover_media_id
         JOIN media_file file ON file.file_id = media.file_id
         WHERE root.root_id IN ({placeholders})"
        );
        let mut statement = connection.prepare_cached(&sql)?;
        let rows = statement.query_map(
            params_from_iter(chunk.iter().map(|root_id| root_id.0)),
            |row| {
                let id = row.get::<_, u32>(0)?;
                Ok(RootSummary {
                    root_id: RootId(id),
                    kind: if snapshot
                        .root_kinds
                        .get(&RootKind::Collection)
                        .is_some_and(|roots| roots.contains(id))
                    {
                        RootKind::Collection
                    } else {
                        RootKind::Media
                    },
                    lifecycle: lifecycle_for(snapshot, id),
                    name: row.get(1)?,
                    cover_media_id: crate::MediaId(row.get(2)?),
                    content_hash: row.get(3)?,
                    mime: row.get(4)?,
                    width: row.get(5)?,
                    height: row.get(6)?,
                    duration_ms: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
                    frame_count: row.get(8)?,
                    palette: snapshot
                        .cover_palettes
                        .get(id)
                        .map(|palette| palette.as_ref().clone())
                        .unwrap_or_default(),
                    imported_at_ms: row.get(9)?,
                    captured_at_ms: row.get(10)?,
                    modified_at_ms: row.get(11)?,
                    media_count: row.get(12)?,
                    total_size_bytes: row.get::<_, i64>(13)? as u64,
                    rating: rating_for(snapshot, id),
                })
            },
        )?;
        for row in rows {
            let summary = row?;
            by_id.insert(summary.root_id, summary);
        }
    }
    ids.iter()
        .map(|root_id| {
            by_id.remove(root_id).ok_or_else(|| {
                LibraryError::InvalidState(format!("missing root summary {root_id}"))
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

fn text_cursor_sql(direction: &str, cursor: Option<&(String, u32)>) -> (String, Vec<Value>) {
    // Keep the indexed columns bare and apply NOCASE to the bound. Putting
    // COLLATE on the column inside the tuple prevents SQLite's range seek.
    cursor.map_or_else(
        || (String::new(), Vec::new()),
        |(value, root_id)| {
            let comparison = if direction == "ASC" { ">" } else { "<" };
            (
                format!("WHERE (root.name, root.root_id) {comparison} (?1 COLLATE NOCASE, ?2)"),
                vec![Value::Text(value.clone()), Value::Integer(*root_id as i64)],
            )
        },
    )
}

fn integer_cursor_sql(
    expression: &str,
    direction: &str,
    cursor: Option<(i64, u32)>,
) -> (String, Vec<Value>) {
    cursor.map_or_else(
        || (String::new(), Vec::new()),
        |(value, root_id)| {
            let comparison = if direction == "ASC" { ">" } else { "<" };
            // A lexicographic bound lets SQLite seek past the cursor instead
            // of rescanning the index prefix for every candidate chunk.
            (
                format!("WHERE ({expression}, root.root_id) {comparison} (?1, ?2)"),
                vec![Value::Integer(value), Value::Integer(root_id as i64)],
            )
        },
    )
}

fn direction_sql(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => "ASC",
        SortDirection::Descending => "DESC",
    }
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
                media_count: 1,
                total_size_bytes: 1,
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
            media_count: 0,
            total_size_bytes: 0,
        }));
        assert_eq!(cache.0.lock().len(), 1);
    }

    #[test]
    fn dense_cached_orders_match_cursor_scans_with_ties_and_null_dates() {
        let directory = tempfile::tempdir().unwrap();
        let library = crate::Library::create(directory.path().join("library.sqlite")).unwrap();
        let snapshot = library.projections().snapshot();
        let connection = scan_fixture();
        connection
            .execute_batch(
                "WITH RECURSIVE n(id) AS (SELECT 31 UNION ALL SELECT id+1 FROM n WHERE id<4100)
            INSERT INTO library_root SELECT id, CASE WHEN id%2=0 THEN 'ALpha' ELSE 'alpha' END,
            id%11, CASE WHEN id%3=0 THEN NULL ELSE id%13 END, id%19 FROM n;
            CREATE TABLE recent_view(root_id INTEGER PRIMARY KEY, viewed_at_ms INTEGER);
            INSERT INTO recent_view SELECT root_id, root_id%9 FROM library_root;",
            )
            .unwrap();
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
                    let (all, _) = ordered_page_ids(
                        &connection,
                        &snapshot,
                        &query,
                        &matches,
                        matches.len() as usize + 1,
                        None,
                    )
                    .unwrap();
                    let pages = walk(|cursor| {
                        ordered_page_ids(&connection, &snapshot, &query, &matches, 137, cursor)
                            .unwrap()
                    });
                    let expected: Vec<_> = pages.into_iter().flat_map(|(ids, _)| ids).collect();
                    assert_eq!(all, expected, "{scope:?} {field:?} {direction:?}");
                }
            }
        }
    }

    const NAMES: &[&str] = &[
        "Alpha", "alpha", "ALPHA", "beta-1", "Beta-2", "Gamma", "gamma", "Art", "art", "zeta",
    ];

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
                 CREATE INDEX idx_probe_imported ON library_root(imported_at_ms, root_id);
                 CREATE INDEX idx_probe_name ON library_root(name COLLATE NOCASE, root_id);
                 CREATE INDEX idx_probe_size ON library_root(total_size_bytes, root_id)",
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
    fn sparse_integer_paging_matches_the_ordered_scan() {
        let connection = scan_fixture();
        let matches = [1u32, 4, 5, 9, 12, 13, 17, 18, 21, 26, 30]
            .into_iter()
            .collect::<RoaringBitmap>();
        for expression in [
            "root.imported_at_ms",
            "COALESCE(root.captured_at_ms, -1)",
            "root.total_size_bytes",
        ] {
            for direction in [SortDirection::Ascending, SortDirection::Descending] {
                for limit in [1usize, 3, 4, 11, 20] {
                    let run = |sparse: bool| {
                        walk(|cursor| {
                            scan_integer(
                                &connection,
                                expression,
                                &matches,
                                sparse,
                                &sort(direction),
                                limit,
                                cursor,
                            )
                            .unwrap()
                        })
                    };
                    assert_eq!(
                        run(true),
                        run(false),
                        "expression {expression}, direction {direction:?}, limit {limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn sparse_text_paging_matches_the_ordered_scan() {
        let connection = scan_fixture();
        let matches = [2u32, 3, 7, 8, 11, 14, 19, 22, 23, 28]
            .into_iter()
            .collect::<RoaringBitmap>();
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            for limit in [1usize, 3, 4, 10, 20] {
                let run = |sparse: bool| {
                    walk(|cursor| {
                        scan_text(
                            &connection,
                            &matches,
                            sparse,
                            &sort(direction),
                            limit,
                            cursor,
                        )
                        .unwrap()
                    })
                };
                assert_eq!(
                    run(true),
                    run(false),
                    "direction {direction:?}, limit {limit}"
                );
            }
        }
    }

    #[test]
    fn sparse_paging_handles_empty_and_full_match_sets() {
        let connection = scan_fixture();
        let empty = RoaringBitmap::new();
        let all = (1u32..=30).collect::<RoaringBitmap>();
        for matches in [&empty, &all] {
            let run = |sparse: bool| {
                walk(|cursor| {
                    scan_integer(
                        &connection,
                        "root.imported_at_ms",
                        matches,
                        sparse,
                        &sort(SortDirection::Ascending),
                        7,
                        cursor,
                    )
                    .unwrap()
                })
            };
            assert_eq!(run(true), run(false));
        }
    }

    #[test]
    fn numeric_cursor_seeks_the_index_in_both_directions() {
        let connection = scan_fixture();
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            for (expression, cursor_value) in
                [("root.imported_at_ms", 103), ("root.total_size_bytes", 5)]
            {
                let direction = direction_sql(direction);
                let (predicate, mut values) =
                    integer_cursor_sql(expression, direction, Some((cursor_value, 15)));
                values.push(Value::Integer(3));
                let sql = format!("EXPLAIN QUERY PLAN SELECT root.root_id, {expression} FROM library_root root {predicate}
                    ORDER BY {expression} {direction}, root.root_id {direction} LIMIT ?{}", values.len());
                let plan = connection
                    .prepare(&sql)
                    .unwrap()
                    .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
                    .unwrap()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .unwrap()
                    .join(" ");
                assert!(
                    plan.contains("SEARCH root USING COVERING INDEX"),
                    "cursor must seek, not rescan: {plan}"
                );
                assert!(
                    !plan.contains("TEMP B-TREE"),
                    "cursor must preserve indexed order: {plan}"
                );
            }
        }
    }

    #[test]
    fn name_cursor_seeks_the_case_insensitive_index_in_both_directions() {
        let connection = scan_fixture();
        for direction in ["ASC", "DESC"] {
            let (predicate, values) = text_cursor_sql(direction, Some(&("beta-1".into(), 15)));
            let sql = format!("EXPLAIN QUERY PLAN SELECT root.root_id, root.name FROM library_root root {predicate}
                ORDER BY root.name COLLATE NOCASE {direction}, root.root_id {direction} LIMIT 3");
            let plan = connection
                .prepare(&sql)
                .unwrap()
                .query_map(params_from_iter(values), |row| row.get::<_, String>(3))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join(" ");
            assert!(
                plan.contains("SEARCH root USING COVERING INDEX"),
                "name cursor must seek: {plan}"
            );
            assert!(
                !plan.contains("TEMP B-TREE"),
                "name cursor must preserve index order: {plan}"
            );
        }
    }

    #[test]
    fn recent_paging_preserves_ties_and_filtered_membership() {
        let connection = scan_fixture();
        connection.execute_batch("CREATE TABLE recent_view(root_id INTEGER PRIMARY KEY, viewed_at_ms INTEGER NOT NULL);
            CREATE INDEX idx_recent_viewed ON recent_view(viewed_at_ms DESC, root_id);
            INSERT INTO recent_view SELECT root_id, imported_at_ms FROM library_root").unwrap();
        let matches = (1u32..=30)
            .filter(|id| id % 3 != 0)
            .collect::<RoaringBitmap>();
        let expected = connection.prepare("SELECT root_id FROM recent_view WHERE root_id % 3 != 0 ORDER BY viewed_at_ms DESC, root_id DESC")
            .unwrap().query_map([], |row| row.get::<_, u32>(0).map(RootId)).unwrap()
            .collect::<std::result::Result<Vec<_>, _>>().unwrap();
        for limit in [1, 3, 7, 20, 30] {
            let actual =
                walk(|cursor| page_recent_order(&connection, &matches, limit, cursor).unwrap())
                    .into_iter()
                    .flat_map(|(ids, _)| ids)
                    .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn cursor_paging_crosses_rejected_chunks_without_losing_or_repeating_items() {
        let connection = scan_fixture();
        connection.execute_batch("WITH RECURSIVE ids(id) AS (VALUES(31) UNION ALL SELECT id + 1 FROM ids WHERE id < 5000)
            INSERT INTO library_root SELECT id, CASE WHEN id % 2 = 0 THEN 'Alpha' ELSE 'alpha' END,
            id / 3, CASE WHEN id % 4 = 0 THEN NULL ELSE id / 5 END, id % 100 FROM ids").unwrap();
        let matches = [10u32, 1023, 1024, 1025, 2030, 4096, 5000]
            .into_iter()
            .collect::<RoaringBitmap>();
        for direction in [SortDirection::Ascending, SortDirection::Descending] {
            for limit in [1, 3, 7] {
                for expression in [
                    "root.imported_at_ms",
                    "COALESCE(root.captured_at_ms, -1)",
                    "root.total_size_bytes",
                ] {
                    let run = |sparse| {
                        walk(|cursor| {
                            scan_integer(
                                &connection,
                                expression,
                                &matches,
                                sparse,
                                &sort(direction),
                                limit,
                                cursor,
                            )
                            .unwrap()
                        })
                    };
                    assert_eq!(run(false), run(true), "{expression} {direction:?} {limit}");
                }
                let run = |sparse| {
                    walk(|cursor| {
                        scan_text(
                            &connection,
                            &matches,
                            sparse,
                            &sort(direction),
                            limit,
                            cursor,
                        )
                        .unwrap()
                    })
                };
                assert_eq!(run(false), run(true), "name {direction:?} {limit}");
            }
        }
    }
}
