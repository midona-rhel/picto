//! System surface — sidebar, settings, library info.

use super::ApplicationEngine;

impl ApplicationEngine {
    pub fn get_sidebar_tree(&self) -> Result<Vec<crate::db::query::sidebar::SidebarNode>, String> {
        self.db.get_sidebar_tree()
    }

    pub fn get_scope_counts(&self) -> Result<crate::db::query::stats::ScopeCounts, String> {
        self.db.get_scope_counts()
    }
}
