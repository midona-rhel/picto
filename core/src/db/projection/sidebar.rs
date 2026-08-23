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
        ("system:active", "All", "IconPhoto", 1, true),
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

    if let Ok(counts) = crate::db::query::stats::get_scope_counts(conn) {
        for (node_id, count) in [
            ("system:active", counts.active),
            ("system:inbox", counts.inbox),
            ("system:trash", counts.trash),
            ("system:uncategorized", counts.uncategorized),
            ("system:untagged", counts.untagged),
        ] {
            let _ = conn.execute(
                "UPDATE sidebar_node SET count = ?1 WHERE node_id = ?2",
                params![count, node_id],
            );
        }
    }

    let recent = crate::db::query::grid::recently_viewed_count(conn).unwrap_or(0);
    let _ = conn.execute(
        "UPDATE sidebar_node SET count = ?1 WHERE node_id = 'system:recent_viewed'",
        [recent],
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
        i64,
        i64,
        i64,
    )> = conn
        .prepare(
            "SELECT folder_id, name, parent_id, icon, color, notes, sort_order, auto_tags, watch_path, watch_enabled, watch_subfolders, watch_import_status_mode, total_size_bytes, pinned, pin_order
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
                    row.get::<_, i64>(12)?,
                    row.get::<_, i64>(13)?,
                    row.get::<_, i64>(14)?,
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
        total_size_bytes,
        pinned,
        pin_order,
    ) in &folders
    {
        let node_id = format!("folder:{fid}");
        let parent = parent_id
            .map(|pid| format!("folder:{pid}"))
            .unwrap_or_else(|| "section:folders".into());
        let count = crate::db::query::grid::folder_visible_count(conn, *fid).unwrap_or(0);
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
            "total_size_bytes": total_size_bytes,
            "pinned": *pinned != 0,
            "pin_order": pin_order,
        });
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, color, sort_order, count, selectable, freshness, epoch, meta_json, date_modified)
             VALUES (?1, 'folder', ?2, ?3, ?4, ?5, ?6, ?7, 1, 'fresh', 1, ?8, ?9)",
            params![node_id, parent, name, icon, color, sort_order, count, meta.to_string(), now],
        );
    }

    // Smart folder nodes (children of section:smart_folders)
    let smart_folders: Vec<(i64, String, Option<i64>, Option<String>, Option<String>, Option<String>, Option<i64>, String, i64, i64, i64)> = conn
        .prepare("SELECT smart_folder_id, name, parent_id, icon, color, notes, display_order, predicate_json, total_size_bytes, pinned, pin_order FROM smart_folder ORDER BY display_order, name")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get::<_, i64>(8)?, row.get::<_, i64>(9)?, row.get::<_, i64>(10)?)))
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
        total_size_bytes,
        pinned,
        pin_order,
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
            "total_size_bytes": total_size_bytes,
            "pinned": *pinned != 0,
            "pin_order": pin_order,
        });
        let count = bitmaps.len(&BitmapKey::SmartFolder(*sfid)) as i64;
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, color, sort_order, count, selectable, freshness, epoch, meta_json, date_modified)
             VALUES (?1, 'smart_folder', ?2, ?3, ?4, ?5, ?6, ?7, 1, 'fresh', 1, ?8, ?9)",
            params![node_id, parent, name, icon, color, display_order, count, meta.to_string(), now],
        );
    }
}
