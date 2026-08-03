//! Canonical metadata helpers for legacy-compatible detail payloads.

use rusqlite::{Connection, OptionalExtension};

use crate::types::{tag_display_key, TagInfo};

pub fn get_implied_tags(conn: &Connection, entity_hash: &str) -> rusqlite::Result<Vec<TagInfo>> {
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
        "SELECT t.tag_id, t.namespace, t.subtag,
                COALESCE(td.display_ns, t.namespace),
                COALESCE(td.display_st, t.subtag)
         FROM entity_tag_implied eti
         JOIN tag t ON t.tag_id = eti.tag_id
         LEFT JOIN tag_display td ON td.tag_id = t.tag_id
         WHERE eti.entity_id = ?1
         ORDER BY t.namespace, t.subtag",
    )?;
    let tags = stmt.query_map([entity_id], |row| {
        let namespace: String = row.get(1)?;
        let subtag: String = row.get(2)?;
        let display_ns: String = row.get(3)?;
        let display_st: String = row.get(4)?;
        Ok(TagInfo {
            tag_id: row.get(0)?,
            namespace,
            subtag,
            display: tag_display_key(&display_ns, &display_st),
            file_count: 0,
            read_only: true,
        })
    })?;
    tags.collect()
}
