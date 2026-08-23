//! Folder and smart-folder read queries for the canonical DB.

use rusqlite::{Connection, OptionalExtension};

/// Minimal folder row for building sidebar patches and meta_json.
#[derive(Debug, Clone, serde::Serialize)]
pub struct FolderRow {
    pub folder_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub sort_order: Option<i64>,
    pub auto_tags: Option<String>,
    pub watch_path: Option<String>,
    pub watch_enabled: bool,
    pub watch_subfolders: bool,
    pub watch_import_status_mode: Option<String>,
}

/// Minimal smart folder row for building sidebar patches and meta_json.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SmartFolderRow {
    pub smart_folder_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub predicate_json: String,
    pub display_order: Option<i64>,
}

pub fn get_folder(conn: &Connection, folder_id: i64) -> rusqlite::Result<Option<FolderRow>> {
    conn.query_row(
        "SELECT folder_id, name, parent_id, icon, color, notes, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode
         FROM folder WHERE folder_id = ?1",
        [folder_id],
        |row| Ok(FolderRow {
            folder_id: row.get(0)?,
            name: row.get(1)?,
            parent_id: row.get(2)?,
            icon: row.get(3)?,
            color: row.get(4)?,
            notes: row.get(5)?,
            sort_order: row.get(6)?,
            auto_tags: row.get(7)?,
            watch_path: row.get(8)?,
            watch_enabled: row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
            watch_subfolders: row.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
            watch_import_status_mode: row.get(11)?,
        }),
    ).optional()
}

pub fn get_smart_folder(
    conn: &Connection,
    smart_folder_id: i64,
) -> rusqlite::Result<Option<SmartFolderRow>> {
    conn.query_row(
        "SELECT smart_folder_id, name, parent_id, icon, color, notes, predicate_json, display_order
         FROM smart_folder WHERE smart_folder_id = ?1",
        [smart_folder_id],
        |row| {
            Ok(SmartFolderRow {
                smart_folder_id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                notes: row.get(5)?,
                predicate_json: row.get(6)?,
                display_order: row.get(7)?,
            })
        },
    )
    .optional()
}

pub fn list_folders(conn: &Connection) -> rusqlite::Result<Vec<FolderRow>> {
    let mut stmt = conn.prepare(
        "SELECT folder_id, name, parent_id, icon, color, notes, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode
         FROM folder ORDER BY sort_order, name"
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(FolderRow {
                folder_id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                notes: row.get(5)?,
                sort_order: row.get(6)?,
                auto_tags: row.get(7)?,
                watch_path: row.get(8)?,
                watch_enabled: row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
                watch_subfolders: row.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
                watch_import_status_mode: row.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Collect all descendant smart folder IDs (recursive children).
pub fn collect_descendant_smart_folder_ids(
    conn: &Connection,
    root_id: i64,
) -> rusqlite::Result<Vec<i64>> {
    let mut result = Vec::new();
    let mut stack = vec![root_id];
    while let Some(parent_id) = stack.pop() {
        let mut stmt =
            conn.prepare_cached("SELECT smart_folder_id FROM smart_folder WHERE parent_id = ?1")?;
        let children: Vec<i64> = stmt
            .query_map([parent_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        for child_id in children {
            result.push(child_id);
            stack.push(child_id);
        }
    }
    Ok(result)
}

pub fn list_smart_folders(conn: &Connection) -> rusqlite::Result<Vec<SmartFolderRow>> {
    let mut stmt = conn.prepare(
        "SELECT smart_folder_id, name, parent_id, icon, color, notes, predicate_json, display_order
         FROM smart_folder ORDER BY display_order, name",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(SmartFolderRow {
                smart_folder_id: row.get(0)?,
                name: row.get(1)?,
                parent_id: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                notes: row.get(5)?,
                predicate_json: row.get(6)?,
                display_order: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn get_folder_entity_hashes(
    conn: &Connection,
    folder_id: i64,
) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT me.entity_hash
         FROM folder_member fm
         JOIN media_entity me ON me.entity_id = fm.entity_id
         WHERE fm.folder_id = ?1
         ORDER BY fm.position_rank ASC, me.entity_hash ASC",
    )?;
    let hashes = stmt.query_map([folder_id], |row| row.get(0))?;
    hashes.collect()
}

pub fn get_folder_cover_hashes(
    conn: &Connection,
    folder_ids: &[i64],
) -> rusqlite::Result<Vec<(i64, Option<String>)>> {
    if folder_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = (1..=folder_ids.len())
        .map(|index| format!("?{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT f.folder_id,
                (SELECT me.entity_hash
                 FROM folder_member fm
                 JOIN media_entity me ON me.entity_id = fm.entity_id
                 WHERE me.status = 1
                   AND fm.folder_id = f.folder_id
                 ORDER BY fm.position_rank ASC, me.entity_hash ASC
                 LIMIT 1) AS cover_hash
         FROM folder f
         WHERE f.folder_id IN ({placeholders})
         ORDER BY f.folder_id ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(folder_ids), |row| {
        Ok((row.get(0)?, row.get(1)?))
    })?;
    rows.collect()
}

pub fn get_entity_folder_memberships(
    conn: &Connection,
    entity_id: i64,
) -> rusqlite::Result<Vec<crate::db::types::FolderMembership>> {
    let mut stmt = conn.prepare(
        "SELECT f.folder_id, f.name
         FROM folder_member fm
         JOIN folder f ON f.folder_id = fm.folder_id
         WHERE fm.entity_id = ?1
         ORDER BY f.name ASC, f.folder_id ASC",
    )?;
    let memberships = stmt.query_map([entity_id], |row| {
        Ok(crate::db::types::FolderMembership {
            folder_id: row.get(0)?,
            folder_name: row.get(1)?,
        })
    })?;
    memberships.collect()
}
