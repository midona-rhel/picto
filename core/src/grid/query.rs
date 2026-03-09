//! Grid page query — resolves paginated file queries for the main image grid.
//!
//! Handles scope resolution (via `crate::scope::resolver`), sorting,
//! pagination, and color filtering.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::time::Instant;

use serde::Serialize;

use crate::scope::resolver::{resolve_scope, ScopeFilter};
use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::files::GridFilters;
use crate::sqlite::files::FileMetadataSlim;
use crate::sqlite::{ScopeSnapshot, ScopeSnapshotKey, SqliteDatabase};
use crate::types::{
    parse_file_status, EntitySlim, GridPageSlimQuery, GridPageSlimResponse,
};

#[derive(Debug, Serialize)]
pub struct GridOutlineResponse {
    pub items: Vec<EntitySlim>,
    pub total_count: Option<i64>,
}

fn slim_cursor_value_for_sort(
    item: &FileMetadataSlim,
    sort_field: &str,
    random_seed: Option<i64>,
) -> Option<String> {
    let sort_val = match sort_field {
        "random" => {
            let seed = random_seed.unwrap_or(0);
            Some(
                ((item.entity_id.wrapping_mul(2654435761).wrapping_add(seed)) % 2147483647)
                    .to_string(),
            )
        }
        "position_rank" => item.position_rank.map(|r| r.to_string()),
        "imported_at" => Some(item.imported_at.to_string()),
        "size" => Some(item.size.to_string()),
        "rating" => Some(item.rating.unwrap_or(0).to_string()),
        "view_count" => Some(item.view_count.to_string()),
        "name" => Some(item.name.clone().unwrap_or_default()),
        "mime" => Some(item.mime.clone()),
        _ => Some(item.imported_at.to_string()),
    };
    // Composite cursor: "sort_value\0entity_id" for stable keyset pagination
    sort_val.map(|v| format!("{}\0{}", v, item.entity_id))
}

/// Build a stable scope key for the scope snapshot cache.
/// The `scope` string encodes the query type (smart_folder, search_tags, folder, untagged, uncategorized, status).
/// The `predicate_hash` captures filter-specific parameters so distinct predicates get distinct entries.
fn build_scope_cache_key(
    query: &GridPageSlimQuery,
    sort_field: &str,
    sort_dir: &str,
) -> ScopeSnapshotKey {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    let scope = if query.collection_entity_id.is_some() {
        "collection".to_string()
    } else if query.smart_folder_predicate.is_some() {
        "smart_folder".to_string()
    } else if query
        .search_tags
        .as_ref()
        .map(|t| !t.is_empty())
        .unwrap_or(false)
        || query
            .search_excluded_tags
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    {
        "search_tags".to_string()
    } else if query
        .folder_ids
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        || query
            .excluded_folder_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    {
        "folder".to_string()
    } else if query.status.as_deref() == Some("uncategorized") {
        "uncategorized".to_string()
    } else if query.status.as_deref() == Some("untagged") {
        "untagged".to_string()
    } else {
        format!("status:{}", query.status.as_deref().unwrap_or("active"))
    };

    if let Some(cid) = query.collection_entity_id {
        cid.hash(&mut hasher);
    }
    if let Some(ref pred) = query.smart_folder_predicate {
        if let Ok(json) = serde_json::to_string(pred) {
            json.hash(&mut hasher);
        }
    }
    if let Some(ref tags) = query.search_tags {
        tags.hash(&mut hasher);
    }
    if let Some(ref tags) = query.search_excluded_tags {
        tags.hash(&mut hasher);
    }
    if let Some(ref mode) = query.tag_match_mode {
        mode.hash(&mut hasher);
    }
    if let Some(ref fids) = query.folder_ids {
        fids.hash(&mut hasher);
    }
    if let Some(ref fids) = query.excluded_folder_ids {
        fids.hash(&mut hasher);
    }
    if let Some(ref mode) = query.folder_match_mode {
        mode.hash(&mut hasher);
    }
    if let Some(ref hex) = query.color_hex {
        hex.hash(&mut hasher);
    }
    if let Some(acc) = query.color_accuracy {
        acc.to_bits().hash(&mut hasher);
    }

    let predicate_hash = hasher.finish();

    ScopeSnapshotKey {
        scope,
        predicate_hash,
        sort_field: sort_field.to_string(),
        sort_dir: sort_dir.to_string(),
    }
}

