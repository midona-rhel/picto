//! Folder write operations — CRUD, membership, reorder.
//! Folder membership (folder_member) stores single entity IDs only.

use rusqlite::{params, Connection};

use crate::db::types::{ExpansionMode, FolderMembershipChange, FolderMirrorRecord};

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
        "INSERT INTO folder (name, parent_id, icon, color, notes, date_added, date_modified) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?5)",
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
    notes: Option<&str>,
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
    if let Some(n) = notes {
        conn.execute(
            "UPDATE folder SET notes = ?1, date_modified = ?2 WHERE folder_id = ?3",
            params![n, now, folder_id],
        )?;
    }
    Ok(())
}

pub fn delete_folder(conn: &Connection, folder_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM folder_member WHERE folder_id = ?1", [folder_id])?;
    conn.execute("DELETE FROM folder WHERE folder_id = ?1", [folder_id])?;
    Ok(())
}

pub fn upsert_folder_record(
    conn: &Connection,
    record: &FolderMirrorRecord,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO folder (
            folder_id, name, parent_id, icon, color, notes, sort_order, auto_tags,
            watch_path, watch_enabled, watch_subfolders, watch_import_status_mode,
            date_added, date_modified
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14
         )
         ON CONFLICT(folder_id) DO UPDATE SET
            name = excluded.name,
            parent_id = excluded.parent_id,
            icon = excluded.icon,
            color = excluded.color,
            notes = excluded.notes,
            sort_order = excluded.sort_order,
            auto_tags = excluded.auto_tags,
            watch_path = excluded.watch_path,
            watch_enabled = excluded.watch_enabled,
            watch_subfolders = excluded.watch_subfolders,
            watch_import_status_mode = excluded.watch_import_status_mode,
            date_added = excluded.date_added,
            date_modified = excluded.date_modified",
        params![
            record.folder_id,
            record.name,
            record.parent_id,
            record.icon,
            record.color,
            record.notes,
            record.sort_order,
            record.auto_tags_json,
            record.watch_path,
            if record.watch_enabled { 1 } else { 0 },
            if record.watch_subfolders { 1 } else { 0 },
            record.watch_import_status_mode,
            record.date_added,
            record.date_modified,
        ],
    )?;
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
