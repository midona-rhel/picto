//! Smart folder orchestration — CRUD with immediate sidebar node insertion
//! for responsive UI, and predicate-based file preview queries.
//!
//! Delegates to `sqlite::smart_folders` for storage and bitmap compilation.

use crate::smart_folders::db::{
    compile_predicate, get_smart_folder, SmartFolder, SmartFolderPredicate,
};
use crate::sqlite::SqliteDatabase;

fn build_smart_folder_sidebar_node(sf: &SmartFolder, count: i64) -> crate::sidebar::db::SidebarNode {
    let mut meta = serde_json::json!({ "smart_folder_id": sf.smart_folder_id });
    if let Ok(pred) = serde_json::from_str::<serde_json::Value>(&sf.predicate_json) {
        meta["predicate"] = pred;
    }
    if let Some(ref f) = sf.sort_field {
        meta["sort_field"] = serde_json::Value::String(f.clone());
    }
    if let Some(ref o) = sf.sort_order {
        meta["sort_order"] = serde_json::Value::String(o.clone());
    }
    crate::sidebar::db::SidebarNode {
        node_id: format!("smart:{}", sf.smart_folder_id),
        kind: "smart_folder".into(),
        parent_id: Some("section:smart_folders".into()),
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
    }
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
                folder.predicate_json,
                folder.icon,
                folder.color,
                folder.sort_field,
                folder.sort_order,
            )
            .await?;

        let node = build_smart_folder_sidebar_node(&created, 0);
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

        let existing_count = db
            .bitmaps
            .len(&crate::sqlite::bitmaps::BitmapKey::SmartFolder(
                updated.smart_folder_id,
            ));
        let node = build_smart_folder_sidebar_node(&updated, existing_count as i64);
        db.with_conn(move |conn| crate::sidebar::db::upsert_sidebar_node(conn, &node))
            .await?;

        Ok((updated, predicate_changed))
    }

    pub async fn delete_smart_folder(db: &SqliteDatabase, id: String) -> Result<(), String> {
        let sf_id: i64 = id
            .parse()
            .map_err(|_| format!("Invalid smart folder id: {}", id))?;
        db.delete_smart_folder(sf_id).await?;
        // Also remove the sidebar_node row so the folder vanishes immediately
        let node_id = format!("smart:{}", sf_id);
        db.with_conn(move |conn| {
            crate::sidebar::db::delete_sidebar_node(conn, &node_id)?;
            Ok(())
        })
        .await?;
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
