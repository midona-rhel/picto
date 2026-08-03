//! Collection operations — split from db/mod.rs, same `impl LibraryDatabase`.

use super::*;

fn resolve_entity_hashes_exact(conn: &Connection, hashes: &[String]) -> rusqlite::Result<Vec<i64>> {
    let mut ids = Vec::with_capacity(hashes.len());
    let mut resolve =
        conn.prepare_cached("SELECT entity_id FROM media_entity WHERE entity_hash = ?1")?;
    for hash in hashes {
        let id = resolve
            .query_row([hash], |row| row.get::<_, i64>(0))
            .map_err(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => rusqlite::Error::InvalidParameterName(
                    format!("Unknown media entity hash: {hash}"),
                ),
                other => other,
            })?;
        ids.push(id);
    }
    Ok(ids)
}

impl LibraryDatabase {
    // ── Collection operations ────────────────────────────────────

    pub fn add_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        self.with_write(|conn| {
            let change = write::collections::add_members(conn, collection_id, member_entity_ids)?;
            emit_collection_membership_op(conn, &self.device_id, collection_id, &change)?;
            Ok(change)
        })
    }

    pub fn create_collection(&self, name: &str) -> Result<i64, String> {
        let n = name.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            let collection_id = write::collections::create_collection(conn, &n, &now)?;
            if let Some(hash) = query::collections::get_collection_hash(conn, collection_id)? {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "collection_created",
                    &hash,
                    &serde_json::json!({ "name": n }),
                )?;
            }
            Ok(collection_id)
        })
    }

    pub fn create_collection_with_members_by_hashes(
        &self,
        name: &str,
        hashes: &[String],
    ) -> Result<i64, String> {
        if hashes.is_empty() {
            return Err("a collection requires at least one member".to_string());
        }

        let name = name.to_string();
        let hashes = hashes.to_vec();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            let ids = resolve_entity_hashes_exact(conn, &hashes)?;
            let collection_id = write::collections::create_collection(conn, &name, &now)?;
            let change = write::collections::add_members(conn, collection_id, &ids)?;
            let collection_hash = query::collections::get_collection_hash(conn, collection_id)?
                .ok_or(rusqlite::Error::QueryReturnedNoRows)?;
            crate::oplog::record_op(
                conn,
                &self.device_id,
                "collection_created",
                &collection_hash,
                &serde_json::json!({ "name": name }),
            )?;
            let members = entity_hashes_for_ids(conn, &ids)?;
            crate::oplog::record_op(
                conn,
                &self.device_id,
                "collection_members_added",
                &collection_hash,
                &serde_json::json!({ "members": members }),
            )?;
            debug_assert_eq!(change.collection_id, collection_id);
            Ok(collection_id)
        })
    }

    pub fn update_collection_name(&self, collection_id: i64, name: &str) -> Result<(), String> {
        let n = name.to_string();
        let now = chrono::Utc::now().to_rfc3339();
        self.with_write(move |conn| {
            write::collections::update_collection_name(conn, collection_id, &n, &now)?;
            if let Some(hash) = query::collections::get_collection_hash(conn, collection_id)? {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "collection_renamed",
                    &hash,
                    &serde_json::json!({ "name": n }),
                )?;
            }
            Ok(())
        })
    }

    pub fn remove_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        self.with_write(|conn| {
            let change =
                write::collections::remove_members(conn, collection_id, member_entity_ids)?;
            emit_collection_membership_op(conn, &self.device_id, collection_id, &change)?;
            Ok(change)
        })
    }

    pub fn reorder_collection_members(
        &self,
        collection_id: i64,
        ordered_entity_ids: &[i64],
    ) -> Result<(), String> {
        let ids = ordered_entity_ids.to_vec();
        self.with_write(move |conn| {
            write::collections::reorder_members(conn, collection_id, &ids)?;
            if let Some(hash) = query::collections::get_collection_hash(conn, collection_id)? {
                let order = entity_hashes_for_ids(conn, &ids)?;
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "collection_members_reordered",
                    &hash,
                    &serde_json::json!({ "order": order }),
                )?;
            }
            Ok(())
        })
    }

    pub fn reorder_collection_members_by_hashes(
        &self,
        collection_id: i64,
        ordered_hashes: &[String],
    ) -> Result<(), String> {
        let hashes = ordered_hashes.to_vec();
        self.with_write(move |conn| {
            let current_rows =
                query::collections::list_collection_member_hash_rows(conn, collection_id)?;
            if current_rows.is_empty() {
                return Ok(());
            }

            let mut by_hash = std::collections::HashMap::<String, i64>::new();
            let mut current_hash_order = Vec::with_capacity(current_rows.len());
            for (entity_id, hash) in current_rows {
                by_hash.insert(hash.clone(), entity_id);
                current_hash_order.push(hash);
            }

            let mut seen = std::collections::HashSet::<String>::new();
            let mut final_order = Vec::with_capacity(current_hash_order.len());
            for hash in &hashes {
                if by_hash.contains_key(hash) && seen.insert(hash.clone()) {
                    final_order.push(*by_hash.get(hash).unwrap());
                }
            }
            for hash in current_hash_order {
                if seen.insert(hash.clone()) {
                    final_order.push(*by_hash.get(&hash).unwrap());
                }
            }

            write::collections::reorder_members(conn, collection_id, &final_order)?;
            if let Some(hash) = query::collections::get_collection_hash(conn, collection_id)? {
                let order = entity_hashes_for_ids(conn, &final_order)?;
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "collection_members_reordered",
                    &hash,
                    &serde_json::json!({ "order": order }),
                )?;
            }
            Ok(())
        })
    }

    pub fn add_collection_members_by_hashes(
        &self,
        collection_id: i64,
        hashes: &[String],
    ) -> Result<CollectionMembershipChange, String> {
        self.with_write(|conn| {
            let ids = resolve_entity_hashes_exact(conn, hashes)?;
            let change = write::collections::add_members(conn, collection_id, &ids)?;
            emit_collection_membership_op(conn, &self.device_id, collection_id, &change)?;
            Ok(change)
        })
    }

    pub fn remove_collection_members_by_hashes(
        &self,
        collection_id: i64,
        hashes: &[String],
    ) -> Result<CollectionMembershipChange, String> {
        let ids = self.resolve_entity_hashes(hashes)?;
        self.remove_collection_members(collection_id, &ids)
    }

    pub fn split_collection(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        self.with_write(|conn| {
            let hash = query::collections::get_collection_hash(conn, collection_id)?;
            let result = write::collections::split_collection(conn, collection_id)?;
            // Splitting dissolves the container; the member singles live on.
            if let Some(hash) = hash {
                crate::oplog::record_op(
                    conn,
                    &self.device_id,
                    "collection_split",
                    &hash,
                    &serde_json::json!({}),
                )?;
            }
            Ok(result)
        })
    }

    pub fn get_collections(&self) -> Result<Vec<CollectionRecord>, String> {
        self.with_read(query::collections::list_collections)
    }

    pub fn get_collection_summary(&self, collection_id: i64) -> Result<CollectionSummary, String> {
        self.with_read(|conn| query::collections::get_collection_summary(conn, collection_id))
    }

    pub fn get_collection_hash(&self, collection_id: i64) -> Result<Option<String>, String> {
        self.with_read(|conn| query::collections::get_collection_hash(conn, collection_id))
    }

    pub fn get_collection_folder_ids(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        self.with_read(|conn| query::collections::get_collection_folder_ids(conn, collection_id))
    }

    pub fn get_folder_ids_for_entities(&self, entity_ids: &[i64]) -> Result<Vec<i64>, String> {
        let ids = entity_ids.to_vec();
        self.with_read(|conn| query::collections::get_folder_ids_for_entities(conn, &ids))
    }

    pub fn get_folder_entity_count(&self, folder_id: i64) -> Result<Option<i64>, String> {
        self.with_read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM folder_member WHERE folder_id = ?1",
                [folder_id],
                |row| row.get(0),
            )
            .optional()
        })
    }
}
