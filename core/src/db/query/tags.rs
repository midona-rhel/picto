//! Canonical tag queries.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::types::{mask_from_db_bits, NamespaceSummary, TagInfo, TagRecord, TagRelation};

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
    if query.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT tag_id, namespace, subtag, file_count, site_mask
             FROM tag
             WHERE file_count > 0
             ORDER BY file_count DESC, subtag ASC, tag_id ASC
             LIMIT ?1 OFFSET ?2",
        )?;
        return stmt
            .query_map(params![limit, offset], map_tag_record)?
            .collect();
    }

    let fts_query = format!("{}*", query.replace('"', ""));
    let mut stmt = conn.prepare(
        "SELECT t.tag_id, t.namespace, t.subtag, t.file_count, t.site_mask
         FROM tag_fts fts
         JOIN tag t ON t.tag_id = fts.rowid
         WHERE tag_fts MATCH ?1
         ORDER BY rank
         LIMIT ?2 OFFSET ?3",
    )?;
    let rows = stmt.query_map(params![fts_query, limit, offset], map_tag_record)?;
    rows.collect()
}

pub fn get_all_tags_with_counts(conn: &Connection) -> rusqlite::Result<Vec<TagRecord>> {
    let mut stmt = conn.prepare(
        "SELECT tag_id, namespace, subtag, file_count, site_mask
         FROM tag
         WHERE file_count > 0
         ORDER BY file_count DESC, subtag ASC, tag_id ASC",
    )?;
    let rows = stmt.query_map([], map_tag_record)?;
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
    if let Some(query) = search {
        if !query.is_empty() {
            let fts_query = format!("{}*", query.replace('"', ""));
            let (sql, use_ns) = match namespace {
                Some(_) => (
                    "SELECT t.tag_id, t.namespace, t.subtag, t.file_count, t.site_mask
                     FROM tag_fts fts
                     JOIN tag t ON t.tag_id = fts.rowid
                     WHERE tag_fts MATCH ?1 AND t.namespace = ?2
                     ORDER BY t.subtag ASC, t.tag_id ASC
                     LIMIT ?3",
                    true,
                ),
                None => (
                    "SELECT t.tag_id, t.namespace, t.subtag, t.file_count, t.site_mask
                     FROM tag_fts fts
                     JOIN tag t ON t.tag_id = fts.rowid
                     WHERE tag_fts MATCH ?1
                     ORDER BY t.subtag ASC, t.tag_id ASC
                     LIMIT ?2",
                    false,
                ),
            };
            let mut stmt = conn.prepare(sql)?;
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

    let has_cursor = cursor_subtag.is_some();
    let has_namespace = namespace.is_some();

    let sql = match (has_namespace, has_cursor) {
        (false, false) => {
            "SELECT tag_id, namespace, subtag, file_count, site_mask FROM tag
             ORDER BY subtag ASC, tag_id ASC LIMIT ?1"
        }
        (false, true) => {
            "SELECT tag_id, namespace, subtag, file_count, site_mask FROM tag
             WHERE (subtag, tag_id) > (?1, ?2)
             ORDER BY subtag ASC, tag_id ASC LIMIT ?3"
        }
        (true, false) => {
            "SELECT tag_id, namespace, subtag, file_count, site_mask FROM tag
             WHERE namespace = ?1
             ORDER BY subtag ASC, tag_id ASC LIMIT ?2"
        }
        (true, true) => {
            "SELECT tag_id, namespace, subtag, file_count, site_mask FROM tag
             WHERE namespace = ?1 AND (subtag, tag_id) > (?2, ?3)
             ORDER BY subtag ASC, tag_id ASC LIMIT ?4"
        }
    };

    let mut stmt = conn.prepare(sql)?;
    match (has_namespace, has_cursor) {
        (false, false) => stmt.query_map(params![limit], map_tag_record)?.collect(),
        (false, true) => stmt
            .query_map(
                params![cursor_subtag.as_deref().unwrap(), cursor_id.unwrap(), limit],
                map_tag_record,
            )?
            .collect(),
        (true, false) => stmt
            .query_map(params![namespace.unwrap(), limit], map_tag_record)?
            .collect(),
        (true, true) => stmt
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
