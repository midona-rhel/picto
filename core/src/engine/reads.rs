//! Entity read surface.

use crate::db::types::{EntityDetails, EntityGridItem, EntityViewPage, EntityViewQuery};

use super::ApplicationEngine;

impl ApplicationEngine {
    /// Primary grid query — one typed query, one result page.
    pub fn query_entity_view(&self, query: EntityViewQuery) -> Result<EntityViewPage, String> {
        self.db.query_entity_view(&query)
    }

    /// Single entity detail read (inspector/detail panel).
    pub fn get_entity_details(
        &self,
        entity_hash: &str,
    ) -> Result<Option<EntityDetails>, String> {
        self.db.get_entity_details(entity_hash)
    }

    /// Batch grid-item read for targeted reconciliation / eager insertion.
    pub fn get_entity_grid_items(
        &self,
        entity_hashes: &[String],
    ) -> Result<Vec<EntityGridItem>, String> {
        self.db.get_entity_grid_items(entity_hashes)
    }
}
