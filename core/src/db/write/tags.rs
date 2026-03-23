//! Tag write operations — add, remove, rename, delete, merge, alias, implication.
//! Tags are stored on single entities only. Collection commands expand to members.

use rusqlite::{params, Connection};

use crate::db::types::{ExpansionMode, TagChange};

use super::entities::expand_ids;

/// Add tags to entities. Expansion determines whether collection members are included.
pub fn add_tags(
    conn: &Connection,
    entity_ids: &[i64],
    tag_strings: &[String],
    expansion: ExpansionMode,
) -> rusqlite::Result<TagChange> {
    let expanded = expand_ids(conn, entity_ids, expansion)?;
    let mut change = TagChange::default();

    for tag_str in tag_strings {
        let tag_id = get_or_create_tag(conn, tag_str)?;
        for eid in &expanded {
            conn.execute(
                "INSERT OR IGNORE INTO entity_tag (entity_id, tag_id, source) VALUES (?1, ?2, 'local')",
                params![eid, tag_id],
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
) -> rusqlite::Result<Option<i64>> {
    let (ns, st) = parse_tag(new_name);

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
            return Ok(None);
        }
        // Merge into existing tag
        conn.execute(
            "UPDATE OR IGNORE entity_tag SET tag_id = ?1 WHERE tag_id = ?2",
            params![target_id, tag_id],
        )?;
        conn.execute("DELETE FROM entity_tag WHERE tag_id = ?1", params![tag_id])?;
        conn.execute("DELETE FROM tag WHERE tag_id = ?1", params![tag_id])?;
        update_tag_count(conn, target_id)?;
        Ok(Some(target_id))
    } else {
        conn.execute(
            "UPDATE tag SET namespace = ?1, subtag = ?2 WHERE tag_id = ?3",
            params![ns, st, tag_id],
        )?;
        Ok(None)
    }
}

/// Delete a tag entirely.
pub fn delete_tag(conn: &Connection, tag_id: i64) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT entity_id FROM entity_tag WHERE tag_id = ?1")?;
    let affected: Vec<i64> = stmt
        .query_map([tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    conn.execute("DELETE FROM entity_tag WHERE tag_id = ?1", [tag_id])?;
    conn.execute("DELETE FROM tag_alias WHERE from_tag_id = ?1 OR to_tag_id = ?1", [tag_id])?;
    conn.execute("DELETE FROM tag_implication WHERE child_tag_id = ?1 OR parent_tag_id = ?1", [tag_id])?;
    conn.execute("DELETE FROM tag WHERE tag_id = ?1", [tag_id])?;

    Ok(affected)
}

/// Merge one tag into another.
pub fn merge_tags(
    conn: &Connection,
    from_tag_id: i64,
    to_tag_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut stmt = conn.prepare("SELECT entity_id FROM entity_tag WHERE tag_id = ?1")?;
    let affected: Vec<i64> = stmt
        .query_map([from_tag_id], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    conn.execute(
        "UPDATE OR IGNORE entity_tag SET tag_id = ?1 WHERE tag_id = ?2",
        params![to_tag_id, from_tag_id],
    )?;
    conn.execute("DELETE FROM entity_tag WHERE tag_id = ?1", [from_tag_id])?;
    conn.execute("DELETE FROM tag WHERE tag_id = ?1", [from_tag_id])?;
    update_tag_count(conn, to_tag_id)?;

    Ok(affected)
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
        "INSERT INTO tag (namespace, subtag) VALUES (?1, ?2)",
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
