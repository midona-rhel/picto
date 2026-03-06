//! Sidebar projection read/write.
//!
//! The sidebar tree is a pre-compiled projection — counts maintained by the
//! compiler system (O(1) via bitmap .len()), never computed on the read path.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::SqliteDatabase;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidebarNode {
    pub node_id: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub count: Option<i64>,
    pub freshness: String,
    pub epoch: i64,
    pub selectable: bool,
    pub expanded_by_default: bool,
    pub meta_json: Option<String>,
    pub updated_at: Option<String>,
}

/// Read the entire sidebar tree.
pub fn get_sidebar_tree(conn: &Connection) -> rusqlite::Result<Vec<SidebarNode>> {
    let mut stmt = conn.prepare_cached(
        "SELECT node_id, kind, parent_id, name, icon, color, sort_order, count,
                freshness, epoch, selectable, expanded_by_default, meta_json, updated_at
         FROM sidebar_node ORDER BY sort_order, name",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(SidebarNode {
            node_id: row.get(0)?,
            kind: row.get(1)?,
            parent_id: row.get(2)?,
            name: row.get(3)?,
            icon: row.get(4)?,
            color: row.get(5)?,
            sort_order: row.get(6)?,
            count: row.get(7)?,
            freshness: row.get(8)?,
            epoch: row.get(9)?,
            selectable: row.get::<_, i64>(10)? != 0,
            expanded_by_default: row.get::<_, i64>(11)? != 0,
            meta_json: row.get(12)?,
            updated_at: row.get(13)?,
        })
    })?;
    rows.collect()
}

pub fn upsert_sidebar_node(conn: &Connection, node: &SidebarNode) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO sidebar_node
         (node_id, kind, parent_id, name, icon, color, sort_order, count,
          freshness, epoch, selectable, expanded_by_default, meta_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            node.node_id,
            node.kind,
            node.parent_id,
            node.name,
            node.icon,
            node.color,
            node.sort_order,
            node.count,
            node.freshness,
            node.epoch,
            node.selectable as i64,
            node.expanded_by_default as i64,
            node.meta_json,
            node.updated_at,
        ],
    )?;
    Ok(())
}

pub fn update_sidebar_count(
    conn: &Connection,
    node_id: &str,
    count: i64,
    epoch: i64,
) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE sidebar_node SET count = ?1, epoch = ?2, freshness = 'fresh', updated_at = ?3
         WHERE node_id = ?4",
        params![count, epoch, now, node_id],
    )?;
    Ok(())
}

pub fn upsert_sidebar_nodes_batch(
    conn: &Connection,
    nodes: &[SidebarNode],
) -> rusqlite::Result<()> {
    if nodes.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare_cached(
        "INSERT OR REPLACE INTO sidebar_node
         (node_id, kind, parent_id, name, icon, color, sort_order, count,
          freshness, epoch, selectable, expanded_by_default, meta_json, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
    )?;
    for node in nodes {
        stmt.execute(params![
            node.node_id,
            node.kind,
            node.parent_id,
            node.name,
            node.icon,
            node.color,
            node.sort_order,
            node.count,
            node.freshness,
            node.epoch,
            node.selectable as i64,
            node.expanded_by_default as i64,
            node.meta_json,
            node.updated_at,
        ])?;
    }
    Ok(())
}

pub fn reorder_sidebar_nodes(conn: &Connection, moves: &[(String, i64)]) -> rusqlite::Result<()> {
    let mut stmt =
        conn.prepare_cached("UPDATE sidebar_node SET sort_order = ?1 WHERE node_id = ?2")?;
    for (node_id, sort_order) in moves {
        stmt.execute(params![sort_order, node_id])?;
    }
    Ok(())
}

pub fn delete_sidebar_node(conn: &Connection, node_id: &str) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM sidebar_node WHERE node_id = ?1", [node_id])?;
    Ok(())
}

