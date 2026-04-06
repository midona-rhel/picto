use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ViewPref {
    pub scope: String,
    pub sort_field: Option<String>,
    pub sort_dir: Option<String>,
    pub layout: Option<String>,
    pub tile_size: Option<i64>,
    pub show_name: Option<bool>,
    pub show_resolution: Option<bool>,
    pub show_extension: Option<bool>,
    pub show_label: Option<bool>,
    pub thumbnail_fit: Option<String>,
}
