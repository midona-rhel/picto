//! Smart folder orchestration — CRUD with immediate sidebar node insertion
//! for responsive UI, and predicate-based file preview queries.
//!
//! Delegates to `sqlite::smart_folders` for storage and bitmap compilation.

use crate::smart_folders::db::{
    build_effective_predicate_for_smart_folder, collect_descendant_smart_folder_ids,
    compile_predicate, get_smart_folder, get_smart_folder_chain, has_local_rules, SmartFolder,
    SmartFolderPredicate,
};
use crate::sqlite::bitmaps::BitmapKey;
use crate::sqlite::SqliteDatabase;

async fn build_smart_folder_sidebar_node(
    db: &SqliteDatabase,
    sf: &SmartFolder,
) -> Result<crate::sidebar::db::SidebarNode, String> {
    let smart_folder_id = sf.smart_folder_id;
    let predicate_json = sf.predicate_json.clone();
    let bitmaps = db.bitmaps.clone();
    let (
        effective_predicate,
        inherited_predicates,
        has_effective_rules,
        has_local_predicate_rules,
        count,
    ) = db
        .with_read_conn(move |conn| {
            let effective = build_effective_predicate_for_smart_folder(conn, smart_folder_id)?;
            let chain = get_smart_folder_chain(conn, smart_folder_id)?;
            let inherited = if chain.len() <= 1 {
                Vec::new()
            } else {
                chain[..chain.len() - 1]
                    .iter()
                    .filter_map(|folder| {
                        serde_json::from_str::<serde_json::Value>(&folder.predicate_json).ok()
                    })
                    .collect()
            };
            let local = serde_json::from_str::<crate::smart_folders::db::SmartFolderPredicate>(
                &predicate_json,
            )
            .unwrap_or(crate::smart_folders::db::SmartFolderPredicate { groups: Vec::new() });
            let compiled = compile_predicate(conn, &effective, &bitmaps)?;
            let count = compiled.len() as i64;
            bitmaps.set(BitmapKey::SmartFolder(smart_folder_id), compiled);
            Ok::<_, rusqlite::Error>((
                serde_json::to_value(&effective)
                    .unwrap_or_else(|_| serde_json::json!({ "groups": [] })),
                inherited,
                has_local_rules(&effective),
                has_local_rules(&local),
                count,
            ))
        })
        .await
        .map_err(|e| e.to_string())?;
    let mut meta = serde_json::json!({
        "smart_folder_id": sf.smart_folder_id,
        "parent_id": sf.parent_id,
        "predicate": effective_predicate,
        "local_predicate": serde_json::from_str::<serde_json::Value>(&sf.predicate_json).unwrap_or_else(|_| serde_json::json!({ "groups": [] })),
        "inherited_predicates": inherited_predicates,
        "has_effective_rules": has_effective_rules,
        "has_local_rules": has_local_predicate_rules,
    });
    if let Some(ref f) = sf.sort_field {
        meta["sort_field"] = serde_json::Value::String(f.clone());
    }
    if let Some(ref o) = sf.sort_order {
        meta["sort_order"] = serde_json::Value::String(o.clone());
    }
    Ok(crate::sidebar::db::SidebarNode {
        node_id: format!("smart:{}", sf.smart_folder_id),
        kind: "smart_folder".into(),
        parent_id: Some(
            sf.parent_id
                .map(|parent_id| format!("smart:{parent_id}"))
                .unwrap_or_else(|| "section:smart_folders".into()),
        ),
        name: sf.name.clone(),
        icon: sf.icon.clone(),
        color: sf.color.clone(),
        sort_order: sf.display_order.or(Some(sf.smart_folder_id)),
        count: Some(count),
        freshness: "stale".into(),
        epoch: 0,
        selectable: true,
        expanded_by_default: false,
        meta_json: Some(meta.to_string()),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    })
}

async fn refresh_smart_folder_sidebar_nodes(
    db: &SqliteDatabase,
    smart_folder_ids: impl IntoIterator<Item = i64>,
) -> Result<(), String> {
    let mut unique_ids = std::collections::BTreeSet::new();
    unique_ids.extend(smart_folder_ids);

    let mut nodes = Vec::new();
    for smart_folder_id in unique_ids {
        let Some(folder) = db
            .with_read_conn(move |conn| get_smart_folder(conn, smart_folder_id))
            .await?
        else {
            continue;
        };
        nodes.push(build_smart_folder_sidebar_node(db, &folder).await?);
    }

    db.with_conn(move |conn| crate::sidebar::db::upsert_sidebar_nodes_batch(conn, &nodes))
        .await?;
    Ok(())
}

