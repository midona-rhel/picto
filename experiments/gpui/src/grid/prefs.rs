//! View preferences for a scope, read from the library's `view_pref` rows with
//! the same fallbacks as `gridController.ts`: the scope's own row, then
//! `grid:defaults`, then the built-in defaults.
use super::layout::ViewMode;
use picto_library::predicate::{ItemSort, SortDirection, SortField};
use picto_library::query::ItemScope;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ViewPrefs {
    pub mode: ViewMode,
    pub target_size: f32,
    pub gap: f32,
    pub show_name: bool,
    pub show_resolution: bool,
    /// The file's extension after its name (`showExtension`).
    pub show_extension: bool,
    pub show_extension_label: bool,
    pub show_item_count: bool,
    pub fit_cover: bool,
    /// A folder shows its subfolders' items too (`showSubfolders`).
    pub show_subfolders: bool,
    pub sort: ItemSort,
}

pub fn scope_key(scope: &ItemScope) -> String {
    match scope {
        ItemScope::All | ItemScope::MediaMatches { .. } => "system:active".into(),
        ItemScope::Inbox => "system:inbox".into(),
        ItemScope::Trash => "system:trash".into(),
        ItemScope::RecentlyViewed => "system:recent_viewed".into(),
        ItemScope::Untagged => "system:untagged".into(),
        ItemScope::Uncategorized => "system:uncategorized".into(),
        ItemScope::Folder { folder_id } | ItemScope::FolderTree { folder_id } => {
            format!("folder:{}", folder_id.0)
        }
        ItemScope::SmartFolder { smart_folder_id } => format!("smart:{}", smart_folder_id.0),
    }
}

impl Default for ViewPrefs {
    fn default() -> Self {
        Self::resolve(&ItemScope::All, &Value::Null, &Value::Null, "")
    }
}

impl ViewPrefs {
    pub fn resolve(scope: &ItemScope, defaults: &Value, overrides: &Value, seed: &str) -> Self {
        let value = |field: &str| {
            overrides
                .get(field)
                .filter(|v| !v.is_null())
                .or_else(|| defaults.get(field).filter(|v| !v.is_null()))
        };
        let flag =
            |field: &str, fallback: bool| value(field).and_then(Value::as_bool).unwrap_or(fallback);
        let text = |field: &str| value(field).and_then(Value::as_str);
        let inbox = matches!(scope, ItemScope::Inbox);
        let field = match text("sort_field") {
            Some("captured_at") => SortField::CapturedAt,
            Some("name") => SortField::Name,
            Some("rating") => SortField::Rating,
            // Older builds saved the File Size choice as 'size'.
            Some("total_size" | "size") => SortField::TotalSize,
            Some("random") => SortField::Random,
            Some("folder_order") => SortField::FolderOrder,
            _ => SortField::ImportedAt,
        };
        let direction = match text("sort_order") {
            Some("ascending") => SortDirection::Ascending,
            Some(_) => SortDirection::Descending,
            None if field == SortField::FolderOrder || (inbox && text("sort_field").is_none()) => {
                SortDirection::Ascending
            }
            None => SortDirection::Descending,
        };
        Self {
            mode: match text("view_mode") {
                Some("grid") => ViewMode::Grid,
                Some("justified") => ViewMode::Justified,
                _ => ViewMode::Waterfall,
            },
            target_size: value("target_size").and_then(Value::as_f64).unwrap_or(220.) as f32,
            gap: if text("spacing") == Some("tight") {
                8.
            } else {
                16.
            },
            show_name: flag("show_name", true),
            show_resolution: flag("show_resolution", false),
            show_extension: flag("show_extension", false),
            show_extension_label: flag("show_label", false),
            show_item_count: flag("show_item_count", true),
            fit_cover: text("thumbnail_fit") == Some("cover"),
            show_subfolders: flag("show_subfolders", true),
            sort: ItemSort {
                field,
                direction,
                random_seed: (field == SortField::Random).then(|| seed.to_owned()),
            },
        }
    }

