use crate::db::projection::compiler::CompilerPlan;
use crate::db::types::{DuplicatePairPage, DuplicateResolutionResult, DuplicateScanSummary};
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::Domain;

use super::ApplicationEngine;

impl ApplicationEngine {
    pub fn scan_duplicates(
        &self,
        threshold: Option<u32>,
        review_threshold: Option<u32>,
    ) -> Result<DuplicateScanSummary, String> {
        let result = self.db.scan_duplicates(threshold, review_threshold)?;
        self.db.run_compiler(CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
        let mut impact = ChangeImpact::new().add_domain(Domain::Sidebar);
        impact.sidebar_counts = self.settled_sidebar_counts();
        crate::events::emit_state_changed("scan_duplicates", impact);
        Ok(result)
    }

    pub fn get_duplicate_pairs(
        &self,
        cursor: Option<String>,
        limit: usize,
        status: Option<String>,
        max_distance: Option<f64>,
    ) -> Result<DuplicatePairPage, String> {
        self.db
            .get_duplicate_pairs(cursor, limit, status, max_distance)
    }

    pub fn resolve_duplicate_pair(
        &self,
        action: &str,
        hash_a: &str,
        hash_b: &str,
    ) -> Result<DuplicateResolutionResult, String> {
        let result = self.db.resolve_duplicate_pair(action, hash_a, hash_b)?;
        if matches!(
            result.status,
            crate::db::types::DuplicateResolveStatus::Resolved
        ) {
            let mut impact = ChangeImpact::new()
                .add_domain(Domain::Sidebar)
                .entity_hashes(
                    result
                        .winner_hash
                        .iter()
                        .cloned()
                        .chain(result.loser_hash.iter().cloned())
                        .collect(),
                );
            if !result.affected_folder_ids.is_empty() {
                impact = impact.folder_membership_changed(result.affected_folder_ids.clone());
            }
            if result.tags_merged > 0 {
                impact = impact.tags_changed().all_smart_folder_scopes_changed();
            }
            self.db.run_compiler(CompilerPlan {
                rebuild_sidebar: true,
                rebuild_all_smart_folders: result.tags_merged > 0,
                ..Default::default()
            });
            if result.tags_merged > 0 {
                if let Ok(counts) = self.all_smart_folder_counts() {
                    impact.smart_folder_ids = Some(counts.iter().map(|(id, _)| *id).collect());
                    impact.smart_folder_counts = Some(counts);
                }
            }
            impact.sidebar_counts = self.settled_sidebar_counts();
            crate::events::emit_state_changed("resolve_duplicate_pair", impact);
        }
        Ok(result)
    }
}
