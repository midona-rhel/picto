use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::OnceLock;
use std::time::Instant;

use chrono::Utc;

use crate::ptr_controller::PtrController;
use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::files::FileMetadataSlim;
use crate::sqlite::projections::ResolvedMetadataFull;
use crate::sqlite::folders::list_uncategorized_entity_ids;
use crate::sqlite::smart_folders;
use crate::sqlite::tags::{find_tag as sql_find_tag, parse_tag_string, FileTagInfo};
use crate::sqlite::{ScopeSnapshot, ScopeSnapshotKey, SqliteDatabase};
use crate::sqlite_ptr::PtrSqliteDatabase;
use crate::tags;
use crate::types::{
    parse_file_status, status_to_string, tag_display_key, DominantColorDto, FileAllMetadata,
    EntityDetails, EntityMetadataBatchResponse, EntitySlim, GridPageSlimQuery, GridPageSlimResponse,
    ResolvedTagInfo,
};

static METADATA_BATCH_PREFETCH_SEMAPHORE: OnceLock<tokio::sync::Semaphore> = OnceLock::new();

fn metadata_batch_prefetch_semaphore() -> &'static tokio::sync::Semaphore {
    METADATA_BATCH_PREFETCH_SEMAPHORE.get_or_init(|| tokio::sync::Semaphore::new(2))
}

