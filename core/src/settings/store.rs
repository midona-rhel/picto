//! Application settings — persisted JSON configuration shared across the app.
//!
//! Holds UI preferences, duplicate settings, and subscription/gallery-dl
//! runtime knobs. This module owns defaults and disk persistence semantics
//! so the rest of the app can treat settings as typed data.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_grid_target_size")]
    pub grid_target_size: f64,
    #[serde(default = "default_grid_view_mode")]
    pub grid_view_mode: String,
    #[serde(default = "default_inspector_width")]
    pub inspector_width: f64,
    #[serde(default = "default_color_scheme")]
    pub color_scheme: String,
    #[serde(default)]
    pub window_x: Option<f64>,
    #[serde(default)]
    pub window_y: Option<f64>,
    #[serde(default = "default_window_width")]
    pub window_width: f64,
    #[serde(default = "default_window_height")]
    pub window_height: f64,
    #[serde(default)]
    pub window_maximized: bool,
    #[serde(default = "default_grid_sort_field")]
    pub grid_sort_field: String,
    #[serde(default = "default_grid_sort_order")]
    pub grid_sort_order: String,
    #[serde(default)]
    pub zoom_factor: Option<f64>,
    #[serde(default = "default_duplicate_detect_similarity_pct")]
    pub duplicate_detect_similarity_pct: u32,
    #[serde(default = "default_duplicate_review_similarity_pct")]
    pub duplicate_review_similarity_pct: u32,
    #[serde(default = "default_duplicate_auto_merge_similarity_pct")]
    pub duplicate_auto_merge_similarity_pct: u32,
    #[serde(default)]
    pub duplicate_auto_merge_require_matching_dimensions: bool,
    #[serde(default = "default_true")]
    pub duplicate_auto_merge_subscriptions_only: bool,
    #[serde(default = "default_true")]
    pub duplicate_auto_merge_enabled: bool,

    // Sidebar
    #[serde(default = "default_true")]
    pub show_tree_guides: bool,

    // Subscription / gallery-dl settings
    /// gallery-dl `-A` value: abort after N consecutive skipped files.
    #[serde(default = "default_sub_abort_threshold")]
    pub sub_abort_threshold: u32,
    /// Pause subscription downloads when inbox file count exceeds this. 0 = no limit.
    #[serde(default = "default_sub_inbox_pause_limit")]
    pub sub_inbox_pause_limit: u32,
    /// Seconds between HTTP requests during gallery-dl extraction (`sleep-request`).
    #[serde(default = "default_sub_rate_limit_secs")]
    pub sub_rate_limit_secs: f64,
    /// Max files per gallery-dl invocation (`--range 1-N`).
    #[serde(default = "default_sub_batch_size")]
    pub sub_batch_size: u32,
    /// Default status for watched-folder imports when a folder uses inherited mode.
    #[serde(default = "default_watch_folder_default_status")]
    pub watch_folder_default_status: String,

    // AI Tagger settings — each model can be enabled independently
    #[serde(default)]
    pub ai_tagger_wd14_enabled: bool,
    #[serde(default)]
    pub ai_tagger_e621_enabled: bool,
    #[serde(default)]
    pub ai_tagger_eva02_enabled: bool,
    #[serde(default)]
    pub ai_tagger_auto_on_import: bool,
    /// Whether AI runs may write `rating:` tags (auto-import path).
    #[serde(default = "default_true")]
    pub ai_tagger_write_rating: bool,
    #[serde(default = "default_ai_threshold_general")]
    pub ai_threshold_general: f32,
    #[serde(default = "default_ai_threshold_character")]
    pub ai_threshold_character: f32,
    #[serde(default = "default_ai_threshold_copyright")]
    pub ai_threshold_copyright: f32,
    #[serde(default = "default_ai_threshold_artist")]
    pub ai_threshold_artist: f32,
    #[serde(default = "default_ai_threshold_species")]
    pub ai_threshold_species: f32,
    #[serde(default = "default_ai_threshold_rating")]
    pub ai_threshold_rating: f32,
}

