//! View preferences orchestration — per-scope layout, sort, and display
//! settings (tile size, label visibility, thumbnail fit).
//!
//! Each scope (e.g. `"system:all"`, `"folder:5"`) can have independent
//! view preferences stored in `sqlite::view_prefs`.

use crate::sqlite::SqliteDatabase;
use crate::types::{ViewPrefsDto, ViewPrefsPatch};

pub struct ViewPrefsController;

fn dto_from_pref(p: crate::sqlite::view_prefs::ViewPref) -> ViewPrefsDto {
    ViewPrefsDto {
        scope_key: p.scope,
        sort_field: p.sort_field,
        sort_order: p.sort_dir,
        view_mode: p.layout,
        target_size: p.tile_size,
        show_name: p.show_name,
        show_resolution: p.show_resolution,
        show_extension: p.show_extension,
        show_label: p.show_label,
        thumbnail_fit: p.thumbnail_fit,
    }
}

impl ViewPrefsController {
    pub async fn get_view_prefs(
        db: &SqliteDatabase,
        scope_key: String,
    ) -> Result<Option<ViewPrefsDto>, String> {
        let pref = db.get_view_pref(&scope_key).await?;
        Ok(pref.map(dto_from_pref))
    }

    pub async fn set_view_prefs(
        db: &SqliteDatabase,
        scope_key: String,
        patch: ViewPrefsPatch,
    ) -> Result<ViewPrefsDto, String> {
        let current = db.get_view_pref(&scope_key).await?;
        let merged = crate::sqlite::view_prefs::ViewPref {
            scope: scope_key.clone(),
            sort_field: patch
                .sort_field
                .or_else(|| current.as_ref().and_then(|p| p.sort_field.clone())),
            sort_dir: patch
                .sort_order
                .or_else(|| current.as_ref().and_then(|p| p.sort_dir.clone())),
            layout: patch
                .view_mode
                .or_else(|| current.as_ref().and_then(|p| p.layout.clone())),
            tile_size: patch
                .target_size
                .or_else(|| current.as_ref().and_then(|p| p.tile_size)),
            show_name: patch
                .show_name
                .or_else(|| current.as_ref().and_then(|p| p.show_name)),
            show_resolution: patch
                .show_resolution
                .or_else(|| current.as_ref().and_then(|p| p.show_resolution)),
            show_extension: patch
                .show_extension
                .or_else(|| current.as_ref().and_then(|p| p.show_extension)),
            show_label: patch
                .show_label
                .or_else(|| current.as_ref().and_then(|p| p.show_label)),
            thumbnail_fit: patch
                .thumbnail_fit
                .or_else(|| current.as_ref().and_then(|p| p.thumbnail_fit.clone())),
        };
        db.set_view_pref(merged.clone()).await?;
        Ok(dto_from_pref(merged))
    }
}
