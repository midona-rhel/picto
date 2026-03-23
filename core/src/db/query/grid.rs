//! Grid page queries — returns EntityGridItem rows.
//! Implements query-time grouping: if any member of a collection matches
//! a scope, the collection appears once in the result set.

use rusqlite::{params, Connection};

use crate::db::types::EntityKind;

/// A single grid tile payload.
#[derive(Debug, Clone, serde::Serialize)]
pub struct EntityGridItem {
    pub entity_hash: String,
    pub entity_kind: EntityKind,
    pub name: Option<String>,
    pub mime_type: String,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub status: i64,
    pub rating: Option<i64>,
    pub date_added: String,
    pub date_created: String,
    pub date_modified: String,
    pub has_thumbnail: bool,
    pub member_count: Option<i64>,
    pub duration_ms: Option<i64>,
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub dominant_color_hex: Option<String>,
    pub size_bytes: i64,
}

/// A page of grid results.
#[derive(Debug, serde::Serialize)]
pub struct EntityViewPage {
    pub items: Vec<EntityGridItem>,
    pub next_cursor: Option<String>,
    pub total_count: Option<i64>,
}

/// Shared SELECT columns for grid queries. Joins through single_media_entity
/// to media_file for singles, and uses collection aggregates for collections.
///
/// Query-time grouping: collection members with parent_collection_entity_id
/// are excluded from top-level results. If a scope filter matches any member,
/// the parent collection appears instead.
const GRID_ITEM_SELECT: &str =
    "SELECT
        me.entity_hash,
        me.entity_kind,
        me.name,
        COALESCE(mf.mime_type, 'application/x-collection') AS mime_type,
        COALESCE(mf.pixel_width, pmf.pixel_width) AS pixel_width,
        COALESCE(mf.pixel_height, pmf.pixel_height) AS pixel_height,
        me.status,
        me.rating,
        me.date_added,
        me.date_created,
        me.date_modified,
        1 AS has_thumbnail,
        me.member_count,
        COALESCE(mf.duration_ms, pmf.duration_ms) AS duration_ms,
        COALESCE(mf.frame_count, pmf.frame_count) AS frame_count,
        COALESCE(mf.has_audio, pmf.has_audio, 0) AS has_audio,
        COALESCE(mf.dominant_color_hex, pmf.dominant_color_hex) AS dominant_color_hex,
        COALESCE(mf.size_bytes, me.total_size_bytes, 0) AS size_bytes
     FROM media_entity me
     LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
     LEFT JOIN media_file mf ON mf.file_id = sme.file_id
     LEFT JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
     LEFT JOIN single_media_entity psme ON psme.entity_id = pm.entity_id
     LEFT JOIN media_file pmf ON pmf.file_id = psme.file_id";

fn read_grid_item(row: &rusqlite::Row) -> rusqlite::Result<EntityGridItem> {
    Ok(EntityGridItem {
        entity_hash: row.get(0)?,
        entity_kind: match row.get::<_, String>(1)?.as_str() {
            "collection" => EntityKind::Collection,
            _ => EntityKind::Single,
        },
        name: row.get(2)?,
        mime_type: row.get(3)?,
        pixel_width: row.get(4)?,
        pixel_height: row.get(5)?,
        status: row.get(6)?,
        rating: row.get(7)?,
        date_added: row.get(8)?,
        date_created: row.get(9)?,
        date_modified: row.get(10)?,
        has_thumbnail: row.get::<_, i64>(11)? != 0,
        member_count: row.get(12)?,
        duration_ms: row.get(13)?,
        frame_count: row.get(14)?,
        has_audio: row.get::<_, i64>(15)? != 0,
        dominant_color_hex: row.get(16)?,
        size_bytes: row.get::<_, i64>(17).unwrap_or(0),
    })
}

