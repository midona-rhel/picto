use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedMetadata {
    pub tags: Vec<(String, String)>,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub source_urls: Vec<String>,
    pub media_url: Option<String>,
    pub rating: Option<String>,
    pub title: Option<String>,
    pub post_id: Option<String>,
    pub created_at: Option<String>,
    pub category: Option<String>,
    pub page_num: Option<u32>,
    pub page_count: Option<u32>,
    pub canonical_post_url: Option<String>,
    pub item_key: Option<String>,
    #[serde(default)]
    pub raw_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct DownloadedItem {
    pub file_path: PathBuf,
    pub metadata: ParsedMetadata,
}

#[derive(Debug, Clone)]
pub struct FailedDownloadedItem {
    pub metadata: ParsedMetadata,
    pub item_url: Option<String>,
    pub error_message: String,
}
