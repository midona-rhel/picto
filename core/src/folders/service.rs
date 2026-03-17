//! Folder orchestration — CRUD, sidebar node construction, entity membership,
//! collection ordering, and auto-tag propagation.
//!
//! Bridges between dispatch handlers and `sqlite::folders` for all folder and
//! collection operations.

use crate::sqlite::SqliteDatabase;
use crate::types::FolderReorderMove;

fn build_folder_sidebar_node(
    folder: &crate::folders::db::Folder,
    count: i64,
) -> crate::sidebar::db::SidebarNode {
    let parent_id = folder
        .parent_id
        .map(|pid| format!("folder:{pid}"))
        .unwrap_or_else(|| "section:folders".to_string());
    crate::sidebar::db::SidebarNode {
        node_id: format!("folder:{}", folder.folder_id),
        kind: "folder".into(),
        parent_id: Some(parent_id),
        name: folder.name.clone(),
        icon: folder.icon.clone(),
        color: folder.color.clone(),
        sort_order: folder.sort_order,
        count: Some(count),
        freshness: "stale".into(),
        epoch: 0,
        selectable: true,
        expanded_by_default: false,
        meta_json: Some(
            serde_json::json!({
                "folder_id": folder.folder_id,
                "auto_tags": folder.auto_tags,
                "watch_path": folder.watch_path,
                "watch_enabled": folder.watch_enabled,
                "watch_subfolders": folder.watch_subfolders,
                "watch_import_status_mode": folder.watch_import_status_mode,
            })
            .to_string(),
        ),
        updated_at: Some(chrono::Utc::now().to_rfc3339()),
    }
}

async fn upsert_folder_sidebar_node(
    db: &SqliteDatabase,
    folder: crate::folders::db::Folder,
) -> Result<(), String> {
    use crate::sqlite::bitmaps::BitmapKey;
    let folder_bm = db.bitmaps.get(&BitmapKey::Folder(folder.folder_id));
    let active_bm = db.bitmaps.get(&BitmapKey::AllActive);
    let count = (&folder_bm & &active_bm).len() as i64;
    let node = build_folder_sidebar_node(&folder, count);
    db.with_conn(move |conn| crate::sidebar::db::upsert_sidebar_node(conn, &node))
        .await?;
    Ok(())
}

pub async fn refresh_sidebar_projection_for_folder_ids(
    db: &SqliteDatabase,
    folder_ids: &[i64],
) -> Result<(), String> {
    if folder_ids.is_empty() {
        return Ok(());
    }
    let mut deduped = folder_ids.to_vec();
    deduped.sort_unstable();
    deduped.dedup();
    for folder_id in deduped {
        let folder = db
            .with_read_conn(move |conn| crate::folders::db::get_folder(conn, folder_id))
            .await?;
        if let Some(folder) = folder {
            upsert_folder_sidebar_node(db, folder).await?;
        }
    }
    Ok(())
}

pub async fn create_folder(
    db: &SqliteDatabase,
    name: String,
    parent_id: Option<i64>,
    icon: Option<String>,
    color: Option<String>,
) -> Result<crate::folders::db::Folder, String> {
    let folder = db
        .create_folder(crate::folders::db::NewFolder {
            name,
            parent_id,
            icon,
            color,
            auto_tags: Vec::new(),
        })
        .await?;
    // Insert sidebar projection immediately so the next sidebar fetch from
    // state-changed sees the new node even before compiler batching.
    upsert_folder_sidebar_node(db, folder.clone()).await?;
    Ok(folder)
}

