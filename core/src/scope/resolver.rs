//! Scope resolver — the canonical implementation of scope semantics.
//!
//! Converts a `ScopeFilter` into a `RoaringBitmap` of matching file IDs.
//! Both `grid::controller` and `selection::helpers` call `resolve_scope`.
//! Sidebar counts use `scope_count` to stay in sync.

use roaring::RoaringBitmap;
use rusqlite::Connection;

use crate::folders::db::{count_uncategorized_entities, list_uncategorized_entity_ids};
use crate::smart_folders::db as smart_folders_db;
use crate::smart_folders::db::SmartFolderPredicate;
use crate::sqlite::bitmaps::{BitmapKey, BitmapStore};
use crate::sqlite::SqliteDatabase;
use crate::tags::db::find_tag as sql_find_tag;
use crate::tags::normalize;

use super::{parse_include_match_mode, IncludeMatchMode};

/// Common scope fields shared between grid queries and selection queries.
///
/// Represents the user's view intent: "which subset of the library am I looking at?"
/// Does NOT include pagination, sorting, grid-specific filters (color, FTS, rating),
/// or selection-specific concerns (excluded_hashes).
#[derive(Debug, Clone, Default)]
pub struct ScopeFilter {
    pub status: Option<String>,
    pub smart_folder_predicate: Option<SmartFolderPredicate>,
    pub search_tags: Option<Vec<String>>,
    pub search_excluded_tags: Option<Vec<String>>,
    pub tag_match_mode: Option<String>,
    pub folder_ids: Option<Vec<i64>>,
    pub excluded_folder_ids: Option<Vec<i64>>,
    pub folder_match_mode: Option<String>,
}

impl ScopeFilter {
    pub fn has_smart_folder(&self) -> bool {
        self.smart_folder_predicate.is_some()
    }

    pub fn has_search_tags(&self) -> bool {
        self.search_tags
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
            || self
                .search_excluded_tags
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false)
    }

    pub fn has_folder(&self) -> bool {
        self.folder_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
            || self
                .excluded_folder_ids
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
    }
}

impl From<&crate::types::GridPageSlimQuery> for ScopeFilter {
    fn from(q: &crate::types::GridPageSlimQuery) -> Self {
        ScopeFilter {
            status: q.status.clone(),
            smart_folder_predicate: q.smart_folder_predicate.clone(),
            search_tags: q.search_tags.clone(),
            search_excluded_tags: q.search_excluded_tags.clone(),
            tag_match_mode: q.tag_match_mode.clone(),
            folder_ids: q.folder_ids.clone(),
            excluded_folder_ids: q.excluded_folder_ids.clone(),
            folder_match_mode: q.folder_match_mode.clone(),
        }
    }
}

impl From<&crate::types::SelectionQuerySpec> for ScopeFilter {
    fn from(s: &crate::types::SelectionQuerySpec) -> Self {
        ScopeFilter {
            status: s.status.clone(),
            smart_folder_predicate: s.smart_folder_predicate.clone(),
            search_tags: s.search_tags.clone(),
            search_excluded_tags: s.search_excluded_tags.clone(),
            tag_match_mode: s.tag_match_mode.clone(),
            folder_ids: s.folder_ids.clone(),
            excluded_folder_ids: s.excluded_folder_ids.clone(),
            folder_match_mode: s.folder_match_mode.clone(),
        }
    }
}

/// Resolve a scope filter to a `RoaringBitmap` of matching file IDs.
///
/// Resolution cascade:
/// 1. Smart folder predicate → `compile_predicate`
/// 2. Tag search → EffectiveTag bitmap ops (AND/OR), intersect AllActive
/// 3. Folder → Folder bitmap ops (AND/OR), intersect AllActive
/// 4. Status fallback: inbox, trash, untagged, uncategorized, recently_viewed, default
pub async fn resolve_scope(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    if filter.has_smart_folder() {
        resolve_smart_folder(db, filter).await
    } else if filter.has_search_tags() {
        resolve_tag_search(db, filter).await
    } else if filter.has_folder() {
        resolve_folder(db, filter)
    } else {
        resolve_status(db, filter).await
    }
}

