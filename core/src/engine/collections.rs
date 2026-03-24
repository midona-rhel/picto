//! Collection membership behavior — add/remove members, reorder, split.
//! Metadata/status/delete for collections go through writes.rs.

use crate::db::types::CollectionMembershipChange;

use super::{ApplicationEngine, WriteChange};

impl ApplicationEngine {
    pub fn add_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        let change = self.db.add_collection_members(collection_id, member_entity_ids)?;
        self.commit_write(&WriteChange {
            entity_ids: change.added.clone(),
            status_changed: true,
            ..Default::default()
        });
        Ok(change)
    }

    pub fn remove_collection_members(
        &self,
        collection_id: i64,
        member_entity_ids: &[i64],
    ) -> Result<CollectionMembershipChange, String> {
        let change = self.db.remove_collection_members(collection_id, member_entity_ids)?;
        self.commit_write(&WriteChange {
            entity_ids: change.removed.clone(),
            status_changed: true,
            ..Default::default()
        });
        Ok(change)
    }

    pub fn reorder_collection_members(
        &self,
        collection_id: i64,
        ordered_entity_ids: &[i64],
    ) -> Result<(), String> {
        self.db.reorder_collection_members(collection_id, ordered_entity_ids)
    }

    pub fn split_collection(&self, collection_id: i64) -> Result<Vec<i64>, String> {
        let freed_ids = self.db.split_collection(collection_id)?;
        self.commit_write(&WriteChange {
            entity_ids: freed_ids.clone(),
            status_changed: true,
            ..Default::default()
        });
        Ok(freed_ids)
    }
}
