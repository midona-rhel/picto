//! Tag write operations — add, remove, rename, delete, merge, alias, implication.
//! Tags are stored on single entities only. Collection commands expand to members.

use rusqlite::{params, Connection};

use crate::db::types::{mask_to_db_bits, ExpansionMode, TagChange, TagStructureChange};

use super::entities::expand_ids;

/// Add tags to entities. Expansion determines whether collection members are included.
pub fn add_tags(
    conn: &Connection,
    entity_ids: &[i64],
    tag_strings: &[String],
    provenance_mask: u64,
    expansion: ExpansionMode,
) -> rusqlite::Result<TagChange> {
    let expanded = expand_ids(conn, entity_ids, expansion)?;
    let mut change = TagChange::default();

    for tag_str in tag_strings {
        let tag_id = get_or_create_tag(conn, tag_str)?;
        for eid in &expanded {
            conn.execute(
                "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source)
                 VALUES (?1, ?2, ?3, 'local')
                 ON CONFLICT(entity_id, tag_id, source)
                 DO UPDATE SET provenance_mask = (entity_tag.provenance_mask | excluded.provenance_mask)",
                params![eid, tag_id, mask_to_db_bits(provenance_mask)],
            )?;
        }
        change.tag_ids.push(tag_id);
        change.tags_added.push(tag_str.clone());
    }
    change.entity_ids = expanded;

    // Update file_count on tag rows
    for tid in &change.tag_ids {
        update_tag_count(conn, *tid)?;
    }

    Ok(change)
}

/// Remove tags from entities.
pub fn remove_tags(
    conn: &Connection,
    entity_ids: &[i64],
    tag_strings: &[String],
    expansion: ExpansionMode,
) -> rusqlite::Result<TagChange> {
    let expanded = expand_ids(conn, entity_ids, expansion)?;
    let mut change = TagChange::default();

    for tag_str in tag_strings {
        if let Some(tag_id) = find_tag(conn, tag_str)? {
            for eid in &expanded {
                conn.execute(
                    "DELETE FROM entity_tag WHERE entity_id = ?1 AND tag_id = ?2",
                    params![eid, tag_id],
                )?;
            }
            change.tag_ids.push(tag_id);
            change.tags_removed.push(tag_str.clone());
            update_tag_count(conn, tag_id)?;
        }
    }
    change.entity_ids = expanded;

    Ok(change)
}

