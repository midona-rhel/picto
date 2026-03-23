//! Folder write operations — CRUD, membership, reorder.
//! Folder membership (folder_member) stores single entity IDs only.

use rusqlite::{params, Connection};

use crate::db::types::{ExpansionMode, FolderMembershipChange};

use super::entities::expand_ids;

pub fn create_folder(
    conn: &Connection,
    name: &str,
    parent_id: Option<i64>,
    icon: Option<&str>,
    color: Option<&str>,
    now: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT INTO folder (name, parent_id, icon, color, date_added, date_modified) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params![name, parent_id, icon, color, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_folder(
    conn: &Connection,
    folder_id: i64,
    name: Option<&str>,
    icon: Option<&str>,
    color: Option<&str>,
    auto_tags: Option<&str>,
    now: &str,
) -> rusqlite::Result<()> {
    if let Some(n) = name {
        conn.execute(
            "UPDATE folder SET name = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![n, now, folder_id],
        )?;
    }
    if let Some(i) = icon {
        conn.execute(
            "UPDATE folder SET icon = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![i, now, folder_id],
        )?;
    }
    if let Some(c) = color {
        conn.execute(
            "UPDATE folder SET color = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![c, now, folder_id],
        )?;
    }
    if let Some(at) = auto_tags {
        conn.execute(
            "UPDATE folder SET auto_tags = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![at, now, folder_id],
        )?;
    }
    Ok(())
}

pub fn delete_folder(conn: &Connection, folder_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM folder_member WHERE folder_id = ?1", [folder_id])?;
    conn.execute("DELETE FROM folder WHERE folder_id = ?1", [folder_id])?;
    Ok(())
}

/// Add entities to a folder. Expands collections to member singles.
pub fn add_members(
    conn: &Connection,
    folder_id: i64,
    entity_ids: &[i64],
    expansion: ExpansionMode,
) -> rusqlite::Result<FolderMembershipChange> {
    let expanded = expand_ids(conn, entity_ids, expansion)?;

    // Only insert singles (entity_kind = 'single')
    for eid in &expanded {
        let kind: String = conn.query_row(
            "SELECT entity_kind FROM media_entity WHERE entity_id = ?1",
            [eid],
            |row| row.get(0),
        )?;
        if kind == "single" {
            conn.execute(
                "INSERT OR IGNORE INTO folder_member (folder_id, entity_id) VALUES (?1, ?2)",
                params![folder_id, eid],
            )?;
        }
    }

    Ok(FolderMembershipChange {
        folder_id,
        entity_ids: expanded,
    })
}

/// Remove entities from a folder.
pub fn remove_members(
    conn: &Connection,
    folder_id: i64,
    entity_ids: &[i64],
    expansion: ExpansionMode,
) -> rusqlite::Result<FolderMembershipChange> {
    let expanded = expand_ids(conn, entity_ids, expansion)?;

    for eid in &expanded {
        conn.execute(
            "DELETE FROM folder_member WHERE folder_id = ?1 AND entity_id = ?2",
            params![folder_id, eid],
        )?;
    }

    Ok(FolderMembershipChange {
        folder_id,
        entity_ids: expanded,
    })
}

/// Reorder folder members by position_rank.
pub fn reorder_members(
    conn: &Connection,
    folder_id: i64,
    moves: &[(i64, i64)],
) -> rusqlite::Result<()> {
    for (entity_id, rank) in moves {
        conn.execute(
            "UPDATE folder_member SET position_rank = ?1 WHERE folder_id = ?2 AND entity_id = ?3",
            params![rank, folder_id, entity_id],
        )?;
    }
    Ok(())
}

/// Move a folder to a new parent.
pub fn move_folder(
    conn: &Connection,
    folder_id: i64,
    new_parent_id: Option<i64>,
    now: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE folder SET parent_id = ?1, date_modified = ?2 WHERE folder_id = ?3",
        params![new_parent_id, now, folder_id],
    )?;
    Ok(())
}

/// Reorder folders (sibling sort order).
pub fn reorder_folders(conn: &Connection, moves: &[(i64, i64)]) -> rusqlite::Result<()> {
    for (folder_id, sort_order) in moves {
        conn.execute(
            "UPDATE folder SET sort_order = ?1 WHERE folder_id = ?2",
            params![sort_order, folder_id],
        )?;
    }
    Ok(())
}
