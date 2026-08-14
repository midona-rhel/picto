//! Canonical tag queries.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::db::types::{mask_from_db_bits, NamespaceSummary, TagInfo, TagRecord, TagRelation};

fn subtag_search(query: &str) -> (String, String, String) {
    let term = query
        .split_once(':')
        .map_or(query, |(_, subtag)| subtag)
        .trim()
        .to_string();
    let escaped = term
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    (term, format!("%{escaped}%"), format!("{escaped}%"))
}

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

fn tag_record_columns(alias: &str) -> String {
    format!("{alias}.tag_id, {alias}.namespace, {alias}.subtag, 0 AS file_count")
}

pub fn find_tag_id(conn: &Connection, tag_str: &str) -> rusqlite::Result<Option<i64>> {
    let Some((namespace, subtag)) = crate::tags::normalize::parse_tag(tag_str) else {
        return Ok(None);
    };
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
            Ok(crate::types::tag_display_key(&namespace, &subtag))
        },
    )
    .optional()
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
        "SELECT t.tag_id, t.namespace, t.subtag, et.provenance_mask, et.source
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
            provenance_mask: mask_from_db_bits(row.get::<_, Option<i64>>(3)?.unwrap_or(0)),
            source: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn get_aliases_for_tag(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<TagRelation>> {
    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, 'to' as relation
           FROM tag_alias ta JOIN tag t ON ta.to_tag_id = t.tag_id
          WHERE ta.from_tag_id = ?1
         UNION
         SELECT t.tag_id, t.namespace, t.subtag, 'from' as relation
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
        "SELECT t.tag_id, t.namespace, t.subtag, 'parent' as relation
           FROM tag_implication ti JOIN tag t ON ti.parent_tag_id = t.tag_id
          WHERE ti.child_tag_id = ?1
         UNION
         SELECT t.tag_id, t.namespace, t.subtag, 'child' as relation
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
) -> rusqlite::Result<crate::db::types::TagPage> {
    let columns = tag_record_columns("t");
    let limit = limit.clamp(1, 500);
    let fetch_limit = limit.saturating_add(1);

    // Namespace priority is part of the cursor key, not just presentation order.
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

    let mut items = if let Some(query) = search.filter(|query| !query.is_empty()) {
        let (term, pattern, prefix) = subtag_search(query);
        let cursor_predicate = match decode_cursor(cursor, CursorKind::Search)? {
            None => String::new(),
            Some(CursorValues::Search(cursor)) => format!(
                "WHERE (search_rank, ns_order, lower_subtag, subtag, tag_id)
                        > ({}, {}, {}, {}, {})",
                cursor.rank,
                cursor.ns_order,
                sql_quote(&cursor.lower_subtag),
                sql_quote(&cursor.subtag),
                cursor.tag_id
            ),
            Some(CursorValues::Standard(_)) => return Err(invalid_cursor()),
        };
        let namespace_filter = "AND (:namespace IS NULL OR LOWER(t.namespace) = LOWER(:namespace))";
        let sql = format!(
            "WITH tagged AS (
                 SELECT {columns},
                        CASE
                            WHEN LOWER(t.subtag) = LOWER(:term) THEN 0
                            WHEN t.subtag LIKE :prefix ESCAPE '\\' COLLATE NOCASE THEN 1
                            ELSE 2
                        END AS search_rank,
                        {NS_ORDER_EXPR} AS ns_order,
                        LOWER(t.subtag) AS lower_subtag
                 FROM tag t
                 WHERE t.subtag LIKE :pattern ESCAPE '\\' COLLATE NOCASE
                   {namespace_filter}
             )
             SELECT tag_id, namespace, subtag, file_count
             FROM tagged
             {cursor_predicate}
             ORDER BY search_rank ASC, ns_order ASC,
                      lower_subtag ASC, subtag ASC, tag_id ASC
             LIMIT :limit"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::named_params! {
                ":term": term,
                ":pattern": pattern,
                ":prefix": prefix,
                ":namespace": namespace,
                ":limit": fetch_limit,
            },
            map_tag_record,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let cursor_predicate = match decode_cursor(cursor, CursorKind::Standard)? {
            None => String::new(),
            Some(CursorValues::Standard(cursor)) => format!(
                "WHERE (ns_order, lower_subtag, subtag, tag_id)
                        > ({}, {}, {}, {})",
                cursor.ns_order,
                sql_quote(&cursor.lower_subtag),
                sql_quote(&cursor.subtag),
                cursor.tag_id
            ),
            Some(CursorValues::Search(_)) => return Err(invalid_cursor()),
        };
        let namespace_filter =
            "WHERE (:namespace IS NULL OR LOWER(t.namespace) = LOWER(:namespace))";
        let sql = format!(
            "WITH tagged AS (
                 SELECT {columns},
                        {NS_ORDER_EXPR} AS ns_order,
                        LOWER(t.subtag) AS lower_subtag
                 FROM tag t
                 {namespace_filter}
             )
             SELECT tag_id, namespace, subtag, file_count
             FROM tagged
             {cursor_predicate}
             ORDER BY ns_order ASC, lower_subtag ASC, subtag ASC, tag_id ASC
             LIMIT :limit"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::named_params! {
                ":namespace": namespace,
                ":limit": fetch_limit,
            },
            map_tag_record,
        )?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };

    let has_more = items.len() as i64 > limit;
    if has_more {
        items.truncate(limit as usize);
    }
    let next_cursor = if has_more {
        items.last().map(|item| {
            if let Some(query) = search.filter(|query| !query.is_empty()) {
                let (term, _, _) = subtag_search(query);
                encode_cursor(&TagCursor::Search {
                    rank: search_rank(&item.subtag, &term),
                    ns_order: namespace_order(&item.namespace),
                    lower_subtag: item.subtag.to_ascii_lowercase(),
                    subtag: item.subtag.clone(),
                    tag_id: item.tag_id,
                })
            } else {
                encode_cursor(&TagCursor::Standard {
                    ns_order: namespace_order(&item.namespace),
                    lower_subtag: item.subtag.to_ascii_lowercase(),
                    subtag: item.subtag.clone(),
                    tag_id: item.tag_id,
                })
            }
        })
    } else {
        None
    };

    Ok(crate::db::types::TagPage { items, next_cursor })
}

