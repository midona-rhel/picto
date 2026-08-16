//! Grid page queries over independent media entities.

use rusqlite::{Connection, ToSql};

use crate::db::types::{
    EntityGridItem, EntityViewPage, EntityViewQuery, FilterOp, QueryFilters, ScopeKind,
    TagMatchMode,
};

use super::tags::effective_tag_exists;

fn folder_scope_membership(parameter_index: usize) -> String {
    format!(
        "EXISTS (SELECT 1 FROM folder_member fm WHERE fm.folder_id = ?{parameter_index} AND fm.entity_id = me.entity_id)"
    )
}

pub fn folder_visible_count(conn: &Connection, folder_id: i64) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM folder_member fm
         JOIN media_entity me ON me.entity_id = fm.entity_id
         WHERE fm.folder_id = ?1 AND me.status = 1",
        [folder_id],
        |row| row.get(0),
    )
}

// Columns: hash, name, mime, width, height, status, rating, dates (3),
// thumbnail flag, duration, frame count, audio, dominant color, size, id, viewed_at.
const GRID_SELECT: &str = "SELECT
        me.entity_hash,
        me.name,
        mf.mime_type,
        mf.pixel_width,
        mf.pixel_height,
        me.status,
        me.rating,
        me.date_added,
        me.date_created,
        me.date_modified,
        1 AS has_thumbnail,
        mf.duration_ms,
        mf.frame_count,
        COALESCE(mf.has_audio, 0),
        mf.dominant_color_hex,
        COALESCE(mf.size_bytes, 0),
        me.entity_id,
        mv.viewed_at
     FROM media_entity me
     JOIN media_file mf ON mf.file_id = me.file_id
     LEFT JOIN media_view mv ON mv.entity_id = me.entity_id";

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

