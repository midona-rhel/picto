use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::SqliteDatabase;
use crate::types::{
    parse_file_status, EntitySlim, GridPageSlimQuery, GridPageSlimResponse, GridSystemScopeKey,
};

use super::common::{GridOutlineResponse, QueryInputs};
use super::cursor::slim_cursor_value_for_sort;

pub(super) async fn get_status_outline(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<GridOutlineResponse, String> {
    let rows = if query.sort.field.as_deref() == Some("random") {
        random_rows(db, inputs, None, query.sort.random_seed.unwrap_or(0)).await?
    } else {
        status_rows_with_total(db, query, inputs, None).await?.0
    };

    Ok(GridOutlineResponse {
        total_count: Some(rows.len() as i64),
        items: rows.into_iter().map(EntitySlim::from).collect(),
    })
}

pub(super) async fn get_status_page(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<GridPageSlimResponse, String> {
    if query.sort.field.as_deref() == Some("random") {
        let random_seed = query.sort.random_seed.unwrap_or(0);
        let mut rows = random_rows(db, inputs, query.cursor.clone(), random_seed).await?;
        let has_more = rows.len() as i64 > inputs.limit;
        if has_more {
            rows.truncate(inputs.limit as usize);
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
            total_count: None,
        });
    }

    let (mut rows, total_count) =
        status_rows_with_total(db, query, inputs, query.cursor.clone()).await?;
    let has_more = rows.len() as i64 > inputs.limit;
    if has_more {
        rows.truncate(inputs.limit as usize);
    }
    let next_cursor = if has_more {
        rows.last()
            .and_then(|row| slim_cursor_value_for_sort(row, &inputs.sort_field, None))
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

async fn random_rows(
    db: &SqliteDatabase,
    inputs: &QueryInputs,
    cursor: Option<String>,
    random_seed: i64,
) -> Result<Vec<crate::sqlite::files::FileMetadataSlim>, String> {
    let active_bm = db.bitmaps.clone().get(&BitmapKey::Status(1));
    let mut filtered_ids: Vec<i64> = active_bm.iter().map(|id| id as i64).collect();
    if let Some(ref color_ids) = inputs.color_file_ids {
        filtered_ids.retain(|id| color_ids.contains(id));
    }
    let fetch_limit = inputs.limit + 1;
    let gf = inputs.grid_filters.clone();
    let seed = random_seed;
    db.with_read_conn(move |conn| {
        crate::sqlite::files::list_files_slim_by_ids(
            conn,
            &filtered_ids,
            fetch_limit,
            "random",
            "asc",
            cursor.as_deref(),
            gf.as_ref(),
            Some(seed),
        )
    })
    .await
}

async fn status_rows_with_total(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
    cursor: Option<String>,
) -> Result<(Vec<crate::sqlite::files::FileMetadataSlim>, Option<i64>), String> {
    if let Some(ref color_ids) = inputs.color_file_ids {
        let status_bm = status_bitmap(db, query)?;
        let filtered_ids: Vec<i64> = status_bm
            .iter()
            .map(|id| id as i64)
            .filter(|id| color_ids.contains(id))
            .collect();
        let total_count = Some(filtered_ids.len() as i64);
        let sf = inputs.sort_field.clone();
        let sd = inputs.sort_dir.clone();
        let fetch_limit = inputs.limit + 1;
        let gf = inputs.grid_filters.clone();
        let rows = db
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
        return Ok((rows, total_count));
    }

    let total_count = if inputs.grid_filters.is_none() {
        Some(status_bitmap(db, query)?.len() as i64)
    } else {
        None
    };
    let rows = db
        .list_files_slim(
            inputs.limit + 1,
            status_int(query)?,
            inputs.sort_field.clone(),
            inputs.sort_dir.clone(),
            cursor,
            inputs.grid_filters.clone(),
        )
        .await?;
    Ok((rows, total_count))
}

fn status_bitmap(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
) -> Result<roaring::RoaringBitmap, String> {
    let bitmaps = db.bitmaps.clone();
    Ok(match status_int(query)? {
        Some(0) => bitmaps.get(&BitmapKey::Status(0)),
        Some(2) => bitmaps.get(&BitmapKey::Status(2)),
        _ => bitmaps.get(&BitmapKey::Status(1)),
    })
}

fn status_int(query: &GridPageSlimQuery) -> Result<Option<i64>, String> {
    match query.scope.system_key {
        Some(GridSystemScopeKey::Inbox) => Ok(Some(parse_file_status("inbox")?)),
        Some(GridSystemScopeKey::Trash) => Ok(Some(parse_file_status("trash")?)),
        Some(_) | None => Ok(None),
    }
}
