//! The application settings and the grid's default view, as
//! `settingsApi.ts` reads and writes them: the library's `setting` row
//! `application` and its `view_pref` row `grid:defaults`. A stored value that
//! is missing or of the wrong kind falls back to its default, field by field.
use serde_json::{Map, Value, json};

/// `APP_SETTINGS_DEFAULTS` and the fields of `AppSettings`. Choices are kept
/// as the strings production stores.
#[derive(Clone, Debug, PartialEq)]
pub struct AppSettings {
    pub grid_spacing: &'static str,
    pub color_scheme: String,
    pub zoom_factor: Option<f64>,
    pub show_tree_guides: bool,
    pub show_sidebar_counts: bool,
    pub show_sidebar_inbox: bool,
    pub show_sidebar_recently_viewed: bool,
    pub show_sidebar_uncategorized: bool,
    pub show_sidebar_untagged: bool,
    pub show_sidebar_tag_manager: bool,
    pub show_sidebar_random: bool,
    pub show_sidebar_subscriptions: bool,
    pub show_sidebar_duplicates: bool,
    pub show_sidebar_quick_access: bool,
    pub show_sidebar_folders: bool,
    pub show_sidebar_smart_folders: bool,
    pub sidebar_double_click_action: &'static str,
    pub grid_wheel_action: &'static str,
    pub viewer_trackpad_gestures: bool,
    pub grid_double_click_action: &'static str,
    pub grid_middle_click_action: &'static str,
    pub space_key_action: &'static str,
    pub image_rendering: &'static str,
    pub image_default_zoom: &'static str,
    pub show_transparency_grid: bool,
    pub video_auto_play: bool,
    pub video_loop: bool,
    pub notification_popups_enabled: bool,
    pub notification_popup_tones: Vec<&'static str>,
    pub auto_import_enabled: bool,
    pub multi_file_import_behavior: &'static str,
    pub reject_action: &'static str,
    pub undo_delete_limit: i64,
    pub subscription_default_schedule: &'static str,
    pub subscription_default_posts_per_run: i64,
    pub subscription_default_group_posts: bool,
    pub subscription_inbox_item_limit: i64,
    pub show_tag_groups: bool,
    pub show_tag_prefixes: bool,
    pub starred_tags: Vec<String>,
    pub tag_group_colors: Map<String, Value>,
    pub sidebar_quick_access: Vec<String>,
    pub ai_tagger_wd14_enabled: bool,
    pub ai_tagger_e621_enabled: bool,
    pub ai_tagger_eva02_enabled: bool,
    pub ai_tagger_oppai_oracle_enabled: bool,
    pub ai_tagger_manual_model_slugs: Option<Vec<String>>,
    pub ai_tagger_auto_on_import: bool,
    pub ai_tagger_write_rating: bool,
    pub ai_threshold_general: f64,
    pub ai_threshold_character: f64,
    pub ai_threshold_copyright: f64,
    pub ai_threshold_artist: f64,
    pub ai_threshold_species: f64,
    pub ai_threshold_rating: f64,
}