    /// These preferences as the `view_pref` fields that give them back,
    /// so a patch can be applied on top and resolved again.
    pub fn to_json(&self) -> Value {
        let sort_field = match self.sort.field {
            SortField::ImportedAt => "imported_at",
            SortField::CapturedAt => "captured_at",
            SortField::Name => "name",
            SortField::Rating => "rating",
            SortField::TotalSize => "total_size",
            SortField::Random => "random",
            SortField::FolderOrder => "folder_order",
        };
        serde_json::json!({
            "view_mode": match self.mode {
                ViewMode::Waterfall => "waterfall",
                ViewMode::Grid => "grid",
                ViewMode::Justified => "justified",
            },
            "target_size": self.target_size,
            "spacing": if self.gap == 8. { "tight" } else { "wide" },
            "show_name": self.show_name,
            "show_resolution": self.show_resolution,
            "show_extension": self.show_extension,
            "show_label": self.show_extension_label,
            "show_item_count": self.show_item_count,
            "thumbnail_fit": if self.fit_cover { "cover" } else { "contain" },
            "show_subfolders": self.show_subfolders,
            "sort_field": sort_field,
            "sort_order": match self.sort.direction {
                SortDirection::Ascending => "ascending",
                SortDirection::Descending => "descending",
            },
        })
    }

    /// These preferences with a `view_pref` patch applied, as the grid shows
    /// them at once while the patch is saved.
    pub fn patched(&self, scope: &ItemScope, patch: &Value) -> Self {
        let seed = self.sort.random_seed.clone().unwrap_or_default();
        Self::resolve(scope, &self.to_json(), patch, &seed)
    }

    pub fn text_height(&self) -> f32 {
        let rows = self.show_name as u8 + self.show_resolution as u8;
        rows as f32 * super::layout::TEXT_ROW_HEIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scope_row_overrides_defaults_and_built_ins() {
        let prefs = ViewPrefs::resolve(
            &ItemScope::All,
            &json!({ "target_size": 300, "show_name": false }),
            &json!({ "sort_field": "captured_at", "sort_order": "descending",
                     "spacing": "tight", "target_size": 260, "show_label": true }),
            "7",
        );
        assert_eq!(prefs.target_size, 260.);
        assert_eq!(prefs.gap, 8.);
        assert!(!prefs.show_name);
        assert!(prefs.show_extension_label);
        assert_eq!(prefs.sort.field, SortField::CapturedAt);
        assert_eq!(prefs.sort.direction, SortDirection::Descending);
        assert_eq!(prefs.sort.random_seed, None);
    }

    #[test]
    fn inbox_without_a_saved_sort_is_oldest_first() {
        let prefs = ViewPrefs::resolve(&ItemScope::Inbox, &json!({}), &json!({}), "7");
        assert_eq!(prefs.sort.field, SortField::ImportedAt);
        assert_eq!(prefs.sort.direction, SortDirection::Ascending);
        let all = ViewPrefs::resolve(&ItemScope::All, &json!({}), &json!({}), "7");
        assert_eq!(all.sort.direction, SortDirection::Descending);
        assert_eq!(
            (all.target_size, all.gap, all.mode),
            (220., 16., ViewMode::Waterfall)
        );
    }

    #[test]
    fn saved_view_options_round_trip() {
        let saved = ViewPrefs::resolve(
            &ItemScope::All,
            &json!({ "target_size": 300 }),
            &json!({ "view_mode": "justified", "spacing": "tight", "show_name": false,
                     "show_resolution": true, "show_extension": true, "show_label": true,
                     "show_item_count": false, "thumbnail_fit": "cover",
                     "show_subfolders": false, "sort_field": "name",
                     "sort_order": "ascending" }),
            "7",
        );
        let again = ViewPrefs::resolve(&ItemScope::All, &json!({}), &saved.to_json(), "7");
        assert_eq!(again, saved);
        // A patch changes what it names and nothing else.
        let patched = saved.patched(
            &ItemScope::All,
            &json!({ "view_mode": "grid", "show_name": true, "spacing": "wide" }),
        );
        assert_eq!(patched.mode, ViewMode::Grid);
        assert!(patched.show_name);
        assert_eq!(patched.gap, 16.);
        assert_eq!(
            ViewPrefs {
                mode: saved.mode,
                show_name: saved.show_name,
                gap: saved.gap,
                ..patched
            },
            saved
        );
    }

    #[test]
    fn random_sort_carries_the_session_seed() {
        let prefs = ViewPrefs::resolve(
            &ItemScope::All,
            &json!({}),
            &json!({ "sort_field": "random" }),
            "7",
        );
        assert_eq!(prefs.sort.random_seed.as_deref(), Some("7"));
    }
}
