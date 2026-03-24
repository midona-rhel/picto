//! Deferred work surface — summary and retry controls.
//! Separate from normal entity reads.

use serde::Serialize;

use super::ApplicationEngine;

#[derive(Debug, Serialize)]
pub struct DeferredWorkSummary {
    pub pending_count: i64,
    pub running_count: i64,
    pub failed_count: i64,
}

impl ApplicationEngine {
    pub fn get_deferred_work_summary(&self) -> Result<DeferredWorkSummary, String> {
        self.db.get_deferred_work_summary()
    }

    pub fn retry_deferred_work(&self, entity_hash: &str) -> Result<(), String> {
        self.db.retry_deferred_work(entity_hash)
    }
}
