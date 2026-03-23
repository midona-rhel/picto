//! Smart folder write operations — CRUD only.
//! Smart folder membership is derived (not authoritative).

use rusqlite::{params, Connection};

pub fn create_smart_folder(
    conn: &Connection,
    name: &str,
    parent_id: Option<i64>,
    predicate_json: &str,
    icon: Option<&str>,
    color: Option<&str>,
    now: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO smart_folder (name, parent_id, predicate_json, icon, color, date_added, date_modified)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
        params![name, parent_id, predicate_json, icon, color, now],
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
    sort_field: Option<&str>,
    sort_order: Option<&str>,
    now: &str,
) -> rusqlite::Result<()> {
    if let Some(n) = name {
        conn.execute("UPDATE smart_folder SET name = ?1, date_modified = ?2 WHERE smart_folder_id = ?3", params![n, now, smart_folder_id])?;
    }
    if let Some(p) = predicate_json {
        conn.execute("UPDATE smart_folder SET predicate_json = ?1, date_modified = ?2 WHERE smart_folder_id = ?3", params![p, now, smart_folder_id])?;
    }
    if let Some(i) = icon {
        conn.execute("UPDATE smart_folder SET icon = ?1, date_modified = ?2 WHERE smart_folder_id = ?3", params![i, now, smart_folder_id])?;
    }
    if let Some(c) = color {
        conn.execute("UPDATE smart_folder SET color = ?1, date_modified = ?2 WHERE smart_folder_id = ?3", params![c, now, smart_folder_id])?;
    }
    if let Some(sf) = sort_field {
        conn.execute("UPDATE smart_folder SET sort_field = ?1 WHERE smart_folder_id = ?2", params![sf, smart_folder_id])?;
    }
    if let Some(so) = sort_order {
        conn.execute("UPDATE smart_folder SET sort_order = ?1 WHERE smart_folder_id = ?2", params![so, smart_folder_id])?;
    }
    Ok(())
}

pub fn delete_smart_folder(conn: &Connection, smart_folder_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM smart_folder WHERE smart_folder_id = ?1", [smart_folder_id])?;
    Ok(())
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