/// Convert a hex color to CIELAB and query the R-tree for matching file IDs.
fn color_filter_ids(
    conn: &rusqlite::Connection,
    hex: &str,
    max_distance: f64,
) -> rusqlite::Result<Vec<i64>> {
    let hex = hex.trim_start_matches('#');
    let (r, g, b) = if hex.len() == 6 {
        (
            u8::from_str_radix(&hex[0..2], 16).unwrap_or(0),
            u8::from_str_radix(&hex[2..4], 16).unwrap_or(0),
            u8::from_str_radix(&hex[4..6], 16).unwrap_or(0),
        )
    } else {
        return Ok(Vec::new());
    };

    use palette::{IntoColor, Lab, Srgb};
    let srgb = Srgb::new(r, g, b);
    let lab: Lab = srgb.into_linear::<f32>().into_color();

    let target_l = lab.l as f64;
    let target_a = lab.a as f64;
    let target_b = lab.b as f64;

    let l_range = max_distance;
    let a_range = max_distance * 2.0;
    let b_range = max_distance * 2.0;

    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT fc.file_id
         FROM file_color_rtree rt
         JOIN file_color fc ON fc.rowid = rt.id
         WHERE rt.l_max >= ?1 AND rt.l_min <= ?2
           AND rt.a_max >= ?3 AND rt.a_min <= ?4
           AND rt.b_max >= ?5 AND rt.b_min <= ?6",
    )?;

    let rows = stmt.query_map(
        rusqlite::params![
            target_l - l_range,
            target_l + l_range,
            target_a - a_range,
            target_a + a_range,
            target_b - b_range,
            target_b + b_range,
        ],
        |row| row.get::<_, i64>(0),
    )?;

    rows.collect()
}

fn build_grid_filters(query: &GridPageSlimQuery) -> Option<GridFilters> {
    let has_any = query.rating_min.is_some()
        || query
            .mime_prefixes
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        || query
            .search_text
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false);

    if !has_any {
        return None;
    }

    Some(GridFilters {
        rating_min: query.rating_min,
        mime_prefixes: query.mime_prefixes.clone(),
        search_text: query.search_text.clone(),
    })
}

