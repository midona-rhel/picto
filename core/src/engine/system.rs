//! System surface — sidebar, settings, library info.

use super::ApplicationEngine;

impl ApplicationEngine {
    pub fn count_library_files(&self) -> Result<i64, String> {
        self.db.count_media_files()
    }

    pub fn get_sidebar_tree(&self) -> Result<Vec<crate::db::query::sidebar::SidebarNode>, String> {
        self.db.get_sidebar_tree()
    }

    pub fn get_sidebar_tree_epoch(&self) -> Result<u64, String> {
        self.db.get_sidebar_tree_epoch()
    }

    pub fn get_scope_counts(&self) -> Result<crate::db::query::stats::ScopeCounts, String> {
        self.db.get_scope_counts()
    }

    pub fn reorder_sidebar_nodes(&self, moves: &[(String, i64)]) -> Result<(), String> {
        let mut folder_moves = Vec::new();
        let mut smart_folder_moves = Vec::new();

        for (node_id, sort_order) in moves {
            if let Some(raw) = node_id.strip_prefix("folder:") {
                if let Ok(folder_id) = raw.parse::<i64>() {
                    folder_moves.push((folder_id, *sort_order));
                }
            } else if let Some(raw) = node_id.strip_prefix("smart:") {
                if let Ok(smart_folder_id) = raw.parse::<i64>() {
                    smart_folder_moves.push((smart_folder_id, *sort_order));
                }
            }
        }

        if !folder_moves.is_empty() {
            self.db.reorder_folders(&folder_moves)?;
        }
        if !smart_folder_moves.is_empty() {
            self.db.reorder_smart_folders(&smart_folder_moves)?;
        }
        self.db
            .run_compiler(crate::db::projection::compiler::CompilerPlan {
                rebuild_sidebar: true,
                ..Default::default()
            });
        Ok(())
    }

    pub fn get_view_prefs(
        &self,
        scope_key: &str,
    ) -> Result<Option<crate::types::ViewPrefsDto>, String> {
        Ok(self.db.get_view_pref(scope_key)?.map(|pref| crate::types::ViewPrefsDto {
            scope_key: pref.scope,
            sort_field: pref.sort_field,
            sort_order: pref.sort_dir,
            view_mode: pref.layout,
            target_size: pref.tile_size,
            show_name: pref.show_name,
            show_resolution: pref.show_resolution,
            show_extension: pref.show_extension,
            show_label: pref.show_label,
            thumbnail_fit: pref.thumbnail_fit,
        }))
    }

    pub fn set_view_prefs(
        &self,
        scope_key: &str,
        patch: crate::types::ViewPrefsPatch,
    ) -> Result<crate::types::ViewPrefsDto, String> {
        let current = self.db.get_view_pref(scope_key)?;
        let merged = crate::settings::db::ViewPref {
            scope: scope_key.to_string(),
            sort_field: patch
                .sort_field
                .or_else(|| current.as_ref().and_then(|pref| pref.sort_field.clone())),
            sort_dir: patch
                .sort_order
                .or_else(|| current.as_ref().and_then(|pref| pref.sort_dir.clone())),
            layout: patch
                .view_mode
                .or_else(|| current.as_ref().and_then(|pref| pref.layout.clone())),
            tile_size: patch
                .target_size
                .or_else(|| current.as_ref().and_then(|pref| pref.tile_size)),
            show_name: patch
                .show_name
                .or_else(|| current.as_ref().and_then(|pref| pref.show_name)),
            show_resolution: patch
                .show_resolution
                .or_else(|| current.as_ref().and_then(|pref| pref.show_resolution)),
            show_extension: patch
                .show_extension
                .or_else(|| current.as_ref().and_then(|pref| pref.show_extension)),
            show_label: patch
                .show_label
                .or_else(|| current.as_ref().and_then(|pref| pref.show_label)),
            thumbnail_fit: patch
                .thumbnail_fit
                .or_else(|| current.as_ref().and_then(|pref| pref.thumbnail_fit.clone())),
        };
        self.db.set_view_pref(merged.clone())?;
        Ok(crate::types::ViewPrefsDto {
            scope_key: merged.scope,
            sort_field: merged.sort_field,
            sort_order: merged.sort_dir,
            view_mode: merged.layout,
            target_size: merged.tile_size,
            show_name: merged.show_name,
            show_resolution: merged.show_resolution,
            show_extension: merged.show_extension,
            show_label: merged.show_label,
            thumbnail_fit: merged.thumbnail_fit,
        })
    }

    pub fn get_storage_stats(
        &self,
    ) -> Result<
        (
            crate::sqlite::files::FileStats,
            crate::sqlite::files::MediaTypeBreakdown,
        ),
        String,
    > {
        Ok((
            self.db.aggregate_file_stats()?,
            self.db.aggregate_media_type_breakdown()?,
        ))
    }
}
