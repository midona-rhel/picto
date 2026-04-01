//! Scope resolver — the canonical implementation of scope semantics.
//!
//! Converts a `ScopeFilter` into a `RoaringBitmap` of matching file IDs.
//! Both `grid::controller` and `selection::helpers` call `resolve_scope`.
//! Sidebar counts use `scope_count` to stay in sync.

use roaring::RoaringBitmap;
use rusqlite::Connection;

use crate::folders::db::{count_uncategorized_entities, list_uncategorized_entity_ids};
use crate::smart_folders::db as smart_folders_db;
use crate::sqlite::bitmaps::{BitmapKey, BitmapStore};
use crate::sqlite::SqliteDatabase;
use crate::tags::db::find_tag as sql_find_tag;
use crate::tags::normalize;
use crate::types::{GridFilterSpec, GridScopeKind, GridScopeSpec, GridSystemScopeKey};

use super::{parse_include_match_mode, IncludeMatchMode};

/// Common scope fields shared between grid queries and selection queries.
///
/// Represents the user's view intent: "which subset of the library am I looking at?"
/// Does NOT include pagination, sorting, grid-specific filters (color, FTS, rating),
/// or selection-specific concerns (excluded_hashes).
#[derive(Debug, Clone, Default)]
pub struct ScopeFilter {
    pub scope: GridScopeSpec,
    pub filters: GridFilterSpec,
}

impl ScopeFilter {
    pub fn has_collection(&self) -> bool {
        self.scope.kind == GridScopeKind::Collection
    }

    pub fn has_smart_folder(&self) -> bool {
        self.scope.kind == GridScopeKind::Smart && self.scope.smart_folder_predicate.is_some()
    }

    pub fn has_search_tags(&self) -> bool {
        self.filters
            .search_tags
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
            || self
                .filters
                .search_excluded_tags
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false)
    }

    pub fn has_folder(&self) -> bool {
        self.scope.kind == GridScopeKind::Folder
            || self
                .filters
                .folder_ids
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || self
                .filters
                .excluded_folder_ids
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
    }

    pub fn system_key(&self) -> Option<GridSystemScopeKey> {
        if self.scope.kind == GridScopeKind::System {
            self.scope.system_key
        } else {
            None
        }
    }

    pub fn folder_ids(&self) -> Option<Vec<i64>> {
        if self.scope.kind == GridScopeKind::Folder {
            self.scope.folder_id.map(|id| vec![id])
        } else {
            self.filters.folder_ids.clone()
        }
    }

    pub fn excluded_folder_ids(&self) -> Option<Vec<i64>> {
        if self.scope.kind == GridScopeKind::Folder {
            None
        } else {
            self.filters.excluded_folder_ids.clone()
        }
    }

    pub fn folder_match_mode(&self) -> Option<String> {
        if self.scope.kind == GridScopeKind::Folder {
            None
        } else {
            self.filters.folder_match_mode.clone()
        }
    }
}

impl From<&crate::types::SelectionQuerySpec> for ScopeFilter {
    fn from(s: &crate::types::SelectionQuerySpec) -> Self {
        ScopeFilter {
            scope: s.scope.clone(),
            filters: s.filters.clone(),
        }
    }
}

/// Resolve a scope filter to a `RoaringBitmap` of matching file IDs.
///
/// Resolution cascade:
/// 1. Collection scope → collection members
/// 2. Smart folder scope → `compile_predicate`
/// 3. Tag search → EffectiveTag bitmap ops (AND/OR), intersect active (status=1)
/// 4. Folder scope/filter → Folder bitmap ops (AND/OR), intersect active (status=1)
/// 5. System fallback: inbox, trash, untagged, uncategorized, default
pub async fn resolve_scope(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    if filter.has_collection() {
        resolve_collection(db, filter).await
    } else if filter.has_smart_folder() {
        resolve_smart_folder(db, filter).await
    } else if filter.has_search_tags() {
        resolve_tag_search(db, filter).await
    } else if filter.has_folder() {
        resolve_folder(db, filter)
    } else {
        resolve_status(db, filter).await
    }
}

async fn resolve_collection(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    let collection_id = filter
        .scope
        .collection_entity_id
        .expect("collection id required");
    let file_ids = db.list_collection_member_file_ids(collection_id).await?;
    Ok(RoaringBitmap::from_iter(
        file_ids.into_iter().map(|id| id as u32),
    ))
}

async fn resolve_smart_folder(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    let pred = filter.scope.smart_folder_predicate.clone().unwrap();
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| smart_folders_db::compile_predicate(conn, &pred, &bitmaps))
        .await
}

