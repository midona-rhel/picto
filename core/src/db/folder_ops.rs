//! Folder operations — split from db/mod.rs, same `impl LibraryDatabase`.

use super::*;

impl LibraryDatabase {
    // ── Folder operations ────────────────────────────────────────

    pub fn create_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        icon: Option<&str>,
        color: Option<&str>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            let folder_id =
                write::folders::create_folder(conn, name, parent_id, icon, color, &now)?;
            let uuid = folder_uuid(conn, folder_id)?.unwrap_or_default();
            let parent_uuid = match parent_id {
                Some(pid) => folder_uuid(conn, pid)?,
                None => None,
            };
            crate::oplog::record_op(
                conn,
                &self.device_id,
                "folder_created",
                &uuid,
                &serde_json::json!({
                    "name": name,
                    "parent": parent_uuid,
                    "icon": icon,
                    "color": color,
                }),
            )?;
            Ok(folder_id)
        })
    }

    pub fn update_folder(&self, folder_id: i64, patch: &types::FolderPatch) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let p = patch.clone();
        self.with_write(move |conn| {
            write::folders::update_folder(conn, folder_id, &p, &now)?;
            // Only truth fields sync; watch config is device-local.
            let mut fields = serde_json::Map::new();
            if let Some(v) = &p.name {
                fields.insert("name".into(), v.clone().into());
            }
            if let Some(v) = &p.icon {
                fields.insert("icon".into(), v.clone().into());
            }
            if let Some(v) = &p.color {
                fields.insert("color".into(), v.clone().into());
            }
            if let Some(v) = &p.notes {
                fields.insert("notes".into(), v.clone().into());
            }
            if !fields.is_empty() {
                if let Some(uuid) = folder_uuid(conn, folder_id)? {
                    crate::oplog::record_op(
                        conn,
                        &self.device_id,
                        "folder_updated",
                        &uuid,
                        &serde_json::Value::Object(fields),
                    )?;
                }
            }
            Ok(())
        })
    }

    pub fn delete_folder(&self, folder_id: i64) -> Result<(), String> {
        self.with_write(|conn| {
            let uuid = folder_uuid(conn, folder_id)?;
            write::folders::delete_folder(conn, folder_id)?;
            if let Some(uuid) = uuid {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "folder_deleted",
                    &uuid,
                    &serde_json::json!({}),
                )?;
            }
            Ok(())
        })
    }

    pub fn move_folder(&self, folder_id: i64, new_parent_id: Option<i64>) -> Result<(), String> {
        if new_parent_id == Some(folder_id) {
            return Err("Cannot move a folder into itself".into());
        }
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            // Prevent cycles: check if new_parent_id is a descendant of folder_id
            if let Some(target) = new_parent_id {
                if write::folders::is_ancestor_of(conn, target, folder_id)? {
                    return Err(rusqlite::Error::InvalidParameterName(
                        "Cannot move a folder into one of its descendants".into(),
                    ));
                }
            }
            write::folders::move_folder(conn, folder_id, new_parent_id, &now)?;
            if let Some(uuid) = folder_uuid(conn, folder_id)? {
                let parent_uuid = match new_parent_id {
                    Some(pid) => folder_uuid(conn, pid)?,
                    None => None,
                };
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "folder_moved",
                    &uuid,
                    &serde_json::json!({ "parent": parent_uuid }),
                )?;
            }
            Ok(())
        })
    }

    pub fn reorder_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::folders::reorder_folders(conn, &m))
    }

    pub fn set_folder_pinned(&self, folder_id: i64, pinned: bool) -> Result<(), String> {
        self.with_write(move |conn| {
            conn.execute(
                "UPDATE folder SET pinned = ?1 WHERE folder_id = ?2",
                rusqlite::params![pinned as i64, folder_id],
            )?;
            Ok(())
        })
    }

    pub fn set_smart_folder_pinned(
        &self,
        smart_folder_id: i64,
        pinned: bool,
    ) -> Result<(), String> {
        self.with_write(move |conn| {
            conn.execute(
                "UPDATE smart_folder SET pinned = ?1 WHERE smart_folder_id = ?2",
                rusqlite::params![pinned as i64, smart_folder_id],
            )?;
            Ok(())
        })
    }

    pub fn reorder_pinned_items(&self, moves: &[(String, i64)]) -> Result<(), String> {
        let moves = moves.to_vec();
        self.with_write(move |conn| {
            for (node_id, pin_order) in &moves {
                if let Some(raw) = node_id.strip_prefix("folder:") {
                    if let Ok(folder_id) = raw.parse::<i64>() {
                        conn.execute(
                            "UPDATE folder SET pin_order = ?1 WHERE folder_id = ?2",
                            rusqlite::params![pin_order, folder_id],
                        )?;
                    }
                } else if let Some(raw) = node_id.strip_prefix("smart:") {
                    if let Ok(smart_folder_id) = raw.parse::<i64>() {
                        conn.execute(
                            "UPDATE smart_folder SET pin_order = ?1 WHERE smart_folder_id = ?2",
                            rusqlite::params![pin_order, smart_folder_id],
                        )?;
                    }
                }
            }
            Ok(())
        })
    }

    pub fn reorder_folder_items(&self, folder_id: i64, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::folders::reorder_members(conn, folder_id, &m))
    }

    pub fn get_folder(&self, folder_id: i64) -> Result<Option<query::folders::FolderRow>, String> {
        self.with_read(|conn| query::folders::get_folder(conn, folder_id))
    }

    pub fn collect_descendant_smart_folder_ids(&self, root_id: i64) -> Result<Vec<i64>, String> {
        self.with_read(|conn| query::folders::collect_descendant_smart_folder_ids(conn, root_id))
    }

    pub fn get_smart_folder(
        &self,
        smart_folder_id: i64,
    ) -> Result<Option<query::folders::SmartFolderRow>, String> {
        self.with_read(|conn| query::folders::get_smart_folder(conn, smart_folder_id))
    }

    pub fn find_child_folder_id(&self, parent_id: i64, name: &str) -> Result<Option<i64>, String> {
        let child_name = name.to_string();
        self.with_read(move |conn| {
            query::ingest::find_child_folder_id(conn, parent_id, &child_name)
        })
    }

    pub fn list_folders_canonical(&self) -> Result<Vec<query::folders::FolderRow>, String> {
        self.with_read(|conn| query::folders::list_folders(conn))
    }

    pub fn list_smart_folders_canonical(
        &self,
    ) -> Result<Vec<query::folders::SmartFolderRow>, String> {
        self.with_read(|conn| query::folders::list_smart_folders(conn))
    }

    pub fn add_folder_members(
        &self,
        folder_id: i64,
        entity_ids: &[i64],
        expansion: ExpansionMode,
    ) -> Result<FolderMembershipChange, String> {
        self.with_write(|conn| {
            let change = write::folders::add_members(conn, folder_id, entity_ids, expansion)?;
            emit_folder_membership_op(
                conn,
                &self.device_id,
                "folder_members_added",
                folder_id,
                &change.entity_ids,
            )?;
            Ok(change)
        })
    }

    pub fn remove_folder_members(
        &self,
        folder_id: i64,
        entity_ids: &[i64],
        expansion: ExpansionMode,
    ) -> Result<FolderMembershipChange, String> {
        self.with_write(|conn| {
            let change = write::folders::remove_members(conn, folder_id, entity_ids, expansion)?;
            emit_folder_membership_op(
                conn,
                &self.device_id,
                "folder_members_removed",
                folder_id,
                &change.entity_ids,
            )?;
            Ok(change)
        })
    }
}
