//! Smart folder CRUD.

use super::ApplicationEngine;

impl ApplicationEngine {
    pub fn create_smart_folder(
        &self,
        name: &str,
        parent_id: Option<i64>,
        predicate_json: &str,
        icon: Option<&str>,
        color: Option<&str>,
    ) -> Result<i64, String> {
        self.db.create_smart_folder(name, parent_id, predicate_json, icon, color)
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: i64,
        name: Option<&str>,
        predicate_json: Option<&str>,
        icon: Option<&str>,
        color: Option<&str>,
        sort_field: Option<&str>,
        sort_order: Option<&str>,
    ) -> Result<(), String> {
        self.db.update_smart_folder(
            smart_folder_id, name, predicate_json, icon, color, sort_field, sort_order,
        )
    }

    pub fn delete_smart_folder(&self, smart_folder_id: i64) -> Result<(), String> {
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
}
