//! Entity read surface.

use crate::db::types::{
    EntityDetails, EntityGridItem, EntityViewPage, EntityViewQuery, EntityViewReconcileRequest,
    EntityViewReconcileResult,
};

use super::ApplicationEngine;

impl ApplicationEngine {
    /// Primary grid query — one typed query, one result page.
    pub fn query_entity_view(&self, query: EntityViewQuery) -> Result<EntityViewPage, String> {
        let page = self.db.query_entity_view(&query)?;
        let entity_hashes = page
            .items
            .iter()
            .map(|item| item.entity_hash.clone())
            .collect::<Vec<_>>();
        let _ =
            crate::background_work::ensure_missing_color_analysis_jobs(&self.db, &entity_hashes);
        Ok(page)
    }

    /// Single entity detail read (inspector/detail panel).
    pub fn get_entity_details(&self, entity_hash: &str) -> Result<Option<EntityDetails>, String> {
        let details = self.db.get_entity_details(entity_hash)?;
        if details.is_some() {
            let _ = crate::background_work::ensure_missing_color_analysis_jobs(
                &self.db,
                &[entity_hash.to_string()],
            );
        }
        Ok(details)
    }

    pub fn get_entity_all_metadata(
        &self,
        entity_hash: &str,
    ) -> Result<Option<crate::types::EntityAllMetadata>, String> {
        self.db.get_entity_all_metadata(entity_hash)
    }

    /// Batch grid-item read for targeted reconciliation / eager insertion.
    pub fn get_entity_grid_items(
        &self,
        entity_hashes: &[String],
    ) -> Result<Vec<EntityGridItem>, String> {
        let items = self.db.get_entity_grid_items(entity_hashes)?;
        let present_hashes = items
            .iter()
            .map(|item| item.entity_hash.clone())
            .collect::<Vec<_>>();
        let _ =
            crate::background_work::ensure_missing_color_analysis_jobs(&self.db, &present_hashes);
        Ok(items)
    }

    /// Reconcile the current grid view after a state change.
    ///
    /// Three result kinds:
    /// - PatchRows: metadata_only=true, all visible hashes still present → patch in place
    /// - ReplaceWindow: membership changed → rerun query for loaded window size, return page
    /// - FullRefreshRequired: only for truly unrecoverable cases
    pub fn reconcile_entity_view(
        &self,
        req: EntityViewReconcileRequest,
    ) -> Result<EntityViewReconcileResult, String> {
        if req.visible_hashes.is_empty() {
            return Ok(EntityViewReconcileResult::NoChange);
        }

        if req.metadata_only {
            // Metadata/derivative-only: patch visible rows if all still present.
            let current_items = self.db.get_entity_grid_items(&req.visible_hashes)?;
            if current_items.len() != req.visible_hashes.len() {
                return Ok(EntityViewReconcileResult::FullRefreshRequired);
            }
            return Ok(EntityViewReconcileResult::PatchRows {
                items: current_items,
            });
        }

        // Membership may have changed. Rerun the query for the current loaded
        // window size (visible_hashes.len()) and return the replacement page.
        let window_size = req.visible_hashes.len() as i64;
        let mut requery = req.query;
        requery.page.limit = window_size;
        requery.page.cursor = None; // Always from the start — loaded prefix model

        let page = self.db.query_entity_view(&requery)?;
        Ok(EntityViewReconcileResult::ReplaceWindow { page })
    }
}