async fn resolve_tag_search(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    let include_tags = filter.filters.search_tags.clone().unwrap_or_default();
    let exclude_tags = filter
        .filters
        .search_excluded_tags
        .clone()
        .unwrap_or_default();
    let match_mode = parse_include_match_mode(
        filter.filters.tag_match_mode.as_deref(),
        IncludeMatchMode::All,
    );
    let bitmaps = db.bitmaps.clone();

    db.with_read_conn(move |conn| {
        let resolve_ids =
            |tag_list: &[String], strict_missing: bool| -> rusqlite::Result<Vec<i64>> {
                let mut out = Vec::new();
                for tag in tag_list {
                    if let Some((ns, st)) = normalize::parse_tag(tag) {
                        if let Some(tag_id) = sql_find_tag(conn, &ns, &st)? {
                            out.push(tag_id);
                        } else if strict_missing {
                            return Ok(Vec::new());
                        }
                    }
                }
                Ok(out)
            };

        let include_ids = resolve_ids(&include_tags, match_mode != IncludeMatchMode::Any)?;
        let exclude_ids = resolve_ids(&exclude_tags, false)?;
        let active = bitmaps.get(&BitmapKey::Status(1));

        if !include_tags.is_empty() && include_ids.is_empty() {
            return Ok(RoaringBitmap::new());
        }

        let mut result = if include_ids.is_empty() {
            active.clone()
        } else if match_mode == IncludeMatchMode::Any {
            let mut union = RoaringBitmap::new();
            for tid in &include_ids {
                union |= &bitmaps.get(&BitmapKey::EffectiveTag(*tid));
            }
            union
        } else {
            let mut iter = include_ids.iter();
            let first = *iter.next().expect("include_ids not empty");
            let mut intersect = bitmaps.get(&BitmapKey::EffectiveTag(first));
            for tid in iter {
                intersect &= &bitmaps.get(&BitmapKey::EffectiveTag(*tid));
            }
            intersect
        };

        if !exclude_ids.is_empty() {
            let mut excluded = RoaringBitmap::new();
            for tid in &exclude_ids {
                excluded |= &bitmaps.get(&BitmapKey::EffectiveTag(*tid));
            }
            result -= &excluded;
        }
        result &= &active;
        Ok(result)
    })
    .await
}

fn resolve_folder(db: &SqliteDatabase, filter: &ScopeFilter) -> Result<RoaringBitmap, String> {
    let include_folders = filter.folder_ids().unwrap_or_default();
    let exclude_folders = filter.excluded_folder_ids().unwrap_or_default();
    let match_mode =
        parse_include_match_mode(filter.folder_match_mode().as_deref(), IncludeMatchMode::Any);
    let active = db.bitmaps.get(&BitmapKey::Status(1));

    let mut result = if include_folders.is_empty() {
        active.clone()
    } else if match_mode == IncludeMatchMode::Any {
        let mut union = RoaringBitmap::new();
        for fid in &include_folders {
            union |= &db.bitmaps.get(&BitmapKey::Folder(*fid));
        }
        union
    } else {
        let mut iter = include_folders.iter();
        let first = *iter.next().expect("include_folders not empty");
        let mut intersect = db.bitmaps.get(&BitmapKey::Folder(first));
        for fid in iter {
            intersect &= &db.bitmaps.get(&BitmapKey::Folder(*fid));
        }
        intersect
    };

    if !exclude_folders.is_empty() {
        let mut excluded = RoaringBitmap::new();
        for fid in &exclude_folders {
            excluded |= &db.bitmaps.get(&BitmapKey::Folder(*fid));
        }
        result -= &excluded;
    }
    result &= &active;
    Ok(result)
}

async fn resolve_status(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    match filter.system_key() {
        Some(GridSystemScopeKey::Inbox) => Ok(db.bitmaps.get(&BitmapKey::Status(0))),
        Some(GridSystemScopeKey::Trash) => Ok(db.bitmaps.get(&BitmapKey::Status(2))),
        Some(GridSystemScopeKey::Untagged) => {
            let active = db.bitmaps.get(&BitmapKey::Status(1));
            let tagged = db.bitmaps.get(&BitmapKey::Tagged);
            Ok(&active - &tagged)
        }
        Some(GridSystemScopeKey::Uncategorized) => {
            let uncategorized_ids = db.with_read_conn(list_uncategorized_entity_ids).await?;
            Ok(RoaringBitmap::from_iter(
                uncategorized_ids.into_iter().map(|id| id as u32),
            ))
        }
        _ => Ok(db.bitmaps.get(&BitmapKey::Status(1))),
    }
}

/// Canonical count for a system scope — synchronous, used by sidebar compiler.
///
/// Encodes the same business rules as `resolve_scope` / `resolve_status`:
/// - `system:active` = active (status=1)
/// - `system:active_files` = active (status=1) legacy alias
/// - `system:inbox` = inbox (status=0)
/// - `system:trash` = trash (status=2)
/// - `system:untagged` = active (status=1) minus Tagged
/// - `system:uncategorized` = active singles not in any folder
pub fn scope_count(
    conn: &Connection,
    bitmaps: &BitmapStore,
    scope_key: &str,
) -> rusqlite::Result<i64> {
    match scope_key {
        "system:active" | "system:active_files" => Ok(bitmaps.len(&BitmapKey::Status(1)) as i64),
        "system:inbox" => Ok(bitmaps.len(&BitmapKey::Status(0)) as i64),
        "system:trash" => Ok(bitmaps.len(&BitmapKey::Status(2)) as i64),
        "system:untagged" => {
            let active = bitmaps.len(&BitmapKey::Status(1));
            let tagged = bitmaps.len(&BitmapKey::Tagged);
            Ok(active.saturating_sub(tagged) as i64)
        }
        "system:uncategorized" => count_uncategorized_entities(conn),
        _ => Ok(0),
    }
}