pub const NOTIFICATION_TONES: [&str; 4] = ["error", "warning", "info", "success"];

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            grid_spacing: "wide",
            color_scheme: "dark".into(),
            zoom_factor: None,
            show_tree_guides: true,
            show_sidebar_counts: true,
            show_sidebar_inbox: true,
            show_sidebar_recently_viewed: true,
            show_sidebar_uncategorized: true,
            show_sidebar_untagged: true,
            show_sidebar_tag_manager: true,
            show_sidebar_random: true,
            show_sidebar_subscriptions: true,
            show_sidebar_duplicates: true,
            show_sidebar_quick_access: true,
            show_sidebar_folders: true,
            show_sidebar_smart_folders: true,
            sidebar_double_click_action: "collapse",
            grid_wheel_action: "scroll",
            viewer_trackpad_gestures: false,
            grid_double_click_action: "detail",
            grid_middle_click_action: "new_window",
            space_key_action: "quick_look",
            image_rendering: "smooth",
            image_default_zoom: "fit",
            show_transparency_grid: false,
            video_auto_play: true,
            video_loop: true,
            notification_popups_enabled: true,
            notification_popup_tones: NOTIFICATION_TONES.to_vec(),
            auto_import_enabled: true,
            multi_file_import_behavior: "ask",
            reject_action: "trash",
            undo_delete_limit: 500,
            subscription_default_schedule: "daily",
            subscription_default_posts_per_run: 100,
            subscription_default_group_posts: true,
            subscription_inbox_item_limit: 1000,
            show_tag_groups: true,
            show_tag_prefixes: false,
            starred_tags: Vec::new(),
            tag_group_colors: Map::new(),
            sidebar_quick_access: Vec::new(),
            ai_tagger_wd14_enabled: false,
            ai_tagger_e621_enabled: false,
            ai_tagger_eva02_enabled: false,
            ai_tagger_oppai_oracle_enabled: false,
            ai_tagger_manual_model_slugs: None,
            ai_tagger_auto_on_import: false,
            ai_tagger_write_rating: true,
            ai_threshold_general: 0.35,
            ai_threshold_character: 0.85,
            ai_threshold_copyright: 0.85,
            ai_threshold_artist: 0.85,
            ai_threshold_species: 0.35,
            ai_threshold_rating: 0.5,
        }
    }
}

/// Reads one stored field, keeping the default when it is absent or invalid.
struct Reader<'a>(&'a Map<String, Value>);

impl Reader<'_> {
    fn flag(&self, key: &str, fallback: bool) -> bool {
        self.0.get(key).and_then(Value::as_bool).unwrap_or(fallback)
    }
    fn number(&self, key: &str, fallback: f64) -> f64 {
        self.0
            .get(key)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .unwrap_or(fallback)
    }
    fn integer(&self, key: &str, fallback: i64, min: i64, max: i64) -> i64 {
        self.0
            .get(key)
            .and_then(Value::as_i64)
            .filter(|value| (min..=max).contains(value))
            .unwrap_or(fallback)
    }
    fn choice(&self, key: &str, allowed: &[&'static str], fallback: &'static str) -> &'static str {
        self.0
            .get(key)
            .and_then(Value::as_str)
            .and_then(|stored| allowed.iter().find(|choice| **choice == stored))
            .copied()
            .unwrap_or(fallback)
    }
    fn strings(&self, key: &str) -> Option<Vec<String>> {
        let list = self.0.get(key)?.as_array()?;
        let mut kept: Vec<String> = Vec::new();
        for item in list {
            let item = item.as_str()?.trim();
            if !item.is_empty() && !kept.iter().any(|seen| seen == item) {
                kept.push(item.to_owned());
            }
        }
        Some(kept)
    }
}

