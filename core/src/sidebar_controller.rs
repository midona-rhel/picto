use chrono::Utc;
use serde_json::Value as JsonValue;

use crate::sqlite::SqliteDatabase;
use crate::types::{SidebarNodeDto, SidebarTreeResponse};

pub struct SidebarController;

impl SidebarController {
    pub async fn reorder_sidebar_nodes(
        db: &SqliteDatabase,
        moves: Vec<(String, i64)>,
    ) -> Result<(), String> {
        db.reorder_sidebar_nodes(moves).await
    }

    pub async fn get_sidebar_tree(db: &SqliteDatabase) -> Result<SidebarTreeResponse, String> {
        let nodes = db.get_sidebar_tree().await?;
        let tree_epoch = db.manifest.published_epoch();

        Ok(SidebarTreeResponse {
            nodes: nodes
                .into_iter()
                .map(|n| {
                    let meta: Option<JsonValue> = n
                        .meta_json
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok());
                    SidebarNodeDto {
                        id: n.node_id,
                        kind: n.kind,
                        parent_id: n.parent_id,
                        name: n.name,
                        icon: n.icon,
                        color: n.color,
                        sort_order: n.sort_order,
                        count: n.count,
                        freshness: n.freshness,
                        selectable: n.selectable,
                        expanded_by_default: n.expanded_by_default,
                        meta,
                    }
                })
                .collect(),
            tree_epoch,
            generated_at: Utc::now().to_rfc3339(),
        })
    }
}