pub async fn get_grid_outline(
    db: &SqliteDatabase,
    query: GridPageSlimQuery,
) -> Result<GridOutlineResponse, String> {
    let sort_field = query
        .sort_field
        .clone()
        .unwrap_or_else(|| "imported_at".to_string());
    let sort_dir = query
        .sort_order
        .clone()
        .unwrap_or_else(|| "desc".to_string());
    let grid_filters = build_grid_filters(&query);

    let color_file_ids: Option<HashSet<i64>> = if let Some(ref hex) = query.color_hex {
        let hex = hex.clone();
        let tolerance = query.color_accuracy.unwrap_or(20.0).clamp(1.0, 30.0);
        let ids: Vec<i64> = db
            .with_read_conn(move |conn| color_filter_ids(conn, &hex, tolerance))
            .await?;
        Some(ids.into_iter().collect())
    } else {
        None
    };

    if let Some(collection_id) = query.collection_entity_id {
        let cache_key = build_scope_cache_key(&query, &sort_field, &sort_dir);
        let member_file_ids = if let Some(snap) = db.scope_cache_get(&cache_key) {
            snap.ids
        } else {
            let mut ids = db.list_collection_member_file_ids(collection_id).await?;
            if let Some(ref color_ids) = color_file_ids {
                ids.retain(|id| color_ids.contains(id));
            }
            db.scope_cache_put(
                cache_key,
                ScopeSnapshot {
                    total_count: ids.len() as i64,
                    ids: ids.clone(),
                    created_at: Instant::now(),
                },
            );
            ids
        };

        if member_file_ids.is_empty() {
            return Ok(GridOutlineResponse {
                items: Vec::new(),
                total_count: Some(0),
            });
        }

        let rows = db
            .with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_collection_rank(
                    conn,
                    &member_file_ids,
                    collection_id,
                    member_file_ids.len() as i64 + 1,
                    None,
                    grid_filters.as_ref(),
                )
            })
            .await?;

        let items: Vec<EntitySlim> = rows.into_iter().map(EntitySlim::from).collect();
        return Ok(GridOutlineResponse {
            total_count: Some(items.len() as i64),
            items,
        });
    }

    let scope_filter = ScopeFilter::from(&query);
    let needs_scope = scope_filter.has_smart_folder()
        || scope_filter.has_search_tags()
        || scope_filter.has_folder()
        || matches!(
            scope_filter.status.as_deref(),
            Some("untagged") | Some("uncategorized")
        );

    if needs_scope {
        let cache_key = build_scope_cache_key(&query, &sort_field, &sort_dir);
        let filtered_ids = if let Some(snap) = db.scope_cache_get(&cache_key) {
            snap.ids
        } else {
            let scope_bm = resolve_scope(db, &scope_filter).await?;
            let mut ids: Vec<i64> = scope_bm.iter().map(|id| id as i64).collect();
            if let Some(ref color_ids) = color_file_ids {
                ids.retain(|id| color_ids.contains(id));
            }
            db.scope_cache_put(
                cache_key,
                ScopeSnapshot {
                    total_count: ids.len() as i64,
                    ids: ids.clone(),
                    created_at: Instant::now(),
                },
            );
            ids
        };

        if filtered_ids.is_empty() {
            return Ok(GridOutlineResponse {
                items: Vec::new(),
                total_count: Some(0),
            });
        }

        let has_excluded_folders = query
            .excluded_folder_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let is_single_folder = query
            .folder_ids
            .as_ref()
            .map(|v| v.len() == 1)
            .unwrap_or(false)
            && !has_excluded_folders
            && query
                .folder_match_mode
                .as_deref()
                .map(|m| m == "all" || m == "exact")
                .unwrap_or(true);

        let rows = if is_single_folder {
            let fid = query.folder_ids.as_ref().unwrap()[0];
            db.with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_folder_rank(
                    conn,
                    &filtered_ids,
                    fid,
                    filtered_ids.len() as i64 + 1,
                    "asc",
                    None,
                    grid_filters.as_ref(),
                )
            })
            .await?
        } else {
            let sf = sort_field.clone();
            let sd = sort_dir.clone();
            db.with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_ids(
                    conn,
                    &filtered_ids,
                    filtered_ids.len() as i64 + 1,
                    &sf,
                    &sd,
                    None,
                    grid_filters.as_ref(),
                    None,
                )
            })
            .await?
        };

        let items: Vec<EntitySlim> = rows.into_iter().map(EntitySlim::from).collect();
        return Ok(GridOutlineResponse {
            total_count: Some(items.len() as i64),
            items,
        });
    }

    if query.status.as_deref() == Some("recently_viewed") {
        const RECENTLY_VIEWED_CAP: i64 = 500;
        let rows: Vec<(crate::sqlite::files::FileMetadataSlim, String)> = db
            .with_read_conn(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT hash, name, mime, width, height, size, status, rating, blurhash,
                            imported_at, dominant_color_hex, duration_ms, num_frames, has_audio, view_count,
                            file_id, last_viewed_at
                     FROM file WHERE view_count > 0 AND status = 1
                     AND file_id IN (
                       SELECT file_id FROM file WHERE view_count > 0 AND status = 1
                       ORDER BY last_viewed_at DESC, file_id DESC LIMIT 500
                     )
                     ORDER BY last_viewed_at DESC, file_id DESC LIMIT 500",
                )?;
                let rows = stmt.query_map([], |row| {
                    let slim = crate::sqlite::files::FileMetadataSlim {
                        file_id: row.get(15)?,
                        entity_id: row.get(15)?,
                        is_collection: false,
                        collection_item_count: None,
                        hash: row.get(0)?,
                        name: row.get(1)?,
                        mime: row.get(2)?,
                        width: row.get(3)?,
                        height: row.get(4)?,
                        size: row.get(5)?,
                        status: row.get::<_, i64>(6)? as u8,
                        rating: row.get(7)?,
                        blurhash: row.get(8)?,
                        imported_at: row.get(9)?,
                        dominant_color_hex: row.get(10)?,
                        duration_ms: row.get(11)?,
                        num_frames: row.get(12)?,
                        has_audio: row.get::<_, i64>(13)? != 0,
                        view_count: row.get(14)?,
                        position_rank: None,
                    };
                    let last_viewed_at: String =
                        row.get::<_, Option<String>>(16)?.unwrap_or_default();
                    Ok((slim, last_viewed_at))
                })?;
                rows.collect()
            })
            .await?;

        let items: Vec<EntitySlim> = rows.into_iter().map(|(r, _)| EntitySlim::from(r)).collect();
        return Ok(GridOutlineResponse {
            total_count: Some(items.len() as i64).map(|count| count.min(RECENTLY_VIEWED_CAP)),
            items,
        });
    }

    if query.status.as_deref() == Some("random") {
        let random_seed = query.random_seed.unwrap_or(0);
        let bitmaps = db.bitmaps.clone();
        let active_bm = bitmaps.get(&BitmapKey::Status(1));
        let mut filtered_ids: Vec<i64> = active_bm.iter().map(|id| id as i64).collect();

        if let Some(ref color_ids) = color_file_ids {
            filtered_ids.retain(|id| color_ids.contains(id));
        }

        let rows = db
            .with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_ids(
                    conn,
                    &filtered_ids,
                    filtered_ids.len() as i64 + 1,
                    "random",
                    "asc",
                    None,
                    grid_filters.as_ref(),
                    Some(random_seed),
                )
            })
            .await?;

        let items: Vec<EntitySlim> = rows.into_iter().map(EntitySlim::from).collect();
        return Ok(GridOutlineResponse {
            total_count: Some(items.len() as i64),
            items,
        });
    }

    let status_int = match query.status.as_deref() {
        Some(s) => Some(parse_file_status(s)?),
        None => None,
    };

    let rows = if let Some(ref color_ids) = color_file_ids {
        let bitmaps = db.bitmaps.clone();
        let status_bm = match status_int {
            Some(0) => bitmaps.get(&BitmapKey::Status(0)),
            Some(2) => bitmaps.get(&BitmapKey::Status(2)),
            _ => bitmaps.get(&BitmapKey::Status(1)),
        };
        let filtered_ids: Vec<i64> = status_bm
            .iter()
            .map(|id| id as i64)
            .filter(|id| color_ids.contains(id))
            .collect();
        let sf = sort_field.clone();
        let sd = sort_dir.clone();
        db.with_read_conn(move |conn| {
            crate::sqlite::files::list_files_slim_by_ids(
                conn,
                &filtered_ids,
                filtered_ids.len() as i64 + 1,
                &sf,
                &sd,
                None,
                grid_filters.as_ref(),
                None,
            )
        })
        .await?
    } else {
        db.list_files_slim(
            i64::MAX / 4,
            status_int,
            sort_field.clone(),
            sort_dir.clone(),
            None,
            grid_filters,
        )
        .await?
    };

    let items: Vec<EntitySlim> = rows.into_iter().map(EntitySlim::from).collect();
    Ok(GridOutlineResponse {
        total_count: Some(items.len() as i64),
        items,
    })
}