impl AppSettings {
    /// The stored settings over the defaults (`parseAppSettings`).
    pub fn from_json(stored: &Value) -> Self {
        let empty = Map::new();
        let read = Reader(stored.as_object().unwrap_or(&empty));
        let d = Self::default();
        Self {
            grid_spacing: read.choice("gridSpacing", &["wide", "tight"], d.grid_spacing),
            color_scheme: read
                .0
                .get("colorScheme")
                .and_then(Value::as_str)
                .map_or(d.color_scheme, str::to_owned),
            zoom_factor: match read.0.get("zoomFactor") {
                Some(value) => value.as_f64().filter(|value| value.is_finite()),
                None => d.zoom_factor,
            },
            show_tree_guides: read.flag("showTreeGuides", d.show_tree_guides),
            show_sidebar_counts: read.flag("showSidebarCounts", d.show_sidebar_counts),
            show_sidebar_inbox: read.flag("showSidebarInbox", d.show_sidebar_inbox),
            show_sidebar_recently_viewed: read
                .flag("showSidebarRecentlyViewed", d.show_sidebar_recently_viewed),
            show_sidebar_uncategorized: read
                .flag("showSidebarUncategorized", d.show_sidebar_uncategorized),
            show_sidebar_untagged: read.flag("showSidebarUntagged", d.show_sidebar_untagged),
            show_sidebar_tag_manager: read
                .flag("showSidebarTagManager", d.show_sidebar_tag_manager),
            show_sidebar_random: read.flag("showSidebarRandom", d.show_sidebar_random),
            show_sidebar_subscriptions: read
                .flag("showSidebarSubscriptions", d.show_sidebar_subscriptions),
            show_sidebar_duplicates: read.flag("showSidebarDuplicates", d.show_sidebar_duplicates),
            show_sidebar_quick_access: read
                .flag("showSidebarQuickAccess", d.show_sidebar_quick_access),
            show_sidebar_folders: read.flag("showSidebarFolders", d.show_sidebar_folders),
            show_sidebar_smart_folders: read
                .flag("showSidebarSmartFolders", d.show_sidebar_smart_folders),
            sidebar_double_click_action: read.choice(
                "sidebarDoubleClickAction",
                &["rename", "collapse"],
                d.sidebar_double_click_action,
            ),
            grid_wheel_action: read.choice(
                "gridWheelAction",
                &["scroll", "zoom"],
                d.grid_wheel_action,
            ),
            viewer_trackpad_gestures: read
                .flag("viewerTrackpadGestures", d.viewer_trackpad_gestures),
            grid_double_click_action: read.choice(
                "gridDoubleClickAction",
                &["detail", "external"],
                d.grid_double_click_action,
            ),
            grid_middle_click_action: read.choice(
                "gridMiddleClickAction",
                &["new_window", "none"],
                d.grid_middle_click_action,
            ),
            space_key_action: read.choice(
                "spaceKeyAction",
                &["quick_look", "scroll"],
                d.space_key_action,
            ),
            image_rendering: read.choice(
                "imageRendering",
                &["smooth", "pixelated"],
                d.image_rendering,
            ),
            image_default_zoom: read.choice(
                "imageDefaultZoom",
                &["fit", "actual"],
                d.image_default_zoom,
            ),
            show_transparency_grid: read.flag("showTransparencyGrid", d.show_transparency_grid),
            video_auto_play: read.flag("videoAutoPlay", d.video_auto_play),
            video_loop: read.flag("videoLoop", d.video_loop),
            notification_popups_enabled: read
                .flag("notificationPopupsEnabled", d.notification_popups_enabled),
            notification_popup_tones: read
                .0
                .get("notificationPopupTones")
                .and_then(Value::as_array)
                .and_then(|tones| {
                    let mut kept: Vec<&'static str> = Vec::new();
                    for tone in tones {
                        let tone = NOTIFICATION_TONES
                            .iter()
                            .find(|known| Some(**known) == tone.as_str())?;
                        if !kept.contains(tone) {
                            kept.push(tone);
                        }
                    }
                    Some(kept)
                })
                .unwrap_or(d.notification_popup_tones),
            auto_import_enabled: read.flag("autoImportEnabled", d.auto_import_enabled),
            multi_file_import_behavior: read.choice(
                "multiFileImportBehavior",
                &["ask", "group", "separate"],
                d.multi_file_import_behavior,
            ),
            reject_action: read.choice("rejectAction", &["trash", "delete"], d.reject_action),
            undo_delete_limit: read.integer("undoDeleteLimit", d.undo_delete_limit, 0, 100_000),
            subscription_default_schedule: read.choice(
                "subscriptionDefaultSchedule",
                &["manual", "daily", "weekly", "monthly"],
                d.subscription_default_schedule,
            ),
            subscription_default_posts_per_run: read.integer(
                "subscriptionDefaultPostsPerRun",
                d.subscription_default_posts_per_run,
                1,
                10_000,
            ),
            subscription_default_group_posts: read.flag(
                "subscriptionDefaultGroupPosts",
                d.subscription_default_group_posts,
            ),
            subscription_inbox_item_limit: read.integer(
                "subscriptionInboxItemLimit",
                d.subscription_inbox_item_limit,
                1,
                1_000_000,
            ),
            show_tag_groups: read.flag("showTagGroups", d.show_tag_groups),
            show_tag_prefixes: read.flag("showTagPrefixes", d.show_tag_prefixes),
            starred_tags: read.strings("starredTags").unwrap_or(d.starred_tags),
            tag_group_colors: read
                .0
                .get("tagGroupColors")
                .and_then(Value::as_object)
                .map(|colors| {
                    colors
                        .iter()
                        .filter(|(name, color)| {
                            !name.trim().is_empty() && color.as_str().is_some_and(|c| !c.is_empty())
                        })
                        .map(|(name, color)| (name.clone(), color.clone()))
                        .collect()
                })
                .unwrap_or(d.tag_group_colors),
            sidebar_quick_access: read
                .strings("sidebarQuickAccess")
                .unwrap_or(d.sidebar_quick_access),
            ai_tagger_wd14_enabled: read.flag("aiTaggerWd14Enabled", d.ai_tagger_wd14_enabled),
            ai_tagger_e621_enabled: read.flag("aiTaggerE621Enabled", d.ai_tagger_e621_enabled),
            ai_tagger_eva02_enabled: read.flag("aiTaggerEva02Enabled", d.ai_tagger_eva02_enabled),
            ai_tagger_oppai_oracle_enabled: read.flag(
                "aiTaggerOppaiOracleEnabled",
                d.ai_tagger_oppai_oracle_enabled,
            ),
            ai_tagger_manual_model_slugs: match read.0.get("aiTaggerManualModelSlugs") {
                Some(Value::Null) | None => None,
                Some(_) => read.strings("aiTaggerManualModelSlugs"),
            },
            ai_tagger_auto_on_import: read.flag("aiTaggerAutoOnImport", d.ai_tagger_auto_on_import),
            ai_tagger_write_rating: read.flag("aiTaggerWriteRating", d.ai_tagger_write_rating),
            ai_threshold_general: read.number("aiThresholdGeneral", d.ai_threshold_general),
            ai_threshold_character: read.number("aiThresholdCharacter", d.ai_threshold_character),
            ai_threshold_copyright: read.number("aiThresholdCopyright", d.ai_threshold_copyright),
            ai_threshold_artist: read.number("aiThresholdArtist", d.ai_threshold_artist),
            ai_threshold_species: read.number("aiThresholdSpecies", d.ai_threshold_species),
            ai_threshold_rating: read.number("aiThresholdRating", d.ai_threshold_rating),
        }
    }

