//! Deferred/background work surface.

use super::ApplicationEngine;

pub use crate::background_work::{DeferredWorkFilter, DeferredWorkItemInfo, DeferredWorkSummary};

impl ApplicationEngine {
    pub fn get_deferred_work_summary(&self) -> Result<DeferredWorkSummary, String> {
        self.db.get_deferred_work_summary()
    }

    pub fn list_deferred_work_items(
        &self,
        filter: DeferredWorkFilter,
    ) -> Result<Vec<DeferredWorkItemInfo>, String> {
        self.db.list_deferred_work_items(filter)
    }

    pub fn retry_deferred_work(&self, entity_hash: &str) -> Result<(), String> {
        self.db.retry_deferred_work(entity_hash)
    }
}
