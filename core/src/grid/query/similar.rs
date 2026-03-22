//! Grid query for "Find Visually Similar" — takes a pre-ordered hash list
//! (sorted by phash distance) and returns paginated results preserving that order.

use crate::sqlite::files::{populate_grid_filter, FileMetadataSlim};
use crate::sqlite::SqliteDatabase;
use crate::types::{EntityGridItem, GridPageSlimQuery, GridPageSlimResponse};

use super::common::{GridOutlineResponse, QueryInputs};

pub(super) async fn get_similar_outline(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    _inputs: &QueryInputs,
) -> Result<GridOutlineResponse, String> {
    let hashes = query
        .scope
        .similar_hashes
        .as_ref()
        .cloned()
        .unwrap_or_default();
    let entity_ids = resolve_hashes_to_entity_ids(db, &hashes).await?;
    Ok(GridOutlineResponse {
        items: Vec::new(),
        total_count: Some(entity_ids.len() as i64),
    })
}

pub(super) async fn get_similar_page(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
    _inputs: &QueryInputs,
) -> Result<GridPageSlimResponse, String> {
    let hashes = query
        .scope
        .similar_hashes
        .as_ref()
        .cloned()
        .unwrap_or_default();

    if hashes.is_empty() {
        return Ok(GridPageSlimResponse {
            items: Vec::new(),
            next_cursor: None,
            has_more: false,
            total_count: Some(0),
        });
    }

    // Resolve hashes to entity_ids, preserving the distance-sorted order.
    let ordered_entity_ids = resolve_hashes_to_entity_ids(db, &hashes).await?;
    let total = ordered_entity_ids.len() as i64;

    // Pagination: use cursor as an offset index into the ordered list.
    let limit = query.limit.unwrap_or(100).min(500);
    let offset: usize = query
        .cursor
        .as_deref()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);

    let page_ids: Vec<i64> = ordered_entity_ids
        .iter()
        .skip(offset)
        .take(limit)
        .copied()
        .collect();

    if page_ids.is_empty() {
        return Ok(GridPageSlimResponse {
            items: Vec::new(),
            next_cursor: None,
            has_more: false,
            total_count: Some(total),
        });
    }

    // Fetch metadata for this page of entity_ids.
    let rows: Vec<FileMetadataSlim> = db
        .with_read_conn({
            let ids = page_ids.clone();
            move |conn| {
                populate_grid_filter(conn, &ids)?;
                let mut stmt = conn.prepare_cached(&format!(
                    "SELECT {}{}
                     WHERE me.entity_id IN (SELECT file_id FROM _grid_filter)
                       AND (me.kind = 'collection' OR me.parent_collection_id IS NULL)",
                    crate::sqlite::files::ENTITY_SLIM_SELECT_PUB,
                    crate::sqlite::files::ENTITY_SLIM_FROM_PUB,
                ))?;
                let rows = stmt.query_map([], crate::sqlite::files::row_to_entity_slim_pub)?;
                rows.collect()
            }
        })
        .await?;

    // Re-order rows to match the distance-sorted input order.
    let id_to_row: std::collections::HashMap<i64, FileMetadataSlim> = rows
        .into_iter()
        .map(|r| {
            let eid = if r.entity_id > 0 {
                r.entity_id
            } else {
                r.file_id
            };
            (eid, r)
        })
        .collect();

    let ordered_items: Vec<EntityGridItem> = page_ids
        .iter()
        .filter_map(|eid| id_to_row.get(eid).cloned().map(EntityGridItem::from))
        .collect();

    let next_offset = offset + ordered_items.len();
    let has_more = next_offset < ordered_entity_ids.len();
    let next_cursor = if has_more {
        Some(next_offset.to_string())
    } else {
        None
    };

    Ok(GridPageSlimResponse {
        items: ordered_items,
        next_cursor,
        has_more,
        total_count: Some(total),
    })
}

/// Resolve an ordered list of file hashes to entity_ids, preserving order.
async fn resolve_hashes_to_entity_ids(
    db: &SqliteDatabase,
    hashes: &[String],
) -> Result<Vec<i64>, String> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let resolved = db.resolve_entity_hashes_batch(hashes).await?;
    let hash_to_id: std::collections::HashMap<String, i64> = resolved.into_iter().collect();
    Ok(hashes
        .iter()
        .filter_map(|h| hash_to_id.get(h).copied())
        .collect())
}