pub async fn update_folder(
    db: &SqliteDatabase,
    folder_id: i64,
    name: Option<String>,
    icon: Option<String>,
    color: Option<String>,
    auto_tags: Option<Vec<String>>,
) -> Result<(), String> {
    let current = db
        .with_read_conn(move |conn| crate::folders::db::get_folder(conn, folder_id))
        .await?
        .ok_or_else(|| format!("Folder {} not found", folder_id))?;
    let final_name = name.unwrap_or(current.name);
    let final_icon = match &icon {
        Some(s) if s.is_empty() => None,
        Some(_) => icon,
        None => current.icon,
    };
    let final_color = match &color {
        Some(s) if s.is_empty() => None,
        Some(_) => color,
        None => current.color,
    };
    let final_auto_tags = auto_tags.unwrap_or(current.auto_tags);
    db.update_folder(
        folder_id,
        final_name,
        final_icon,
        final_color,
        final_auto_tags,
    )
    .await?;
    let updated = db
        .with_read_conn(move |conn| crate::folders::db::get_folder(conn, folder_id))
        .await?
        .ok_or_else(|| format!("Folder {} not found after update", folder_id))?;
    upsert_folder_sidebar_node(db, updated).await?;
    Ok(())
}

pub async fn set_folder_watch_config(
    db: &SqliteDatabase,
    folder_id: i64,
    watch_path: String,
    watch_enabled: bool,
    watch_subfolders: bool,
    watch_import_status_mode: String,
) -> Result<(), String> {
    let existing = db.get_folder_by_watch_path(watch_path.clone()).await?;
    if let Some(existing) = existing {
        if existing.folder_id != folder_id {
            return Err(format!(
                "That disk folder is already attached to \"{}\"",
                existing.name
            ));
        }
    }
    db.update_folder_watch_config(
        folder_id,
        watch_path,
        watch_enabled,
        watch_subfolders,
        watch_import_status_mode,
    )
    .await?;
    let updated = db
        .with_read_conn(move |conn| crate::folders::db::get_folder(conn, folder_id))
        .await?
        .ok_or_else(|| format!("Folder {} not found after watch update", folder_id))?;
    upsert_folder_sidebar_node(db, updated).await?;
    Ok(())
}

pub async fn clear_folder_watch_config(db: &SqliteDatabase, folder_id: i64) -> Result<(), String> {
    db.clear_folder_watch_config(folder_id).await?;
    let updated = db
        .with_read_conn(move |conn| crate::folders::db::get_folder(conn, folder_id))
        .await?
        .ok_or_else(|| format!("Folder {} not found after watch clear", folder_id))?;
    upsert_folder_sidebar_node(db, updated).await?;
    Ok(())
}

pub async fn ensure_child_folder(
    db: &SqliteDatabase,
    parent_id: i64,
    name: &str,
) -> Result<crate::folders::db::Folder, String> {
    if let Some(existing) = db
        .find_child_folder_by_name(Some(parent_id), name.to_string())
        .await?
    {
        return Ok(existing);
    }
    create_folder(db, name.to_string(), Some(parent_id), None, None).await
}

pub async fn delete_folder(db: &SqliteDatabase, folder_id: i64) -> Result<(), String> {
    db.delete_folder(folder_id).await?;
    let node_id = format!("folder:{}", folder_id);
    db.with_conn(move |conn| {
        crate::sidebar::db::delete_sidebar_node(conn, &node_id)?;
        Ok(())
    })
    .await?;
    Ok(())
}

pub async fn update_folder_parent(
    db: &SqliteDatabase,
    folder_id: i64,
    new_parent_id: Option<i64>,
) -> Result<(), String> {
    db.update_folder_parent(folder_id, new_parent_id).await?;
    let updated = db
        .with_read_conn(move |conn| crate::folders::db::get_folder(conn, folder_id))
        .await?
        .ok_or_else(|| format!("Folder {} not found after parent update", folder_id))?;
    upsert_folder_sidebar_node(db, updated).await?;
    Ok(())
}

pub async fn reorder_folder_items(
    db: &SqliteDatabase,
    folder_id: i64,
    moves: Vec<FolderReorderMove>,
) -> Result<(), String> {
    for mv in &moves {
        if mv.before_hash.is_some() && mv.after_hash.is_some() {
            return Err(
                "Each reorder move must specify only one of before_hash or after_hash".to_string(),
            );
        }
    }
    db.reorder_folder_items_batch(folder_id, moves).await?;
    Ok(())
}