pub fn query_entity_view(
    conn: &Connection,
    q: &EntityViewQuery,
    preresolved_ids: Option<&[i64]>,
) -> rusqlite::Result<EntityViewPage> {
    let is_folder_scope = matches!(q.base_scope.kind, ScopeKind::Folder);
    let is_recently_viewed_scope = matches!(q.base_scope.kind, ScopeKind::System)
        && q.base_scope.key.as_deref() == Some("recent_viewed");
    let sort_field = if is_recently_viewed_scope {
        "viewed_at"
    } else {
        &q.sort.field
    };
    let sort_direction = if is_recently_viewed_scope {
        "desc"
    } else {
        &q.sort.direction
    };

    let mut where_parts = vec!["1=1".to_string()];
    let mut bound: Vec<Box<dyn ToSql>> = Vec::new();
    apply_scope(&q.base_scope, &mut where_parts, &mut bound, preresolved_ids);
    apply_filters(&q.filters, &mut where_parts, &mut bound);

    if !is_folder_scope {
        if let Some((cursor_value, cursor_id)) = q
            .page
            .cursor
            .as_deref()
            .and_then(|cursor| parse_cursor(conn, cursor))
        {
            let op = if sort_direction == "asc" { ">" } else { "<" };
            let first = bound.len() + 1;
            let second = first + 1;
            let column = sort_column(sort_field);
            where_parts.push(format!(
                "({column} {op} ?{first} OR ({column} = ?{first} AND me.entity_id > ?{second}))"
            ));
            bound.push(Box::new(cursor_value));
            bound.push(Box::new(cursor_id));
        }
    }

    let folder_join = if is_folder_scope {
        let index = bound.len() + 1;
        bound.push(Box::new(q.base_scope.id.unwrap_or_default()));
        format!(
            " LEFT JOIN folder_member fm_sort ON fm_sort.entity_id = me.entity_id AND fm_sort.folder_id = ?{index}"
        )
    } else {
        String::new()
    };
    let order = if is_folder_scope {
        "fm_sort.position_rank ASC, me.entity_id ASC".to_string()
    } else {
        format!(
            "{}, me.entity_id ASC",
            validated_sort(sort_field, sort_direction)
        )
    };
    let limit_index = bound.len() + 1;
    bound.push(Box::new(q.page.limit));
    let where_clause = where_parts.join(" AND ");

    let mut count_where = vec!["1=1".to_string()];
    let mut count_bound: Vec<Box<dyn ToSql>> = Vec::new();
    apply_scope(
        &q.base_scope,
        &mut count_where,
        &mut count_bound,
        preresolved_ids,
    );
    apply_filters(&q.filters, &mut count_where, &mut count_bound);
    let count_sql = format!(
        "SELECT COUNT(*), COALESCE(SUM(mf.size_bytes), 0)
         FROM media_entity me
         JOIN media_file mf ON mf.file_id = me.file_id
         LEFT JOIN media_view mv ON mv.entity_id = me.entity_id
         WHERE {}",
        count_where.join(" AND ")
    );
    let count_refs: Vec<&dyn ToSql> = count_bound.iter().map(|value| value.as_ref()).collect();
    let (total_count, total_size_bytes) =
        conn.query_row(&count_sql, count_refs.as_slice(), |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;

    let data_sql = format!(
        "{GRID_SELECT}{folder_join} WHERE {where_clause} ORDER BY {order} LIMIT ?{limit_index}"
    );
    let refs: Vec<&dyn ToSql> = bound.iter().map(|value| value.as_ref()).collect();
    let mut stmt = conn.prepare(&data_sql)?;
    let rows: Vec<(EntityGridItem, Option<String>)> = stmt
        .query_map(refs.as_slice(), |row| {
            Ok((read_grid_item(row)?, row.get(17)?))
        })?
        .collect::<rusqlite::Result<_>>()?;

    let next_cursor = if rows.len() as i64 == q.page.limit && !is_folder_scope {
        rows.last().map(|(item, viewed_at)| {
            let value = match sort_field {
                "viewed_at" => viewed_at.as_deref().unwrap_or(""),
                "date_added" => item.date_added.as_str(),
                "date_created" => item.date_created.as_str(),
                "date_modified" => item.date_modified.as_str(),
                "name" => item.name.as_deref().unwrap_or(""),
                _ => item.date_added.as_str(),
            };
            format!("{value}|{}", item.entity_hash)
        })
    } else {
        None
    };

    Ok(EntityViewPage {
        items: rows.into_iter().map(|(item, _)| item).collect(),
        next_cursor,
        total_count: Some(total_count),
        total_size_bytes: Some(total_size_bytes),
    })
}

fn apply_scope(
    scope: &crate::db::types::BaseScope,
    parts: &mut Vec<String>,
    bound: &mut Vec<Box<dyn ToSql>>,
    preresolved_ids: Option<&[i64]>,
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
            let index = bound.len() + 1;
            parts.push("me.status = 1".into());
            parts.push(folder_scope_membership(index));
            bound.push(Box::new(scope.id.unwrap_or_default()));
        }
        ScopeKind::SmartFolder => {
            parts.push("me.status = 1".into());
            match preresolved_ids {
                Some(ids) if !ids.is_empty() => parts.push(format!(
                    "me.entity_id IN ({})",
                    ids.iter().map(i64::to_string).collect::<Vec<_>>().join(",")
                )),
                _ => parts.push("1=0".into()),
            }
        }
        ScopeKind::Tag => {
            let index = bound.len() + 1;
            parts.push("me.status = 1".into());
            parts.push(effective_tag_exists("me.entity_id", index));
            bound.push(Box::new(scope.key.clone().unwrap_or_default()));
        }
        ScopeKind::Search => {
            parts.push("me.status = 1".into());
            let search = scope.key.as_deref().unwrap_or("");
            if !search.is_empty() {
                let index = bound.len() + 1;
                parts.push(format!(
                    "me.entity_id IN (SELECT rowid FROM entity_fts WHERE entity_fts MATCH ?{index})"
                ));
                bound.push(Box::new(format!("{search}*")));
            }
        }
    }
}

