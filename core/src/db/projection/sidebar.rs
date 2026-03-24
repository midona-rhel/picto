//! Sidebar node projection compilation.
//! Builds the sidebar_node table from authoritative data.

use rusqlite::{params, Connection};

/// Rebuild the entire sidebar projection from authoritative tables.
pub fn compile_sidebar(conn: &Connection) {
    // Clear existing sidebar nodes
    let _ = conn.execute("DELETE FROM sidebar_node", []);

    let now = chrono::Utc::now().to_rfc3339();

    // System scopes
    let system_nodes = [
        ("system:library", "system", None, "Library", Some("IconPhoto"), true, true),
        ("system:active", "system", Some("system:library"), "All Active", Some("IconPhoto"), true, false),
        ("system:inbox", "system", Some("system:library"), "Inbox", Some("IconInbox"), true, false),
        ("system:uncategorized", "system", Some("system:library"), "Uncategorized", Some("IconFolderQuestion"), true, false),
        ("system:untagged", "system", Some("system:library"), "Untagged", Some("IconTag"), true, false),
        ("system:trash", "system", Some("system:library"), "Trash", Some("IconTrash"), true, false),
    ];

    for (id, kind, parent, name, icon, selectable, expanded) in &system_nodes {
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, selectable, expanded_by_default, freshness, epoch, date_modified)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'fresh', 1, ?8)",
            params![id, kind, parent, name, icon, *selectable as i64, *expanded as i64, now],
        );
    }

    // System scope counts (top-level only, excludes collection members)
    for (node_id, status) in [("system:active", 1i64), ("system:inbox", 0), ("system:trash", 2)] {
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
    let _ = conn.execute("UPDATE sidebar_node SET count = ?1 WHERE node_id = 'system:uncategorized'", [uncategorized]);

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
    let _ = conn.execute("UPDATE sidebar_node SET count = ?1 WHERE node_id = 'system:untagged'", [untagged]);

    // Folder nodes
    let folders: Vec<(i64, String, Option<i64>, Option<String>, Option<String>, Option<i64>)> = conn
        .prepare("SELECT folder_id, name, parent_id, icon, color, sort_order FROM folder ORDER BY sort_order, name")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for (fid, name, parent_id, icon, color, sort_order) in &folders {
        let node_id = format!("folder:{fid}");
        let parent = parent_id.map(|pid| format!("folder:{pid}"));
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
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, color, sort_order, count, selectable, freshness, epoch, date_modified)
             VALUES (?1, 'folder', ?2, ?3, ?4, ?5, ?6, ?7, 1, 'fresh', 1, ?8)",
            params![node_id, parent, name, icon, color, sort_order, count, now],
        );
    }

    // Smart folder nodes
    let smart_folders: Vec<(i64, String, Option<i64>, Option<String>, Option<String>, Option<i64>, String)> = conn
        .prepare("SELECT smart_folder_id, name, parent_id, icon, color, display_order, predicate_json FROM smart_folder ORDER BY display_order, name")
        .and_then(|mut stmt| {
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)))
                .map(|rows| rows.flatten().collect())
        })
        .unwrap_or_default();

    for (sfid, name, parent_id, icon, color, display_order, predicate_json) in &smart_folders {
        let node_id = format!("smart:{sfid}");
        let parent = parent_id.map(|pid| format!("smart:{pid}"));
        let meta = serde_json::json!({
            "predicate": serde_json::from_str::<serde_json::Value>(predicate_json).unwrap_or_default(),
            "parent_id": parent_id,
        });
        let _ = conn.execute(
            "INSERT OR REPLACE INTO sidebar_node (node_id, kind, parent_id, name, icon, color, sort_order, count, selectable, freshness, epoch, meta_json, date_modified)
             VALUES (?1, 'smart_folder', ?2, ?3, ?4, ?5, ?6, 0, 1, 'stale', 1, ?7, ?8)",
            params![node_id, parent, name, icon, color, display_order, meta.to_string(), now],
        );
    }
}
