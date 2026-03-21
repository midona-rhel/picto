use std::collections::HashSet;

use serde::Serialize;

use crate::sqlite::files::GridFilters;
use crate::sqlite::SqliteDatabase;
use crate::types::{EntitySlim, GridPageSlimQuery};

#[derive(Debug, Serialize)]
pub struct GridOutlineResponse {
    pub items: Vec<EntitySlim>,
    pub total_count: Option<i64>,
}

#[derive(Clone)]
pub(super) struct QueryInputs {
    pub limit: i64,
    pub sort_field: String,
    pub sort_dir: String,
    pub grid_filters: Option<GridFilters>,
    pub color_file_ids: Option<HashSet<i64>>,
}

impl QueryInputs {
    pub(super) async fn build(
        db: &SqliteDatabase,
        query: &GridPageSlimQuery,
    ) -> Result<Self, String> {
        let sort_field = query
            .sort
            .field
            .clone()
            .unwrap_or_else(|| "date_added".to_string());
        let sort_dir = query
            .sort
            .order
            .clone()
            .unwrap_or_else(|| "desc".to_string());
        let grid_filters = build_grid_filters(query);
        let color_file_ids = color_file_ids(db, query).await?;

        Ok(Self {
            limit: query.limit.unwrap_or(100).clamp(1, 200) as i64,
            sort_field,
            sort_dir,
            grid_filters,
            color_file_ids,
        })
    }
}

fn build_grid_filters(query: &GridPageSlimQuery) -> Option<GridFilters> {
    let has_any = query.filters.rating_min.is_some()
        || query
            .filters
            .mime_prefixes
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
        || query
            .filters
            .search_text
            .as_ref()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
        || query.filters.collections_only == Some(true);

    if !has_any {
        return None;
    }

    Some(GridFilters {
        rating_min: query.filters.rating_min,
        mime_prefixes: query.filters.mime_prefixes.clone(),
        search_text: query.filters.search_text.clone(),
        collections_only: query.filters.collections_only,
    })
}

async fn color_file_ids(
    db: &SqliteDatabase,
    query: &GridPageSlimQuery,
) -> Result<Option<HashSet<i64>>, String> {
    let Some(ref hex) = query.filters.color_hex else {
        return Ok(None);
    };

    let hex = hex.clone();
    let tolerance = query
        .filters
        .color_accuracy
        .unwrap_or(20.0)
        .clamp(1.0, 30.0);
    let ids: Vec<i64> = db
        .with_read_conn(move |conn| color_filter_ids(conn, &hex, tolerance))
        .await?;
    Ok(Some(ids.into_iter().collect()))
}

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