    /// Every field, as `settings.replace` stores it.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        let mut put = |key: &str, value: Value| {
            map.insert(key.to_owned(), value);
        };
        put("gridSpacing", json!(self.grid_spacing));
        put("colorScheme", json!(self.color_scheme));
        put("zoomFactor", json!(self.zoom_factor));
        put("showTreeGuides", json!(self.show_tree_guides));
        put("showSidebarCounts", json!(self.show_sidebar_counts));
        put("showSidebarInbox", json!(self.show_sidebar_inbox));
        put(
            "showSidebarRecentlyViewed",
            json!(self.show_sidebar_recently_viewed),
        );
        put(
            "showSidebarUncategorized",
            json!(self.show_sidebar_uncategorized),
        );
        put("showSidebarUntagged", json!(self.show_sidebar_untagged));
        put(
            "showSidebarTagManager",
            json!(self.show_sidebar_tag_manager),
        );
        put("showSidebarRandom", json!(self.show_sidebar_random));
        put(
            "showSidebarSubscriptions",
            json!(self.show_sidebar_subscriptions),
        );
        put("showSidebarDuplicates", json!(self.show_sidebar_duplicates));
        put(
            "showSidebarQuickAccess",
            json!(self.show_sidebar_quick_access),
        );
        put("showSidebarFolders", json!(self.show_sidebar_folders));
        put(
            "showSidebarSmartFolders",
            json!(self.show_sidebar_smart_folders),
        );
        put(
            "sidebarDoubleClickAction",
            json!(self.sidebar_double_click_action),
        );
        put("gridWheelAction", json!(self.grid_wheel_action));
        put(
            "viewerTrackpadGestures",
            json!(self.viewer_trackpad_gestures),
        );
        put(
            "gridDoubleClickAction",
            json!(self.grid_double_click_action),
        );
        put(
            "gridMiddleClickAction",
            json!(self.grid_middle_click_action),
        );
        put("spaceKeyAction", json!(self.space_key_action));
        put("imageRendering", json!(self.image_rendering));
        put("imageDefaultZoom", json!(self.image_default_zoom));
        put("showTransparencyGrid", json!(self.show_transparency_grid));
        put("videoAutoPlay", json!(self.video_auto_play));
        put("videoLoop", json!(self.video_loop));
        put(
            "notificationPopupsEnabled",
            json!(self.notification_popups_enabled),
        );
        put(
            "notificationPopupTones",
            json!(self.notification_popup_tones),
        );
        put("autoImportEnabled", json!(self.auto_import_enabled));
        put(
            "multiFileImportBehavior",
            json!(self.multi_file_import_behavior),
        );
        put("rejectAction", json!(self.reject_action));
        put("undoDeleteLimit", json!(self.undo_delete_limit));
        put(
            "subscriptionDefaultSchedule",
            json!(self.subscription_default_schedule),
        );
        put(
            "subscriptionDefaultPostsPerRun",
            json!(self.subscription_default_posts_per_run),
        );
        put(
            "subscriptionDefaultGroupPosts",
            json!(self.subscription_default_group_posts),
        );
        put(
            "subscriptionInboxItemLimit",
            json!(self.subscription_inbox_item_limit),
        );
        put("showTagGroups", json!(self.show_tag_groups));
        put("showTagPrefixes", json!(self.show_tag_prefixes));
        put("starredTags", json!(self.starred_tags));
        put("tagGroupColors", json!(self.tag_group_colors));
        put("sidebarQuickAccess", json!(self.sidebar_quick_access));
        put("aiTaggerWd14Enabled", json!(self.ai_tagger_wd14_enabled));
        put("aiTaggerE621Enabled", json!(self.ai_tagger_e621_enabled));
        put("aiTaggerEva02Enabled", json!(self.ai_tagger_eva02_enabled));
        put(
            "aiTaggerOppaiOracleEnabled",
            json!(self.ai_tagger_oppai_oracle_enabled),
        );
        put(
            "aiTaggerManualModelSlugs",
            json!(self.ai_tagger_manual_model_slugs),
        );
        put("aiTaggerAutoOnImport", json!(self.ai_tagger_auto_on_import));
        put("aiTaggerWriteRating", json!(self.ai_tagger_write_rating));
        put("aiThresholdGeneral", json!(self.ai_threshold_general));
        put("aiThresholdCharacter", json!(self.ai_threshold_character));
        put("aiThresholdCopyright", json!(self.ai_threshold_copyright));
        put("aiThresholdArtist", json!(self.ai_threshold_artist));
        put("aiThresholdSpecies", json!(self.ai_threshold_species));
        put("aiThresholdRating", json!(self.ai_threshold_rating));
        Value::Object(map)
    }

    /// The setting that shows or hides a sidebar system row or section,
    /// by the row's node id (`system:inbox`) or the section's key
    /// (`section:folders`), as `visibleSystemNodes` and the section flags in
    /// `AppShell.tsx` map them. All and Trash have none: they always show.
    pub fn sidebar_visibility(&mut self, key: &str) -> Option<&mut bool> {
        Some(match key {
            "system:inbox" => &mut self.show_sidebar_inbox,
            "system:recent_viewed" => &mut self.show_sidebar_recently_viewed,
            "system:uncategorized" => &mut self.show_sidebar_uncategorized,
            "system:untagged" => &mut self.show_sidebar_untagged,
            "system:tag_manager" => &mut self.show_sidebar_tag_manager,
            "system:random" => &mut self.show_sidebar_random,
            "system:subscriptions" => &mut self.show_sidebar_subscriptions,
            "system:duplicates" => &mut self.show_sidebar_duplicates,
            "section:quick_access" => &mut self.show_sidebar_quick_access,
            "section:folders" => &mut self.show_sidebar_folders,
            "section:smart_folders" => &mut self.show_sidebar_smart_folders,
            _ => return None,
        })
    }

    /// Whether the sidebar shows this system row or section.
    pub fn sidebar_shows(&self, key: &str) -> bool {
        self.clone().sidebar_visibility(key).is_none_or(|on| *on)
    }

    /// The threshold for one confidence row, by its settings key.
    pub fn threshold(&self, key: Threshold) -> f64 {
        match key {
            Threshold::General => self.ai_threshold_general,
            Threshold::Character => self.ai_threshold_character,
            Threshold::Species => self.ai_threshold_species,
            Threshold::Copyright => self.ai_threshold_copyright,
            Threshold::Artist => self.ai_threshold_artist,
            Threshold::Rating => self.ai_threshold_rating,
        }
    }

    pub fn set_threshold(&mut self, key: Threshold, value: f64) {
        *match key {
            Threshold::General => &mut self.ai_threshold_general,
            Threshold::Character => &mut self.ai_threshold_character,
            Threshold::Species => &mut self.ai_threshold_species,
            Threshold::Copyright => &mut self.ai_threshold_copyright,
            Threshold::Artist => &mut self.ai_threshold_artist,
            Threshold::Rating => &mut self.ai_threshold_rating,
        } = value;
    }

    /// Whether the model with this slug runs on new imports.
    pub fn runs_model(&self, slug: &str) -> bool {
        match slug {
            "wd14-swinv2-v3" => self.ai_tagger_wd14_enabled,
            "z3d-e621-convnext" => self.ai_tagger_e621_enabled,
            "wd14-eva02-large-v3" => self.ai_tagger_eva02_enabled,
            "oppai-oracle-v1-1" => self.ai_tagger_oppai_oracle_enabled,
            _ => false,
        }
    }

    pub fn toggle_notification_tone(&mut self, tone: &'static str) {
        if let Some(at) = self
            .notification_popup_tones
            .iter()
            .position(|t| *t == tone)
        {
            self.notification_popup_tones.remove(at);
        } else {
            self.notification_popup_tones.push(tone);
        }
    }
}

