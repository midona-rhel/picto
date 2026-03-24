//! Selection summary.

use serde::Serialize;

use crate::db::types::EntityTarget;

use super::{target, ApplicationEngine};

#[derive(Debug, Serialize)]
pub struct SelectionSummary {
    pub total_count: i64,
    pub entity_hashes: Vec<String>,
}

impl ApplicationEngine {
    pub fn get_selection_summary(
        &self,
        target: EntityTarget,
    ) -> Result<SelectionSummary, String> {
        let resolved = target::resolve(&self.db, &target)?;
        match resolved {
            target::ResolvedTarget::Ids(ids) => {
                let hashes = self.db.get_entity_hashes_by_ids(&ids)?;
                Ok(SelectionSummary {
                    total_count: hashes.len() as i64,
                    entity_hashes: hashes,
                })
            }
            target::ResolvedTarget::Query { view_query, exclusions } => {
                let summary = self.db.get_selection_summary_from_query(&view_query, &exclusions)?;
                Ok(summary)
            }
        }
    }
}
