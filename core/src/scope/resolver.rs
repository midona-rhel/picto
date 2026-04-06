//! Scope resolver — canonical scope semantics over `LibraryDatabase`.
//!
//! Converts a `ScopeFilter` into a `RoaringBitmap` of matching top-level entity IDs.

use roaring::RoaringBitmap;
use rusqlite::OptionalExtension;

use crate::db::projection::bitmaps::BitmapKey;
use crate::db::LibraryDatabase;
use crate::tags::normalize;
use crate::types::{GridFilterSpec, GridScopeKind, GridScopeSpec, GridSystemScopeKey};

use super::{parse_include_match_mode, IncludeMatchMode};

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

pub async fn resolve_scope(
    db: &LibraryDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    if filter.has_collection() {
        resolve_collection(db, filter)
    } else if filter.has_smart_folder() {
        resolve_smart_folder(db, filter)
    } else if filter.has_search_tags() {
        resolve_tag_search(db, filter)
    } else if filter.has_folder() {
        resolve_folder(db, filter)
    } else {
        resolve_status(db, filter)
    }
}

pub fn scope_count(db: &LibraryDatabase, scope_key: &str) -> Result<i64, String> {
    let counts = db.get_scope_counts()?;
    Ok(match scope_key {
        "system:active" | "system:active_files" => counts.active,
        "system:inbox" => counts.inbox,
        "system:trash" => counts.trash,
        "system:untagged" => counts.untagged,
        "system:uncategorized" => counts.uncategorized,
        _ => 0,
    })
}

fn resolve_collection(db: &LibraryDatabase, filter: &ScopeFilter) -> Result<RoaringBitmap, String> {
    let collection_id = filter
        .scope
        .collection_entity_id
        .expect("collection id required");

    db.with_read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT entity_id
             FROM media_entity
             WHERE parent_collection_entity_id = ?1
             ORDER BY entity_id ASC",
        )?;
        let ids = stmt
            .query_map([collection_id], |row| row.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(RoaringBitmap::from_iter(ids.into_iter().map(|id| id as u32)))
    })
}

fn resolve_smart_folder(
    db: &LibraryDatabase,
    filter: &ScopeFilter,
) -> Result<RoaringBitmap, String> {
    let pred = filter.scope.smart_folder_predicate.clone().unwrap();
    db.with_read(|conn| crate::db::projection::smart_folders::compile_predicate(conn, &pred, &db.bitmaps))
}

fn resolve_tag_search(db: &LibraryDatabase, filter: &ScopeFilter) -> Result<RoaringBitmap, String> {
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

    db.with_read(|conn| {
        let resolve_ids =
            |tag_list: &[String], strict_missing: bool| -> rusqlite::Result<Vec<i64>> {
                let mut out = Vec::new();
                for tag in tag_list {
                    if let Some((namespace, subtag)) = normalize::parse_tag(tag) {
                        let maybe_tag_id = conn
                            .query_row(
                                "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
                                rusqlite::params![namespace, subtag],
                                |row| row.get::<_, i64>(0),
                            )
                            .optional()?;
                        if let Some(tag_id) = maybe_tag_id {
                            out.push(tag_id);
                        } else if strict_missing {
                            return Ok(Vec::new());
                        }
                    } else if strict_missing {
                        return Ok(Vec::new());
                    }
                }
                Ok(out)
            };

        let include_ids = resolve_ids(&include_tags, match_mode != IncludeMatchMode::Any)?;
        let exclude_ids = resolve_ids(&exclude_tags, false)?;
        let active = active_top_level_bitmap(conn)?;

        if !include_tags.is_empty() && include_ids.is_empty() {
            return Ok(RoaringBitmap::new());
        }

        let mut result = if include_ids.is_empty() {
            active.clone()
        } else if match_mode == IncludeMatchMode::Any {
            let mut union = RoaringBitmap::new();
            for tid in &include_ids {
                union |= &db.bitmaps.get(&BitmapKey::EffectiveTag(*tid));
            }
            union
        } else {
            let mut iter = include_ids.iter();
            let first = *iter.next().expect("include_ids not empty");
            let mut intersect = db.bitmaps.get(&BitmapKey::EffectiveTag(first));
            for tid in iter {
                intersect &= &db.bitmaps.get(&BitmapKey::EffectiveTag(*tid));
            }
            intersect
        };

        if !exclude_ids.is_empty() {
            let mut excluded = RoaringBitmap::new();
            for tid in &exclude_ids {
                excluded |= &db.bitmaps.get(&BitmapKey::EffectiveTag(*tid));
            }
            result -= &excluded;
        }

        result &= &active;
        Ok(result)
    })
}

