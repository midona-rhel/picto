//! Application settings — stored as a JSON file on disk.

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
    #[serde(default = "default_properties_panel_width")]
    pub properties_panel_width: f64,
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
    pub ptr_server_url: Option<String>,
    #[serde(default)]
    pub ptr_access_key: Option<String>,
    #[serde(default)]
    pub ptr_enabled: bool,
    #[serde(default)]
    pub ptr_auto_sync: bool,
    #[serde(default = "default_ptr_sync_schedule")]
    pub ptr_sync_schedule: String,
    #[serde(default)]
    pub ptr_last_sync_time: Option<String>,
    /// Custom path for PTR database. If None, defaults to `{library_root}/../ptr/`.
    #[serde(default)]
    pub ptr_data_path: Option<String>,
    #[serde(default)]
    pub zoom_factor: Option<f64>,
    #[serde(default = "default_duplicate_detect_similarity_pct")]
    pub duplicate_detect_similarity_pct: u32,
    #[serde(default = "default_duplicate_review_similarity_pct")]
    pub duplicate_review_similarity_pct: u32,
    #[serde(default = "default_duplicate_auto_merge_similarity_pct")]
    pub duplicate_auto_merge_similarity_pct: u32,
    #[serde(default = "default_true")]
    pub duplicate_auto_merge_subscriptions_only: bool,
    #[serde(default = "default_true")]
    pub duplicate_auto_merge_enabled: bool,

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
}

fn default_grid_target_size() -> f64 {
    250.0
}
fn default_grid_view_mode() -> String {
    "waterfall".into()
}
fn default_properties_panel_width() -> f64 {
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
fn default_ptr_sync_schedule() -> String {
    "weekly".into()
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

/// Convert similarity percentage (0-100) to Hamming distance (0-64).
/// `similarity_pct_to_distance(97)` → 1 (distance = floor((100-97)*64/100))
pub fn similarity_pct_to_distance(pct: u32) -> u32 {
    ((100u32.saturating_sub(pct)) * 64) / 100
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            grid_target_size: default_grid_target_size(),
            grid_view_mode: default_grid_view_mode(),
            properties_panel_width: default_properties_panel_width(),
            color_scheme: default_color_scheme(),
            window_x: None,
            window_y: None,
            window_width: default_window_width(),
            window_height: default_window_height(),
            window_maximized: false,
            grid_sort_field: default_grid_sort_field(),
            grid_sort_order: default_grid_sort_order(),
            ptr_server_url: None,
            ptr_access_key: None,
            ptr_enabled: false,
            ptr_auto_sync: false,
            ptr_sync_schedule: default_ptr_sync_schedule(),
            ptr_last_sync_time: None,
            ptr_data_path: None,
            zoom_factor: None,
            duplicate_detect_similarity_pct: default_duplicate_detect_similarity_pct(),
            duplicate_review_similarity_pct: default_duplicate_review_similarity_pct(),
            duplicate_auto_merge_similarity_pct: default_duplicate_auto_merge_similarity_pct(),
            duplicate_auto_merge_subscriptions_only: true,
            duplicate_auto_merge_enabled: true,
            sub_abort_threshold: default_sub_abort_threshold(),
            sub_inbox_pause_limit: default_sub_inbox_pause_limit(),
            sub_rate_limit_secs: default_sub_rate_limit_secs(),
            sub_batch_size: default_sub_batch_size(),
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
