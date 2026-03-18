use crate::sqlite::SqliteDatabase;
use crate::types::{EntitySlim, GridPageSlimQuery, GridPageSlimResponse};

use super::common::{GridOutlineResponse, QueryInputs};

pub(super) async fn get_collection_outline(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<GridOutlineResponse, String> {
    let collection_id = query
        .scope
        .collection_entity_id
        .expect("collection id required");
    let member_file_ids = collection_member_ids(db, query, inputs).await?;
    if member_file_ids.is_empty() {
        return Ok(GridOutlineResponse {
            items: Vec::new(),
            total_count: Some(0),
        });
    }

    let gf = inputs.grid_filters.clone();
    let rows = db
        .with_read_conn(move |conn| {
            crate::sqlite::files::list_files_slim_by_collection_rank(
                conn,
                &member_file_ids,
                collection_id,
                member_file_ids.len() as i64 + 1,
                None,
                gf.as_ref(),
            )
        })
        .await?;

    Ok(GridOutlineResponse {
        total_count: Some(rows.len() as i64),
        items: rows.into_iter().map(EntitySlim::from).collect(),
    })
}

pub(super) async fn get_collection_page(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<GridPageSlimResponse, String> {
    let collection_id = query
        .scope
        .collection_entity_id
        .expect("collection id required");
    let (member_file_ids, total_count) = collection_member_snapshot(db, query, inputs).await?;

    if member_file_ids.is_empty() {
        return Ok(GridPageSlimResponse {
            items: Vec::new(),
            next_cursor: None,
            has_more: false,
            total_count,
        });
    }

    let cursor = query.cursor.clone();
    let fetch_limit = inputs.limit + 1;
    let gf = inputs.grid_filters.clone();
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

    let has_more = rows.len() as i64 > inputs.limit;
    if has_more {
        rows.truncate(inputs.limit as usize);
    }

    let next_cursor = if has_more {
        rows.last()
            .and_then(|row| row.position_rank.map(|rank| format!("{}\0{}", rank, row.file_id)))
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

async fn collection_member_ids(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<Vec<i64>, String> {
    Ok(collection_member_snapshot(db, query, inputs).await?.0)
}

async fn collection_member_snapshot(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    inputs: &QueryInputs,
) -> Result<(Vec<i64>, Option<i64>), String> {
    let collection_id = query
        .scope
        .collection_entity_id
        .expect("collection id required");

    let mut ids = db.list_collection_member_file_ids(collection_id).await?;
    if let Some(ref color_ids) = inputs.color_file_ids {
        ids.retain(|id| color_ids.contains(id));
    }
    let total_count = ids.len() as i64;
    Ok((ids, Some(total_count)))
}