fn apply_filters(filters: &QueryFilters, parts: &mut Vec<String>, bound: &mut Vec<Box<dyn ToSql>>) {
    if let Some(rating) = &filters.rating {
        let index = bound.len() + 1;
        let op = match rating.op {
            FilterOp::Eq => "=",
            FilterOp::Gte => ">=",
            FilterOp::Lte => "<=",
            FilterOp::Gt => ">",
            FilterOp::Lt => "<",
        };
        parts.push(format!("me.rating {op} ?{index}"));
        bound.push(Box::new(rating.value));
    }

    if let Some(mimes) = &filters.mime_types {
        if !mimes.is_empty() {
            let placeholders = (0..mimes.len())
                .map(|offset| format!("?{}", bound.len() + offset + 1))
                .collect::<Vec<_>>();
            parts.push(format!("mf.mime_type IN ({})", placeholders.join(",")));
            bound.extend(
                mimes
                    .iter()
                    .cloned()
                    .map(|mime| Box::new(mime) as Box<dyn ToSql>),
            );
        }
    }

    if let Some(types) = &filters.entity_types {
        let mut type_parts = Vec::new();
        for media_type in types {
            let index = bound.len() + 1;
            match media_type.as_str() {
                "image" | "video" | "audio" => {
                    type_parts.push(format!("mf.mime_type LIKE ?{index}"));
                    bound.push(Box::new(format!("{media_type}/%")));
                }
                _ => {}
            }
        }
        if !type_parts.is_empty() {
            parts.push(format!("({})", type_parts.join(" OR ")));
        }
    }

    if let Some(tags) = &filters.tags {
        for tag in tags {
            let index = bound.len() + 1;
            let exists = effective_tag_exists("me.entity_id", index);
            match tag.match_mode {
                TagMatchMode::Include => parts.push(exists),
                TagMatchMode::Exclude => parts.push(format!("NOT {exists}")),
            }
            bound.push(Box::new(tag.tag.clone()));
        }
    }

    apply_date_filter("me.date_created", &filters.date_created, parts, bound);
    apply_date_filter("me.date_added", &filters.date_added, parts, bound);
    apply_date_filter("me.date_modified", &filters.date_modified, parts, bound);

    if let Some(text) = &filters.search_text {
        if !text.is_empty() {
            let index = bound.len() + 1;
            parts.push(format!("(me.name LIKE ?{index} OR me.notes LIKE ?{index})"));
            bound.push(Box::new(format!("%{text}%")));
        }
    }

    if let Some(colors) = &filters.colors {
        if !colors.is_empty() {
            let placeholders = (0..colors.len())
                .map(|offset| format!("?{}", bound.len() + offset + 1))
                .collect::<Vec<_>>();
            parts.push(format!(
                "EXISTS (SELECT 1 FROM file_color fc WHERE fc.file_id = mf.file_id AND fc.hex IN ({}))",
                placeholders.join(",")
            ));
            bound.extend(
                colors
                    .iter()
                    .cloned()
                    .map(|color| Box::new(color) as Box<dyn ToSql>),
            );
        }
    }
}

fn apply_date_filter(
    column: &str,
    range: &Option<crate::db::types::DateRange>,
    parts: &mut Vec<String>,
    bound: &mut Vec<Box<dyn ToSql>>,
) {
    if let Some(range) = range {
        if let Some(from) = &range.from {
            let index = bound.len() + 1;
            parts.push(format!("{column} >= ?{index}"));
            bound.push(Box::new(from.clone()));
        }
        if let Some(to) = &range.to {
            let index = bound.len() + 1;
            parts.push(format!("{column} <= ?{index}"));
            bound.push(Box::new(to.clone()));
        }
    }
}

fn sort_column(field: &str) -> &str {
    match field {
        "viewed_at" => "mv.viewed_at",
        "date_added" => "me.date_added",
        "date_created" => "me.date_created",
        "date_modified" => "me.date_modified",
        "rating" => "me.rating",
        "size_bytes" => "mf.size_bytes",
        "name" => "me.name",
        _ => "me.date_added",
    }
}

fn validated_sort(field: &str, direction: &str) -> String {
    let direction = if direction == "asc" { "ASC" } else { "DESC" };
    format!("{} {direction}", sort_column(field))
}

pub fn recently_viewed_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*)
         FROM media_view mv
         JOIN media_entity me ON me.entity_id = mv.entity_id
         WHERE me.status = 1",
        [],
        |row| row.get(0),
    )
}

fn parse_cursor(conn: &Connection, cursor: &str) -> Option<(String, i64)> {
    let split = cursor.rfind('|')?;
    let sort_value = cursor[..split].to_string();
    let hash = &cursor[split + 1..];
    let entity_id = conn
        .query_row(
            "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
            [hash],
            |row| row.get(0),
        )
        .ok()?;
    Some((sort_value, entity_id))
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
    let sql = format!(
        "{GRID_SELECT} WHERE me.entity_hash IN ({})",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn ToSql> = hashes.iter().map(|hash| hash as &dyn ToSql).collect();
    let rows = stmt.query_map(params.as_slice(), read_grid_item)?;
    rows.collect()
}
