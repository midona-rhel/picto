//! Smart folder CRUD.

use super::ApplicationEngine;
use std::collections::HashSet;

impl ApplicationEngine {
    pub fn create_smart_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        predicate_json: &str,
        icon: Option<&str>,
        color: Option<&str>,
        notes: Option<&str>,
    ) -> Result<i64, String> {
        self.db
            .create_smart_folder(name, parent_id, predicate_json, icon, color, notes)
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: i64,
        name: Option<&str>,
        predicate_json: Option<&str>,
        icon: Option<&str>,
        color: Option<&str>,
        notes: Option<&str>,
        sort_field: Option<&str>,
        sort_order: Option<&str>,
    ) -> Result<(), String> {
        self.db.update_smart_folder(
            smart_folder_id,
            name,
            predicate_json,
            icon,
            color,
            notes,
            sort_field,
            sort_order,
        )
    }

    /// Delete a smart folder. Returns (promoted_child_ids, deleted_parent_id).
    pub fn delete_smart_folder(
        &self,
        smart_folder_id: i64,
    ) -> Result<(Vec<i64>, Option<i64>), String> {
        self.db.delete_smart_folder(smart_folder_id)
    }

    pub fn move_smart_folder(
        &self,
        smart_folder_id: i64,
        new_parent_id: Option<i64>,
    ) -> Result<(), String> {
        self.db.move_smart_folder(smart_folder_id, new_parent_id)
    }

    pub fn reorder_smart_folders(&self, moves: &[(i64, i64)]) -> Result<(), String> {
        self.db.reorder_smart_folders(moves)
    }

    pub fn collect_descendant_smart_folder_ids(&self, root_id: i64) -> Result<Vec<i64>, String> {
        self.db.collect_descendant_smart_folder_ids(root_id)
    }

    pub fn smart_folder_subtree_ids(&self, root_id: i64) -> Result<Vec<i64>, String> {
        let mut ids = vec![root_id];
        ids.extend(self.collect_descendant_smart_folder_ids(root_id)?);
        Ok(ids)
    }

    /// Rebuild the requested smart-folder memberships and return their exact counts.
    pub fn settle_smart_folders(&self, smart_folder_ids: &[i64]) -> Vec<(i64, i64)> {
        let mut seen = HashSet::new();
        let ids: Vec<i64> = smart_folder_ids
            .iter()
            .copied()
            .filter(|id| seen.insert(*id))
            .collect();
        self.run_compiler(crate::db::projection::compiler::CompilerPlan {
            rebuild_sidebar: true,
            dirty_smart_folder_ids: ids.clone(),
            ..Default::default()
        });
        ids.into_iter()
            .filter_map(|id| {
                self.get_smart_folder(id)
                    .ok()
                    .flatten()
                    .map(|_| (id, self.smart_folder_bitmap_len(id)))
            })
            .collect()
    }

    pub(crate) fn all_smart_folder_counts(&self) -> Result<Vec<(i64, i64)>, String> {
        Ok(self
            .db
            .list_smart_folders_canonical()?
            .into_iter()
            .map(|row| {
                let id = row.smart_folder_id;
                (id, self.smart_folder_bitmap_len(id))
            })
            .collect())
    }

    /// Get the current bitmap length for a smart folder (after compiler has run).
    pub fn smart_folder_bitmap_len(&self, smart_folder_id: i64) -> i64 {
        self.db
            .bitmap_len(&crate::db::projection::bitmaps::BitmapKey::SmartFolder(
                smart_folder_id,
            )) as i64
    }
}
