//! Canonical scope/filter SQL and paged grid reads.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rusqlite::{types::Value, Connection, OptionalExtension, ToSql};
use serde::{Deserialize, Serialize};

use super::tags::effective_tag_exists;
use crate::db::types::{
    EntityGridItem, EntityViewPage, EntityViewQuery, FilterOp, QueryFilters, ScopeKind,
    TagMatchMode,
};

pub(crate) struct EntityFilterSql {
    pub where_clause: String,
    pub params: Vec<Value>,
}

pub(crate) fn build_entity_filter(query: &EntityViewQuery) -> EntityFilterSql {
    let mut parts = vec!["1=1".to_string()];
    let mut params = Vec::new();
    apply_scope(&query.base_scope, &mut parts, &mut params);
    apply_filters(&query.filters, &mut parts, &mut params);
    EntityFilterSql {
        where_clause: parts.join(" AND "),
        params,
    }
}

pub fn folder_visible_count(conn: &Connection, folder_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM folder_member fm JOIN media_entity me ON me.entity_id = fm.entity_id WHERE fm.folder_id = ?1 AND me.status = 1",
        [folder_id], |row| row.get(0),
    )
}

const GRID_SELECT: &str = "SELECT
    me.entity_hash, me.name, mf.mime_type, mf.pixel_width, mf.pixel_height,
    me.status, me.rating, me.date_added, me.date_created, me.date_modified,
    1, mf.duration_ms, mf.frame_count, COALESCE(mf.has_audio, 0),
    mf.dominant_color_hex, COALESCE(mf.size_bytes, 0), me.entity_id, mv.viewed_at";

