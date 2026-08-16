//! Entity write surface — patch, status, delete.
//!
//! Every write: resolve target → call db → commit_write (compile + emit).

use crate::db::types::*;

use super::{target, ApplicationEngine, WriteChange};

impl ApplicationEngine {
    /// Patch entity metadata (name, notes, rating, source_urls).
    pub fn patch_media_entities(
        &self,
        target: EntityTarget,
        patch: MediaEntityPatch,
    ) -> Result<EntityChange, String> {
        let resolved = target::resolve(&self.db, &target)?;
        let change = match resolved {
            target::ResolvedTarget::Ids(ids) => self.db.patch_entity_metadata(&ids, &patch)?,
            target::ResolvedTarget::Query {
                view_query,
                exclusions,
            } => self
                .db
                .patch_entity_metadata_bulk(&view_query, &exclusions, &patch)?,
        };
        self.commit_write(&WriteChange::from_entity(&change));
        Ok(change)
    }

    /// Change entity status (inbox/active/trash).
    pub fn set_entity_status(
        &self,
        target: EntityTarget,
        status: i64,
    ) -> Result<StatusChange, String> {
        let resolved = target::resolve(&self.db, &target)?;
        let change = match resolved {
            target::ResolvedTarget::Ids(ids) => self.db.set_entity_status(&ids, status)?,
            target::ResolvedTarget::Query {
                view_query,
                exclusions,
            } => self
                .db
                .set_entity_status_bulk(&view_query, &exclusions, status)?,
        };
        self.commit_write(&WriteChange::from_status(&change));
        Ok(change)
    }

    /// Permanently delete entities.
    pub fn delete_entities(&self, target: EntityTarget) -> Result<EntityChange, String> {
        let resolved = target::resolve(&self.db, &target)?;
        let change = match resolved {
            target::ResolvedTarget::Ids(ids) => self.db.delete_entities(&ids)?,
            target::ResolvedTarget::Query {
                view_query,
                exclusions,
            } => self.db.delete_entities_bulk(&view_query, &exclusions)?,
        };
        self.commit_write(&WriteChange::from_entity_delete(&change));
        Ok(change)
    }
}
