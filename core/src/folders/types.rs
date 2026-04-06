use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Folder {
    pub folder_id: i64,
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub auto_tags: Vec<String>,
    pub watch_path: Option<String>,
    pub watch_enabled: bool,
    pub watch_subfolders: bool,
    pub watch_import_status_mode: String,
    pub sort_order: Option<i64>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewFolder {
    pub name: String,
    pub parent_id: Option<i64>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub auto_tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderMembership {
    pub folder_id: i64,
    pub folder_name: String,
}
