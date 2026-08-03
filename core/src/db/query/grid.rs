//! Grid page queries — returns EntityGridItem rows via the EntityViewQuery model.
//! All user-controlled values are bound as parameters, never inlined into SQL.
//! Implements query-time grouping for collection visibility.

use base64::Engine;
use rusqlite::{Connection, ToSql};

use crate::db::types::{
    EntityGridItem, EntityKind, EntityViewPage, EntityViewQuery, FilterOp, QueryFilters, ScopeKind,
    TagMatchMode,
};

fn folder_scope_membership(parameter_index: usize) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM folder_member fm WHERE fm.folder_id = ?{parameter_index} AND fm.entity_id = me.entity_id) \
         OR (me.entity_kind = 'collection' AND EXISTS (\
             SELECT 1 FROM media_entity child \
             JOIN folder_member fm ON fm.entity_id = child.entity_id \
             WHERE child.parent_collection_entity_id = me.entity_id AND fm.folder_id = ?{parameter_index})))"
    )
}

/// Count the same active top-level entities rendered by an unfiltered folder grid.
pub fn folder_visible_count(conn: &Connection, folder_id: i64) -> rusqlite::Result<i64> {
    let sql = format!(
        "SELECT COUNT(*)
         FROM media_entity me
         WHERE me.parent_collection_entity_id IS NULL
           AND me.status = 1
           AND {}",
        folder_scope_membership(1)
    );
    conn.query_row(&sql, [folder_id], |row| row.get(0))
}

/// Base SELECT for grid items. Also selects me.entity_id at position 18
/// (not exposed in EntityGridItem but used for cursor construction).
const GRID_SELECT: &str = "SELECT
        me.entity_hash,
        me.entity_kind,
        me.name,
        COALESCE(mf.mime_type, pmf.mime_type, 'application/x-collection') AS mime_type,
        COALESCE(mf.pixel_width, pmf.pixel_width) AS pixel_width,
        COALESCE(mf.pixel_height, pmf.pixel_height) AS pixel_height,
        me.status,
        CASE WHEN me.entity_kind = 'collection' THEN (
            SELECT MAX(child.rating)
            FROM media_entity child
            WHERE child.parent_collection_entity_id = me.entity_id
        ) ELSE me.rating END AS rating,
        me.date_added,
        me.date_created,
        me.date_modified,
        1 AS has_thumbnail,
        me.member_count,
        COALESCE(mf.duration_ms, pmf.duration_ms) AS duration_ms,
        COALESCE(mf.frame_count, pmf.frame_count) AS frame_count,
        COALESCE(mf.has_audio, pmf.has_audio, 0) AS has_audio,
        COALESCE(mf.dominant_color_hex, pmf.dominant_color_hex) AS dominant_color_hex,
        COALESCE(mf.size_bytes, me.total_size_bytes, 0) AS size_bytes,
        me.entity_id,
        COALESCE(pm.entity_hash, me.entity_hash) AS thumbnail_hash
     FROM media_entity me
     LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
     LEFT JOIN media_file mf ON mf.file_id = sme.file_id
     LEFT JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
     LEFT JOIN single_media_entity psme ON psme.entity_id = pm.entity_id
     LEFT JOIN media_file pmf ON pmf.file_id = psme.file_id";

