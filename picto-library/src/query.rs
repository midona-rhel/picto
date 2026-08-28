use std::cmp::Reverse;
use std::collections::HashMap;

use roaring::RoaringBitmap;
use rusqlite::{params_from_iter, types::Value, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fts;
use crate::model::{FolderId, Lifecycle, Rating, RootId, RootKind, SmartFolderId};
use crate::predicate::{self, ItemSort, SortDirection, SortField, ViewQuerySpec};
use crate::projection::ProjectionSnapshot;
use crate::{LibraryError, Result};

const SCAN_CHUNK: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ItemScope {
    All,
    Inbox,
    Trash,
    RecentlyViewed,
    Untagged,
    Uncategorized,
    Folder { folder_id: FolderId },
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
    pub name: String,
    pub cover_media_id: crate::MediaId,
    pub mime: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum Cursor {
    Integer { value: i64, root_id: u32 },
    Text { value: String, root_id: u32 },
    Rating { value: u8, root_id: u32 },
    Random { key: u64, root_id: u32 },
    Vector { index: usize },
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

pub fn page(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    query: &RootQuery,
    request: &PageRequest,
) -> Result<RootPage> {
    let limit = request.limit.clamp(1, 1000);
    let matches = matching_roots(connection, snapshot, query)?;
    let cursor = request
        .cursor
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let (ids, next_cursor) = match query.view.sort.field {
        SortField::FolderOrder => page_folder_order(snapshot, query, &matches, limit, cursor)?,
        SortField::Rating => page_rating(snapshot, &matches, &query.view.sort, limit, cursor)?,
        SortField::Random => page_random(&matches, &query.view.sort, limit, cursor)?,
        SortField::Name => scan_text(connection, &matches, &query.view.sort, limit, cursor)?,
        SortField::ImportedAt => scan_integer(
            connection,
            "root.imported_at_ms",
            &matches,
            &query.view.sort,
            limit,
            cursor,
        )?,
        SortField::CapturedAt => scan_integer(
            connection,
            "COALESCE(root.captured_at_ms, -1)",
            &matches,
            &query.view.sort,
            limit,
            cursor,
        )?,
        SortField::TotalSize => scan_integer(
            connection,
            "root.total_size_bytes",
            &matches,
            &query.view.sort,
            limit,
            cursor,
        )?,
    };
    let items = load_summaries(connection, snapshot, &ids)?;
    Ok(RootPage {
        items,
        next_cursor,
        total: matches.len(),
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
            let mut values = snapshot.folders.get(folder_id).cloned().unwrap_or_default();
            values &= snapshot.active();
            values
        }
        ItemScope::SmartFolder { smart_folder_id } => {
            let mut values = snapshot
                .smart_results
                .get(&smart_folder_id.0)
                .cloned()
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

fn scan_integer(
    connection: &Connection,
    expression: &str,
    matches: &RoaringBitmap,
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

fn scan_text(
    connection: &Connection,
    matches: &RoaringBitmap,
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

fn load_summaries(
    connection: &Connection,
    snapshot: &ProjectionSnapshot,
    ids: &[RootId],
) -> Result<Vec<RootSummary>> {
    let mut statement = connection.prepare_cached(
        "SELECT root.root_id, item.item_kind, root.name, root.cover_media_id,
                file.mime, file.width, file.height, file.duration_ms,
                root.imported_at_ms, root.captured_at_ms, root.modified_at_ms,
                root.media_count, root.total_size_bytes
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
                name: row.get(2)?,
                cover_media_id: crate::MediaId(row.get(3)?),
                mime: row.get(4)?,
                width: row.get(5)?,
                height: row.get(6)?,
                duration_ms: row.get::<_, Option<i64>>(7)?.map(|value| value as u64),
                imported_at_ms: row.get(8)?,
                captured_at_ms: row.get(9)?,
                modified_at_ms: row.get(10)?,
                media_count: row.get(11)?,
                total_size_bytes: row.get::<_, i64>(12)? as u64,
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
