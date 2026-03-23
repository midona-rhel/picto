//! Sidebar tree reads.

use rusqlite::Connection;

#[derive(Debug, Clone, serde::Serialize)]
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
    pub selectable: bool,
    pub expanded_by_default: bool,
    pub meta_json: Option<String>,
}

/// Read the full sidebar tree.
pub fn get_sidebar_tree(conn: &Connection) -> rusqlite::Result<Vec<SidebarNode>> {
    let mut stmt = conn.prepare(
        "SELECT node_id, kind, parent_id, name, icon, color, sort_order, count, freshness, selectable, expanded_by_default, meta_json
         FROM sidebar_node
         ORDER BY sort_order ASC, name ASC",
    )?;
    let nodes = stmt
        .query_map([], |row| {
            Ok(SidebarNode {
                node_id: row.get(0)?,
                kind: row.get(1)?,
                parent_id: row.get(2)?,
                name: row.get(3)?,
                icon: row.get(4)?,
                color: row.get(5)?,
                sort_order: row.get(6)?,
                count: row.get(7)?,
                freshness: row.get::<_, String>(8)?,
                selectable: row.get::<_, i64>(9)? != 0,
                expanded_by_default: row.get::<_, Option<i64>>(10)?.unwrap_or(0) != 0,
                meta_json: row.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(nodes)
}
