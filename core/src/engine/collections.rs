//! Collection CRUD + membership behavior.

use crate::db::types::{CollectionMembershipChange, CollectionRecord, CollectionSummary};

use super::{ApplicationEngine, WriteChange};

fn collection_grid_scopes(collection_id: i64, folder_ids: &[i64]) -> Vec<String> {
    let mut scopes = vec![
        format!("collection:{collection_id}"),
        "system:active".to_string(),
    ];
    for folder_id in folder_ids {
        scopes.push(format!("folder:{folder_id}"));
    }
    scopes
}

impl ApplicationEngine {
    pub fn get_collections(&self) -> Result<Vec<CollectionRecord>, String> {
        self.db.get_collections()
    }

    pub fn get_collection_summary(&self, collection_id: i64) -> Result<CollectionSummary, String> {
        self.db.get_collection_summary(collection_id)
    }

    pub fn list_collection_member_hashes(&self, collection_id: i64) -> Result<Vec<String>, String> {
        self.db.list_collection_member_hashes(collection_id)
    }

    pub fn create_collection(&self, name: &str) -> Result<i64, String> {
        let collection_id = self.db.create_collection(name)?;
        let collection_hash = self
            .db
            .get_collection_hash(collection_id)?
            .into_iter()
            .collect();
        self.commit_write(&WriteChange {
            origin: "create_collection".to_string(),
            entity_hashes: collection_hash,
            status_changed: true,
            extra_grid_scopes: vec!["system:active".to_string()],
            ..Default::default()
        });
        Ok(collection_id)
    }

    pub fn update_collection(&self, collection_id: i64, name: &str) -> Result<(), String> {
        self.db.update_collection_name(collection_id, name)?;
        let collection_hash = self
            .db
            .get_collection_hash(collection_id)?
            .into_iter()
            .collect();
        let folder_ids = self.db.get_collection_folder_ids(collection_id)?;
        self.commit_write(&WriteChange {
            origin: "update_collection".to_string(),
            entity_hashes: collection_hash,
            metadata_changed: true,
            extra_grid_scopes: collection_grid_scopes(collection_id, &folder_ids),
            ..Default::default()
        });
        Ok(())
    }

    pub fn delete_collection(&self, collection_id: i64) -> Result<(), String> {
        let folder_ids = self.db.get_collection_folder_ids(collection_id)?;
        // Get collection's own hash before deletion so the grid can remove the tile
        let collection_hash = self.db.get_collection_hash(collection_id)?;
        let freed_member_ids = self.db.delete_collection(collection_id)?;
        let mut entity_hashes = self.db.get_entity_hashes_by_ids(&freed_member_ids)?;
        if let Some(ch) = collection_hash {
            entity_hashes.push(ch);
        }
        self.commit_write(&WriteChange {
            origin: "delete_collection".to_string(),
            entity_hashes,
            entity_ids: freed_member_ids,
            status_changed: true,
            extra_grid_scopes: collection_grid_scopes(collection_id, &folder_ids),
            ..Default::default()
        });
        Ok(())
    }

    pub fn add_collection_members_by_hashes(
        &self,
        collection_id: i64,
        member_hashes: &[String],
    ) -> Result<CollectionMembershipChange, String> {
        let change = self
            .db
            .add_collection_members_by_hashes(collection_id, member_hashes)?;
        let folder_ids = self.db.get_folder_ids_for_entities(&change.added)?;
        self.commit_write(&WriteChange {
            origin: "add_collection_members".to_string(),
            entity_ids: change.added.clone(),
            entity_hashes: self.db.get_entity_hashes_by_ids(&change.added)?,
            status_changed: true,
            extra_grid_scopes: collection_grid_scopes(collection_id, &folder_ids),
            ..Default::default()
        });
        Ok(change)
    }

    pub fn remove_collection_members_by_hashes(
        &self,
        collection_id: i64,
        member_hashes: &[String],
    ) -> Result<CollectionMembershipChange, String> {
        let change = self
            .db
            .remove_collection_members_by_hashes(collection_id, member_hashes)?;
        let folder_ids = self.db.get_folder_ids_for_entities(&change.removed)?;
        self.commit_write(&WriteChange {
            origin: "remove_collection_members".to_string(),
            entity_ids: change.removed.clone(),
            entity_hashes: self.db.get_entity_hashes_by_ids(&change.removed)?,
            status_changed: true,
            extra_grid_scopes: collection_grid_scopes(collection_id, &folder_ids),
            ..Default::default()
        });
        Ok(change)
    }

    pub fn reorder_collection_members_by_hashes(
        &self,
        collection_id: i64,
        ordered_hashes: &[String],
    ) -> Result<(), String> {
        self.db
            .reorder_collection_members_by_hashes(collection_id, ordered_hashes)?;
        let collection_hash = self
            .db
            .get_collection_hash(collection_id)?
            .into_iter()
            .collect();
        let folder_ids = self.db.get_collection_folder_ids(collection_id)?;
        self.commit_write(&WriteChange {
            origin: "reorder_collection_members".to_string(),
            entity_hashes: collection_hash,
            metadata_changed: true,
            extra_grid_scopes: collection_grid_scopes(collection_id, &folder_ids),
            ..Default::default()
        });
        Ok(())
    }

    pub fn split_collection(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        let folder_ids = self.db.get_collection_folder_ids(collection_id)?;
        let freed_ids = self.db.split_collection(collection_id)?;
        self.commit_write(&WriteChange {
            origin: "split_collection".to_string(),
            entity_ids: freed_ids.clone(),
            entity_hashes: self.db.get_entity_hashes_by_ids(&freed_ids)?,
            status_changed: true,
            extra_grid_scopes: collection_grid_scopes(collection_id, &folder_ids),
            ..Default::default()
        });
        Ok(freed_ids)
    }
}