/// Reads an EntityGridItem from a row. entity_id at column 18 and
/// thumbnail_hash at column 19 are read separately by the caller / here.
fn read_grid_item(row: &rusqlite::Row) -> rusqlite::Result<EntityGridItem> {
    let entity_hash: String = row.get(0)?;
    let thumbnail_hash: String = row.get(19).unwrap_or_else(|_| entity_hash.clone());
    Ok(EntityGridItem {
        entity_id: row.get(18)?,
        entity_hash,
        thumbnail_hash,
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

/// Single entry point for all grid queries. All values are parameterized.
///
/// `preresolved_ids` carries pre-resolved entity_id sets for bitmap-backed
/// scopes (SmartFolder). Scopes that need DB access (Similar) resolve inline.
pub fn query_entity_view(
    conn: &Connection,
    q: &EntityViewQuery,
    preresolved_ids: Option<&[i64]>,
) -> rusqlite::Result<EntityViewPage> {
    let limit = q.page.limit;
    // Folder and collection scopes use custom manual order instead of the user's sort preference
    let is_folder_scope = matches!(q.base_scope.kind, ScopeKind::Folder);
    let is_collection_scope = matches!(q.base_scope.kind, ScopeKind::Collection);
    let order = if is_folder_scope {
        "fm_sort.position_rank ASC".to_string()
    } else if is_collection_scope {
        "COALESCE(me.collection_ordinal, 9223372036854775807) ASC, me.entity_id ASC".to_string()
    } else {
        validated_sort(&q.sort.field, &q.sort.direction)
    };

    // Collect all WHERE fragments and their bound parameter values
    let mut where_parts: Vec<String> = vec!["1=1".into()];
    let mut bound: Vec<Box<dyn ToSql>> = Vec::new();

    // Top-level filter (exclude collection members except in collection scope)
    if !matches!(q.base_scope.kind, ScopeKind::Collection) {
        where_parts.push("me.parent_collection_entity_id IS NULL".into());
    }

    // Scope
    apply_scope(
        conn,
        &q.base_scope,
        &mut where_parts,
        &mut bound,
        preresolved_ids,
    );

    // Filters
    apply_filters(&q.filters, &mut where_parts, &mut bound);

    // Cursor: opaque value encoding (sort_field_value|entity_hash)
    // parse_cursor resolves entity_hash → entity_id for stable tie-breaking
    // Folder/collection scopes use custom ordering — cursor not supported (views are bounded)
    if !is_folder_scope && !is_collection_scope {
        let cursor_parsed = q.page.cursor.as_deref().and_then(|c| parse_cursor(conn, c));
        if let Some((ref cursor_val, cursor_id)) = cursor_parsed {
            let dir_op = if q.sort.direction == "asc" { ">" } else { "<" };
            let p_idx = bound.len() + 1;
            let p_idx2 = p_idx + 1;
            where_parts.push(format!(
                "({} {dir_op} ?{p_idx} OR ({} = ?{p_idx} AND me.entity_id > ?{p_idx2}))",
                sort_column(&q.sort.field),
                sort_column(&q.sort.field),
            ));
            bound.push(Box::new(cursor_val.clone()));
            bound.push(Box::new(cursor_id));
        }
    }

    let where_clause = where_parts.join(" AND ");

    // Folder sort join param (must be before limit)
    let folder_sort_join = if is_folder_scope {
        let fid_idx = bound.len() + 1;
        bound.push(Box::new(q.base_scope.id.unwrap_or(0)));
        format!(" LEFT JOIN folder_member fm_sort ON fm_sort.entity_id = me.entity_id AND fm_sort.folder_id = ?{fid_idx}")
    } else {
        String::new()
    };

    // Limit param (always last)
    let limit_idx = bound.len() + 1;
    bound.push(Box::new(limit));

    // Aggregates (without cursor/limit) — reuses the same scope and filter logic
    let (total_count, total_size_bytes) = {
        let mut cw: Vec<String> = vec!["1=1".into()];
        if !matches!(q.base_scope.kind, ScopeKind::Collection) {
            cw.push("me.parent_collection_entity_id IS NULL".into());
        }
        let mut count_bound: Vec<Box<dyn ToSql>> = Vec::new();
        apply_scope(
            conn,
            &q.base_scope,
            &mut cw,
            &mut count_bound,
            preresolved_ids,
        );
        apply_filters(&q.filters, &mut cw, &mut count_bound);
        let count_sql = format!(
            "SELECT
                COUNT(*),
                COALESCE(SUM(COALESCE(mf.size_bytes, me.total_size_bytes, 0)), 0)
             FROM media_entity me
             LEFT JOIN single_media_entity sme ON sme.entity_id = me.entity_id
             LEFT JOIN media_file mf ON mf.file_id = sme.file_id
             LEFT JOIN media_entity pm ON pm.entity_id = me.primary_member_entity_id
             LEFT JOIN single_media_entity psme ON psme.entity_id = pm.entity_id
             LEFT JOIN media_file pmf ON pmf.file_id = psme.file_id
             WHERE {}",
            cw.join(" AND ")
        );
        let refs: Vec<&dyn ToSql> = count_bound.iter().map(|b| b.as_ref()).collect();
        conn.query_row(&count_sql, refs.as_slice(), |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })?
    };

    // Data query — reads both EntityGridItem and entity_id per row
    let data_sql = format!(
        "{GRID_SELECT}{folder_sort_join} WHERE {where_clause} ORDER BY {order}, me.entity_id ASC LIMIT ?{limit_idx}"
    );
    let refs: Vec<&dyn ToSql> = bound.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn.prepare(&data_sql)?;
    let rows: Vec<(EntityGridItem, i64)> = stmt
        .query_map(refs.as_slice(), |row| {
            let item = read_grid_item(row)?;
            let entity_id: i64 = row.get(18)?;
            Ok((item, entity_id))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Build next cursor from the last row's sort value + entity_id
    let next_cursor = if rows.len() as i64 == limit {
        rows.last().map(|(item, _entity_id)| {
            let sort_val = match q.sort.field.as_str() {
                "date_added" => &item.date_added,
                "date_created" => &item.date_created,
                "date_modified" => &item.date_modified,
                "name" => item.name.as_deref().unwrap_or(""),
                _ => &item.date_added,
            };
            // Encode as sort_value|entity_hash (entity_hash is the public handle;
            // parse_cursor resolves it back to entity_id on the next page request)
            format!("{}|{}", sort_val, item.entity_hash)
        })
    } else {
        None
    };

    let items: Vec<EntityGridItem> = rows.into_iter().map(|(item, _)| item).collect();

    Ok(EntityViewPage {
        items,
        next_cursor,
        total_count: Some(total_count),
        total_size_bytes: Some(total_size_bytes),
    })
}

fn apply_scope(
    conn: &Connection,
    scope: &crate::db::types::BaseScope,
    parts: &mut Vec<String>,
    bound: &mut Vec<Box<dyn ToSql>>,
    preresolved_ids: Option<&[i64]>,
) {
    match &scope.kind {
        ScopeKind::System => {
            let key = scope.key.as_deref().unwrap_or("all");
            match key {
                "all" => {
                    parts.push("me.status = 1".into());
                }
                "inbox" => {
                    parts.push("me.status = 0".into());
                }
                "trash" => {
                    parts.push("me.status = 2".into());
                }
                "uncategorized" => {
                    parts.push("me.status = 1".into());
                    parts.push("NOT EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = me.entity_id)".into());
                    parts.push("NOT EXISTS (SELECT 1 FROM media_entity child WHERE child.parent_collection_entity_id = me.entity_id AND EXISTS (SELECT 1 FROM folder_member fm2 WHERE fm2.entity_id = child.entity_id))".into());
                }
                "untagged" => {
                    parts.push("me.status = 1".into());
                    parts.push("NOT EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = me.entity_id)".into());
                    parts.push("NOT EXISTS (SELECT 1 FROM media_entity child WHERE child.parent_collection_entity_id = me.entity_id AND EXISTS (SELECT 1 FROM entity_tag et2 WHERE et2.entity_id = child.entity_id))".into());
                }
                _ => {
                    parts.push("me.status = 1".into());
                }
            }
        }
        ScopeKind::Folder => {
            let idx = bound.len() + 1;
            parts.push("me.status = 1".into());
            parts.push(folder_scope_membership(idx));
            bound.push(Box::new(scope.id.unwrap_or(0)));
        }
        ScopeKind::SmartFolder => {
            // Pre-resolved from bitmap by LibraryDatabase::query_entity_view.
            // Entity_ids from BitmapKey::SmartFolder(id) are passed in preresolved_ids.
            parts.push("me.status = 1".into());
            match preresolved_ids {
                Some(ids) if !ids.is_empty() => {
                    // Internal entity_ids from bitmap — safe to inline as integers
                    let id_list = ids
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    parts.push(format!("me.entity_id IN ({id_list})"));
                }
                _ => {
                    parts.push("1=0".into());
                }
            }
        }
        ScopeKind::Collection => {
            let idx = bound.len() + 1;
            parts.push(format!("me.parent_collection_entity_id = ?{idx}"));
            bound.push(Box::new(scope.id.unwrap_or(0)));
        }
        ScopeKind::Tag => {
            let idx = bound.len() + 1;
            parts.push("me.status = 1".into());
            parts.push(format!(
                "({} OR (me.entity_kind = 'collection' AND EXISTS (\
                     SELECT 1 FROM media_entity child \
                     WHERE child.parent_collection_entity_id = me.entity_id AND {})))",
                effective_tag_exists("me.entity_id", idx),
                effective_tag_exists("child.entity_id", idx),
            ));
            bound.push(Box::new(scope.key.clone().unwrap_or_default()));
        }
        ScopeKind::Search => {
            let idx = bound.len() + 1;
            parts.push("me.status = 1".into());
            let search_text = scope.key.as_deref().unwrap_or("");
            if !search_text.is_empty() {
                parts.push(format!(
                    "me.entity_id IN (SELECT rowid FROM entity_fts WHERE entity_fts MATCH ?{idx})"
                ));
                bound.push(Box::new(format!("{}*", search_text)));
            }
        }
        ScopeKind::Similar => {
            // Resolve perceptual hash similarity inline using the DB connection.
            // scope.key carries the source entity_hash.
            parts.push("me.status = 1".into());
            let source_hash = scope.key.as_deref().unwrap_or("");
            if !source_hash.is_empty() {
                let similar_ids = resolve_similar_ids(conn, source_hash);
                if similar_ids.is_empty() {
                    parts.push("1=0".into());
                } else {
                    let id_list = similar_ids
                        .iter()
                        .map(|id| id.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    parts.push(format!("me.entity_id IN ({id_list})"));
                }
            } else {
                parts.push("1=0".into());
            }
        }
    }
}

fn apply_filters(filters: &QueryFilters, parts: &mut Vec<String>, bound: &mut Vec<Box<dyn ToSql>>) {
    if let Some(ref rf) = filters.rating {
        let idx = bound.len() + 1;
        let op = match rf.op {
            FilterOp::Eq => "=",
            FilterOp::Gte => ">=",
            FilterOp::Lte => "<=",
            FilterOp::Gt => ">",
            FilterOp::Lt => "<",
        };
        parts.push(format!(
            "(CASE WHEN me.entity_kind = 'collection' THEN (\
                SELECT MAX(child.rating) FROM media_entity child \
                WHERE child.parent_collection_entity_id = me.entity_id\
             ) ELSE me.rating END) {op} ?{idx}"
        ));
        bound.push(Box::new(rf.value));
    }

    if let Some(ref mimes) = filters.mime_types {
        if !mimes.is_empty() {
            let placeholders: Vec<String> = mimes
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let idx = bound.len() + 1 + i;
                    format!("?{idx}")
                })
                .collect();
            parts.push(format!(
                "COALESCE(mf.mime_type, pmf.mime_type, '') IN ({})",
                placeholders.join(",")
            ));
            for m in mimes {
                bound.push(Box::new(m.clone()));
            }
        }
    }

    if let Some(ref types) = filters.entity_types {
        if !types.is_empty() {
            let mut type_parts = Vec::new();
            for t in types {
                let idx = bound.len() + 1;
                match t.as_str() {
                    "image" => {
                        type_parts.push(format!(
                            "COALESCE(mf.mime_type, pmf.mime_type, '') LIKE ?{idx}"
                        ));
                        bound.push(Box::new("image/%".to_string()));
                    }
                    "video" => {
                        type_parts.push(format!(
                            "COALESCE(mf.mime_type, pmf.mime_type, '') LIKE ?{idx}"
                        ));
                        bound.push(Box::new("video/%".to_string()));
                    }
                    "audio" => {
                        type_parts.push(format!(
                            "COALESCE(mf.mime_type, pmf.mime_type, '') LIKE ?{idx}"
                        ));
                        bound.push(Box::new("audio/%".to_string()));
                    }
                    "collection" => {
                        type_parts.push("me.entity_kind = 'collection'".into());
                    }
                    _ => {}
                }
            }
            if !type_parts.is_empty() {
                parts.push(format!("({})", type_parts.join(" OR ")));
            }
        }
    }

    if let Some(ref tag_filters) = filters.tags {
        for tf in tag_filters {
            let idx = bound.len() + 1;
            let tag_exists = format!(
                "({} OR (me.entity_kind = 'collection' AND EXISTS (\
                    SELECT 1 FROM media_entity child \
                    WHERE child.parent_collection_entity_id = me.entity_id AND {})))",
                effective_tag_exists("me.entity_id", idx),
                effective_tag_exists("child.entity_id", idx),
            );
            match tf.match_mode {
                TagMatchMode::Include => parts.push(tag_exists),
                TagMatchMode::Exclude => parts.push(format!("NOT {tag_exists}")),
            }
            bound.push(Box::new(tf.tag.clone()));
        }
    }

    apply_date_filter("me.date_created", &filters.date_created, parts, bound);
    apply_date_filter("me.date_added", &filters.date_added, parts, bound);
    apply_date_filter("me.date_modified", &filters.date_modified, parts, bound);

    if let Some(ref text) = filters.search_text {
        if !text.is_empty() {
            let idx = bound.len() + 1;
            parts.push(format!("(me.name LIKE ?{idx} OR me.notes LIKE ?{idx})"));
            bound.push(Box::new(format!("%{text}%")));
        }
    }

    if let Some(ref colors) = filters.colors {
        if !colors.is_empty() {
            let placeholders: Vec<String> = colors
                .iter()
                .enumerate()
                .map(|(i, _)| {
                    let idx = bound.len() + 1 + i;
                    format!("?{idx}")
                })
                .collect();
            parts.push(format!(
                "EXISTS (
                    SELECT 1
                    FROM file_color fc
                    WHERE fc.file_id IN (COALESCE(mf.file_id, -1), COALESCE(pmf.file_id, -1))
                      AND fc.hex IN ({})
                )",
                placeholders.join(",")
            ));
            for c in colors {
                bound.push(Box::new(c.clone()));
            }
        }
    }
}

fn effective_tag_exists(entity_id: &str, parameter_index: usize) -> String {
    let matches_requested_tag = |membership_alias: &str| {
        format!(
            "({membership_alias}.tag_id = requested.tag_id
              OR EXISTS (
                  SELECT 1 FROM tag_alias ta
                  WHERE ta.from_tag_id = requested.tag_id
                    AND ta.to_tag_id = {membership_alias}.tag_id
              )
              OR EXISTS (
                  SELECT 1 FROM tag_alias ta
                  WHERE ta.from_tag_id = {membership_alias}.tag_id
                    AND ta.to_tag_id = requested.tag_id
              )
              OR EXISTS (
                  SELECT 1
                  FROM tag_alias requested_alias
                  JOIN tag_alias member_alias
                    ON member_alias.to_tag_id = requested_alias.to_tag_id
                  WHERE requested_alias.from_tag_id = requested.tag_id
                    AND member_alias.from_tag_id = {membership_alias}.tag_id
              ))"
        )
    };

    format!(
        "EXISTS (
            SELECT 1
            FROM tag requested
            WHERE (requested.namespace || ':' || requested.subtag = ?{parameter_index}
                   OR requested.subtag = ?{parameter_index})
              AND (
                  EXISTS (
                      SELECT 1 FROM entity_tag direct
                      WHERE direct.entity_id = {entity_id}
                        AND {direct_match}
                  )
                  OR EXISTS (
                      SELECT 1 FROM entity_tag_implied implied
                      WHERE implied.entity_id = {entity_id}
                        AND {implied_match}
                  )
              )
        )",
        direct_match = matches_requested_tag("direct"),
        implied_match = matches_requested_tag("implied"),
    )
}