/// The confidence rows of the AI Models panel, in their order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Threshold {
    General,
    Character,
    Species,
    Copyright,
    Artist,
    Rating,
}

/// The `grid:defaults` view preferences (`ViewPrefsDto`). An unset field
/// inherits the built-in default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GridDefaults {
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub view_mode: Option<String>,
    pub target_size: Option<f64>,
    pub spacing: Option<String>,
    pub show_name: Option<bool>,
    pub show_resolution: Option<bool>,
    pub show_extension: Option<bool>,
    pub show_label: Option<bool>,
    pub show_item_count: Option<bool>,
    pub thumbnail_fit: Option<String>,
    pub show_subfolders: Option<bool>,
}

impl GridDefaults {
    pub fn from_json(stored: &Value) -> Self {
        let text = |key: &str| stored.get(key).and_then(Value::as_str).map(str::to_owned);
        let flag = |key: &str| stored.get(key).and_then(Value::as_bool);
        Self {
            sort_field: text("sort_field"),
            sort_order: text("sort_order"),
            view_mode: text("view_mode"),
            target_size: stored.get("target_size").and_then(Value::as_f64),
            spacing: text("spacing"),
            show_name: flag("show_name"),
            show_resolution: flag("show_resolution"),
            show_extension: flag("show_extension"),
            show_label: flag("show_label"),
            show_item_count: flag("show_item_count"),
            thumbnail_fit: text("thumbnail_fit"),
            show_subfolders: flag("show_subfolders"),
        }
    }

