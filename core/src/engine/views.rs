//! Viewer history behavior.

use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::{Domain, SidebarNodePatch};

use super::ApplicationEngine;

impl ApplicationEngine {
    pub fn record_media_view(&self, entity_hash: &str) -> Result<(), String> {
        let (top_level_hash, count) = self.db.record_media_view(entity_hash)?;
        crate::events::emit_state_changed(
            "record_media_view",
            ChangeImpact::new()
                .add_domain(Domain::Sidebar)
                .entity_hashes(vec![top_level_hash])
                .extra_grid_scopes(vec!["system:recent_viewed".to_string()])
                .grid_reorder()
                .sidebar_node_patch(SidebarNodePatch {
                    node_id: "system:recent_viewed".to_string(),
                    count: Some(Some(count)),
                    ..Default::default()
                }),
        );
        Ok(())
    }
}