fn apply_date_filter(
    column: &str,
    range: &Option<crate::db::types::DateRange>,
    parts: &mut Vec<String>,
    bound: &mut Vec<Box<dyn ToSql>>,
) {
    if let Some(ref dr) = range {
        if let Some(ref from) = dr.from {
            let idx = bound.len() + 1;
            parts.push(format!("{column} >= ?{idx}"));
            bound.push(Box::new(from.clone()));
        }
        if let Some(ref to) = dr.to {
            let idx = bound.len() + 1;
            parts.push(format!("{column} <= ?{idx}"));
            bound.push(Box::new(to.clone()));
        }
    }
}

fn sort_column(field: &str) -> &str {
    match field {
        "date_added" => "me.date_added",
        "date_created" => "me.date_created",
        "date_modified" => "me.date_modified",
        "rating" => {
            "CASE WHEN me.entity_kind = 'collection' THEN (\
                 SELECT MAX(child.rating) FROM media_entity child \
                 WHERE child.parent_collection_entity_id = me.entity_id\
             ) ELSE me.rating END"
        }
        "size_bytes" => "size_bytes",
        "name" => "me.name",
        _ => "me.date_added",
    }
}

fn validated_sort(field: &str, dir: &str) -> String {
    let col = sort_column(field);
    let direction = if dir == "asc" { "ASC" } else { "DESC" };
    format!("{col} {direction}")
}

