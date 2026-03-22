use std::sync::Arc;

use crate::scope::resolver::scope_count;
use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::SqliteDatabase;

use super::db::{
    delete_sidebar_node, seed_sidebar_if_empty, update_sidebar_count, upsert_sidebar_nodes_batch,
    SidebarNode,
};

pub(crate) async fn compile_sidebar(db: &Arc<SqliteDatabase>) -> Result<(), String> {
    let bitmaps = db.bitmaps.clone();
    let pre_epoch = db.manifest.published_artifact_version("sidebar");
    let epoch = pre_epoch + 1;

    db.with_conn(move |conn| {
        seed_sidebar_if_empty(conn)?;

        for key in &[
            "system:active",
            "system:inbox",
            "system:trash",
            "system:untagged",
            "system:uncategorized",
            "system:recent_viewed",
        ] {
            let count = scope_count(conn, &bitmaps, key)?;
            update_sidebar_count(conn, key, count, epoch as i64)?;
        }

        let duplicate_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM duplicate WHERE status = 'detected'",
            [],
            |row| row.get(0),
        )?;
        update_sidebar_count(conn, "system:duplicates", duplicate_count, epoch as i64)?;

        let mut nodes: Vec<SidebarNode> = Vec::new();

        let smart_folders = crate::smart_folders::db::list_smart_folders(conn)?;
        for smart_folder in smart_folders {
            let node_id = format!("smart:{}", smart_folder.smart_folder_id);
            let count = bitmaps.len(&BitmapKey::SmartFolder(smart_folder.smart_folder_id));
            let effective_predicate =
                crate::smart_folders::db::build_effective_predicate_for_smart_folder(
                    conn,
                    smart_folder.smart_folder_id,
                )?;
            let local_predicate =
                serde_json::from_str::<serde_json::Value>(&smart_folder.predicate_json)
                    .unwrap_or_else(|_| serde_json::json!({ "groups": [] }));
            let has_effective_rules =
                crate::smart_folders::db::has_local_rules(&effective_predicate);
            let local_rules = serde_json::from_value::<
                crate::smart_folders::db::SmartFolderPredicate,
            >(local_predicate.clone())
            .unwrap_or(crate::smart_folders::db::SmartFolderPredicate { groups: Vec::new() });
            nodes.push(SidebarNode {
                node_id,
                kind: "smart_folder".into(),
                parent_id: Some(
                    smart_folder
                        .parent_id
                        .map(|parent_id| format!("smart:{parent_id}"))
                        .unwrap_or_else(|| "section:smart_folders".into()),
                ),
                name: smart_folder.name.clone(),
                icon: smart_folder.icon.clone(),
                color: smart_folder.color.clone(),
                sort_order: smart_folder
                    .display_order
                    .or(Some(smart_folder.smart_folder_id)),
                count: Some(count as i64),
                freshness: "fresh".into(),
                epoch: epoch as i64,
                selectable: true,
                expanded_by_default: false,
                meta_json: {
                    let mut meta = serde_json::json!({
                        "smart_folder_id": smart_folder.smart_folder_id,
                        "parent_id": smart_folder.parent_id,
                        "predicate": serde_json::to_value(&effective_predicate)
                            .unwrap_or_else(|_| serde_json::json!({ "groups": [] })),
                        "local_predicate": local_predicate,
                        "has_effective_rules": has_effective_rules,
                        "has_local_rules": crate::smart_folders::db::has_local_rules(&local_rules),
                    });
                    if let Some(ref sort_field) = smart_folder.sort_field {
                        meta["sort_field"] = serde_json::Value::String(sort_field.clone());
                    }
                    if let Some(ref sort_order) = smart_folder.sort_order {
                        meta["sort_order"] = serde_json::Value::String(sort_order.clone());
                    }
                    Some(meta.to_string())
                },
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
            });
        }

        let active_bitmap = bitmaps.get(&BitmapKey::Status(1));
        let folders = crate::folders::db::list_folders(conn)?;
        for folder in folders {
            let node_id = format!("folder:{}", folder.folder_id);
            let count = (bitmaps.get(&BitmapKey::Folder(folder.folder_id)) & &active_bitmap).len();
            let parent_id = folder
                .parent_id
                .map(|parent_id| format!("folder:{parent_id}"))
                .unwrap_or_else(|| "section:folders".into());
            nodes.push(SidebarNode {
                node_id,
                kind: "folder".into(),
                parent_id: Some(parent_id),
                name: folder.name,
                icon: folder.icon,
                color: folder.color,
                sort_order: folder.sort_order,
                count: Some(count as i64),
                freshness: "fresh".into(),
                epoch: epoch as i64,
                selectable: true,
                expanded_by_default: false,
                meta_json: Some(
                    serde_json::json!({
                        "folder_id": folder.folder_id,
                        "auto_tags": folder.auto_tags,
                    })
                    .to_string(),
                ),
                updated_at: Some(chrono::Utc::now().to_rfc3339()),
            });
        }

        let live_ids: std::collections::HashSet<String> =
            nodes.iter().map(|node| node.node_id.clone()).collect();
        let existing: Vec<(String, String)> = conn
            .prepare_cached(
                "SELECT node_id, kind FROM sidebar_node WHERE kind IN ('smart_folder', 'folder')",
            )?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|row| row.ok())
            .collect();
        for (existing_id, _kind) in &existing {
            if !live_ids.contains(existing_id) {
                delete_sidebar_node(conn, existing_id)?;
            }
        }

        upsert_sidebar_nodes_batch(conn, &nodes)?;
        Ok(())
    })
    .await?;

    Ok(())
}