pub fn seed_sidebar_if_empty(conn: &Connection) -> rusqlite::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let nodes = vec![
        SidebarNode {
            node_id: "system:library".into(),
            kind: "section".into(),
            parent_id: None,
            name: "Library".into(),
            icon: None,
            color: None,
            sort_order: Some(0),
            count: None,
            freshness: "stale".into(),
            epoch: 0,
            selectable: false,
            expanded_by_default: true,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "system:all_files".into(),
            kind: "system".into(),
            parent_id: Some("system:library".into()),
            name: "All Files".into(),
            icon: Some("IconPhoto".into()),
            color: None,
            sort_order: Some(1),
            count: Some(0),
            freshness: "stale".into(),
            epoch: 0,
            selectable: true,
            expanded_by_default: false,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "system:inbox".into(),
            kind: "system".into(),
            parent_id: Some("system:library".into()),
            name: "Inbox".into(),
            icon: Some("IconInbox".into()),
            color: None,
            sort_order: Some(2),
            count: Some(0),
            freshness: "stale".into(),
            epoch: 0,
            selectable: true,
            expanded_by_default: false,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "system:uncategorized".into(),
            kind: "system".into(),
            parent_id: Some("system:library".into()),
            name: "Uncategorized".into(),
            icon: Some("IconFolderQuestion".into()),
            color: None,
            sort_order: Some(3),
            count: Some(0),
            freshness: "stale".into(),
            epoch: 0,
            selectable: true,
            expanded_by_default: false,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "system:untagged".into(),
            kind: "system".into(),
            parent_id: Some("system:library".into()),
            name: "Untagged".into(),
            icon: Some("IconTagOff".into()),
            color: None,
            sort_order: Some(4),
            count: Some(0),
            freshness: "stale".into(),
            epoch: 0,
            selectable: true,
            expanded_by_default: false,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "system:recent_viewed".into(),
            kind: "system".into(),
            parent_id: Some("system:library".into()),
            name: "Recently Viewed".into(),
            icon: Some("IconEye".into()),
            color: None,
            sort_order: Some(5),
            count: Some(0),
            freshness: "stale".into(),
            epoch: 0,
            selectable: true,
            expanded_by_default: false,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "system:duplicates".into(),
            kind: "system".into(),
            parent_id: Some("system:library".into()),
            name: "Duplicates".into(),
            icon: Some("IconCopy".into()),
            color: None,
            sort_order: Some(6),
            count: Some(0),
            freshness: "stale".into(),
            epoch: 0,
            selectable: true,
            expanded_by_default: false,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "system:trash".into(),
            kind: "system".into(),
            parent_id: Some("system:library".into()),
            name: "Trash".into(),
            icon: Some("IconTrash".into()),
            color: None,
            sort_order: Some(7),
            count: Some(0),
            freshness: "stale".into(),
            epoch: 0,
            selectable: true,
            expanded_by_default: false,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "section:folders".into(),
            kind: "section".into(),
            parent_id: None,
            name: "Folders".into(),
            icon: None,
            color: None,
            sort_order: Some(10),
            count: None,
            freshness: "fresh".into(),
            epoch: 0,
            selectable: false,
            expanded_by_default: true,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
        SidebarNode {
            node_id: "section:smart_folders".into(),
            kind: "section".into(),
            parent_id: None,
            name: "Smart Folders".into(),
            icon: None,
            color: None,
            sort_order: Some(20),
            count: None,
            freshness: "fresh".into(),
            epoch: 0,
            selectable: false,
            expanded_by_default: true,
            meta_json: None,
            updated_at: Some(now.clone()),
        },
    ];

    for node in &nodes {
        conn.execute(
            "INSERT OR IGNORE INTO sidebar_node
             (node_id, kind, parent_id, name, icon, color, sort_order, count,
              freshness, epoch, selectable, expanded_by_default, meta_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                node.node_id,
                node.kind,
                node.parent_id,
                node.name,
                node.icon,
                node.color,
                node.sort_order,
                node.count,
                node.freshness,
                node.epoch,
                node.selectable as i64,
                node.expanded_by_default as i64,
                node.meta_json,
                node.updated_at,
            ],
        )?;
    }

    Ok(())
}

impl SqliteDatabase {
    pub async fn get_sidebar_tree(&self) -> Result<Vec<SidebarNode>, String> {
        self.with_read_conn(get_sidebar_tree).await
    }

    pub async fn reorder_sidebar_nodes(&self, moves: Vec<(String, i64)>) -> Result<(), String> {
        self.with_conn(move |conn| reorder_sidebar_nodes(conn, &moves))
            .await
    }
}