/// Rename a tag by ID.
pub fn rename_tag(
    conn: &Connection,
    tag_id: i64,
    new_name: &str,
) -> rusqlite::Result<TagStructureChange> {
    let (ns, st) = parse_tag(new_name);
    let mut affected_stmt = conn.prepare("SELECT entity_id FROM entity_tag WHERE tag_id = ?1")?;
    let affected_entity_ids: Vec<i64> = affected_stmt
        .query_map([tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Check if target already exists
    let existing: Option<i64> = conn
        .query_row(
            "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
            params![ns, st],
            |row| row.get(0),
        )
        .ok();

    if let Some(target_id) = existing {
        if target_id == tag_id {
            return Ok(TagStructureChange {
                entity_ids: Vec::new(),
                dirty_tag_ids: vec![tag_id],
                merged_into_tag_id: None,
            });
        }
        conn.execute(
            "UPDATE tag
             SET site_mask = (
                SELECT COALESCE((SELECT site_mask FROM tag WHERE tag_id = ?2), 0) | COALESCE((SELECT site_mask FROM tag WHERE tag_id = ?1), 0)
             )
             WHERE tag_id = ?2",
            params![tag_id, target_id],
        )?;
        // Merge into existing tag
        conn.execute(
            "UPDATE OR IGNORE entity_tag SET tag_id = ?1 WHERE tag_id = ?2",
            params![target_id, tag_id],
        )?;
        conn.execute("DELETE FROM entity_tag WHERE tag_id = ?1", params![tag_id])?;
        conn.execute("DELETE FROM tag WHERE tag_id = ?1", params![tag_id])?;
        update_tag_count(conn, target_id)?;
        Ok(TagStructureChange {
            entity_ids: affected_entity_ids,
            dirty_tag_ids: vec![tag_id, target_id],
            merged_into_tag_id: Some(target_id),
        })
    } else {
        conn.execute(
            "UPDATE tag SET namespace = ?1, subtag = ?2 WHERE tag_id = ?3",
            params![ns, st, tag_id],
        )?;
        Ok(TagStructureChange {
            entity_ids: affected_entity_ids,
            dirty_tag_ids: vec![tag_id],
            merged_into_tag_id: None,
        })
    }
}

/// Delete a tag entirely.
pub fn delete_tag(conn: &Connection, tag_id: i64) -> rusqlite::Result<TagStructureChange> {
    let mut stmt = conn.prepare("SELECT entity_id FROM entity_tag WHERE tag_id = ?1")?;
    let affected: Vec<i64> = stmt
        .query_map([tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    conn.execute("DELETE FROM entity_tag WHERE tag_id = ?1", [tag_id])?;
    conn.execute(
        "DELETE FROM tag_alias WHERE from_tag_id = ?1 OR to_tag_id = ?1",
        [tag_id],
    )?;
    conn.execute(
        "DELETE FROM tag_implication WHERE child_tag_id = ?1 OR parent_tag_id = ?1",
        [tag_id],
    )?;
    conn.execute("DELETE FROM tag WHERE tag_id = ?1", [tag_id])?;

    Ok(TagStructureChange {
        entity_ids: affected,
        dirty_tag_ids: vec![tag_id],
        merged_into_tag_id: None,
    })
}

/// Merge one tag into another.
pub fn merge_tags(
    conn: &Connection,
    from_tag_id: i64,
    to_tag_id: i64,
) -> rusqlite::Result<TagStructureChange> {
    let mut stmt = conn.prepare("SELECT entity_id FROM entity_tag WHERE tag_id = ?1")?;
    let affected: Vec<i64> = stmt
        .query_map([from_tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    conn.execute(
        "UPDATE tag
         SET site_mask = (
            SELECT COALESCE((SELECT site_mask FROM tag WHERE tag_id = ?2), 0) | COALESCE((SELECT site_mask FROM tag WHERE tag_id = ?1), 0)
         )
         WHERE tag_id = ?2",
        params![from_tag_id, to_tag_id],
    )?;
    conn.execute(
        "UPDATE OR IGNORE entity_tag SET tag_id = ?1 WHERE tag_id = ?2",
        params![to_tag_id, from_tag_id],
    )?;
    conn.execute("DELETE FROM entity_tag WHERE tag_id = ?1", [from_tag_id])?;
    conn.execute("DELETE FROM tag WHERE tag_id = ?1", [from_tag_id])?;
    update_tag_count(conn, to_tag_id)?;

    Ok(TagStructureChange {
        entity_ids: affected,
        dirty_tag_ids: vec![from_tag_id, to_tag_id],
        merged_into_tag_id: Some(to_tag_id),
    })
}

/// Set or remove a tag alias.
pub fn manage_alias(
    conn: &Connection,
    from_tag_id: i64,
    to_tag_id: Option<i64>,
) -> rusqlite::Result<()> {
    if let Some(tid) = to_tag_id {
        conn.execute(
            "INSERT OR REPLACE INTO tag_alias (from_tag_id, to_tag_id, source) VALUES (?1, ?2, 'local')",
            params![from_tag_id, tid],
        )?;
    } else {
        conn.execute(
            "DELETE FROM tag_alias WHERE from_tag_id = ?1 AND source = 'local'",
            [from_tag_id],
        )?;
    }
    Ok(())
}

/// Set or remove a tag implication.
pub fn manage_implication(
    conn: &Connection,
    child_tag_id: i64,
    parent_tag_id: i64,
    add: bool,
) -> rusqlite::Result<()> {
    if add {
        conn.execute(
            "INSERT OR IGNORE INTO tag_implication (child_tag_id, parent_tag_id, source) VALUES (?1, ?2, 'local')",
            params![child_tag_id, parent_tag_id],
        )?;
    } else {
        conn.execute(
            "DELETE FROM tag_implication WHERE child_tag_id = ?1 AND parent_tag_id = ?2 AND source = 'local'",
            params![child_tag_id, parent_tag_id],
        )?;
    }
    Ok(())
}

/// Set concept-level site support for a tag.
pub fn set_tag_site_mask(conn: &Connection, tag_id: i64, site_mask: u64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tag SET site_mask = ?1 WHERE tag_id = ?2",
        params![mask_to_db_bits(site_mask), tag_id],
    )?;
    Ok(())
}

/// Resolve or create a tag by display string.
pub fn ensure_tag(conn: &Connection, tag_str: &str) -> rusqlite::Result<i64> {
    get_or_create_tag(conn, tag_str)
}

// ── Helpers ──────────────────────────────────────────────────────

fn get_or_create_tag(conn: &Connection, tag_str: &str) -> rusqlite::Result<i64> {
    let (ns, st) = parse_tag(tag_str);
    if let Ok(id) = conn.query_row(
        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
        params![ns, st],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO tag (namespace, subtag, site_mask) VALUES (?1, ?2, 0)",
        params![ns, st],
    )?;
    Ok(conn.last_insert_rowid())
}

fn find_tag(conn: &Connection, tag_str: &str) -> rusqlite::Result<Option<i64>> {
    let (ns, st) = parse_tag(tag_str);
    conn.query_row(
        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
        params![ns, st],
        |row| row.get(0),
    )
    .optional()
}

fn update_tag_count(conn: &Connection, tag_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE tag SET file_count = (SELECT COUNT(*) FROM entity_tag WHERE tag_id = ?1) WHERE tag_id = ?1",
        [tag_id],
    )?;
    Ok(())
}

fn parse_tag(s: &str) -> (String, String) {
    if let Some(idx) = s.find(':') {
        let ns = &s[..idx];
        let st = &s[idx + 1..];
        if ns.is_empty() {
            (String::new(), s.to_string())
        } else {
            (ns.to_string(), st.to_string())
        }
    } else {
        (String::new(), s.to_string())
    }
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::core::schema::LIBRARY_DDL;
    use crate::db::types::{
        mask_to_db_bits, TAG_PROVENANCE_AI, TAG_PROVENANCE_MANUAL, TAG_SITE_E621,
    };

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute(
            "INSERT INTO media_entity
             (entity_id, entity_hash, entity_kind, status, date_created, date_added, date_modified)
             VALUES (1, 'hash_1', 'single', 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("insert entity");
        conn
    }

    #[test]
    fn add_tags_persists_and_ors_provenance_mask() {
        let conn = setup_conn();

        add_tags(
            &conn,
            &[1],
            &["tag_a".to_string()],
            TAG_PROVENANCE_MANUAL,
            ExpansionMode::EntityOnly,
        )
        .expect("add manual tag");
        add_tags(
            &conn,
            &[1],
            &["tag_a".to_string()],
            TAG_PROVENANCE_AI,
            ExpansionMode::EntityOnly,
        )
        .expect("add ai provenance");

        let mask: i64 = conn
            .query_row(
                "SELECT provenance_mask FROM entity_tag WHERE entity_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("get provenance mask");
        assert_eq!(
            mask,
            mask_to_db_bits(TAG_PROVENANCE_MANUAL | TAG_PROVENANCE_AI)
        );
    }

    #[test]
    fn removing_entity_tag_does_not_clear_tag_site_mask() {
        let conn = setup_conn();

        add_tags(
            &conn,
            &[1],
            &["tag_a".to_string()],
            TAG_PROVENANCE_MANUAL,
            ExpansionMode::EntityOnly,
        )
        .expect("add tag");
        let tag_id: i64 = conn
            .query_row("SELECT tag_id FROM tag WHERE subtag = 'tag_a'", [], |row| {
                row.get(0)
            })
            .expect("get tag id");
        set_tag_site_mask(&conn, tag_id, TAG_SITE_E621).expect("set site mask");

        remove_tags(
            &conn,
            &[1],
            &["tag_a".to_string()],
            ExpansionMode::EntityOnly,
        )
        .expect("remove tag");

        let site_mask: i64 = conn
            .query_row(
                "SELECT site_mask FROM tag WHERE tag_id = ?1",
                [tag_id],
                |row| row.get(0),
            )
            .expect("get site mask");
        assert_eq!(site_mask, mask_to_db_bits(TAG_SITE_E621));
    }
}
