//! Smart folder write operations — CRUD only.
//! Smart folder membership is derived (not authoritative).

use rusqlite::{params, Connection, OptionalExtension};

pub fn create_smart_folder(
    conn: &Connection,
    name: &str,
    parent_id: Option<i64>,
    predicate_json: &str,
    icon: Option<&str>,
    color: Option<&str>,
    notes: Option<&str>,
    now: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO smart_folder (name, parent_id, predicate_json, icon, color, notes, date_added, date_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
        params![name, parent_id, predicate_json, icon, color, notes, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_smart_folder(
    conn: &Connection,
    smart_folder_id: i64,
    name: Option<&str>,
    predicate_json: Option<&str>,
    icon: Option<&str>,
    color: Option<&str>,
    notes: Option<&str>,
    sort_field: Option<&str>,
    sort_order: Option<&str>,
    now: &str,
) -> rusqlite::Result<()> {
    if let Some(n) = name {
        conn.execute(
            "UPDATE smart_folder SET name = ?1, date_modified = ?2 WHERE smart_folder_id = ?3",
            params![n, now, smart_folder_id],
        )?;
    }
    if let Some(p) = predicate_json {
        conn.execute("UPDATE smart_folder SET predicate_json = ?1, date_modified = ?2 WHERE smart_folder_id = ?3", params![p, now, smart_folder_id])?;
    }
    if let Some(i) = icon {
        conn.execute(
            "UPDATE smart_folder SET icon = ?1, date_modified = ?2 WHERE smart_folder_id = ?3",
            params![i, now, smart_folder_id],
        )?;
    }
    if let Some(c) = color {
        conn.execute(
            "UPDATE smart_folder SET color = ?1, date_modified = ?2 WHERE smart_folder_id = ?3",
            params![c, now, smart_folder_id],
        )?;
    }
    if let Some(n) = notes {
        conn.execute(
            "UPDATE smart_folder SET notes = ?1, date_modified = ?2 WHERE smart_folder_id = ?3",
            params![n, now, smart_folder_id],
        )?;
    }
    if let Some(sf) = sort_field {
        conn.execute(
            "UPDATE smart_folder SET sort_field = ?1 WHERE smart_folder_id = ?2",
            params![sf, smart_folder_id],
        )?;
    }
    if let Some(so) = sort_order {
        conn.execute(
            "UPDATE smart_folder SET sort_order = ?1 WHERE smart_folder_id = ?2",
            params![so, smart_folder_id],
        )?;
    }
    Ok(())
}

/// Delete a smart folder and promote its children to its parent.
/// Returns (promoted_child_ids, deleted_parent_id).
pub fn delete_smart_folder(
    conn: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<(Vec<i64>, Option<i64>)> {
    // Read parent before delete
    let parent_id: Option<i64> = conn
        .query_row(
            "SELECT parent_id FROM smart_folder WHERE smart_folder_id = ?1",
            [smart_folder_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    // Find direct children
    let mut stmt = conn.prepare("SELECT smart_folder_id FROM smart_folder WHERE parent_id = ?1")?;
    let children: Vec<i64> = stmt
        .query_map([smart_folder_id], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    // Promote children to deleted folder's parent
    for &child_id in &children {
        conn.execute(
            "UPDATE smart_folder SET parent_id = ?1 WHERE smart_folder_id = ?2",
            params![parent_id, child_id],
        )?;
    }
    // Delete the folder
    conn.execute(
        "DELETE FROM smart_folder WHERE smart_folder_id = ?1",
        [smart_folder_id],
    )?;
    Ok((children, parent_id))
}

pub fn move_smart_folder(
    conn: &Connection,
    smart_folder_id: i64,
    new_parent_id: Option<i64>,
    now: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE smart_folder SET parent_id = ?1, date_modified = ?2 WHERE smart_folder_id = ?3",
        params![new_parent_id, now, smart_folder_id],
    )?;
    Ok(())
}

pub fn reorder_smart_folders(conn: &Connection, moves: &[(i64, i64)]) -> rusqlite::Result<()> {
    for (sf_id, display_order) in moves {
        conn.execute(
            "UPDATE smart_folder SET display_order = ?1 WHERE smart_folder_id = ?2",
            params![display_order, sf_id],
        )?;
    }
    Ok(())
}
