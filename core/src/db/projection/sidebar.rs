//! Sidebar node projection compilation.
//! Builds the sidebar_node table from authoritative data.
//! Matches the live sidebar contract from core/src/sidebar/db.rs.

use rusqlite::{params, Connection};

use super::bitmaps::{BitmapKey, BitmapStore};

/// Rebuild the entire sidebar projection from authoritative tables.
pub fn compile_sidebar(conn: &Connection, bitmaps: &BitmapStore) {
    // Clear existing sidebar nodes
    let _ = conn.execute("DELETE FROM sidebar_node", []);

    let now = chrono::Utc::now().to_rfc3339();

    // Section nodes (non-selectable structural containers)
    let sections = [
        ("system:library", "Library", 0i64, true),
        ("section:folders", "Folders", 10, true),
        ("section:smart_folders", "Smart Folders", 20, true),
    ];

    for (id, name, sort_order, expanded) in &sections {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, sort_order, count, selectable, expanded_by_default, freshness, epoch, date_modified)
             VALUES (?1, 'section', NULL, ?2, ?3, NULL, 0, ?4, 'fresh', 1, ?5)",
            params![id, name, sort_order, *expanded as i64, now],
        );
    }

    // System scope nodes (children of system:library)
    //                       (id,                    name,              icon,                sort, selectable)
    let system_nodes: &[(&str, &str, &str, i64, bool)] = &[
        ("system:active", "All Active", "IconPhoto", 1, true),
        ("system:inbox", "Inbox", "IconInbox", 2, true),
        (
            "system:uncategorized",
            "Uncategorized",
            "IconFolderQuestion",
            3,
            true,
        ),
        ("system:untagged", "Untagged", "IconTagOff", 4, true),
        (
            "system:recent_viewed",
            "Recently Viewed",
            "IconEye",
            5,
            true,
        ),
        ("system:duplicates", "Duplicates", "IconCopy", 6, true),
        ("system:trash", "Trash", "IconTrash", 7, true),
    ];

    for (id, name, icon, sort_order, selectable) in system_nodes {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, sort_order, count, selectable, expanded_by_default, freshness, epoch, date_modified)
             VALUES (?1, 'system', 'system:library', ?2, ?3, ?4, 0, ?5, 0, 'stale', 1, ?6)",
            params![id, name, icon, sort_order, *selectable as i64, now],
        );
    }

    // System scope counts (top-level only, excludes collection members)
    for (node_id, status) in [
        ("system:active", 1i64),
        ("system:inbox", 0),
        ("system:trash", 2),
    ] {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM media_entity WHERE status = ?1 AND parent_collection_entity_id IS NULL",
                [status],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let _ = conn.execute(
            "UPDATE sidebar_node SET count = ?1 WHERE node_id = ?2",
            params![count, node_id],
        );
    }

    // Uncategorized count
    let uncategorized: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_entity me
             WHERE me.status = 1 AND me.parent_collection_entity_id IS NULL
               AND NOT EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = me.entity_id)
               AND NOT EXISTS (
                   SELECT 1 FROM media_entity child
                   WHERE child.parent_collection_entity_id = me.entity_id
                     AND EXISTS (SELECT 1 FROM folder_member fm WHERE fm.entity_id = child.entity_id)
               )",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let _ = conn.execute(
        "UPDATE sidebar_node SET count = ?1 WHERE node_id = 'system:uncategorized'",
        [uncategorized],
    );

    // Untagged count
    let untagged: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM media_entity me
             WHERE me.status = 1 AND me.parent_collection_entity_id IS NULL
               AND NOT EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = me.entity_id)
               AND NOT EXISTS (
                   SELECT 1 FROM media_entity child
                   WHERE child.parent_collection_entity_id = me.entity_id
                     AND EXISTS (SELECT 1 FROM entity_tag et WHERE et.entity_id = child.entity_id)
               )",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let _ = conn.execute(
        "UPDATE sidebar_node SET count = ?1 WHERE node_id = 'system:untagged'",
        [untagged],
    );

    // Duplicates count
    let duplicates: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);
    let _ = conn.execute(
        "UPDATE sidebar_node SET count = ?1 WHERE node_id = 'system:duplicates'",
        [duplicates],
    );

    // Folder nodes (children of section:folders)
    let folders: Vec<(
        i64,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<String>,
        bool,
        bool,
        String,
    )> = conn
        .prepare(
            "SELECT folder_id, name, parent_id, icon, color, notes, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode
             FROM folder
             ORDER BY sort_order, name",
        )
        .and_then(|mut stmt| {
            stmt.query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                    row.get::<_, Option<i64>>(9)?.unwrap_or(0) != 0,
                    row.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
                    row.get::<_, Option<String>>(11)?.unwrap_or_else(|| "inherit".into()),
                ))
            })
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for (
        fid,
        name,
        parent_id,
        icon,
        color,
        notes,
        sort_order,
        auto_tags_raw,
        watch_path,
        watch_enabled,
        watch_subfolders,
        watch_import_status_mode,
    ) in &folders
    {
        let node_id = format!("folder:{fid}");
        let parent = parent_id
            .map(|pid| format!("folder:{pid}"))
            .unwrap_or_else(|| "section:folders".into());
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT COALESCE(me.parent_collection_entity_id, me.entity_id))
                 FROM folder_member fm
                 JOIN media_entity me ON me.entity_id = fm.entity_id
                 WHERE fm.folder_id = ?1 AND me.status = 1",
                [fid],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let meta = serde_json::json!({
            "folder_id": fid,
            "notes": notes,
            "auto_tags": auto_tags_raw
                .as_deref()
                .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                .unwrap_or(serde_json::Value::Null),
            "watch_path": watch_path,
            "watch_enabled": watch_enabled,
            "watch_subfolders": watch_subfolders,
            "watch_import_status_mode": watch_import_status_mode,
        });
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, color, sort_order, count, selectable, freshness, epoch, meta_json, date_modified)
             VALUES (?1, 'folder', ?2, ?3, ?4, ?5, ?6, ?7, 1, 'fresh', 1, ?8, ?9)",
            params![node_id, parent, name, icon, color, sort_order, count, meta.to_string(), now],
        );
    }

    // Smart folder nodes (children of section:smart_folders)
    let smart_folders: Vec<(i64, String, Option<i64>, Option<String>, Option<String>, Option<String>, Option<i64>, String, Option<String>, Option<String>)> = conn
        .prepare("SELECT smart_folder_id, name, parent_id, icon, color, notes, display_order, predicate_json, sort_field, sort_order FROM smart_folder ORDER BY display_order, name")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?)))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for (
        sfid,
        name,
        parent_id,
        icon,
        color,
        notes,
        display_order,
        predicate_json,
        sort_field,
        sort_order,
    ) in &smart_folders
    {
        let node_id = format!("smart:{sfid}");
        let parent = parent_id
            .map(|pid| format!("smart:{pid}"))
            .unwrap_or_else(|| "section:smart_folders".into());
        let meta = serde_json::json!({
            "smart_folder_id": sfid,
            "parent_id": parent_id,
            "notes": notes,
            "predicate": serde_json::from_str::<serde_json::Value>(predicate_json).unwrap_or_default(),
            "sort_field": sort_field,
            "sort_order": sort_order,
        });
        let count = bitmaps.len(&BitmapKey::SmartFolder(*sfid)) as i64;
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, color, sort_order, count, selectable, freshness, epoch, meta_json, date_modified)
             VALUES (?1, 'smart_folder', ?2, ?3, ?4, ?5, ?6, ?7, 1, 'fresh', 1, ?8, ?9)",
            params![node_id, parent, name, icon, color, display_order, count, meta.to_string(), now],
        );
    }
}