fn resolve_folder(db: &LibraryDatabase, filter: &ScopeFilter) -> Result<RoaringBitmap, String> {
    let include_folders = filter.folder_ids().unwrap_or_default();
    let exclude_folders = filter.excluded_folder_ids().unwrap_or_default();
    let match_mode =
        parse_include_match_mode(filter.folder_match_mode().as_deref(), IncludeMatchMode::Any);

    db.with_read(|conn| {
        let mut result = if include_folders.is_empty() {
            active_top_level_bitmap(conn)?
        } else {
            folder_membership_bitmap(conn, &include_folders, match_mode)?
        };

        if !exclude_folders.is_empty() {
            let excluded = folder_membership_bitmap(conn, &exclude_folders, IncludeMatchMode::Any)?;
            result -= &excluded;
        }

        result &= &active_top_level_bitmap(conn)?;
        Ok(result)
    })
}

fn resolve_status(db: &LibraryDatabase, filter: &ScopeFilter) -> Result<RoaringBitmap, String> {
    db.with_read(|conn| match filter.system_key() {
        Some(GridSystemScopeKey::Inbox) => top_level_status_bitmap(conn, 0),
        Some(GridSystemScopeKey::Trash) => top_level_status_bitmap(conn, 2),
        Some(GridSystemScopeKey::Untagged) => untagged_top_level_bitmap(conn),
        Some(GridSystemScopeKey::Uncategorized) => uncategorized_top_level_bitmap(conn),
        _ => top_level_status_bitmap(conn, 1),
    })
}

fn top_level_status_bitmap(conn: &rusqlite::Connection, status: i64) -> rusqlite::Result<RoaringBitmap> {
    let mut stmt = conn.prepare(
        "SELECT entity_id
         FROM media_entity
         WHERE status = ?1 AND parent_collection_entity_id IS NULL
         ORDER BY entity_id ASC",
    )?;
    let ids = stmt
        .query_map([status], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(RoaringBitmap::from_iter(ids.into_iter().map(|id| id as u32)))
}

fn active_top_level_bitmap(conn: &rusqlite::Connection) -> rusqlite::Result<RoaringBitmap> {
    top_level_status_bitmap(conn, 1)
}

fn untagged_top_level_bitmap(conn: &rusqlite::Connection) -> rusqlite::Result<RoaringBitmap> {
    let mut stmt = conn.prepare(
        "SELECT me.entity_id
         FROM media_entity me
         WHERE me.status = 1
           AND me.parent_collection_entity_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = me.entity_id)
           AND NOT EXISTS (
               SELECT 1 FROM media_entity child
               WHERE child.parent_collection_entity_id = me.entity_id
                 AND EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = child.entity_id)
           )
         ORDER BY me.entity_id ASC",
    )?;
    let ids = stmt
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(RoaringBitmap::from_iter(ids.into_iter().map(|id| id as u32)))
}

fn uncategorized_top_level_bitmap(
    conn: &rusqlite::Connection,
) -> rusqlite::Result<RoaringBitmap> {
    let mut stmt = conn.prepare(
        "SELECT me.entity_id
         FROM media_entity me
         WHERE me.status = 1
           AND me.parent_collection_entity_id IS NULL
           AND NOT EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = me.entity_id)
           AND NOT EXISTS (
               SELECT 1 FROM media_entity child
               WHERE child.parent_collection_entity_id = me.entity_id
                 AND EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = child.entity_id)
           )
         ORDER BY me.entity_id ASC",
    )?;
    let ids = stmt
        .query_map([], |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(RoaringBitmap::from_iter(ids.into_iter().map(|id| id as u32)))
}

fn folder_membership_bitmap(
    conn: &rusqlite::Connection,
    folder_ids: &[i64],
    match_mode: IncludeMatchMode,
) -> rusqlite::Result<RoaringBitmap> {
    if folder_ids.is_empty() {
        return Ok(RoaringBitmap::new());
    }

    let placeholders = std::iter::repeat_n("?", folder_ids.len())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = if match_mode == IncludeMatchMode::Any {
        format!(
            "SELECT DISTINCT COALESCE(child.parent_collection_entity_id, child.entity_id) AS top_entity_id
             FROM folder_member fm
             JOIN media_entity child ON child.entity_id = fm.entity_id
             WHERE fm.folder_id IN ({placeholders})
             ORDER BY top_entity_id ASC"
        )
    } else {
        format!(
            "SELECT top_entity_id
             FROM (
                 SELECT COALESCE(child.parent_collection_entity_id, child.entity_id) AS top_entity_id,
                        COUNT(DISTINCT fm.folder_id) AS match_count
                 FROM folder_member fm
                 JOIN media_entity child ON child.entity_id = fm.entity_id
                 WHERE fm.folder_id IN ({placeholders})
                 GROUP BY top_entity_id
             )
             WHERE match_count = ?{}
             ORDER BY top_entity_id ASC",
            folder_ids.len() + 1
        )
    };

    let mut params: Vec<&dyn rusqlite::ToSql> = folder_ids
        .iter()
        .map(|folder_id| folder_id as &dyn rusqlite::ToSql)
        .collect();
    let all_count = folder_ids.len() as i64;
    if match_mode != IncludeMatchMode::Any {
        params.push(&all_count);
    }

    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt
        .query_map(rusqlite::params_from_iter(params), |row| row.get::<_, i64>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(RoaringBitmap::from_iter(ids.into_iter().map(|id| id as u32)))
}
