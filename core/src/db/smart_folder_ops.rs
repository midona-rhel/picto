//! Smart folder operations — split from db/mod.rs, same `impl LibraryDatabase`.

use super::*;

impl LibraryDatabase {
    // ── Smart folder operations ──────────────────────────────────

    pub fn create_smart_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        predicate_json: &str,
        icon: Option<&str>,
        color: Option<&str>,
        notes: Option<&str>,
    ) -> Result<i64, String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(|conn| {
            let smart_folder_id = write::smart_folders::create_smart_folder(
                conn,
                name,
                parent_id,
                predicate_json,
                icon,
                color,
                notes,
                &now,
            )?;
            if let Some(uuid) = smart_folder_uuid(conn, smart_folder_id)? {
                let parent_uuid = match parent_id {
                    Some(pid) => smart_folder_uuid(conn, pid)?,
                    None => None,
                };
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "smart_folder_created",
                    &uuid,
                    &serde_json::json!({
                        "name": name,
                        "parent": parent_uuid,
                        "predicate": predicate_json,
                        "icon": icon,
                        "color": color,
                        "notes": notes,
                    }),
                )?;
            }
            Ok(smart_folder_id)
        })
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: i64,
        name: Option<&str>,
        predicate_json: Option<&str>,
        icon: Option<&str>,
        color: Option<&str>,
        notes: Option<&str>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        let n = name.map(str::to_string);
        let p = predicate_json.map(str::to_string);
        let i = icon.map(str::to_string);
        let c = color.map(str::to_string);
        let notes = notes.map(str::to_string);
        self.with_write(move |conn| {
            write::smart_folders::update_smart_folder(
                conn,
                smart_folder_id,
                n.as_deref(),
                p.as_deref(),
                i.as_deref(),
                c.as_deref(),
                notes.as_deref(),
                &now,
            )?;
            let mut fields = serde_json::Map::new();
            if let Some(v) = &n {
                fields.insert("name".into(), v.clone().into());
            }
            if let Some(v) = &p {
                fields.insert("predicate".into(), v.clone().into());
            }
            if let Some(v) = &i {
                fields.insert("icon".into(), v.clone().into());
            }
            if let Some(v) = &c {
                fields.insert("color".into(), v.clone().into());
            }
            if let Some(v) = &notes {
                fields.insert("notes".into(), v.clone().into());
            }
            if !fields.is_empty() {
                if let Some(uuid) = smart_folder_uuid(conn, smart_folder_id)? {
                    crate::oplog::record_op(
                        conn,
                        &self.device_id,
                        "smart_folder_updated",
                        &uuid,
                        &serde_json::Value::Object(fields),
                    )?;
                }
            }
            Ok(())
        })
    }

    pub fn delete_smart_folder(
        &self,
        smart_folder_id: i64,
    ) -> Result<(Vec<i64>, Option<i64>), String> {
        self.with_write(|conn| {
            let uuid = smart_folder_uuid(conn, smart_folder_id)?;
            let result = write::smart_folders::delete_smart_folder(conn, smart_folder_id)?;
            if let Some(uuid) = uuid {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "smart_folder_deleted",
                    &uuid,
                    &serde_json::json!({}),
                )?;
            }
            Ok(result)
        })
    }

    pub fn move_smart_folder(
        &self,
        smart_folder_id: i64,
        new_parent_id: Option<i64>,
    ) -> Result<(), String> {
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            write::smart_folders::move_smart_folder(conn, smart_folder_id, new_parent_id, &now)?;
            if let Some(uuid) = smart_folder_uuid(conn, smart_folder_id)? {
                let parent_uuid = match new_parent_id {
                    Some(pid) => smart_folder_uuid(conn, pid)?,
                    None => None,
                };
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "smart_folder_moved",
                    &uuid,
                    &serde_json::json!({ "parent": parent_uuid }),
                )?;
            }
            Ok(())
        })
    }

    pub fn reorder_smart_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        let m = moves.to_vec();
        self.with_write(move |conn| write::smart_folders::reorder_smart_folders(conn, &m))
    }
}