fn read_grid_item(row: &rusqlite::Row) -> rusqlite::Result<EntityGridItem> {
    Ok(EntityGridItem {
        entity_id: row.get(16)?,
        entity_hash: row.get(0)?,
        name: row.get(1)?,
        mime_type: row.get(2)?,
        pixel_width: row.get(3)?,
        pixel_height: row.get(4)?,
        status: row.get(5)?,
        rating: row.get(6)?,
        date_added: row.get(7)?,
        date_created: row.get(8)?,
        date_modified: row.get(9)?,
        has_thumbnail: row.get::<_, i64>(10)? != 0,
        duration_ms: row.get(11)?,
        frame_count: row.get(12)?,
        has_audio: row.get::<_, i64>(13)? != 0,
        dominant_color_hex: row.get(14)?,
        size_bytes: row.get(15)?,
    })
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct GridCursor {
    value: CursorValue,
    entity_id: i64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum CursorValue {
    Text(String),
    Integer(i64),
}

impl CursorValue {
    fn into_sql(self) -> Value {
        match self {
            Self::Text(value) => Value::Text(value),
            Self::Integer(value) => Value::Integer(value),
        }
    }
    fn from_sql(value: Value) -> Option<Self> {
        match value {
            Value::Text(value) => Some(Self::Text(value)),
            Value::Integer(value) => Some(Self::Integer(value)),
            _ => None,
        }
    }
}

fn encode_cursor(value: Value, entity_id: i64) -> Option<String> {
    let cursor = GridCursor {
        value: CursorValue::from_sql(value)?,
        entity_id,
    };
    serde_json::to_vec(&cursor)
        .ok()
        .map(|bytes| URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_cursor(cursor: &str) -> Option<GridCursor> {
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(cursor).ok()?).ok()
}

pub fn query_entity_view(
    conn: &Connection,
    query: &EntityViewQuery,
) -> rusqlite::Result<EntityViewPage> {
    let folder_scope = matches!(query.base_scope.kind, ScopeKind::Folder);
    let recent_scope = matches!(query.base_scope.kind, ScopeKind::System)
        && query.base_scope.key.as_deref() == Some("recent_viewed");
    let field = if recent_scope {
        "viewed_at"
    } else {
        &query.sort.field
    };
    let direction = if recent_scope {
        "desc"
    } else {
        &query.sort.direction
    };
    let sort_expression = if folder_scope {
        "COALESCE(fm_sort.position_rank, 0)"
    } else {
        sort_column(field)
    };

    let EntityFilterSql {
        mut where_clause,
        mut params,
    } = build_entity_filter(query);
    let folder_join = if folder_scope {
        let index = params.len() + 1;
        params.push(Value::Integer(query.base_scope.id.unwrap_or_default()));
        format!(" LEFT JOIN folder_member fm_sort ON fm_sort.entity_id = me.entity_id AND fm_sort.folder_id = ?{index}")
    } else {
        String::new()
    };

    if let Some(cursor) = query.page.cursor.as_deref().and_then(decode_cursor) {
        let op = if folder_scope || direction == "asc" {
            ">"
        } else {
            "<"
        };
        let value_index = params.len() + 1;
        let id_index = value_index + 1;
        where_clause.push_str(&format!(
            " AND ({sort_expression} {op} ?{value_index} OR ({sort_expression} = ?{value_index} AND me.entity_id > ?{id_index}))"
        ));
        params.push(cursor.value.into_sql());
        params.push(Value::Integer(cursor.entity_id));
    }

    let order = format!(
        "{sort_expression} {}, me.entity_id ASC",
        if folder_scope || direction == "asc" {
            "ASC"
        } else {
            "DESC"
        }
    );
    let limit_index = params.len() + 1;
    params.push(Value::Integer(query.page.limit));
    let data_sql = format!(
        "{GRID_SELECT}, {sort_expression} AS cursor_value FROM media_entity me
         JOIN media_file mf ON mf.file_id = me.file_id
         LEFT JOIN media_view mv ON mv.entity_id = me.entity_id
         {folder_join} WHERE {where_clause} ORDER BY {order} LIMIT ?{limit_index}"
    );
    let refs: Vec<&dyn ToSql> = params.iter().map(|value| value as &dyn ToSql).collect();
    let mut stmt = conn.prepare(&data_sql)?;
    let rows = stmt
        .query_map(refs.as_slice(), |row| {
            Ok((read_grid_item(row)?, row.get::<_, Value>(18)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let next_cursor = (rows.len() as i64 == query.page.limit)
        .then(|| {
            rows.last()
                .and_then(|(item, value)| encode_cursor(value.clone(), item.entity_id))
        })
        .flatten();
    let (total_count, total_size_bytes) = if query.page.cursor.is_none() {
        if matches!(query.base_scope.kind, ScopeKind::SmartFolder)
            && filters_are_empty(&query.filters)
        {
            let smart_folder_id = query.base_scope.id.unwrap_or_default();
            let projected = conn
                .query_row(
                    "SELECT smart_folder_count(?1), total_size_bytes
                     FROM smart_folder WHERE smart_folder_id = ?1",
                    [smart_folder_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()?;
            if let Some((count, size)) = projected {
                return Ok(EntityViewPage {
                    items: rows.into_iter().map(|(item, _)| item).collect(),
                    next_cursor,
                    total_count: Some(count),
                    total_size_bytes: Some(size),
                });
            }
        }
        let filter = build_entity_filter(query);
        let sql = format!(
            "SELECT COUNT(*), COALESCE(SUM(mf.size_bytes), 0) FROM media_entity me
             JOIN media_file mf ON mf.file_id = me.file_id
             LEFT JOIN media_view mv ON mv.entity_id = me.entity_id WHERE {}",
            filter.where_clause
        );
        let refs: Vec<&dyn ToSql> = filter
            .params
            .iter()
            .map(|value| value as &dyn ToSql)
            .collect();
        let totals = conn.query_row(&sql, refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))?;
        (Some(totals.0), Some(totals.1))
    } else {
        (None, None)
    };

    Ok(EntityViewPage {
        items: rows.into_iter().map(|(item, _)| item).collect(),
        next_cursor,
        total_count,
        total_size_bytes,
    })
}

fn filters_are_empty(filters: &QueryFilters) -> bool {
    filters.rating.is_none()
        && filters.colors.as_ref().is_none_or(Vec::is_empty)
        && filters.mime_types.as_ref().is_none_or(Vec::is_empty)
        && filters.entity_types.as_ref().is_none_or(Vec::is_empty)
        && filters.tags.as_ref().is_none_or(Vec::is_empty)
        && filters.date_created.is_none()
        && filters.date_added.is_none()
        && filters.date_modified.is_none()
        && filters.search_text.as_deref().is_none_or(str::is_empty)
}

fn bind(params: &mut Vec<Value>, value: impl Into<Value>) -> usize {
    params.push(value.into());
    params.len()
}

fn apply_scope(
    scope: &crate::db::types::BaseScope,
    parts: &mut Vec<String>,
    params: &mut Vec<Value>,
) {
    match scope.kind {
        ScopeKind::System => {
            match scope.key.as_deref().unwrap_or("all") {
                "inbox" => parts.push("me.status = 0".into()),
                "trash" => parts.push("me.status = 2".into()),
                "uncategorized" => {
                    parts.push("me.status = 1".into());
                    parts.push("NOT EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = me.entity_id)".into());
                }
                "untagged" => {
                    parts.push("me.status = 1".into());
                    parts.push("NOT EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = me.entity_id)".into());
                }
                "recent_viewed" => {
                    parts.push("me.status = 1".into());
                    parts.push("mv.entity_id IS NOT NULL".into());
                }
                _ => parts.push("me.status = 1".into()),
            }
        }
        ScopeKind::Folder => {
            parts.push("me.status = 1".into());
            let index = bind(params, scope.id.unwrap_or_default());
            parts.push(format!("EXISTS (SELECT 1 FROM folder_member fm WHERE fm.folder_id = ?{index} AND fm.entity_id = me.entity_id)"));
        }
        ScopeKind::SmartFolder => {
            parts.push("me.status = 1".into());
            let index = bind(params, scope.id.unwrap_or_default());
            parts.push(format!("smart_folder_contains(?{index}, me.entity_id) = 1"));
        }
        ScopeKind::Tag => {
            parts.push("me.status = 1".into());
            let index = bind(params, scope.key.clone().unwrap_or_default());
            parts.push(effective_tag_exists("me.entity_id", index));
        }
        ScopeKind::Search => {
            parts.push("me.status = 1".into());
            if let Some(search) = scope.key.as_deref().filter(|value| !value.is_empty()) {
                let index = bind(params, format!("{search}*"));
                parts.push(format!("me.entity_id IN (SELECT rowid FROM entity_fts WHERE entity_fts MATCH ?{index})"));
            }
        }
    }
}

fn apply_filters(filters: &QueryFilters, parts: &mut Vec<String>, params: &mut Vec<Value>) {
    if let Some(rating) = &filters.rating {
        let op = match rating.op {
            FilterOp::Eq => "=",
            FilterOp::Gte => ">=",
            FilterOp::Lte => "<=",
            FilterOp::Gt => ">",
            FilterOp::Lt => "<",
        };
        let index = bind(params, rating.value);
        parts.push(format!("me.rating {op} ?{index}"));
    }
    if let Some(values) = filters
        .mime_types
        .as_ref()
        .filter(|values| !values.is_empty())
    {
        let placeholders = values
            .iter()
            .map(|value| format!("?{}", bind(params, value.clone())))
            .collect::<Vec<_>>();
        parts.push(format!("mf.mime_type IN ({})", placeholders.join(",")));
    }
    if let Some(types) = &filters.entity_types {
        let values = types
            .iter()
            .filter(|value| matches!(value.as_str(), "image" | "video" | "audio"))
            .map(|value| format!("mf.mime_type LIKE ?{}", bind(params, format!("{value}/%"))))
            .collect::<Vec<_>>();
        if !values.is_empty() {
            parts.push(format!("({})", values.join(" OR ")));
        }
    }
    if let Some(tags) = &filters.tags {
        for tag in tags {
            let index = bind(params, tag.tag.clone());
            let exists = effective_tag_exists("me.entity_id", index);
            parts.push(if matches!(tag.match_mode, TagMatchMode::Exclude) {
                format!("NOT {exists}")
            } else {
                exists
            });
        }
    }
    apply_date_filter("me.date_created", &filters.date_created, parts, params);
    apply_date_filter("me.date_added", &filters.date_added, parts, params);
    apply_date_filter("me.date_modified", &filters.date_modified, parts, params);
    if let Some(text) = filters
        .search_text
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        let index = bind(params, format!("%{text}%"));
        parts.push(format!("(me.name LIKE ?{index} OR me.notes LIKE ?{index})"));
    }
    if let Some(colors) = filters.colors.as_ref().filter(|values| !values.is_empty()) {
        let placeholders = colors
            .iter()
            .map(|value| format!("?{}", bind(params, value.clone())))
            .collect::<Vec<_>>();
        parts.push(format!(
            "EXISTS (SELECT 1 FROM file_color fc WHERE fc.file_id = mf.file_id AND fc.hex IN ({}))",
            placeholders.join(",")
        ));
    }
}

fn apply_date_filter(
    column: &str,
    range: &Option<crate::db::types::DateRange>,
    parts: &mut Vec<String>,
    params: &mut Vec<Value>,
) {
    if let Some(range) = range {
        if let Some(from) = &range.from {
            let index = bind(params, from.clone());
            parts.push(format!("{column} >= ?{index}"));
        }
        if let Some(to) = &range.to {
            let index = bind(params, to.clone());
            parts.push(format!("{column} <= ?{index}"));
        }
    }
}

fn sort_column(field: &str) -> &str {
    match field {
        "viewed_at" => "COALESCE(mv.viewed_at, '')",
        "date_added" => "COALESCE(me.date_added, '')",
        "date_created" => "COALESCE(me.date_created, '')",
        "date_modified" => "COALESCE(me.date_modified, '')",
        "rating" => "COALESCE(me.rating, -1)",
        "size_bytes" => "COALESCE(mf.size_bytes, 0)",
        "duration" | "duration_ms" => "COALESCE(mf.duration_ms, -1)",
        "name" => "COALESCE(me.name, '')",
        _ => "COALESCE(me.date_added, '')",
    }
}

pub fn recently_viewed_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM media_view mv JOIN media_entity me ON me.entity_id = mv.entity_id WHERE me.status = 1", [], |row| row.get(0))
}

pub fn get_entity_grid_items_by_hash(
    conn: &Connection,
    hashes: &[String],
) -> rusqlite::Result<Vec<EntityGridItem>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=hashes.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>();
    let sql = format!("{GRID_SELECT} FROM media_entity me JOIN media_file mf ON mf.file_id = me.file_id LEFT JOIN media_view mv ON mv.entity_id = me.entity_id WHERE me.entity_hash IN ({})", placeholders.join(","));
    let params: Vec<&dyn ToSql> = hashes.iter().map(|hash| hash as &dyn ToSql).collect();
    conn.prepare(&sql)?
        .query_map(params.as_slice(), read_grid_item)?
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_round_trips_typed_text_and_integer_values() {
        for (value, entity_id) in [
            (Value::Text("name|with|separators".into()), 41),
            (Value::Integer(-1), 42),
            (Value::Integer(9_223_372_036_854_775_000), 43),
        ] {
            let encoded = encode_cursor(value.clone(), entity_id).expect("encode cursor");
            let expected = GridCursor {
                value: CursorValue::from_sql(value).expect("supported value"),
                entity_id,
            };
            assert_eq!(decode_cursor(&encoded), Some(expected));
        }
    }

    #[test]
    fn every_supported_sort_uses_a_normalized_cursor_expression() {
        for field in [
            "name",
            "rating",
            "size_bytes",
            "duration",
            "duration_ms",
            "date_added",
            "date_created",
            "date_modified",
            "viewed_at",
        ] {
            assert!(sort_column(field).starts_with("COALESCE("), "{field}");
        }
    }
}
