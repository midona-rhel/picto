//! Folder CRUD + membership operations.

use crate::db::types::*;

use super::{target, ApplicationEngine, WriteChange};

/// Folder membership operation kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MembershipOperation {
    Add,
    Remove,
}

impl ApplicationEngine {
    /// Add or remove exactly the targeted entities from a folder.
    pub fn update_folder_membership(
        &self,
        target: EntityTarget,
        folder_id: i64,
        operation: MembershipOperation,
    ) -> Result<FolderMembershipChange, String> {
        let resolved = target::resolve(&self.db, &target)?;
        let change = match resolved {
            target::ResolvedTarget::Ids(ids) => match operation {
                MembershipOperation::Add => {
                    self.db.add_folder_members(folder_id, &ids)?
                }
                MembershipOperation::Remove => {
                    self.db.remove_folder_members(folder_id, &ids)?
                }
            },
            target::ResolvedTarget::Query {
                view_query,
                exclusions,
            } => match operation {
                MembershipOperation::Add => self.db.add_folder_members_bulk(
                    folder_id,
                    &view_query,
                    &exclusions,
                )?,
                MembershipOperation::Remove => self.db.remove_folder_members_bulk(
                    folder_id,
                    &view_query,
                    &exclusions,
                )?,
            },
        };
        self.commit_write(&WriteChange::from_folder(&change));
        Ok(change)
    }

    // ── Folder CRUD ────────────────────────────────────────────

    pub fn create_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        icon: Option<&str>,
        color: Option<&str>,
    ) -> Result<i64, String> {
        self.db.create_folder(name, parent_id, icon, color)
    }

    pub fn update_folder(
        &self,
        folder_id: i64,
        patch: &crate::db::types::FolderPatch,
    ) -> Result<(), String> {
        self.db.update_folder(folder_id, patch)
    }

    pub fn delete_folder(
        &self,
        folder_id: i64,
    ) -> Result<crate::db::types::FolderDeleteResult, String> {
        self.db.delete_folder(folder_id)
    }

    pub fn move_folder(&self, folder_id: i64, new_parent_id: Option<i64>) -> Result<(), String> {
        self.db.move_folder(folder_id, new_parent_id)
    }

    pub fn reorder_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        self.db.reorder_folders(moves)
    }

    pub fn reorder_folder_items(&self, folder_id: i64, moves: &[(i64, i64)]) -> Result<(), String> {
        self.db.reorder_folder_items(folder_id, moves)
    }

    pub fn get_folder(
        &self,
        folder_id: i64,
    ) -> Result<Option<crate::db::query::folders::FolderRow>, String> {
        self.db.get_folder(folder_id)
    }

    pub fn get_smart_folder(
        &self,
        smart_folder_id: i64,
    ) -> Result<Option<crate::db::query::folders::SmartFolderRow>, String> {
        self.db.get_smart_folder(smart_folder_id)
    }

    pub fn get_folder_cover_hash(&self, folder_id: i64) -> Result<Option<String>, String> {
        self.db.get_folder_cover_hash(folder_id)
    }

    pub fn run_compiler(&self, plan: crate::db::projection::compiler::CompilerPlan) {
        self.db.run_compiler(plan);
    }

    /// Recompile just the sidebar projection — the most common compiler plan.
    pub fn rebuild_sidebar(&self) {
        self.run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            ..Default::default()
        });
    }
}
