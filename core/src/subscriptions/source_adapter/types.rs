use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PostMetadata {
    pub tags: Vec<(String, String)>,
    pub description: Option<String>,
    pub source_url: Option<String>,
    pub source_urls: Vec<String>,
    pub rating: Option<String>,
    pub title: Option<String>,
    pub post_id: Option<String>,
    pub created_at: Option<String>,
    pub category: Option<String>,
    pub canonical_post_url: Option<String>,
    #[serde(default)]
    pub raw_metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AssetMetadata {
    pub media_url: Option<String>,
    pub page_num: Option<u32>,
    pub page_count: Option<u32>,
    pub item_key: Option<String>,
}

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

impl ParsedMetadata {
    pub fn post_metadata(&self) -> PostMetadata {
        PostMetadata {
            tags: self.tags.clone(),
            description: self.description.clone(),
            source_url: self.source_url.clone(),
            source_urls: self.source_urls.clone(),
            rating: self.rating.clone(),
            title: self.title.clone(),
            post_id: self.post_id.clone(),
            created_at: self.created_at.clone(),
            category: self.category.clone(),
            canonical_post_url: self.canonical_post_url.clone(),
            raw_metadata: self.raw_metadata.clone(),
        }
    }

    pub fn asset_metadata(&self) -> AssetMetadata {
        AssetMetadata {
            media_url: self.media_url.clone(),
            page_num: self.page_num,
            page_count: self.page_count,
            item_key: self.item_key.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadedItem {
    pub file_path: PathBuf,
    pub metadata: ParsedMetadata,
}

#[derive(Debug, Clone)]
pub struct FailedDownloadedItem {
    pub metadata: ParsedMetadata,
    pub error_message: String,
}

#[derive(Debug, Clone)]
pub enum SourceRunEvent {
    RunStarted,
    PostDiscovered(PostMetadata),
    AssetDiscovered(DownloadedItem),
    AssetSkipped { metadata: ParsedMetadata, reason: String },
    AssetDownloadFailed(FailedDownloadedItem),
    Progress { discovered_items: usize, skipped_archive_items: usize },
    RunWarning { message: String },
    RunCompleted,
    RunFailed { message: String },
}
