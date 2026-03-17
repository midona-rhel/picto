use std::hash::{Hash, Hasher};
use std::time::Instant;

use crate::scope::resolver::{resolve_scope, ScopeFilter};
use crate::sqlite::files::FileMetadataSlim;
use crate::sqlite::{ScopeSnapshot, ScopeSnapshotKey, SqliteDatabase};
use crate::types::{
    EntitySlim, GridPageSlimQuery, GridPageSlimResponse, GridScopeKind, GridSystemScopeKey,
};

use super::common::{GridOutlineResponse, QueryInputs};
use super::cursor::slim_cursor_value_for_sort;

pub(super) fn needs_scope(query: &GridPageSlimQuery) -> bool {
    let scope_filter = ScopeFilter::from(query);
    scope_filter.has_smart_folder()
        || scope_filter.has_search_tags()
        || scope_filter.has_folder()
        || matches!(
            scope_filter.system_key(),
            Some(GridSystemScopeKey::Untagged) | Some(GridSystemScopeKey::Uncategorized)
        )
}

pub(super) async fn get_scoped_outline(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<GridOutlineResponse, String> {
    let filtered_ids = get_scoped_ids(db, query, inputs).await?;
    if filtered_ids.is_empty() {
        return Ok(GridOutlineResponse {
            items: Vec::new(),
            total_count: Some(0),
        });
    }

    let rows = list_scoped_rows(
        db,
        query,
        inputs,
        filtered_ids.len() as i64 + 1,
        None,
        filtered_ids,
    )
    .await?;

    Ok(GridOutlineResponse {
        total_count: Some(rows.len() as i64),
        items: rows.into_iter().map(EntitySlim::from).collect(),
    })
}

pub(super) async fn get_scoped_page(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<GridPageSlimResponse, String> {
    let (filtered_ids, total_count) = get_scoped_snapshot(db, query, inputs).await?;
    let mut rows = list_scoped_rows(
        db,
        query,
        inputs,
        inputs.limit + 1,
        query.cursor.clone(),
        filtered_ids,
    )
    .await?;

    let has_more = rows.len() as i64 > inputs.limit;
    if has_more {
        rows.truncate(inputs.limit as usize);
    }

    let effective_sort = if is_single_folder(query) {
        "position_rank"
    } else {
        &inputs.sort_field
    };
    let next_cursor = if has_more {
        rows.last()
            .and_then(|row| slim_cursor_value_for_sort(row, effective_sort, None))
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

fn build_scope_cache_key(
    query: &GridPageSlimQuery,
    sort_field: &str,
    sort_dir: &str,
) -> ScopeSnapshotKey {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();

    let scope = if query.scope.kind == GridScopeKind::Collection {
        "collection".to_string()
    } else if query.scope.kind == GridScopeKind::Smart {
        "smart_folder".to_string()
    } else if query
        .filters
        .search_tags
        .as_ref()
        .map(|t| !t.is_empty())
        .unwrap_or(false)
        || query
            .filters
            .search_excluded_tags
            .as_ref()
            .map(|t| !t.is_empty())
            .unwrap_or(false)
    {
        "search_tags".to_string()
    } else if query
        .filters
        .folder_ids
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        || query
            .filters
            .excluded_folder_ids
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        || query.scope.kind == GridScopeKind::Folder
    {
        "folder".to_string()
    } else if query.scope.system_key == Some(GridSystemScopeKey::Uncategorized) {
        "uncategorized".to_string()
    } else if query.scope.system_key == Some(GridSystemScopeKey::Untagged) {
        "untagged".to_string()
    } else {
        format!(
            "status:{}",
            match query.scope.system_key.unwrap_or(GridSystemScopeKey::All) {
                GridSystemScopeKey::All => "active",
                GridSystemScopeKey::Inbox => "inbox",
                GridSystemScopeKey::Trash => "trash",
                GridSystemScopeKey::Untagged => "untagged",
                GridSystemScopeKey::Uncategorized => "uncategorized",
            }
        )
    };

    if let Some(cid) = query.scope.collection_entity_id {
        cid.hash(&mut hasher);
    }
    if let Some(ref pred) = query.scope.smart_folder_predicate {
        if let Ok(json) = serde_json::to_string(pred) {
            json.hash(&mut hasher);
        }
    }
    if let Some(ref tags) = query.filters.search_tags {
        tags.hash(&mut hasher);
    }
    if let Some(ref tags) = query.filters.search_excluded_tags {
        tags.hash(&mut hasher);
    }
    if let Some(ref mode) = query.filters.tag_match_mode {
        mode.hash(&mut hasher);
    }
    if let Some(ref fids) = query.filters.folder_ids {
        fids.hash(&mut hasher);
    }
    if let Some(ref fids) = query.filters.excluded_folder_ids {
        fids.hash(&mut hasher);
    }
    if let Some(ref mode) = query.filters.folder_match_mode {
        mode.hash(&mut hasher);
    }
    if let Some(ref hex) = query.filters.color_hex {
        hex.hash(&mut hasher);
    }
    if let Some(acc) = query.filters.color_accuracy {
        acc.to_bits().hash(&mut hasher);
    }

    ScopeSnapshotKey {
        scope,
        predicate_hash: hasher.finish(),
        sort_field: sort_field.to_string(),
        sort_dir: sort_dir.to_string(),
    }
}

async fn get_scoped_ids(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<Vec<i64>, String> {
    Ok(get_scoped_snapshot(db, query, inputs).await?.0)
}

async fn get_scoped_snapshot(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<(Vec<i64>, Option<i64>), String> {
    let cache_key = build_scope_cache_key(query, &inputs.sort_field, &inputs.sort_dir);
    if let Some(snap) = db.scope_cache_get(&cache_key) {
        return Ok((snap.ids, Some(snap.total_count)));
    }

    let scope_filter = ScopeFilter::from(query);
    let scope_bm = resolve_scope(db, &scope_filter).await?;
    let mut ids: Vec<i64> = scope_bm.iter().map(|id| id as i64).collect();
    if let Some(ref color_ids) = inputs.color_file_ids {
        ids.retain(|id| color_ids.contains(id));
    }
    if query.scope.kind != GridScopeKind::Collection {
        ids = db.filter_visible_entity_ids(&ids).await?;
    }

    let total_count = ids.len() as i64;
    db.scope_cache_put(
        cache_key,
        ScopeSnapshot {
            ids: ids.clone(),
            total_count,
            created_at: Instant::now(),
        },
    );
    Ok((ids, Some(total_count)))
}

async fn list_scoped_rows(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
    fetch_limit: i64,
    cursor: Option<String>,
    filtered_ids: Vec<i64>,
) -> Result<Vec<FileMetadataSlim>, String> {
    let gf = inputs.grid_filters.clone();
    if is_single_folder(query) {
        let fid = query
            .scope
            .folder_id
            .or_else(|| query.filters.folder_ids.as_ref().and_then(|ids| ids.first().copied()))
            .expect("single folder id required");
        db.with_read_conn(move |conn| {
            crate::sqlite::files::list_files_slim_by_folder_rank(
                conn,
                &filtered_ids,
                fid,
                fetch_limit,
                "asc",
                cursor.as_deref(),
                gf.as_ref(),
            )
        })
        .await
    } else {
        let sf = inputs.sort_field.clone();
        let sd = inputs.sort_dir.clone();
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
        .await
    }
}

fn is_single_folder(query: &GridPageSlimQuery) -> bool {
    let has_excluded_folders = query
        .filters
        .excluded_folder_ids
        .as_ref()
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let single_folder = if query.scope.kind == GridScopeKind::Folder {
        query.scope.folder_id.is_some()
    } else {
        query
            .filters
            .folder_ids
            .as_ref()
            .map(|v| v.len() == 1)
            .unwrap_or(false)
    };
    single_folder
        && !has_excluded_folders
        && query
            .filters
            .folder_match_mode
            .as_deref()
            .map(|m| m == "all" || m == "exact")
            .unwrap_or(true)
}
