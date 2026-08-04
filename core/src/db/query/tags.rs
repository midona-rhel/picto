//! Canonical tag queries.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::types::{mask_from_db_bits, NamespaceSummary, TagInfo, TagRecord, TagRelation};

fn equivalent_tag_match(membership_alias: &str, requested_tag_id: &str) -> String {
    format!(
        "({membership_alias}.tag_id = {requested_tag_id}
          OR EXISTS (
              SELECT 1 FROM tag_alias ta
              WHERE ta.from_tag_id = {requested_tag_id}
                AND ta.to_tag_id = {membership_alias}.tag_id
          )
          OR EXISTS (
              SELECT 1 FROM tag_alias ta
              WHERE ta.from_tag_id = {membership_alias}.tag_id
                AND ta.to_tag_id = {requested_tag_id}
          )
          OR EXISTS (
              SELECT 1
              FROM tag_alias requested_alias
              JOIN tag_alias member_alias
                ON member_alias.to_tag_id = requested_alias.to_tag_id
              WHERE requested_alias.from_tag_id = {requested_tag_id}
                AND member_alias.from_tag_id = {membership_alias}.tag_id
          ))"
    )
}

fn effective_tag_id_exists(entity_id: &str, requested_tag_id: &str) -> String {
    format!(
        "(EXISTS (
             SELECT 1 FROM entity_tag direct
             WHERE direct.entity_id = {entity_id}
               AND {direct_match}
         )
         OR EXISTS (
             SELECT 1 FROM entity_tag_implied implied
             WHERE implied.entity_id = {entity_id}
               AND {implied_match}
         ))",
        direct_match = equivalent_tag_match("direct", requested_tag_id),
        implied_match = equivalent_tag_match("implied", requested_tag_id),
    )
}

pub(crate) fn effective_tag_exists(entity_id: &str, parameter_index: usize) -> String {
    format!(
        "EXISTS (
             SELECT 1
             FROM tag requested
             WHERE (requested.namespace || ':' || requested.subtag = ?{parameter_index}
                    OR requested.subtag = ?{parameter_index})
               AND {membership}
         )",
        membership = effective_tag_id_exists(entity_id, "requested.tag_id"),
    )
}

fn visible_tag_count_expr(requested_tag_id: &str) -> String {
    format!(
        "(SELECT COUNT(*)
          FROM media_entity me
          WHERE me.status = 1
            AND me.parent_collection_entity_id IS NULL
            AND (
                {top_level}
                OR (me.entity_kind = 'collection' AND EXISTS (
                    SELECT 1
                    FROM media_entity child
                    WHERE child.parent_collection_entity_id = me.entity_id
                      AND {child}
                ))
            ))",
        top_level = effective_tag_id_exists("me.entity_id", requested_tag_id),
        child = effective_tag_id_exists("child.entity_id", requested_tag_id),
    )
}

fn tag_record_columns(alias: &str) -> String {
    format!(
        "{alias}.tag_id, {alias}.namespace, {alias}.subtag, {} AS file_count, {alias}.site_mask",
        visible_tag_count_expr(&format!("{alias}.tag_id"))
    )
}

pub fn find_tag_id(conn: &Connection, tag_str: &str) -> rusqlite::Result<Option<i64>> {
    let (namespace, subtag) = parse_tag(tag_str);
    conn.query_row(
        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
        params![namespace, subtag],
        |row| row.get(0),
    )
    .optional()
}

pub fn get_tag_string(conn: &Connection, tag_id: i64) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT namespace, subtag FROM tag WHERE tag_id = ?1",
        [tag_id],
        |row| {
            let namespace: String = row.get(0)?;
            let subtag: String = row.get(1)?;
            Ok(combine_tag(&namespace, &subtag))
        },
    )
    .optional()
}

