//! Folder write operations — CRUD, membership, reorder.
//! Folder membership (folder_member) stores single entity IDs only.

use rusqlite::{params, Connection, OptionalExtension};

use crate::db::types::{ExpansionMode, FolderMembershipChange, FolderPatch};

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
        "INSERT INTO folder (name, parent_id, icon, color, notes, uuid, date_added, date_modified) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?6)",
        params![name, parent_id, icon, color, crate::oplog::new_uuid(), now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_folder(
    conn: &Connection,
    folder_id: i64,
    patch: &FolderPatch,
    now: &str,
) -> rusqlite::Result<()> {
    if let Some(ref n) = patch.name {
        conn.execute(
            "UPDATE folder SET name = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![n, now, folder_id],
        )?;
    }
    if let Some(ref i) = patch.icon {
        conn.execute(
            "UPDATE folder SET icon = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![i, now, folder_id],
        )?;
    }
    if let Some(ref c) = patch.color {
        conn.execute(
            "UPDATE folder SET color = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![c, now, folder_id],
        )?;
    }
    if let Some(ref at) = patch.auto_tags {
        conn.execute(
            "UPDATE folder SET auto_tags = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![at, now, folder_id],
        )?;
    }
    if let Some(ref n) = patch.notes {
        conn.execute(
            "UPDATE folder SET notes = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![n, now, folder_id],
        )?;
    }
    if let Some(ref wp) = patch.watch_path {
        conn.execute(
            "UPDATE folder SET watch_path = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![wp, now, folder_id],
        )?;
    }
    if let Some(we) = patch.watch_enabled {
        conn.execute(
            "UPDATE folder SET watch_enabled = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![we as i64, now, folder_id],
        )?;
    }
    if let Some(ws) = patch.watch_subfolders {
        conn.execute(
            "UPDATE folder SET watch_subfolders = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![ws as i64, now, folder_id],
        )?;
    }
    if let Some(ref wm) = patch.watch_import_status_mode {
        conn.execute("UPDATE folder SET watch_import_status_mode = ?1, date_modified = ?2 WHERE folder_id = ?3", params![wm, now, folder_id])?;
    }
    Ok(())
}

pub fn delete_folder(conn: &Connection, folder_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM folder_member WHERE folder_id = ?1",
        [folder_id],
    )?;
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

/// Walk up the parent chain from `start_id`. Returns true if `ancestor_id` is found.
pub fn is_ancestor_of(conn: &Connection, start_id: i64, ancestor_id: i64) -> rusqlite::Result<bool> {
    let mut current = start_id;
    for _ in 0..200 {
        let parent: Option<Option<i64>> = conn
            .query_row(
                "SELECT parent_id FROM folder WHERE folder_id = ?1",
                [current],
                |row| row.get(0),
            )
            .optional()?;
        match parent {
            Some(Some(pid)) => {
                if pid == ancestor_id { return Ok(true); }
                current = pid;
            }
            _ => return Ok(false),
        }
    }
    Ok(false)
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