async fn resolve_smart_folder(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    let pred = filter.smart_folder_predicate.clone().unwrap();
    let bitmaps = db.bitmaps.clone();
    db.with_read_conn(move |conn| smart_folders_db::compile_predicate(conn, &pred, &bitmaps))
        .await
}

async fn resolve_tag_search(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    let include_tags = filter.search_tags.clone().unwrap_or_default();
    let exclude_tags = filter.search_excluded_tags.clone().unwrap_or_default();
    let match_mode = parse_include_match_mode(
        filter.tag_match_mode.as_deref(),
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
        let all_active = bitmaps.get(&BitmapKey::AllActive);

        // If the user searched for tags but none resolved to valid tag_ids,
        // return empty — the tags don't exist so no files can match.
        if !include_tags.is_empty() && include_ids.is_empty() {
            return Ok(RoaringBitmap::new());
        }

        let mut result = if include_ids.is_empty() {
            all_active.clone()
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
        result &= &all_active;
        Ok(result)
    })
    .await
}

fn resolve_folder(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    let include_folders = filter.folder_ids.clone().unwrap_or_default();
    let exclude_folders = filter.excluded_folder_ids.clone().unwrap_or_default();
    let match_mode = parse_include_match_mode(
        filter.folder_match_mode.as_deref(),
        IncludeMatchMode::Any,
    );
    let all_active = db.bitmaps.get(&BitmapKey::AllActive);

    let mut result = if include_folders.is_empty() {
        all_active.clone()
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
    result &= &all_active;
    Ok(result)
}

async fn resolve_status(
    db: &SqliteDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    match filter.status.as_deref() {
        Some("inbox") => Ok(db.bitmaps.get(&BitmapKey::Status(0))),
        Some("trash") => Ok(db.bitmaps.get(&BitmapKey::Status(2))),
        Some("untagged") => {
            let all_active = db.bitmaps.get(&BitmapKey::AllActive);
            let tagged = db.bitmaps.get(&BitmapKey::Tagged);
            Ok(&all_active - &tagged)
        }
        Some("uncategorized") => {
            let uncategorized_ids = db.with_read_conn(list_uncategorized_entity_ids).await?;
            Ok(RoaringBitmap::from_iter(
                uncategorized_ids.into_iter().map(|id| id as u32),
            ))
        }
        Some("recently_viewed") => {
            // Bitmap approximation — AllActive. Actual view_count check
            // happens in the grid controller's SQL query.
            Ok(db.bitmaps.get(&BitmapKey::AllActive))
        }
        // Default "All Images" = status=1 (active only).
        _ => Ok(db.bitmaps.get(&BitmapKey::Status(1))),
    }
}

/// Canonical count for a system scope — synchronous, used by sidebar compiler.
///
/// Encodes the same business rules as `resolve_scope` / `resolve_status`:
/// - `system:all_files` = active (status=1)
/// - `system:inbox` = inbox (status=0)
/// - `system:trash` = trash (status=2)
/// - `system:untagged` = AllActive minus Tagged
/// - `system:uncategorized` = active singles not in any folder
/// - `system:recent_viewed` = active singles with view_count > 0
pub fn scope_count(
    conn: &Connection,
    bitmaps: &BitmapStore,
    scope_key: &str,
) -> rusqlite::Result<i64> {
    match scope_key {
        "system:all_files" => Ok(bitmaps.len(&BitmapKey::Status(1)) as i64),
        "system:inbox" => Ok(bitmaps.len(&BitmapKey::Status(0)) as i64),
        "system:trash" => Ok(bitmaps.len(&BitmapKey::Status(2)) as i64),
        "system:untagged" => {
            let all_active = bitmaps.len(&BitmapKey::AllActive);
            let tagged = bitmaps.len(&BitmapKey::Tagged);
            Ok(all_active.saturating_sub(tagged) as i64)
        }
        "system:uncategorized" => count_uncategorized_entities(conn),
        "system:recent_viewed" => conn.query_row(
            "SELECT COUNT(*)
             FROM media_entity me
             JOIN entity_file ef ON ef.entity_id = me.entity_id
             JOIN file f ON f.file_id = ef.file_id
             WHERE me.status = 1
               AND me.kind = 'single'
               AND f.view_count > 0
               AND me.parent_collection_id IS NULL",
            [],
            |row| row.get(0),
        ),
        _ => Ok(0),
    }
}
