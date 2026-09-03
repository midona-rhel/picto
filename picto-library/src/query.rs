use std::cmp::Reverse;
use std::collections::HashMap;
use std::sync::Arc;

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
    Random { key: u64, root_id: u32 },
    Vector { index: usize },
}

/// Bounded text-match cache owned by one open library. Reads are lock-free;
/// revision is part of the key, so old RCU readers and new publications cannot
/// observe each other's results.
pub struct MatchCache {
    entries: ArcSwap<Vec<MatchCacheEntry>>,
    writes: Mutex<()>,
}

#[derive(Clone)]
struct MatchCacheEntry {
    revision: u64,
    scope: ItemScope,
    filter: FilterExpr,
    matches: Arc<RoaringBitmap>,
}

const MATCH_CACHE_LIMIT: usize = 8;

impl Default for MatchCache {
    fn default() -> Self {
        Self {
            entries: ArcSwap::from_pointee(Vec::new()),
            writes: Mutex::new(()),
        }
    }
}

impl MatchCache {
    fn get(
        &self,
        revision: u64,
        scope: &ItemScope,
        filter: &FilterExpr,
    ) -> Option<Arc<RoaringBitmap>> {
        let entries = self.entries.load();
        entries
            .iter()
            .find(|entry| {
                entry.revision == revision && entry.scope == *scope && entry.filter == *filter
            })
            .map(|entry| entry.matches.clone())
    }

    fn insert(
        &self,
        revision: u64,
        scope: &ItemScope,
        filter: &FilterExpr,
        matches: Arc<RoaringBitmap>,
    ) {
        let _write = self.writes.lock();
        let mut entries = self.entries.load_full().as_ref().clone();
        if entries.iter().any(|entry| {
            entry.revision == revision && entry.scope == *scope && entry.filter == *filter
        }) {
            return;
        }
        if entries.len() == MATCH_CACHE_LIMIT {
            entries.remove(0);
        }
        entries.push(MatchCacheEntry {
            revision,
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
    let cacheable = predicate::contains_text(&query.view.filter);
    let matches = if cacheable {
        cache
            .and_then(|cache| cache.get(snapshot.revision, &query.scope, &query.view.filter))
            .map(Ok)
            .unwrap_or_else(|| {
                let matches = Arc::new(matching_roots(connection, snapshot, query)?);
                if let Some(cache) = cache {
                    cache.insert(
                        snapshot.revision,
                        &query.scope,
                        &query.view.filter,
                        matches.clone(),
                    );
                }
                Ok::<_, crate::LibraryError>(matches)
            })?
    } else {
        Arc::new(matching_roots(connection, snapshot, query)?)
    };
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
    let sparse = matches.len() <= SPARSE_SCAN_MAXIMUM;
    let (ids, next_cursor) = match query.scope {
        ItemScope::RecentlyViewed => page_recent_order(connection, &matches, limit, cursor)?,
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
                    &matches,
                    sparse,
                    &sort,
                    limit,
                    cursor,
                )?
            }
            SortField::FolderOrder => page_folder_order(snapshot, query, &matches, limit, cursor)?,
            SortField::Rating => page_rating(snapshot, &matches, &query.view.sort, limit, cursor)?,
            SortField::Random => page_random(&matches, &query.view.sort, limit, cursor)?,
            SortField::Name => scan_text(
                connection,
                &matches,
                sparse,
                &query.view.sort,
                limit,
                cursor,
            )?,
            SortField::ImportedAt => scan_integer(
                connection,
                "root.imported_at_ms",
                &matches,
                sparse,
                &query.view.sort,
                limit,
                cursor,
            )?,
            SortField::CapturedAt => scan_integer(
                connection,
                "COALESCE(root.captured_at_ms, -1)",
                &matches,
                sparse,
                &query.view.sort,
                limit,
                cursor,
            )?,
            SortField::TotalSize => scan_integer(
                connection,
                "root.total_size_bytes",
                &matches,
                sparse,
                &query.view.sort,
                limit,
                cursor,
            )?,
        },
    };
    let items = load_summaries(connection, snapshot, &ids)?;
    Ok(RootPage {
        items,
        next_cursor,
        total: matches.len(),
        media_count: snapshot.media_count.sum(&matches),
        total_size_bytes: snapshot.total_bytes.sum(&matches),
        revision: snapshot.revision,
    })
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
        let mut statement = connection.prepare(&sql)?;
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
        let (where_sql, mut values) = if let Some((value, root_id)) = &scan_cursor {
            let comparison = if direction == "ASC" { ">" } else { "<" };
            (
                format!(
                    "WHERE (root.name COLLATE NOCASE {comparison} ?1 OR
                     (root.name COLLATE NOCASE = ?2 AND root.root_id {comparison} ?3))"
                ),
                vec![
                    Value::Text(value.clone()),
                    Value::Text(value.clone()),
                    Value::Integer(*root_id as i64),
                ],
            )
        } else {
            (String::new(), Vec::new())
        };
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
    let seed = sort.random_seed.as_deref().unwrap_or("picto");
    let cursor = match cursor {
        Some(Cursor::Random { key, root_id }) => Some((key, root_id)),
        None => None,
        _ => {
            return Err(LibraryError::InvalidInput(
                "cursor does not match sort".into(),
            ))
        }
    };
    let mut values = matches
        .iter()
        .map(|root_id| (random_key(seed, root_id), root_id))
        .collect::<Vec<_>>();
    values.sort_unstable();
    if sort.direction == SortDirection::Descending {
        values.sort_unstable_by_key(|value| Reverse(*value));
    }
    let values = values
        .into_iter()
        .filter(|value| {
            cursor.is_none_or(|cursor| match sort.direction {
                SortDirection::Ascending => *value > cursor,
                SortDirection::Descending => *value < cursor,
            })
        })
        .take(limit)
        .collect::<Vec<_>>();
    let next = (values.len() == limit)
        .then(|| values.last().copied())
        .flatten()
        .map(|(key, root_id)| serde_json::to_string(&Cursor::Random { key, root_id }))
        .transpose()?;
    Ok((
        values
            .into_iter()
            .map(|(_, root_id)| RootId(root_id))
            .collect(),
        next,
    ))
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
                 WHERE viewed_at_ms < ?1 OR (viewed_at_ms = ?1 AND root_id < ?2)
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