pub struct SmartFolderService;

impl SmartFolderService {
    pub async fn create_smart_folder(
        db: &SqliteDatabase,
        folder: SmartFolder,
    ) -> Result<SmartFolder, String> {
        let created = db
            .create_smart_folder(
                folder.name,
                folder.parent_id,
                folder.predicate_json,
                folder.icon,
                folder.color,
                folder.sort_field,
                folder.sort_order,
            )
            .await?;

        let node = build_smart_folder_sidebar_node(db, &created).await?;
        db.with_conn(move |conn| crate::sidebar::db::upsert_sidebar_node(conn, &node))
            .await?;

        Ok(created)
    }

    /// Returns `(updated_folder, predicate_changed)`.
    pub async fn update_smart_folder(
        db: &SqliteDatabase,
        id: String,
        folder: SmartFolder,
    ) -> Result<(SmartFolder, bool), String> {
        let sf_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid smart folder id: {}", id))?;

        let old_predicate = db
            .with_read_conn(move |conn| {
                Ok(get_smart_folder(conn, sf_id)?.map(|sf| sf.predicate_json))
            })
            .await?;
        let predicate_changed = old_predicate.as_deref() != Some(&folder.predicate_json);

        let sf = SmartFolder {
            smart_folder_id: sf_id,
            ..folder
        };
        db.update_smart_folder(sf).await?;

        let updated = db
            .with_read_conn(move |conn| get_smart_folder(conn, sf_id))
            .await?
            .ok_or_else(|| "Smart folder not found after update".to_string())?;

        let node = build_smart_folder_sidebar_node(db, &updated).await?;
        db.with_conn(move |conn| crate::sidebar::db::upsert_sidebar_node(conn, &node))
            .await?;

        Ok((updated, predicate_changed))
    }

    pub async fn move_smart_folder(
        db: &SqliteDatabase,
        smart_folder_id: i64,
        new_parent_id: Option<i64>,
        sibling_order: Vec<(i64, i64)>,
    ) -> Result<(), String> {
        let descendant_ids = db
            .with_read_conn(move |conn| collect_descendant_smart_folder_ids(conn, smart_folder_id))
            .await?;

        db.move_smart_folder(smart_folder_id, new_parent_id, sibling_order.clone())
            .await?;

        let mut affected_ids: Vec<i64> = sibling_order.into_iter().map(|(id, _)| id).collect();
        affected_ids.push(smart_folder_id);
        affected_ids.extend(descendant_ids);
        refresh_smart_folder_sidebar_nodes(db, affected_ids).await?;
        Ok(())
    }

    pub async fn reorder_smart_folders(
        db: &SqliteDatabase,
        parent_id: Option<i64>,
        moves: Vec<(i64, i64)>,
    ) -> Result<(), String> {
        let affected_ids: Vec<i64> = moves.iter().map(|(id, _)| *id).collect();
        db.reorder_smart_folders(parent_id, moves).await?;
        refresh_smart_folder_sidebar_nodes(db, affected_ids).await?;
        Ok(())
    }

    pub async fn delete_smart_folder(db: &SqliteDatabase, id: String) -> Result<(), String> {
        let sf_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid smart folder id: {}", id))?;
        let promoted_ids = db.delete_smart_folder(sf_id).await?;
        // Also remove the sidebar_node row so the folder vanishes immediately
        let node_id = format!("smart:{}", sf_id);
        db.with_conn(move |conn| {
            crate::sidebar::db::delete_sidebar_node(conn, &node_id)?;
            Ok(())
        })
        .await?;
        for promoted_id in promoted_ids {
            db.emit_read_model_event(crate::sqlite::ReadModelEvent::SmartFolderChanged {
                smart_folder_id: promoted_id,
            });
        }
        Ok(())
    }

    pub async fn count_smart_folder(
        db: &SqliteDatabase,
        predicate: SmartFolderPredicate,
    ) -> Result<i64, String> {
        let bitmaps = db.bitmaps.clone();
        db.with_read_conn(move |conn| {
            let bm = compile_predicate(conn, &predicate, &bitmaps)?;
            Ok(bm.len() as i64)
        })
        .await
    }
}