    /// `viewPrefsToPatch`: every field, with unset ones as `null`, which
    /// removes them from the stored row.
    pub fn to_patch(&self) -> Map<String, Value> {
        let value = json!({
            "sort_field": self.sort_field,
            "sort_order": self.sort_order,
            "view_mode": self.view_mode,
            "target_size": self.target_size,
            "spacing": self.spacing,
            "show_name": self.show_name,
            "show_resolution": self.show_resolution,
            "show_extension": self.show_extension,
            "show_label": self.show_label,
            "show_item_count": self.show_item_count,
            "thumbnail_fit": self.thumbnail_fit,
            "show_subfolders": self.show_subfolders,
        });
        match value {
            Value::Object(map) => map,
            _ => Map::new(),
        }
    }
}

/// Applies a view-preference patch to a stored row as `settings.view.patch`
/// does: `null` removes a field, anything else replaces it.
pub fn patched(stored: Option<&str>, patch: &Map<String, Value>) -> Value {
    let mut value = stored
        .and_then(|encoded| serde_json::from_str::<Value>(encoded).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let target = value.as_object_mut().expect("an object");
    for (name, replacement) in patch {
        if replacement.is_null() {
            target.remove(name);
        } else {
            target.insert(name.clone(), replacement.clone());
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_invalid_fields_keep_their_defaults() {
        let settings = AppSettings::from_json(&json!({
            "gridSpacing": "tight",
            "showTagPrefixes": true,
            "rejectAction": "shred",
            "undoDeleteLimit": 100_001,
            "notificationPopupTones": ["error", "error", "info"],
            "starredTags": [" a ", "a", ""],
            "zoomFactor": null,
        }));
        assert_eq!(settings.grid_spacing, "tight");
        assert!(settings.show_tag_prefixes);
        assert_eq!(settings.reject_action, "trash");
        assert_eq!(settings.undo_delete_limit, 500);
        assert_eq!(settings.notification_popup_tones, vec!["error", "info"]);
        assert_eq!(settings.starred_tags, vec!["a".to_owned()]);
        assert_eq!(settings.zoom_factor, None);
        assert_eq!(AppSettings::from_json(&Value::Null), AppSettings::default());
    }

    #[test]
    fn stored_settings_read_back_unchanged() {
        let mut settings = AppSettings::default();
        settings.show_sidebar_counts = false;
        settings.subscription_inbox_item_limit = 42;
        settings.set_threshold(Threshold::Artist, 0.6);
        settings.toggle_notification_tone("info");
        assert_eq!(AppSettings::from_json(&settings.to_json()), settings);
    }

    #[test]
    fn system_rows_follow_their_settings() {
        let mut settings = AppSettings::default();
        settings.show_sidebar_tag_manager = false;
        settings.show_sidebar_inbox = false;
        assert!(!settings.sidebar_shows("system:tag_manager"));
        assert!(!settings.sidebar_shows("system:inbox"));
        assert!(settings.sidebar_shows("system:active"));
        assert!(settings.sidebar_shows("system:trash"));
        assert!(settings.sidebar_shows("section:folders"));
    }

    #[test]
    fn a_view_patch_removes_unset_fields() {
        let defaults = GridDefaults {
            view_mode: Some("grid".into()),
            target_size: Some(300.),
            ..GridDefaults::default()
        };
        let row = patched(
            Some(r#"{"show_name":false,"view_mode":"justified","other":1}"#),
            &defaults.to_patch(),
        );
        assert_eq!(
            row,
            json!({ "view_mode": "grid", "target_size": 300.0, "other": 1 })
        );
        assert_eq!(GridDefaults::from_json(&row), defaults);
    }
}