fn file_tag_to_resolved_info(t: FileTagInfo) -> ResolvedTagInfo {
    let raw_tag = tags::combine_tag(&t.namespace, &t.subtag);
    let disp_ns = t.display_ns.as_deref().unwrap_or(&t.namespace);
    let disp_st = t.display_st.as_deref().unwrap_or(&t.subtag);
    let display_tag = tag_display_key(disp_ns, disp_st);
    let read_only = t.source != "local";
    ResolvedTagInfo {
        raw_tag,
        display_tag,
        namespace: t.display_ns.unwrap_or(t.namespace),
        subtag: t.display_st.unwrap_or(t.subtag),
        source: t.source,
        read_only,
    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IncludeMatchMode {
    Any,
    All,
    Exact,
}

fn parse_include_match_mode(raw: Option<&str>, default_mode: IncludeMatchMode) -> IncludeMatchMode {
    match raw {
        Some("any") => IncludeMatchMode::Any,
        Some("exact") => IncludeMatchMode::Exact,
        Some("all") => IncludeMatchMode::All,
        _ => default_mode,
    }
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

pub struct GridController;

impl GridController {
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

        let grid_filters = {
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
            if has_any {
                Some(crate::sqlite::files::GridFilters {
                    rating_min: query.rating_min,
                    mime_prefixes: query.mime_prefixes.clone(),
                    search_text: query.search_text.clone(),
                })
            } else {
                None
            }
        };

        let color_file_ids: Option<std::collections::HashSet<i64>> =
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

        let has_smart_folder = query.smart_folder_predicate.is_some();
        let has_search_tags = query
            .search_tags
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
            || query
                .search_excluded_tags
                .as_ref()
                .map(|t| !t.is_empty())
                .unwrap_or(false);
        let has_folder = query
            .folder_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
            || query
                .excluded_folder_ids
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false);

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

        if has_smart_folder || has_search_tags || has_folder {
            let cache_key = build_scope_cache_key(&query, &sort_field, &sort_dir);
            let (filtered_ids, total_count) = if let Some(snap) = db.scope_cache_get(&cache_key) {
                (snap.ids, Some(snap.total_count))
            } else {
                let ids: Vec<i64> = if let Some(predicate) = &query.smart_folder_predicate {
                    let pred = predicate.clone();
                    let bitmaps = db.bitmaps.clone();
                    db.with_read_conn(move |conn| {
                        let bm = smart_folders::compile_predicate(conn, &pred, &bitmaps)?;
                        Ok(bm.iter().map(|id| id as i64).collect::<Vec<_>>())
                    })
                    .await?
                } else if has_search_tags {
                    let include_tags = query.search_tags.clone().unwrap_or_default();
                    let exclude_tags = query.search_excluded_tags.clone().unwrap_or_default();
                    let match_mode = parse_include_match_mode(
                        query.tag_match_mode.as_deref(),
                        IncludeMatchMode::All,
                    );
                    let bitmaps = db.bitmaps.clone();
                    db.with_read_conn(move |conn| {
                        let resolve_ids = |tag_list: &[String],
                                           strict_missing: bool|
                         -> rusqlite::Result<Vec<i64>> {
                            let mut out = Vec::new();
                            for tag in tag_list {
                                let parsed = tags::parse_tag(tag).or_else(|| {
                                    let (ns, st) = parse_tag_string(tag);
                                    Some((ns, st))
                                });
                                if let Some((ns, st)) = parsed {
                                    if let Some(tag_id) = sql_find_tag(conn, &ns, &st)? {
                                        out.push(tag_id);
                                    } else if strict_missing {
                                        return Ok(Vec::new());
                                    }
                                }
                            }
                            Ok(out)
                        };

                        let include_ids =
                            resolve_ids(&include_tags, match_mode != IncludeMatchMode::Any)?;
                        let exclude_ids = resolve_ids(&exclude_tags, false)?;
                        let all_active = bitmaps.get(&BitmapKey::AllActive);

                        if !include_tags.is_empty() && include_ids.is_empty() {
                            return Ok(Vec::new());
                        }

                        let mut result = if include_ids.is_empty() {
                            all_active.clone()
                        } else if match_mode == IncludeMatchMode::Any {
                            let mut union = roaring::RoaringBitmap::new();
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
                            let mut excluded = roaring::RoaringBitmap::new();
                            for tid in &exclude_ids {
                                excluded |= &bitmaps.get(&BitmapKey::EffectiveTag(*tid));
                            }
                            result -= &excluded;
                        }
                        result &= &all_active;
                        Ok(result.iter().map(|id| id as i64).collect::<Vec<_>>())
                    })
                    .await?
                } else if has_folder {
                    let include_folders = query.folder_ids.clone().unwrap_or_default();
                    let exclude_folders = query.excluded_folder_ids.clone().unwrap_or_default();
                    let match_mode = parse_include_match_mode(
                        query.folder_match_mode.as_deref(),
                        IncludeMatchMode::Any,
                    );
                    let bitmaps = db.bitmaps.clone();
                    let all_active = bitmaps.get(&BitmapKey::AllActive);

                    let mut result = if include_folders.is_empty() {
                        all_active.clone()
                    } else if match_mode == IncludeMatchMode::Any {
                        let mut union = roaring::RoaringBitmap::new();
                        for fid in &include_folders {
                            union |= &bitmaps.get(&BitmapKey::Folder(*fid));
                        }
                        union
                    } else {
                        let mut iter = include_folders.iter();
                        let first = *iter.next().expect("include_folders not empty");
                        let mut intersect = bitmaps.get(&BitmapKey::Folder(first));
                        for fid in iter {
                            intersect &= &bitmaps.get(&BitmapKey::Folder(*fid));
                        }
                        intersect
                    };

                    if !exclude_folders.is_empty() {
                        let mut excluded = roaring::RoaringBitmap::new();
                        for fid in &exclude_folders {
                            excluded |= &bitmaps.get(&BitmapKey::Folder(*fid));
                        }
                        result -= &excluded;
                    }
                    result &= &all_active;
                    result.iter().map(|id| id as i64).collect::<Vec<_>>()
                } else {
                    Vec::new()
                };

                let ids = if let Some(ref color_ids) = color_file_ids {
                    ids.into_iter()
                        .filter(|id| color_ids.contains(id))
                        .collect()
                } else {
                    ids
                };

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

            let filtered_ids = filtered_ids;

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

        if query.status.as_deref() == Some("uncategorized") {
            let cache_key = build_scope_cache_key(&query, &sort_field, &sort_dir);
            let (filtered_ids, total_count) = if let Some(snap) = db.scope_cache_get(&cache_key) {
                (snap.ids, Some(snap.total_count))
            } else {
                let ids = db.with_read_conn(list_uncategorized_entity_ids).await?;

                let ids = if let Some(ref color_ids) = color_file_ids {
                    ids.into_iter()
                        .filter(|id| color_ids.contains(id))
                        .collect()
                } else {
                    ids
                };

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

            let mut rows = db
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

            return Ok(GridPageSlimResponse {
                items: rows.into_iter().map(EntitySlim::from).collect(),
                next_cursor,
                has_more,
                total_count,
            });
        }

        if query.status.as_deref() == Some("untagged") {
            let cache_key = build_scope_cache_key(&query, &sort_field, &sort_dir);
            let (filtered_ids, total_count) = if let Some(snap) = db.scope_cache_get(&cache_key) {
                (snap.ids, Some(snap.total_count))
            } else {
                let bitmaps = db.bitmaps.clone();
                let all_active = bitmaps.get(&BitmapKey::AllActive);
                let tagged = bitmaps.get(&BitmapKey::Tagged);
                let untagged = &all_active - &tagged;
                let ids: Vec<i64> = untagged.iter().map(|id| id as i64).collect();

                let ids = if let Some(ref color_ids) = color_file_ids {
                    ids.into_iter()
                        .filter(|id| color_ids.contains(id))
                        .collect()
                } else {
                    ids
                };

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

            let mut rows = db
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
                // Default: active only (status=1). AllActive includes inbox.
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

    pub async fn get_files_metadata_batch(
        db: &SqliteDatabase,
        ptr_db: &PtrSqliteDatabase,
        hashes: Vec<String>,
    ) -> Result<EntityMetadataBatchResponse, String> {
        const MAX_BATCH: usize = 200;
        const SLOW_BATCH_WARN_MS: f64 = 200.0;
        const SLOW_STAGE_WARN_MS: f64 = 50.0;

        let _permit = metadata_batch_prefetch_semaphore()
            .acquire()
            .await
            .map_err(|_| "metadata batch prefetch queue closed".to_string())?;

        let batch_started = Instant::now();
        let mut seen = HashSet::with_capacity(hashes.len());
        let hashes: Vec<String> = hashes
            .into_iter()
            .filter(|h| seen.insert(h.clone()))
            .take(MAX_BATCH)
            .collect();
        let mut items: HashMap<String, FileAllMetadata> = HashMap::with_capacity(hashes.len());
        let mut missing = Vec::new();

        let local_hashes_req = hashes.clone();
        let ptr_hashes_req = hashes.clone();
        let local_fut = async {
            let local_started = Instant::now();
            let projections = db.get_files_metadata_batch(local_hashes_req).await?;
            let local_ms = local_started.elapsed().as_secs_f64() * 1000.0;
            Ok::<_, String>((projections, local_ms))
        };
        let ptr_fut = async {
            let ptr_started = Instant::now();
            let negative_cached: std::collections::HashSet<String> =
                PtrController::batch_check_negative(ptr_db, ptr_hashes_req.clone())
                    .await?
                    .into_iter()
                    .collect();

            let ptr_lookup_hashes: Vec<String> = ptr_hashes_req
                .iter()
                .filter(|h| !negative_cached.contains(*h))
                .cloned()
                .collect();
            let ptr_lookup_count = ptr_lookup_hashes.len();

            let ptr_overlay_map: HashMap<String, Vec<crate::sqlite_ptr::tags::PtrResolvedTag>> =
                PtrController::batch_get_overlay(ptr_db, ptr_lookup_hashes.clone())
                    .await?
                    .into_iter()
                    .collect();

            let ptr_overlay_hits: std::collections::HashSet<String> =
                ptr_overlay_map.keys().cloned().collect();
            let new_negative_hashes: Vec<String> = ptr_lookup_hashes
                .into_iter()
                .filter(|h| !ptr_overlay_hits.contains(h))
                .collect();
            if !new_negative_hashes.is_empty() {
                // Memory-only — avoids writer lock contention during sync.
                // DB negative cache is populated during overlay rebuild.
                ptr_db
                    .add_negative_cache_mem_only(new_negative_hashes)
                    .await;
            }
            let ptr_ms = ptr_started.elapsed().as_secs_f64() * 1000.0;

            Ok::<_, String>((ptr_lookup_count, ptr_overlay_map, ptr_overlay_hits, ptr_ms))
        };

        let (local_res, ptr_res) = tokio::join!(local_fut, ptr_fut);
        let (projections, local_ms) = local_res?;
        let (ptr_lookup_count, mut ptr_overlay_map, ptr_overlay_hits, ptr_ms) = ptr_res?;

        let mut proj_map: HashMap<String, ResolvedMetadataFull> = HashMap::new();
        for p in projections {
            proj_map.insert(p.resolved.file.hash.clone(), p);
        }

        let local_hashes: Vec<String> = proj_map.keys().cloned().collect();

        let merge_started = Instant::now();
        for hash in &hashes {
            if let Some(full) = proj_map.remove(hash) {
                let ptr_tags = ptr_overlay_map.remove(hash).unwrap_or_default();

                let mut seen = std::collections::HashSet::new();
                let mut tags: Vec<ResolvedTagInfo> = full
                    .resolved
                    .tags
                    .into_iter()
                    .map(|t| {
                        let info = file_tag_to_resolved_info(t);
                        seen.insert(info.display_tag.clone());
                        info
                    })
                    .collect();

                for pt in ptr_tags {
                    let display = tag_display_key(&pt.display_ns, &pt.display_st);
                    if !seen.contains(&display) {
                        seen.insert(display.clone());
                        tags.push(ResolvedTagInfo {
                            raw_tag: tags::combine_tag(&pt.raw_ns, &pt.raw_st),
                            display_tag: display,
                            namespace: pt.display_ns,
                            subtag: pt.display_st,
                            source: "ptr".to_string(),
                            read_only: true,
                        });
                    }
                }

                let source_urls: Option<serde_json::Value> = full
                    .source_urls_json
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                let notes: Option<serde_json::Value> = full
                    .notes
                    .as_deref()
                    .and_then(|s| serde_json::from_str(s).ok());
                let dominant_colors: Option<Vec<DominantColorDto>> = if full.colors.is_empty() {
                    None
                } else {
                    Some(
                        full.colors
                            .into_iter()
                            .map(|(hex, l, a, b)| DominantColorDto { hex, l, a, b })
                            .collect(),
                    )
                };

                let slim = full.resolved.file;
                let has_thumbnail =
                    slim.mime.starts_with("image/") || slim.mime.starts_with("video/");
                items.insert(
                    hash.clone(),
                    FileAllMetadata {
                        file: EntityDetails {
                            hash: slim.hash,
                            name: slim.name,
                            size: slim.size,
                            mime: slim.mime,
                            width: slim.width,
                            height: slim.height,
                            duration_ms: slim.duration_ms,
                            num_frames: slim.num_frames,
                            has_audio: slim.has_audio,
                            status: status_to_string(slim.status as i64).to_string(),
                            rating: slim.rating,
                            view_count: slim.view_count,
                            source_urls,
                            imported_at: slim.imported_at,
                            has_thumbnail,
                            blurhash: slim.blurhash,
                            dominant_color_hex: slim.dominant_color_hex,
                            dominant_colors,
                            notes,
                        },
                        tags,
                        parent_tags: Vec::new(),
                    },
                );
            } else {
                missing.push(hash.clone());
            }
        }
        let merge_ms = merge_started.elapsed().as_secs_f64() * 1000.0;
        let total_ms = batch_started.elapsed().as_secs_f64() * 1000.0;

        if total_ms >= SLOW_BATCH_WARN_MS
            || local_ms >= SLOW_STAGE_WARN_MS
            || ptr_ms >= SLOW_STAGE_WARN_MS
            || merge_ms >= SLOW_STAGE_WARN_MS
        {
            tracing::warn!(
                target: "picto::core::grid_controller",
                "slow get_files_metadata_batch total_ms={:.2} local_ms={:.2} ptr_ms={:.2} merge_ms={:.2} req_hashes={} local_hits={} ptr_lookup={} ptr_hits={} missing={}",
                total_ms,
                local_ms,
                ptr_ms,
                merge_ms,
                hashes.len(),
                local_hashes.len(),
                ptr_lookup_count,
                ptr_overlay_hits.len(),
                missing.len(),
            );
        }

        crate::perf::record_files_metadata_batch(
            total_ms,
            local_ms,
            ptr_ms,
            merge_ms,
            hashes.len(),
            local_hashes.len(),
            ptr_lookup_count,
            ptr_overlay_hits.len(),
            missing.len(),
        );

        Ok(EntityMetadataBatchResponse {
            items,
            missing,
            generated_at: Utc::now().to_rfc3339(),
        })
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
