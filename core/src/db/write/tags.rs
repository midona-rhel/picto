//! Tag write operations — add, remove, rename, delete, merge, alias, implication.
//! Tags are stored directly on media entities.

use rusqlite::{params, Connection};

use crate::db::types::{mask_to_db_bits, TagChange, TagStructureChange};

/// Add tags to entities.
pub fn add_tags(
    conn: &Connection,
    entity_ids: &[i64],
    tag_strings: &[String],
    provenance_mask: u64,
) -> rusqlite::Result<TagChange> {
    let mut change = TagChange::default();

    for tag_str in tag_strings {
        let tag_id = get_or_create_tag(conn, tag_str)?;
        for eid in entity_ids {
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
    change.entity_ids = entity_ids.to_vec();

    Ok(change)
}

/// Remove tags from entities.
pub fn remove_tags(
    conn: &Connection,
    entity_ids: &[i64],
    tag_strings: &[String],
) -> rusqlite::Result<TagChange> {
    let mut change = TagChange::default();

    for tag_str in tag_strings {
        if let Some(tag_id) = find_tag(conn, tag_str)? {
            for eid in entity_ids {
                conn.execute(
                    "DELETE FROM entity_tag WHERE entity_id = ?1 AND tag_id = ?2",
                    params![eid, tag_id],
                )?;
            }
            change.tag_ids.push(tag_id);
            change.tags_removed.push(tag_str.clone());
        }
    }
    change.entity_ids = entity_ids.to_vec();

    Ok(change)
}

/// Rename a tag by ID.
pub fn rename_tag(
    conn: &Connection,
    tag_id: i64,
    new_name: &str,
) -> rusqlite::Result<TagStructureChange> {
    let (ns, st) = parse_tag(new_name)?;
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
        merge_tag_rows(conn, tag_id, target_id)?;
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
    if from_tag_id == to_tag_id {
        return Err(rusqlite::Error::InvalidParameterName(
            "cannot merge a tag into itself".to_string(),
        ));
    }

    let mut stmt = conn.prepare(
        "SELECT entity_id FROM entity_tag WHERE tag_id IN (?1, ?2)
         UNION
         SELECT entity_id FROM entity_tag_implied WHERE tag_id IN (?1, ?2)",
    )?;
    let affected: Vec<i64> = stmt
        .query_map(params![from_tag_id, to_tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    merge_tag_rows(conn, from_tag_id, to_tag_id)?;
    Ok(TagStructureChange {
        entity_ids: affected,
        dirty_tag_ids: vec![from_tag_id, to_tag_id],
        merged_into_tag_id: Some(to_tag_id),
    })
}

fn merge_tag_rows(conn: &Connection, from_tag_id: i64, to_tag_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO entity_tag (entity_id, tag_id, provenance_mask, source)
         SELECT entity_id, ?1, provenance_mask, source
         FROM entity_tag
         WHERE tag_id = ?2
         ON CONFLICT(entity_id, tag_id, source)
         DO UPDATE SET provenance_mask = entity_tag.provenance_mask | excluded.provenance_mask",
        params![to_tag_id, from_tag_id],
    )?;
    conn.execute("DELETE FROM entity_tag WHERE tag_id = ?1", [from_tag_id])?;

    conn.execute(
        "UPDATE tag_alias
         SET to_tag_id = ?1
         WHERE to_tag_id = ?2 AND from_tag_id <> ?1",
        params![to_tag_id, from_tag_id],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO tag_alias (from_tag_id, to_tag_id, source)
         SELECT ?1, to_tag_id, source
         FROM tag_alias
         WHERE from_tag_id = ?2 AND to_tag_id <> ?1",
        params![to_tag_id, from_tag_id],
    )?;
    conn.execute(
        "DELETE FROM tag_alias WHERE from_tag_id = ?1 OR to_tag_id = ?1",
        [from_tag_id],
    )?;

    conn.execute(
        "INSERT OR IGNORE INTO tag_implication (child_tag_id, parent_tag_id, source)
         SELECT child_tag_id, ?1, source
         FROM tag_implication
         WHERE parent_tag_id = ?2 AND child_tag_id <> ?1",
        params![to_tag_id, from_tag_id],
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO tag_implication (child_tag_id, parent_tag_id, source)
         SELECT ?1, parent_tag_id, source
         FROM tag_implication
         WHERE child_tag_id = ?2 AND parent_tag_id <> ?1",
        params![to_tag_id, from_tag_id],
    )?;
    conn.execute(
        "DELETE FROM tag_implication WHERE child_tag_id = ?1 OR parent_tag_id = ?1",
        [from_tag_id],
    )?;

    conn.execute("DELETE FROM tag WHERE tag_id = ?1", [from_tag_id])?;
    Ok(())
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

/// Resolve or create a tag by display string.
pub fn ensure_tag(conn: &Connection, tag_str: &str) -> rusqlite::Result<i64> {
    get_or_create_tag(conn, tag_str)
}

// ── Helpers ──────────────────────────────────────────────────────

fn get_or_create_tag(conn: &Connection, tag_str: &str) -> rusqlite::Result<i64> {
    let (ns, st) = parse_tag(tag_str)?;
    if let Ok(id) = conn.query_row(
        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
        params![ns, st],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)",
        params![ns, st],
    )?;
    Ok(conn.last_insert_rowid())
}

fn find_tag(conn: &Connection, tag_str: &str) -> rusqlite::Result<Option<i64>> {
    let (ns, st) = parse_tag(tag_str)?;
    conn.query_row(
        "SELECT tag_id FROM tag WHERE namespace = ?1 AND subtag = ?2",
        params![ns, st],
        |row| row.get(0),
    )
    .optional()
}

fn parse_tag(s: &str) -> rusqlite::Result<(String, String)> {
    crate::tags::normalize::parse_tag(s)
        .ok_or_else(|| rusqlite::Error::InvalidParameterName(format!("Invalid tag: {s}")))
}

use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::core::schema::LIBRARY_DDL;
    use crate::db::types::{mask_to_db_bits, TAG_PROVENANCE_AI, TAG_PROVENANCE_MANUAL};

    fn setup_conn() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory db");
        conn.execute_batch(LIBRARY_DDL).expect("create schema");
        conn.execute(
            "INSERT INTO media_file
             (file_id, file_hash, mime_type, size_bytes, date_added)
             VALUES (1, 'hash_1', 'image/png', 1, '2026-01-01T00:00:00Z')",
            [],
        )
        .expect("insert file");
        conn.execute(
            "INSERT INTO media_entity
             (entity_id, entity_hash, file_id, status, date_created, date_added, date_modified)
             VALUES (1, 'hash_1', 1, 1, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
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
        )
        .expect("add manual tag");
        add_tags(
            &conn,
            &[1],
            &["tag_a".to_string()],
            TAG_PROVENANCE_AI,
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
    fn merge_combines_provenance_and_rewires_relationships() {
        let conn = setup_conn();
        let from = ensure_tag(&conn, "from").unwrap();
        let into = ensure_tag(&conn, "into").unwrap();
        let alias_in = ensure_tag(&conn, "alias_in").unwrap();
        let alias_out = ensure_tag(&conn, "alias_out").unwrap();
        let child = ensure_tag(&conn, "child").unwrap();
        let parent = ensure_tag(&conn, "parent").unwrap();

        add_tags(
            &conn,
            &[1],
            &["from".to_string()],
            TAG_PROVENANCE_AI,
        )
        .unwrap();
        add_tags(
            &conn,
            &[1],
            &["into".to_string()],
            TAG_PROVENANCE_MANUAL,
        )
        .unwrap();
        manage_alias(&conn, alias_in, Some(from)).unwrap();
        manage_alias(&conn, from, Some(alias_out)).unwrap();
        manage_implication(&conn, child, from, true).unwrap();
        manage_implication(&conn, from, parent, true).unwrap();

        merge_tags(&conn, from, into).unwrap();

        let provenance: i64 = conn
            .query_row(
                "SELECT provenance_mask FROM entity_tag WHERE entity_id = 1 AND tag_id = ?1",
                [into],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            provenance,
            mask_to_db_bits(TAG_PROVENANCE_MANUAL | TAG_PROVENANCE_AI)
        );
        let aliases = conn
            .prepare("SELECT from_tag_id, to_tag_id FROM tag_alias ORDER BY from_tag_id")
            .unwrap()
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();
        assert!(aliases.contains(&(alias_in, into)), "aliases: {aliases:?}");
        assert!(aliases.contains(&(into, alias_out)), "aliases: {aliases:?}");
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM tag_implication
                 WHERE (child_tag_id = ?1 AND parent_tag_id = ?2)
                    OR (child_tag_id = ?2 AND parent_tag_id = ?3)",
                params![child, into, parent],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM tag WHERE tag_id = ?1",
                [from],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn merge_rejects_the_same_tag_without_mutating_it() {
        let conn = setup_conn();
        let tag_id = ensure_tag(&conn, "same").unwrap();
        add_tags(
            &conn,
            &[1],
            &["same".to_string()],
            TAG_PROVENANCE_MANUAL,
        )
        .unwrap();

        assert!(merge_tags(&conn, tag_id, tag_id).is_err());
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM tag WHERE tag_id = ?1",
                [tag_id],
                |row| { row.get::<_, i64>(0) }
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM entity_tag WHERE tag_id = ?1",
                [tag_id],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );
    }
}