/// Query grid items for the "all active" system scope.
/// Top-level only: excludes collection members (parent_collection_entity_id IS NOT NULL).
pub fn query_all_active(
    conn: &Connection,
    limit: i64,
    offset: i64,
    sort_field: &str,
    sort_dir: &str,
) -> rusqlite::Result<EntityViewPage> {
    let order = validated_sort(sort_field, sort_dir);
    let sql = format!(
        "{GRID_ITEM_SELECT}
         WHERE me.status = 1
           AND me.parent_collection_entity_id IS NULL
         ORDER BY {order}
         LIMIT ?1 OFFSET ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items: Vec<EntityGridItem> = stmt
        .query_map(params![limit, offset], read_grid_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity WHERE status = 1 AND parent_collection_entity_id IS NULL",
        [],
        |row| row.get(0),
    )?;

    Ok(EntityViewPage {
        next_cursor: if items.len() as i64 == limit { Some((offset + limit).to_string()) } else { None },
        items,
        total_count: Some(total),
    })
}

/// Query grid items for inbox scope.
pub fn query_inbox(
    conn: &Connection,
    limit: i64,
    offset: i64,
    sort_field: &str,
    sort_dir: &str,
) -> rusqlite::Result<EntityViewPage> {
    let order = validated_sort(sort_field, sort_dir);
    let sql = format!(
        "{GRID_ITEM_SELECT}
         WHERE me.status = 0
           AND me.parent_collection_entity_id IS NULL
         ORDER BY {order}
         LIMIT ?1 OFFSET ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items: Vec<EntityGridItem> = stmt
        .query_map(params![limit, offset], read_grid_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity WHERE status = 0 AND parent_collection_entity_id IS NULL",
        [],
        |row| row.get(0),
    )?;

    Ok(EntityViewPage {
        next_cursor: if items.len() as i64 == limit { Some((offset + limit).to_string()) } else { None },
        items,
        total_count: Some(total),
    })
}

/// Query grid items for trash scope.
pub fn query_trash(
    conn: &Connection,
    limit: i64,
    offset: i64,
    sort_field: &str,
    sort_dir: &str,
) -> rusqlite::Result<EntityViewPage> {
    let order = validated_sort(sort_field, sort_dir);
    let sql = format!(
        "{GRID_ITEM_SELECT}
         WHERE me.status = 2
           AND me.parent_collection_entity_id IS NULL
         ORDER BY {order}
         LIMIT ?1 OFFSET ?2"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items: Vec<EntityGridItem> = stmt
        .query_map(params![limit, offset], read_grid_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity WHERE status = 2 AND parent_collection_entity_id IS NULL",
        [],
        |row| row.get(0),
    )?;

    Ok(EntityViewPage {
        next_cursor: if items.len() as i64 == limit { Some((offset + limit).to_string()) } else { None },
        items,
        total_count: Some(total),
    })
}

/// Query grid items for a folder scope with collection grouping.
/// Members in the folder are grouped: if any member of a collection is in the folder,
/// the collection entity appears once in the result.
pub fn query_folder(
    conn: &Connection,
    folder_id: i64,
    limit: i64,
    offset: i64,
    sort_field: &str,
    sort_dir: &str,
) -> rusqlite::Result<EntityViewPage> {
    let order = validated_sort(sort_field, sort_dir);
    // Grouped query: select top-level entities whose entity_id is in folder_member,
    // OR whose parent collection has at least one member in folder_member.
    let sql = format!(
        "{GRID_ITEM_SELECT}
         WHERE me.parent_collection_entity_id IS NULL
           AND me.status = 1
           AND (
               EXISTS (SELECT 1 FROM folder_member fm WHERE fm.folder_id = ?1 AND fm.entity_id = me.entity_id)
               OR (me.entity_kind = 'collection' AND EXISTS (
                   SELECT 1 FROM media_entity child
                   JOIN folder_member fm ON fm.entity_id = child.entity_id
                   WHERE child.parent_collection_entity_id = me.entity_id AND fm.folder_id = ?1
               ))
           )
         ORDER BY {order}
         LIMIT ?2 OFFSET ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items: Vec<EntityGridItem> = stmt
        .query_map(params![folder_id, limit, offset], read_grid_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity me
         WHERE me.parent_collection_entity_id IS NULL
           AND me.status = 1
           AND (
               EXISTS (SELECT 1 FROM folder_member fm WHERE fm.folder_id = ?1 AND fm.entity_id = me.entity_id)
               OR (me.entity_kind = 'collection' AND EXISTS (
                   SELECT 1 FROM media_entity child
                   JOIN folder_member fm ON fm.entity_id = child.entity_id
                   WHERE child.parent_collection_entity_id = me.entity_id AND fm.folder_id = ?1
               ))
           )",
        [folder_id],
        |row| row.get(0),
    )?;

    Ok(EntityViewPage {
        next_cursor: if items.len() as i64 == limit { Some((offset + limit).to_string()) } else { None },
        items,
        total_count: Some(total),
    })
}

/// Query collection members (for viewing inside a collection).
pub fn query_collection_members(
    conn: &Connection,
    collection_entity_id: i64,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<EntityViewPage> {
    let sql = format!(
        "{GRID_ITEM_SELECT}
         WHERE me.parent_collection_entity_id = ?1
         ORDER BY me.collection_ordinal ASC
         LIMIT ?2 OFFSET ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let items: Vec<EntityGridItem> = stmt
        .query_map(params![collection_entity_id, limit, offset], read_grid_item)?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM media_entity WHERE parent_collection_entity_id = ?1",
        [collection_entity_id],
        |row| row.get(0),
    )?;

    Ok(EntityViewPage {
        next_cursor: if items.len() as i64 == limit { Some((offset + limit).to_string()) } else { None },
        items,
        total_count: Some(total),
    })
}

fn validated_sort(field: &str, dir: &str) -> String {
    let col = match field {
        "date_added" => "me.date_added",
        "date_created" => "me.date_created",
        "date_modified" => "me.date_modified",
        "rating" => "me.rating",
        "size_bytes" => "size_bytes",
        "name" => "me.name",
        _ => "me.date_added",
    };
    let direction = if dir == "asc" { "ASC" } else { "DESC" };
    format!("{col} {direction}")
}
