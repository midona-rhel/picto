//! Shared DTO types for the Picto core library.
//!
//! These types are serialized to JSON across the IPC boundary.
//! Extracted from the former `commands.rs` IPC glue.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use ts_rs::TS;

use crate::smart_folders::types::SmartFolderPredicate;

pub fn parse_file_status(status: &str) -> Result<i64, String> {
    match status {
        "inbox" => Ok(0),
        "active" => Ok(1),
        "trash" => Ok(2),
        _ => Err(format!(
            "Invalid status: {}. Must be inbox, active, or trash.",
            status
        )),
    }
}

pub fn status_to_string(status: i64) -> &'static str {
    match status {
        0 => "inbox",
        1 => "active",
        2 => "trash",
        _ => "unknown",
    }
}

pub fn tag_display_key(namespace: &str, subtag: &str) -> String {
    if namespace.is_empty() {
        subtag.to_string()
    } else {
        format!("{}:{}", namespace, subtag)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DominantColorDto {
    pub hex: String,
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

#[derive(Debug, Serialize)]
pub struct EntityDetails {
    pub entity_id: i64,
    pub hash: String,
    pub name: Option<String>,
    pub size: i64,
    pub mime: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub status: String,
    pub rating: Option<i64>,
    pub view_count: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_urls: Option<JsonValue>,
    #[serde(rename = "date_added")]
    pub imported_at: String,
    pub has_thumbnail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_color_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_colors: Option<Vec<DominantColorDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<JsonValue>,
    #[serde(rename = "date_created", skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(rename = "date_modified", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

/// Slim entity info for grid display — omits heavy fields.
#[derive(Debug, Serialize)]
pub struct EntityGridItem {
    pub entity_id: i64,
    pub hash: String,
    pub name: Option<String>,
    pub size: i64,
    pub mime: String,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub num_frames: Option<i64>,
    pub has_audio: bool,
    pub status: String,
    pub rating: Option<i64>,
    pub view_count: i64,
    #[serde(rename = "date_added")]
    pub imported_at: String,
    pub has_thumbnail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_color_hex: Option<String>,
}

impl From<crate::db::types::EntityGridItem> for EntityGridItem {
    fn from(item: crate::db::types::EntityGridItem) -> Self {
        Self {
            entity_id: item.entity_id,
            hash: item.entity_hash,
            name: item.name,
            size: item.size_bytes,
            mime: item.mime_type,
            width: item.pixel_width,
            height: item.pixel_height,
            duration_ms: item.duration_ms,
            num_frames: item.frame_count,
            has_audio: item.has_audio,
            status: status_to_string(item.status).to_string(),
            rating: item.rating,
            view_count: 0,
            imported_at: item.date_added,
            has_thumbnail: item.has_thumbnail,
            dominant_color_hex: item.dominant_color_hex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "snake_case")]
pub enum GridScopeKind {
    System,
    Folder,
    Smart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
#[serde(rename_all = "snake_case")]
pub enum GridSystemScopeKey {
    All,
    Inbox,
    Trash,
    Untagged,
    Uncategorized,
    RecentViewed,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, Default)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GridScopeSpec {
    pub kind: GridScopeKind,
    pub system_key: Option<GridSystemScopeKey>,
    #[serde(alias = "folderId")]
    #[ts(type = "number | null")]
    pub folder_id: Option<i64>,
    #[serde(alias = "smartFolderPredicate")]
    pub smart_folder_predicate: Option<SmartFolderPredicate>,
}

impl Default for GridScopeKind {
    fn default() -> Self {
        Self::System
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, Default)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GridFilterSpec {
    #[serde(alias = "searchTags")]
    pub search_tags: Option<Vec<String>>,
    #[serde(alias = "searchExcludedTags")]
    pub search_excluded_tags: Option<Vec<String>>,
    #[serde(alias = "tagMatchMode")]
    pub tag_match_mode: Option<String>,
    #[serde(alias = "folderIds")]
    #[ts(type = "number[] | null")]
    pub folder_ids: Option<Vec<i64>>,
    #[serde(alias = "excludedFolderIds")]
    #[ts(type = "number[] | null")]
    pub excluded_folder_ids: Option<Vec<i64>>,
    #[serde(alias = "folderMatchMode")]
    pub folder_match_mode: Option<String>,
    #[serde(alias = "ratingMin")]
    #[ts(type = "number | null")]
    pub rating_min: Option<i64>,
    #[serde(alias = "mimePrefixes")]
    pub mime_prefixes: Option<Vec<String>>,
    #[serde(alias = "colorHex")]
    pub color_hex: Option<String>,
    #[serde(alias = "colorAccuracy")]
    #[ts(type = "number | null")]
    pub color_accuracy: Option<f64>,
    #[serde(alias = "searchText")]
    pub search_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, Default)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct GridSortSpec {
    #[serde(alias = "field")]
    pub field: Option<String>,
    #[serde(alias = "order")]
    pub order: Option<String>,
    #[serde(alias = "randomSeed")]
    #[ts(type = "number | null")]
    pub random_seed: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct TagInfo {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub display: String,
    pub read_only: bool,
}

#[derive(Debug, Serialize)]
pub struct ResolvedTagInfo {
    pub raw_tag: String,
    pub display_tag: String,
    pub namespace: String,
    pub subtag: String,
    pub source: String,
    pub read_only: bool,
}

#[derive(Debug, Serialize)]
pub struct EntityAllMetadata {
    pub entity: EntityDetails,
    pub tags: Vec<ResolvedTagInfo>,
    pub parent_tags: Vec<TagInfo>,
}

pub type EntityInfo = EntityDetails;
pub type EntityGridInfo = EntityGridItem;
pub type FileAllMetadata = EntityAllMetadata;
pub type FileInfo = EntityInfo;
pub type FileGridInfo = EntityGridInfo;

#[derive(Debug, Serialize)]
pub struct SelectionTagCount {
    pub tag: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct SelectionSummaryStats {
    pub total_size_bytes: Option<i64>,
    pub mime_counts: Option<HashMap<String, i64>>,
    pub rating_stats: Option<JsonValue>,
}

#[derive(Debug, Serialize)]
pub struct SelectionFolderInfo {
    pub folder_id: i64,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct SelectionSummary {
    pub total_count: i64,
    pub selected_count: i64,
    pub sample_hashes: Vec<String>,
    pub shared_tags: Vec<SelectionTagCount>,
    pub top_tags: Vec<SelectionTagCount>,
    pub shared_folders: Vec<SelectionFolderInfo>,
    pub stats: SelectionSummaryStats,
    pub pending: bool,
    pub generated_at: String,
}

#[derive(Debug, Serialize)]
pub struct SidebarNodeDto {
    pub id: String,
    pub kind: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub sort_order: Option<i64>,
    pub count: Option<i64>,
    pub freshness: String,
    pub selectable: bool,
    pub expanded_by_default: bool,
    pub meta: Option<JsonValue>,
}

#[derive(Debug, Serialize)]
pub struct SidebarTreeResponse {
    pub nodes: Vec<SidebarNodeDto>,
    pub tree_epoch: u64,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewPrefsDto {
    pub scope_key: String,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub view_mode: Option<String>,
    pub target_size: Option<i64>,
    pub show_name: Option<bool>,
    pub show_resolution: Option<bool>,
    pub show_extension: Option<bool>,
    pub show_label: Option<bool>,
    pub thumbnail_fit: Option<String>,
    pub show_subfolders: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct ViewPrefsPatch {
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub view_mode: Option<String>,
    #[ts(type = "number | null")]
    pub target_size: Option<i64>,
    pub show_name: Option<bool>,
    pub show_resolution: Option<bool>,
    pub show_extension: Option<bool>,
    pub show_label: Option<bool>,
    pub thumbnail_fit: Option<String>,
    pub show_subfolders: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/commands/")]
pub struct FolderReorderMove {
    pub hash: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ScanDuplicatesResponse {
    pub candidates_found: usize,
    pub pairs_inserted: usize,
    pub reviewable_detected_total: usize,
    pub reviewable_detected_new: usize,
    pub total_files: usize,
    pub files_with_phash: usize,
    pub files_scanned: usize,
    pub closest_distance: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct DuplicatePairDto {
    pub hash_a: String,
    pub hash_b: String,
    pub distance: f64,
    pub similarity_pct: f64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct DuplicatePairsResponse {
    pub items: Vec<DuplicatePairDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct SmartMergeResult {
    pub winner_hash: String,
    pub loser_hash: String,
    pub tags_merged: usize,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionInfo {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub paused: bool,
    pub initial_post_limit: u32,
    pub periodic_post_limit: u32,
    pub created_at: String,
    pub total_files: u64,
    pub queries: Vec<SubscriptionQueryInfo>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionQueryInfo {
    pub id: String,
    pub site_id: String,
    pub query_kind: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub notes: Option<String>,
    pub paused: bool,
    pub last_check_time: Option<String>,
    pub files_found: u64,
    pub posts_found: u64,
    pub completed_initial_run: bool,
    pub resume_cursor: Option<String>,
    pub resume_strategy: Option<String>,
    pub last_success_at: Option<String>,
    pub last_failure_at: Option<String>,
    pub last_failure_kind: Option<String>,
    pub last_failure_message: Option<String>,
}

/// Running subscriptions tracker. Key = subscription ID string.
pub type RunningSubscriptions =
    std::sync::Arc<tokio::sync::Mutex<HashMap<String, tokio_util::sync::CancellationToken>>>;
