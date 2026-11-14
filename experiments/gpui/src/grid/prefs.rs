//! View preferences for a scope, read from the library's `view_pref` rows with
//! the same fallbacks as `gridController.ts`: the scope's own row, then
//! `grid:defaults`, then the built-in defaults.
use super::layout::ViewMode;
use crate::i18n::t;
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
    pub show_extension_label: bool,
    /// Names with their extension (`show_extension`).
    pub show_extension: bool,
    /// A folder shows its subfolders' items too (`show_subfolders`).
    pub show_subfolders: bool,
    pub show_item_count: bool,
    pub fit_cover: bool,
    pub sort: ItemSort,
}

/// `LAYOUT_OPTIONS`: each layout, its `view_mode` value and its label.
// i18n: translated where drawn (select choices)
pub const LAYOUTS: [(ViewMode, &str, &str); 3] = [
    (ViewMode::Waterfall, "waterfall", "Waterfall"),
    (ViewMode::Grid, "grid", "Grid"),
    (ViewMode::Justified, "justified", "Justified"),
];

/// `SORT_OPTIONS` of the view panel, in its order: each sort, its
/// `sort_field` value and its label.
// i18n: translated where drawn (select choices)
pub const SORTS: [(SortField, &str, &str); 6] = [
    (SortField::ImportedAt, "imported_at", "Date Added"),
    (SortField::CapturedAt, "captured_at", "Date Created"),
    (SortField::Name, "name", "Name"),
    (SortField::TotalSize, "total_size", "File Size"),
    (SortField::Rating, "rating", "Rating"),
    (SortField::Random, "random", "Random"),
];

/// A layout's `view_mode` value.
pub fn mode_value(mode: ViewMode) -> &'static str {
    LAYOUTS
        .iter()
        .find(|(each, _, _)| *each == mode)
        .map_or("waterfall", |(_, value, _)| value)
}

/// A sort's `sort_field` value. A folder's own order is not offered.
pub fn sort_value(field: SortField) -> &'static str {
    SORTS
        .iter()
        .find(|(each, _, _)| *each == field)
        .map_or("folder_order", |(_, value, _)| value)
}

/// A sort's label in the view panel.
pub fn sort_label(field: SortField) -> &'static str {
    SORTS
        .iter()
        .find(|(each, _, _)| *each == field)
        .map_or(crate::i18n::en("Folder Order"), |(_, _, label)| label)
}

/// A direction's `sort_order` value.
pub fn direction_value(direction: SortDirection) -> &'static str {
    match direction {
        SortDirection::Ascending => "ascending",
        SortDirection::Descending => "descending",
    }
}

/// `hasFixedOrder`: a folder or Recently Viewed keeps its own order, so its
/// view offers no sort.
pub fn fixed_order(scope: &ItemScope) -> bool {
    matches!(
        scope,
        ItemScope::Folder { .. } | ItemScope::FolderTree { .. } | ItemScope::RecentlyViewed
    )
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
            // Older builds saved the File Size choice as 'size'.
            Some("size") => SortField::TotalSize,
            Some("folder_order") => SortField::FolderOrder,
            Some(value) => SORTS
                .iter()
                .find(|(_, each, _)| *each == value)
                .map_or(SortField::ImportedAt, |(field, _, _)| *field),
            None => SortField::ImportedAt,
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
            mode: LAYOUTS
                .iter()
                .find(|(_, value, _)| Some(*value) == text("view_mode"))
                .map_or(ViewMode::Waterfall, |(mode, _, _)| *mode),
            target_size: value("target_size").and_then(Value::as_f64).unwrap_or(220.) as f32,
            gap: if text("spacing") == Some("tight") {
                8.
            } else {
                16.
            },
            show_name: flag("show_name", true),
            show_resolution: flag("show_resolution", false),
            show_extension_label: flag("show_label", false),
            show_extension: flag("show_extension", false),
            show_subfolders: flag("show_subfolders", true),
            show_item_count: flag("show_item_count", true),
            fit_cover: text("thumbnail_fit") == Some("cover"),
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
        serde_json::json!({
            "view_mode": mode_value(self.mode),
            "target_size": self.target_size,
            "spacing": if self.gap == 8. { "tight" } else { "wide" },
            "show_name": self.show_name,
            "show_resolution": self.show_resolution,
            "show_extension": self.show_extension,
            "show_label": self.show_extension_label,
            "show_item_count": self.show_item_count,
            "thumbnail_fit": if self.fit_cover { "cover" } else { "contain" },
            "show_subfolders": self.show_subfolders,
            "sort_field": sort_value(self.sort.field),
            "sort_order": direction_value(self.sort.direction),
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
    fn every_value_reads_back_as_what_wrote_it() {
        for (mode, value, _) in LAYOUTS {
            assert_eq!(mode_value(mode), value);
            let prefs = ViewPrefs::resolve(
                &ItemScope::All,
                &json!({}),
                &json!({ "view_mode": value }),
                "7",
            );
            assert_eq!(prefs.mode, mode);
        }
        for field in SORTS
            .map(|(field, _, _)| field)
            .into_iter()
            .chain([SortField::FolderOrder])
        {
            let prefs = ViewPrefs::resolve(
                &ItemScope::All,
                &json!({}),
                &json!({ "sort_field": sort_value(field) }),
                "7",
            );
            assert_eq!(prefs.sort.field, field);
        }
        assert_eq!(sort_label(SortField::TotalSize), "File Size");
        assert_eq!(direction_value(SortDirection::Ascending), "ascending");
        assert!(fixed_order(&ItemScope::RecentlyViewed));
        assert!(!fixed_order(&ItemScope::Inbox));
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