/// Parse an opaque cursor "sort_value|entity_hash".
/// Resolves entity_hash → entity_id via DB lookup for stable tie-breaking.
fn parse_cursor(conn: &Connection, cursor: &str) -> Option<(String, i64)> {
    let pipe = cursor.rfind('|')?;
    let sort_val = cursor[..pipe].to_string();
    let entity_hash = &cursor[pipe + 1..];
    let entity_id: i64 = conn
        .query_row(
            "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
            [entity_hash],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Some((sort_val, entity_id))
}

// ── Similar scope resolution ─────────────────────────────────────────

/// Hamming distance threshold for perceptual hash similarity.
const SIMILAR_HAMMING_THRESHOLD: u32 = 10;

/// Resolve entity_ids of entities with perceptual hashes similar to the
/// source entity identified by `source_entity_hash`.
/// The simple linear scan is intentional until representative-library profiling proves it slow.
fn resolve_similar_ids(conn: &Connection, source_entity_hash: &str) -> Vec<i64> {
    let engine = base64::engine::general_purpose::STANDARD;

    // Look up the source entity's perceptual hash
    let source_phash: Option<String> = conn
        .query_row(
            "SELECT mf.perceptual_hash
             FROM media_entity me
             JOIN single_media_entity sme ON sme.entity_id = me.entity_id
             JOIN media_file mf ON mf.file_id = sme.file_id
             WHERE me.entity_hash = ?1",
            [source_entity_hash],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    let source_bytes = match source_phash.and_then(|h| engine.decode(h).ok()) {
        Some(b) => b,
        None => return vec![],
    };

    // Scan all entities with perceptual hashes and compute hamming distances
    let mut stmt = match conn.prepare(
        "SELECT me.entity_id, mf.perceptual_hash
         FROM media_entity me
         JOIN single_media_entity sme ON sme.entity_id = me.entity_id
         JOIN media_file mf ON mf.file_id = sme.file_id
         WHERE mf.perceptual_hash IS NOT NULL
           AND me.entity_hash != ?1
           AND me.status = 1",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let rows = match stmt.query_map([source_entity_hash], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    }) {
        Ok(r) => r,
        Err(_) => return vec![],
    };

    let mut result = Vec::new();
    for row in rows.flatten() {
        let (entity_id, phash_b64) = row;
        if let Ok(candidate_bytes) = engine.decode(&phash_b64) {
            if candidate_bytes.len() == source_bytes.len() {
                let distance = hamming_distance(&source_bytes, &candidate_bytes);
                if distance <= SIMILAR_HAMMING_THRESHOLD {
                    result.push(entity_id);
                }
            }
        }
    }

    result
}

/// Batch fetch grid items by entity_hash. Used for targeted reconciliation
/// and eager grid insertion, not for driving the main grid.
pub fn get_entity_grid_items_by_hash(
    conn: &Connection,
    hashes: &[String],
) -> rusqlite::Result<Vec<EntityGridItem>> {
    if hashes.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders: Vec<String> = (1..=hashes.len()).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "{GRID_SELECT} WHERE me.entity_hash IN ({}) AND me.parent_collection_entity_id IS NULL",
        placeholders.join(",")
    );
    let mut stmt = conn.prepare(&sql)?;
    let params: Vec<&dyn ToSql> = hashes.iter().map(|h| h as &dyn ToSql).collect();
    let rows = stmt.query_map(params.as_slice(), read_grid_item)?;
    rows.collect()
}

/// Bitwise hamming distance between two byte slices of equal length.
fn hamming_distance(a: &[u8], b: &[u8]) -> u32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::query_entity_view;
    use crate::db::core::schema::LIBRARY_DDL;
    use crate::db::types::{
        BaseScope, EntityViewQuery, QueryFilters, QueryPage, QuerySort, ScopeKind,
    };
    use rusqlite::params;

    #[test]
    fn color_filters_match_any_palette_color_not_only_dominant_hex() {
        let conn = rusqlite::Connection::open_in_memory().expect("open db");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");

        conn.execute(
            "INSERT INTO media_entity (
                entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified
             ) VALUES (?1, ?2, 'single', 1, ?3, ?4, ?4, ?4)",
            params![1_i64, "entity_1", "Test", "2026-04-01T00:00:00Z"],
        )
        .expect("insert entity");
        conn.execute(
            "INSERT INTO media_file (
                file_id, file_hash, mime_type, size_bytes, has_audio, dominant_color_hex, date_added
             ) VALUES (?1, ?2, 'image/png', 1024, 0, ?3, ?4)",
            params![1_i64, "file_1", "#111111", "2026-04-01T00:00:00Z"],
        )
        .expect("insert file");
        conn.execute(
            "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1)",
            [],
        )
        .expect("link file");
        conn.execute(
            "INSERT INTO file_color (file_id, hex, l, a, b) VALUES (1, '#222222', 20.0, 0.0, 0.0)",
            [],
        )
        .expect("insert palette color");

        let page = query_entity_view(
            &conn,
            &EntityViewQuery {
                base_scope: BaseScope {
                    kind: ScopeKind::System,
                    key: Some("all".into()),
                    id: None,
                },
                filters: QueryFilters {
                    colors: Some(vec!["#222222".into()]),
                    ..Default::default()
                },
                sort: QuerySort::default(),
                page: QueryPage::default(),
            },
            None,
        )
        .expect("query");

        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].entity_hash, "entity_1");
    }
}