pub fn search_tags(
    conn: &Connection,
    query: &str,
    limit: i64,
    offset: i64,
) -> rusqlite::Result<Vec<TagRecord>> {
    let columns = tag_record_columns("t");
    if query.is_empty() {
        let sql = format!(
            "WITH counted AS (SELECT {columns} FROM tag t)
             SELECT tag_id, namespace, subtag, file_count, site_mask
             FROM counted
             WHERE file_count > 0
             ORDER BY file_count DESC, subtag ASC, tag_id ASC
             LIMIT ?1 OFFSET ?2"
        );
        let mut stmt = conn.prepare(&sql)?;
        return stmt
            .query_map(params![limit, offset], map_tag_record)?
            .collect();
    }

    let fts_query = format!("{}*", query.replace('"', ""));
    let sql = format!(
        "SELECT {columns}
         FROM tag_fts fts
         JOIN tag t ON t.tag_id = fts.rowid
         WHERE tag_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2 OFFSET ?3"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![fts_query, limit, offset], map_tag_record)?;
    rows.collect()
}

pub fn get_all_tag_keys(conn: &Connection) -> rusqlite::Result<Vec<(i64, String, String)>> {
    let mut stmt = conn.prepare("SELECT tag_id, namespace, subtag FROM tag ORDER BY tag_id")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
    rows.collect()
}

pub fn get_entity_tags(conn: &Connection, entity_hash: &str) -> rusqlite::Result<Vec<TagInfo>> {
    let entity_id: Option<i64> = conn
        .query_row(
            "SELECT entity_id FROM media_entity WHERE entity_hash = ?1",
            [entity_hash],
            |row| row.get(0),
        )
        .optional()?;

    let Some(entity_id) = entity_id else {
        return Ok(Vec::new());
    };

    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, t.site_mask, et.provenance_mask, et.source
         FROM entity_tag et
         JOIN tag t ON t.tag_id = et.tag_id
         WHERE et.entity_id = ?1
         ORDER BY t.namespace, t.subtag, et.source",
    )?;
    let rows = stmt.query_map([entity_id], |row| {
        Ok(TagInfo {
            tag_id: row.get(0)?,
            namespace: row.get(1)?,
            subtag: row.get(2)?,
            site_mask: mask_from_db_bits(row.get::<_, Option<i64>>(3)?.unwrap_or(0)),
            provenance_mask: mask_from_db_bits(row.get::<_, Option<i64>>(4)?.unwrap_or(0)),
            source: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn get_aliases_for_tag(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<TagRelation>> {
    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, 'to' as relation, t.site_mask
           FROM tag_alias ta JOIN tag t ON ta.to_tag_id = t.tag_id
          WHERE ta.from_tag_id = ?1
         UNION
         SELECT t.tag_id, t.namespace, t.subtag, 'from' as relation, t.site_mask
           FROM tag_alias ta JOIN tag t ON ta.from_tag_id = t.tag_id
          WHERE ta.to_tag_id = ?1
         ORDER BY namespace, subtag, tag_id",
    )?;
    let rows = stmt.query_map([tag_id], map_tag_relation)?;
    rows.collect()
}

pub fn get_implications_for_tag(
    conn: &Connection,
    tag_id: i64,
) -> rusqlite::Result<Vec<TagRelation>> {
    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, 'parent' as relation, t.site_mask
           FROM tag_implication ti JOIN tag t ON ti.parent_tag_id = t.tag_id
          WHERE ti.child_tag_id = ?1
         UNION
         SELECT t.tag_id, t.namespace, t.subtag, 'child' as relation, t.site_mask
           FROM tag_implication ti JOIN tag t ON ti.child_tag_id = t.tag_id
          WHERE ti.parent_tag_id = ?1
         ORDER BY namespace, subtag, tag_id",
    )?;
    let rows = stmt.query_map([tag_id], map_tag_relation)?;
    rows.collect()
}

pub fn get_tags_paginated(
    conn: &Connection,
    namespace: Option<&str>,
    search: Option<&str>,
    cursor: Option<&str>,
    limit: i64,
) -> rusqlite::Result<Vec<TagRecord>> {
    let columns = tag_record_columns("t");
    // Namespace sort priority — matches the Hydrus-compatible ordering used throughout the app
    const NS_ORDER_EXPR: &str = "CASE LOWER(t.namespace)
        WHEN 'creator'   THEN 0
        WHEN 'studio'    THEN 1
        WHEN 'series'    THEN 2
        WHEN 'character'  THEN 3
        WHEN 'person'    THEN 4
        WHEN 'species'   THEN 5
        WHEN 'photoset'  THEN 6
        WHEN 'rating'    THEN 7
        WHEN 'meta'      THEN 8
        WHEN 'system'    THEN 9
        WHEN 'general'   THEN 10
        WHEN ''          THEN 10
        ELSE 11 END";

    if let Some(query) = search {
        if !query.is_empty() {
            let fts_query = format!("{}*", query.replace('"', ""));
            let (sql, use_ns) = match namespace {
                Some(_) => (
                    format!(
                        "SELECT {columns}
                         FROM tag_fts fts
                         JOIN tag t ON t.tag_id = fts.rowid
                         WHERE tag_fts MATCH ?1 AND t.namespace = ?2
                         ORDER BY t.subtag ASC, t.tag_id ASC
                         LIMIT ?3"
                    ),
                    true,
                ),
                None => (
                    format!(
                        "SELECT {columns}
                         FROM tag_fts fts
                         JOIN tag t ON t.tag_id = fts.rowid
                         WHERE tag_fts MATCH ?1
                         ORDER BY {NS_ORDER_EXPR} ASC, t.subtag ASC, t.tag_id ASC
                         LIMIT ?2"
                    ),
                    false,
                ),
            };
            let mut stmt = conn.prepare(&sql)?;
            return if use_ns {
                stmt.query_map(
                    params![fts_query, namespace.unwrap(), limit],
                    map_tag_record,
                )?
                .collect()
            } else {
                stmt.query_map(params![fts_query, limit], map_tag_record)?
                    .collect()
            };
        }
    }

    let (cursor_subtag, cursor_id) = if let Some(c) = cursor {
        if let Some(sep) = c.find('\0') {
            let subtag = &c[..sep];
            let tag_id = c[sep + 1..].parse().unwrap_or(0);
            (Some(subtag.to_string()), Some(tag_id))
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let has_namespace = namespace.is_some();

    // When a specific namespace is selected, no namespace ordering needed — just subtag ASC.
    // When showing all, sort by namespace priority, then subtag, then tag_id.
    // Cursor-based pagination uses (ns_order, subtag, tag_id) tuple comparison.
    if has_namespace {
        // Single-namespace view — simple subtag sort, cursor on (subtag, tag_id)
        let has_cursor = cursor_subtag.is_some();
        let sql = match has_cursor {
            false => format!(
                "SELECT {columns} FROM tag t
                 WHERE t.namespace = ?1
                 ORDER BY t.subtag ASC, t.tag_id ASC LIMIT ?2"
            ),
            true => format!(
                "SELECT {columns} FROM tag t
                 WHERE t.namespace = ?1 AND (t.subtag, t.tag_id) > (?2, ?3)
                 ORDER BY t.subtag ASC, t.tag_id ASC LIMIT ?4"
            ),
        };
        let mut stmt = conn.prepare(&sql)?;
        return match has_cursor {
            false => stmt
                .query_map(params![namespace.unwrap(), limit], map_tag_record)?
                .collect(),
            true => stmt
                .query_map(
                    params![
                        namespace.unwrap(),
                        cursor_subtag.as_deref().unwrap(),
                        cursor_id.unwrap(),
                        limit
                    ],
                    map_tag_record,
                )?
                .collect(),
        };
    }

    // All namespaces — sort by namespace priority, then subtag, then tag_id.
    // Cursor is (ns_order, subtag, tag_id) for stable pagination.
    let (cursor_ns_order, cursor_subtag_val, cursor_tid) = if let Some(c) = cursor {
        // Cursor format: "ns_order\0subtag\0tag_id"
        let parts: Vec<&str> = c.splitn(3, '\0').collect();
        if parts.len() == 3 {
            let ns_ord: i64 = parts[0].parse().unwrap_or(0);
            let sub = parts[1].to_string();
            let tid: i64 = parts[2].parse().unwrap_or(0);
            (Some(ns_ord), Some(sub), Some(tid))
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };
    let has_cursor = cursor_ns_order.is_some();

    let sql = if has_cursor {
        format!(
            "SELECT {columns} FROM tag t
             WHERE ({NS_ORDER_EXPR}, t.subtag, t.tag_id) > (?1, ?2, ?3)
             ORDER BY {NS_ORDER_EXPR} ASC, t.subtag ASC, t.tag_id ASC LIMIT ?4"
        )
    } else {
        format!(
            "SELECT {columns} FROM tag t
             ORDER BY {NS_ORDER_EXPR} ASC, t.subtag ASC, t.tag_id ASC LIMIT ?1"
        )
    };

    let mut stmt = conn.prepare(&sql)?;
    if has_cursor {
        stmt.query_map(
            params![
                cursor_ns_order.unwrap(),
                cursor_subtag_val.as_deref().unwrap(),
                cursor_tid.unwrap(),
                limit
            ],
            map_tag_record,
        )?
        .collect()
    } else {
        stmt.query_map(params![limit], map_tag_record)?.collect()
    }
}

pub fn get_namespace_summary(conn: &Connection) -> rusqlite::Result<Vec<NamespaceSummary>> {
    let mut stmt = conn.prepare(
        "SELECT namespace, COUNT(*) AS count
         FROM tag
         GROUP BY namespace
         ORDER BY count DESC, namespace ASC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(NamespaceSummary {
            namespace: row.get(0)?,
            count: row.get(1)?,
        })
    })?;
    rows.collect()
}

fn map_tag_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<TagRecord> {
    Ok(TagRecord {
        tag_id: row.get(0)?,
        namespace: row.get(1)?,
        subtag: row.get(2)?,
        file_count: row.get(3)?,
        site_mask: mask_from_db_bits(row.get::<_, Option<i64>>(4)?.unwrap_or(0)),
    })
}

fn map_tag_relation(row: &rusqlite::Row<'_>) -> rusqlite::Result<TagRelation> {
    Ok(TagRelation {
        tag_id: row.get(0)?,
        namespace: row.get(1)?,
        subtag: row.get(2)?,
        relation: row.get(3)?,
        site_mask: mask_from_db_bits(row.get::<_, Option<i64>>(4)?.unwrap_or(0)),
    })
}

fn parse_tag(s: &str) -> (String, String) {
    if let Some(idx) = s.find(':') {
        let namespace = &s[..idx];
        let subtag = &s[idx + 1..];
        if namespace.is_empty() {
            (String::new(), s.to_string())
        } else {
            (namespace.to_string(), subtag.to_string())
        }
    } else {
        (String::new(), s.to_string())
    }
}

fn combine_tag(namespace: &str, subtag: &str) -> String {
    if namespace.is_empty() {
        subtag.to_string()
    } else {
        format!("{namespace}:{subtag}")
    }
}