pub async fn get_grid_page_slim(
    db: &SqliteDatabase,
    query: GridPageSlimQuery,
) -> Result<GridPageSlimResponse, String> {
    let limit = query.limit.unwrap_or(100).clamp(1, 200) as i64;
    let sort_field = query
        .sort_field
        .clone()
        .unwrap_or_else(|| "imported_at".to_string());
    let sort_dir = query
        .sort_order
        .clone()
        .unwrap_or_else(|| "desc".to_string());

    let grid_filters = build_grid_filters(&query);

    let color_file_ids: Option<HashSet<i64>> =
        if let Some(ref hex) = query.color_hex {
            let hex = hex.clone();
            let tolerance = query.color_accuracy.unwrap_or(20.0).clamp(1.0, 30.0);
            let ids: Vec<i64> = db
                .with_read_conn(move |conn| color_filter_ids(conn, &hex, tolerance))
                .await?;
            Some(ids.into_iter().collect())
        } else {
            None
        };

    if let Some(collection_id) = query.collection_entity_id {
        let cache_key = build_scope_cache_key(&query, &sort_field, &sort_dir);
        let (member_file_ids, total_count) = if let Some(snap) = db.scope_cache_get(&cache_key)
        {
            (snap.ids, Some(snap.total_count))
        } else {
            let mut ids = db.list_collection_member_file_ids(collection_id).await?;
            if let Some(ref color_ids) = color_file_ids {
                ids.retain(|id| color_ids.contains(id));
            }
            let tc = ids.len() as i64;
            db.scope_cache_put(
                cache_key,
                ScopeSnapshot {
                    ids: ids.clone(),
                    total_count: tc,
                    created_at: Instant::now(),
                },
            );
            (ids, Some(tc))
        };

        if member_file_ids.is_empty() {
            return Ok(GridPageSlimResponse {
                items: Vec::new(),
                next_cursor: None,
                has_more: false,
                total_count,
            });
        }

        let cursor = query.cursor.clone();
        let fetch_limit = limit + 1;
        let gf = grid_filters.clone();

        let mut rows = db
            .with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_collection_rank(
                    conn,
                    &member_file_ids,
                    collection_id,
                    fetch_limit,
                    cursor.as_deref(),
                    gf.as_ref(),
                )
            })
            .await?;

        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }

        let next_cursor = if has_more {
            rows.last().and_then(|row| {
                row.position_rank
                    .map(|rank| format!("{}\0{}", rank, row.file_id))
            })
        } else {
            None
        };

        return Ok(GridPageSlimResponse {
            items: rows.into_iter().map(EntitySlim::from).collect(),
            next_cursor,
            has_more,
            total_count,
        });
    }

    // Unified scope resolution for smart_folder, tag search, folder,
    // uncategorized, and untagged views.
    let scope_filter = ScopeFilter::from(&query);
    let needs_scope = scope_filter.has_smart_folder()
        || scope_filter.has_search_tags()
        || scope_filter.has_folder()
        || matches!(
            scope_filter.status.as_deref(),
            Some("untagged") | Some("uncategorized")
        );

    if needs_scope {
        let cache_key = build_scope_cache_key(&query, &sort_field, &sort_dir);
        let (filtered_ids, total_count) = if let Some(snap) = db.scope_cache_get(&cache_key) {
            (snap.ids, Some(snap.total_count))
        } else {
            let scope_bm = resolve_scope(db, &scope_filter).await?;
            let mut ids: Vec<i64> = scope_bm.iter().map(|id| id as i64).collect();

            if let Some(ref color_ids) = color_file_ids {
                ids.retain(|id| color_ids.contains(id));
            }

            let tc = ids.len() as i64;
            db.scope_cache_put(
                cache_key,
                ScopeSnapshot {
                    ids: ids.clone(),
                    total_count: tc,
                    created_at: Instant::now(),
                },
            );
            (ids, Some(tc))
        };

        let sf = sort_field.clone();
        let sd = sort_dir.clone();
        let cursor = query.cursor.clone();
        let fetch_limit = limit + 1;
        let gf = grid_filters.clone();

        // Folders ALWAYS use position_rank ordering regardless of sort_field.
        let has_excluded_folders = query
            .excluded_folder_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false);
        let is_single_folder = query
            .folder_ids
            .as_ref()
            .map(|v| v.len() == 1)
            .unwrap_or(false)
            && !has_excluded_folders
            && query
                .folder_match_mode
                .as_deref()
                .map(|m| m == "all" || m == "exact")
                .unwrap_or(true);
        let mut rows = if is_single_folder {
            let fid = query.folder_ids.as_ref().unwrap()[0];
            db.with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_folder_rank(
                    conn,
                    &filtered_ids,
                    fid,
                    fetch_limit,
                    "asc", // position_rank is always ascending
                    cursor.as_deref(),
                    gf.as_ref(),
                )
            })
            .await?
        } else {
            db.with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_ids(
                    conn,
                    &filtered_ids,
                    fetch_limit,
                    &sf,
                    &sd,
                    cursor.as_deref(),
                    gf.as_ref(),
                    None,
                )
            })
            .await?
        };

        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }

        let effective_sort = if is_single_folder {
            "position_rank"
        } else {
            &sort_field
        };
        let next_cursor = if has_more {
            rows.last()
                .and_then(|row| slim_cursor_value_for_sort(row, effective_sort, None))
        } else {
            None
        };

        return Ok(GridPageSlimResponse {
            items: rows.into_iter().map(EntitySlim::from).collect(),
            next_cursor,
            has_more,
            total_count,
        });
    }

    if query.status.as_deref() == Some("recently_viewed") {
        let cursor = query.cursor.clone();
        let fetch_limit = limit + 1;
        const RECENTLY_VIEWED_CAP: i64 = 500;

        let total_count: Option<i64> = db
            .with_read_conn(|conn| {
                conn.query_row(
                    "SELECT COUNT(*) FROM file WHERE view_count > 0 AND status = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map(|c| Some(c.min(RECENTLY_VIEWED_CAP)))
            })
            .await?;

        let mut rows: Vec<(crate::sqlite::files::FileMetadataSlim, String)> = db
            .with_read_conn(move |conn| {
                let mut sql = String::from(
                    "SELECT hash, name, mime, width, height, size, status, rating, blurhash,
                            imported_at, dominant_color_hex, duration_ms, num_frames, has_audio, view_count,
                            file_id, last_viewed_at
                     FROM file WHERE view_count > 0 AND status = 1
                     AND file_id IN (
                       SELECT file_id FROM file WHERE view_count > 0 AND status = 1
                       ORDER BY last_viewed_at DESC, file_id DESC LIMIT 500
                     )"
                );
                let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

                if let Some(c) = cursor.as_deref() {
                    let parts: Vec<&str> = c.splitn(2, '\0').collect();
                    if parts.len() == 2 {
                        let cursor_file_id: i64 = parts[1].parse().unwrap_or(0);
                        let p1 = param_values.len() + 1;
                        let p2 = param_values.len() + 2;
                        sql.push_str(&format!(
                            " AND (last_viewed_at, file_id) < (?{p1}, ?{p2})",
                        ));
                        param_values.push(Box::new(parts[0].to_string()));
                        param_values.push(Box::new(cursor_file_id));
                    }
                }

                sql.push_str(&format!(
                    " ORDER BY last_viewed_at DESC, file_id DESC LIMIT ?{}",
                    param_values.len() + 1
                ));
                param_values.push(Box::new(fetch_limit));

                let params_refs: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
                let mut stmt = conn.prepare(&sql)?;
                let rows = stmt.query_map(params_refs.as_slice(), |row| {
                    let slim = crate::sqlite::files::FileMetadataSlim {
                        file_id: row.get(15)?,
                        entity_id: row.get(15)?,
                        is_collection: false,
                        collection_item_count: None,
                        hash: row.get(0)?,
                        name: row.get(1)?,
                        mime: row.get(2)?,
                        width: row.get(3)?,
                        height: row.get(4)?,
                        size: row.get(5)?,
                        status: row.get::<_, i64>(6)? as u8,
                        rating: row.get(7)?,
                        blurhash: row.get(8)?,
                        imported_at: row.get(9)?,
                        dominant_color_hex: row.get(10)?,
                        duration_ms: row.get(11)?,
                        num_frames: row.get(12)?,
                        has_audio: row.get::<_, i64>(13)? != 0,
                        view_count: row.get(14)?,
                        position_rank: None,
                    };
                    let last_viewed_at: String = row.get::<_, Option<String>>(16)?.unwrap_or_default();
                    Ok((slim, last_viewed_at))
                })?;
                rows.collect()
            })
            .await?;

        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }

        let next_cursor = if has_more {
            rows.last()
                .map(|(row, viewed_at)| format!("{}\0{}", viewed_at, row.file_id))
        } else {
            None
        };

        return Ok(GridPageSlimResponse {
            items: rows.into_iter().map(|(r, _)| EntitySlim::from(r)).collect(),
            next_cursor,
            has_more,
            total_count,
        });
    }

    if query.status.as_deref() == Some("random") {
        let random_seed = query.random_seed.unwrap_or(0);
        let bitmaps = db.bitmaps.clone();
        let active_bm = bitmaps.get(&BitmapKey::Status(1));
        let mut filtered_ids: Vec<i64> = active_bm.iter().map(|id| id as i64).collect();

        if let Some(ref color_ids) = color_file_ids {
            filtered_ids.retain(|id| color_ids.contains(id));
        }

        // Don't send total_count — bitmap includes collection members that
        // NON_MEMBER_SINGLE_CLAUSE filters out, causing overestimated scroll height.
        let total_count: Option<i64> = None;
        let cursor = query.cursor.clone();
        let fetch_limit = limit + 1;

        let gf = grid_filters;

        let mut rows = db
            .with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_ids(
                    conn,
                    &filtered_ids,
                    fetch_limit,
                    "random",
                    "asc",
                    cursor.as_deref(),
                    gf.as_ref(),
                    Some(random_seed),
                )
            })
            .await?;

        let has_more = rows.len() as i64 > limit;
        if has_more {
            rows.truncate(limit as usize);
        }

        let next_cursor = if has_more {
            rows.last()
                .and_then(|row| slim_cursor_value_for_sort(row, "random", Some(random_seed)))
        } else {
            None
        };

        return Ok(GridPageSlimResponse {
            items: rows.into_iter().map(EntitySlim::from).collect(),
            next_cursor,
            has_more,
            total_count,
        });
    }

    let status_int = match query.status.as_deref() {
        Some(s) => Some(parse_file_status(s)?),
        None => None,
    };

    let (mut rows, total_count) = if let Some(ref color_ids) = color_file_ids {
        let bitmaps = db.bitmaps.clone();
        let status_bm = match status_int {
            Some(0) => bitmaps.get(&BitmapKey::Status(0)),
            Some(2) => bitmaps.get(&BitmapKey::Status(2)),
            // Default: active only (status=1).
            _ => bitmaps.get(&BitmapKey::Status(1)),
        };
        let filtered_ids: Vec<i64> = status_bm
            .iter()
            .map(|id| id as i64)
            .filter(|id| color_ids.contains(id))
            .collect();

        let tc = Some(filtered_ids.len() as i64);

        let sf = sort_field.clone();
        let sd = sort_dir.clone();
        let cursor = query.cursor.clone();
        let fetch_limit = limit + 1;
        let gf = grid_filters;
        let r = db
            .with_read_conn(move |conn| {
                crate::sqlite::files::list_files_slim_by_ids(
                    conn,
                    &filtered_ids,
                    fetch_limit,
                    &sf,
                    &sd,
                    cursor.as_deref(),
                    gf.as_ref(),
                    None,
                )
            })
            .await?;
        (r, tc)
    } else {
        let r = db
            .list_files_slim(
                limit + 1,
                status_int,
                sort_field.clone(),
                sort_dir.clone(),
                query.cursor.clone(),
                grid_filters,
            )
            .await?;
        (r, None)
    };

    let has_more = rows.len() as i64 > limit;
    if has_more {
        rows.truncate(limit as usize);
    }

    let next_cursor = if has_more {
        rows.last()
            .and_then(|row| slim_cursor_value_for_sort(row, &sort_field, None))
    } else {
        None
    };

    Ok(GridPageSlimResponse {
        items: rows.into_iter().map(EntitySlim::from).collect(),
        next_cursor,
        has_more,
        total_count,
    })
}