fn load_summaries(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    ids: &[RootId],
) -> Result<Vec<RootSummary>> {
    let mut statement = connection.prepare_cached(
        "SELECT root.root_id, item.item_kind, root.name, root.cover_media_id,
                file.content_hash, file.mime, file.width, file.height, file.duration_ms,
                file.frame_count, file.palette_json, root.imported_at_ms,
                root.captured_at_ms, root.modified_at_ms, root.media_count,
                root.total_size_bytes
         FROM library_root root
         JOIN library_item item ON item.local_id = root.root_id
         JOIN media_item media ON media.media_id = root.cover_media_id
         JOIN media_file file ON file.file_id = media.file_id
         WHERE root.root_id = ?1",
    )?;
    let mut output = Vec::with_capacity(ids.len());
    for root_id in ids {
        output.push(statement.query_row([root_id.0], |row| {
            let id = row.get::<_, u32>(0)?;
            Ok(RootSummary {
                root_id: RootId(id),
                kind: if row.get::<_, u8>(1)? == 1 {
                    RootKind::Media
                } else {
                    RootKind::Collection
                },
                lifecycle: lifecycle_for(snapshot, id),
                name: row.get(2)?,
                cover_media_id: crate::MediaId(row.get(3)?),
                content_hash: row.get(4)?,
                mime: row.get(5)?,
                width: row.get(6)?,
                height: row.get(7)?,
                duration_ms: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                frame_count: row.get(9)?,
                palette: serde_json::from_str(&row.get::<_, String>(10)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        10,
                        rusqlite::types::Type::Text,
                        Box::new(error),
                    )
                })?,
                imported_at_ms: row.get(11)?,
                captured_at_ms: row.get(12)?,
                modified_at_ms: row.get(13)?,
                media_count: row.get(14)?,
                total_size_bytes: row.get::<_, i64>(15)? as u64,
                rating: rating_for(snapshot, id),
            })
        })?);
    }
    Ok(output)
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

fn integer_cursor_sql(
    expression: &str,
    direction: &str,
    cursor: Option<(i64, u32)>,
) -> (String, Vec<Value>) {
    cursor.map_or_else(
        || (String::new(), Vec::new()),
        |(value, root_id)| {
            let comparison = if direction == "ASC" { ">" } else { "<" };
            (
                format!(
                    "WHERE ({expression} {comparison} ?1 OR
                     ({expression} = ?2 AND root.root_id {comparison} ?3))"
                ),
                vec![
                    Value::Integer(value),
                    Value::Integer(value),
                    Value::Integer(root_id as i64),
                ],
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

fn random_key(seed: &str, root_id: u32) -> u64 {
    let mut hash = Sha256::new();
    hash.update(seed.as_bytes());
    hash.update(root_id.to_le_bytes());
    u64::from_le_bytes(hash.finalize()[..8].try_into().expect("eight bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
                 )",
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
}