fn default_grid_target_size() -> f64 {
    250.0
}
fn default_grid_view_mode() -> String {
    "waterfall".into()
}
fn default_inspector_width() -> f64 {
    280.0
}
fn default_color_scheme() -> String {
    "dark".into()
}
fn default_window_width() -> f64 {
    1200.0
}
fn default_window_height() -> f64 {
    800.0
}
fn default_grid_sort_field() -> String {
    "imported_at".into()
}
fn default_grid_sort_order() -> String {
    "asc".into()
}
fn default_duplicate_detect_similarity_pct() -> u32 {
    97
}
fn default_duplicate_review_similarity_pct() -> u32 {
    97
}
fn default_duplicate_auto_merge_similarity_pct() -> u32 {
    100
}
fn default_true() -> bool {
    true
}
fn default_sub_abort_threshold() -> u32 {
    10
}
fn default_sub_inbox_pause_limit() -> u32 {
    1000
}
fn default_sub_rate_limit_secs() -> f64 {
    2.0
}
fn default_sub_batch_size() -> u32 {
    100
}
fn default_watch_folder_default_status() -> String {
    "inbox".into()
}
fn default_ai_threshold_general() -> f32 {
    0.35
}
fn default_ai_threshold_character() -> f32 {
    0.85
}
fn default_ai_threshold_copyright() -> f32 {
    0.85
}
fn default_ai_threshold_artist() -> f32 {
    0.85
}
fn default_ai_threshold_species() -> f32 {
    0.35
}
fn default_ai_threshold_rating() -> f32 {
    0.50
}

/// Convert similarity percentage (0-100) to Hamming distance (0-256).
/// At 16x16 hash size (256-bit): `similarity_pct_to_distance(97)` → 7.
pub fn similarity_pct_to_distance(pct: u32) -> u32 {
    ((100u32.saturating_sub(pct)) * 256) / 100
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            grid_target_size: default_grid_target_size(),
            grid_view_mode: default_grid_view_mode(),
            inspector_width: default_inspector_width(),
            color_scheme: default_color_scheme(),
            window_x: None,
            window_y: None,
            window_width: default_window_width(),
            window_height: default_window_height(),
            window_maximized: false,
            grid_sort_field: default_grid_sort_field(),
            grid_sort_order: default_grid_sort_order(),
            zoom_factor: None,
            duplicate_detect_similarity_pct: default_duplicate_detect_similarity_pct(),
            duplicate_review_similarity_pct: default_duplicate_review_similarity_pct(),
            duplicate_auto_merge_similarity_pct: default_duplicate_auto_merge_similarity_pct(),
            duplicate_auto_merge_require_matching_dimensions: false,
            duplicate_auto_merge_subscriptions_only: true,
            duplicate_auto_merge_enabled: true,
            show_tree_guides: true,
            sub_abort_threshold: default_sub_abort_threshold(),
            sub_inbox_pause_limit: default_sub_inbox_pause_limit(),
            sub_rate_limit_secs: default_sub_rate_limit_secs(),
            sub_batch_size: default_sub_batch_size(),
            watch_folder_default_status: default_watch_folder_default_status(),
            ai_tagger_wd14_enabled: false,
            ai_tagger_e621_enabled: false,
            ai_tagger_eva02_enabled: false,
            ai_tagger_auto_on_import: false,
            ai_tagger_write_rating: true,
            ai_threshold_general: default_ai_threshold_general(),
            ai_threshold_character: default_ai_threshold_character(),
            ai_threshold_copyright: default_ai_threshold_copyright(),
            ai_threshold_artist: default_ai_threshold_artist(),
            ai_threshold_species: default_ai_threshold_species(),
            ai_threshold_rating: default_ai_threshold_rating(),
        }
    }
}

/// Thread-safe settings store backed by a JSON file.
pub struct SettingsStore {
    path: PathBuf,
    settings: Mutex<AppSettings>,
}

impl SettingsStore {
    /// Load settings from disk, or create defaults if the file doesn't exist.
    pub fn load(dir: &std::path::Path) -> Self {
        let path = dir.join("settings.json");
        let settings = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(_) => AppSettings::default(),
            }
        } else {
            AppSettings::default()
        };
        Self {
            path,
            settings: Mutex::new(settings),
        }
    }

    pub fn get(&self) -> AppSettings {
        crate::poison::mutex_or_recover(&self.settings, "settings::get").clone()
    }

    pub fn update(&self, new_settings: AppSettings) {
        let mut guard = crate::poison::mutex_or_recover(&self.settings, "settings::update");
        *guard = new_settings;
        if let Ok(json) = serde_json::to_string_pretty(&*guard) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}