#[derive(Debug, Clone, Copy)]
enum CursorKind {
    Standard,
    Search,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TagCursor {
    Standard {
        ns_order: i64,
        lower_subtag: String,
        subtag: String,
        tag_id: i64,
    },
    Search {
        rank: i64,
        ns_order: i64,
        lower_subtag: String,
        subtag: String,
        tag_id: i64,
    },
}

#[derive(Debug)]
struct StandardCursor {
    ns_order: i64,
    lower_subtag: String,
    subtag: String,
    tag_id: i64,
}

#[derive(Debug)]
struct SearchCursor {
    rank: i64,
    ns_order: i64,
    lower_subtag: String,
    subtag: String,
    tag_id: i64,
}

fn encode_cursor(cursor: &TagCursor) -> String {
    URL_SAFE_NO_PAD.encode(serde_json::to_vec(cursor).expect("tag cursor serialization"))
}

fn decode_cursor(
    cursor: Option<&str>,
    expected_kind: CursorKind,
) -> rusqlite::Result<Option<CursorValues>> {
    let Some(cursor) = cursor else {
        return Ok(None);
    };
    let bytes = URL_SAFE_NO_PAD
        .decode(cursor)
        .map_err(|_| invalid_cursor())?;
    let decoded: TagCursor = serde_json::from_slice(&bytes).map_err(|_| invalid_cursor())?;
    let values = match (expected_kind, decoded) {
        (
            CursorKind::Standard,
            TagCursor::Standard {
                ns_order,
                lower_subtag,
                subtag,
                tag_id,
            },
        ) => CursorValues::Standard(StandardCursor {
            ns_order,
            lower_subtag,
            subtag,
            tag_id,
        }),
        (
            CursorKind::Search,
            TagCursor::Search {
                rank,
                ns_order,
                lower_subtag,
                subtag,
                tag_id,
            },
        ) => CursorValues::Search(SearchCursor {
            rank,
            ns_order,
            lower_subtag,
            subtag,
            tag_id,
        }),
        _ => return Err(invalid_cursor()),
    };
    Ok(Some(values))
}

enum CursorValues {
    Standard(StandardCursor),
    Search(SearchCursor),
}

fn invalid_cursor() -> rusqlite::Error {
    rusqlite::Error::InvalidParameterName("invalid tag cursor".to_string())
}

fn sql_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn namespace_order(namespace: &str) -> i64 {
    match namespace.to_ascii_lowercase().as_str() {
        "creator" => 0,
        "studio" => 1,
        "series" => 2,
        "character" => 3,
        "person" => 4,
        "species" => 5,
        "photoset" => 6,
        "rating" => 7,
        "meta" => 8,
        "system" => 9,
        "general" | "" => 10,
        _ => 11,
    }
}

fn search_rank(subtag: &str, term: &str) -> i64 {
    if subtag.eq_ignore_ascii_case(term) {
        0
    } else if subtag
        .to_ascii_lowercase()
        .starts_with(&term.to_ascii_lowercase())
    {
        1
    } else {
        2
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
    })
}

fn map_tag_relation(row: &rusqlite::Row<'_>) -> rusqlite::Result<TagRelation> {
    Ok(TagRelation {
        tag_id: row.get(0)?,
        namespace: row.get(1)?,
        subtag: row.get(2)?,
        relation: row.get(3)?,
    })
}
