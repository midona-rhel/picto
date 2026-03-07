//! Shared DTO types for the Picto core library.
//!
//! These types are serialized to JSON across the IPC boundary.
//! Extracted from the former `commands.rs` IPC glue.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use ts_rs::TS;

use crate::sqlite::files::{FileMetadataSlim, FileRecord};
use crate::sqlite::smart_folders::SmartFolderPredicate;

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

#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ImportResult {
    pub hash: String,
    pub mime: String,
    #[ts(type = "number")]
    pub size: u64,
    pub has_thumbnail: bool,
    pub tags_applied: Vec<String>,
}

#[derive(Debug, Serialize, TS)]
#[ts(export, export_to = "../../src/types/generated/commands/")]
pub struct ImportBatchResult {
    pub imported: Vec<ImportResult>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DominantColorDto {
    pub hex: String,
    pub l: f64,
    pub a: f64,
    pub b: f64,
}

#[derive(Debug, Serialize)]
pub struct EntityDetails {
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
    pub imported_at: String,
    pub has_thumbnail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_color_hex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dominant_colors: Option<Vec<DominantColorDto>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<JsonValue>,
}

impl From<FileRecord> for EntityDetails {
    fn from(f: FileRecord) -> Self {
        let has_thumbnail = f.mime.starts_with("image/") || f.mime.starts_with("video/");
        let source_urls: Option<JsonValue> = f
            .source_urls_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let notes: Option<JsonValue> = f
            .notes
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        Self {
            hash: f.hash,
            name: f.name,
            size: f.size,
            mime: f.mime,
            width: f.width,
            height: f.height,
            duration_ms: f.duration_ms,
            num_frames: f.num_frames,
            has_audio: f.has_audio,
            status: status_to_string(f.status).to_string(),
            rating: f.rating,
            view_count: f.view_count,
            source_urls,
            imported_at: f.imported_at,
            has_thumbnail,
            blurhash: f.blurhash,
            dominant_color_hex: f.dominant_color_hex,
            dominant_colors: None,
            notes,
        }
    }
}

/// Slim entity info for grid display — omits heavy fields.
#[derive(Debug, Serialize)]
pub struct EntitySlim {
    pub entity_id: i64,
    pub is_collection: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_item_count: Option<i64>,
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
    pub imported_at: String,
    pub has_thumbnail: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blurhash: Option<String>,
}

impl From<FileMetadataSlim> for EntitySlim {
    fn from(f: FileMetadataSlim) -> Self {
        let has_thumbnail = if f.is_collection {
            // Collection has thumbnail if a cover hash was resolved (non-empty)
            !f.hash.is_empty()
        } else {
            f.mime.starts_with("image/") || f.mime.starts_with("video/")
        };
        Self {
            entity_id: if f.entity_id > 0 {
                f.entity_id
            } else {
                f.file_id
            },
            is_collection: f.is_collection,
            collection_item_count: f.collection_item_count,
            hash: f.hash,
            name: f.name,
            size: f.size,
            mime: f.mime,
            width: f.width,
            height: f.height,
            duration_ms: f.duration_ms,
            num_frames: f.num_frames,
            has_audio: f.has_audio,
            status: status_to_string(f.status as i64).to_string(),
            rating: f.rating,
            view_count: f.view_count,
            imported_at: f.imported_at,
            has_thumbnail,
            blurhash: f.blurhash,
        }
    }
}

impl From<crate::sqlite::files::FileRecord> for EntitySlim {
    fn from(f: crate::sqlite::files::FileRecord) -> Self {
        let has_thumbnail = f.mime.starts_with("image/") || f.mime.starts_with("video/");
        Self {
            entity_id: f.file_id,
            is_collection: false,
            collection_item_count: None,
            hash: f.hash,
            name: f.name,
            size: f.size,
            mime: f.mime,
            width: f.width,
            height: f.height,
            duration_ms: f.duration_ms,
            num_frames: f.num_frames,
            has_audio: f.has_audio,
            status: status_to_string(f.status).to_string(),
            rating: f.rating,
            view_count: f.view_count,
            imported_at: f.imported_at,
            has_thumbnail,
            blurhash: f.blurhash,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct GridPageSlimQuery {
    pub limit: Option<usize>,
    pub cursor: Option<String>,
    pub status: Option<String>,
    #[serde(alias = "sortField")]
    pub sort_field: Option<String>,
    #[serde(alias = "sortOrder")]
    pub sort_order: Option<String>,
    #[serde(alias = "smartFolderPredicate")]
    pub smart_folder_predicate: Option<SmartFolderPredicate>,
    #[serde(alias = "searchTags")]
    pub search_tags: Option<Vec<String>>,
    #[serde(alias = "searchExcludedTags")]
    pub search_excluded_tags: Option<Vec<String>>,
    #[serde(alias = "tagMatchMode")]
    pub tag_match_mode: Option<String>,
    #[serde(alias = "folderIds")]
    pub folder_ids: Option<Vec<i64>>,
    #[serde(alias = "excludedFolderIds")]
    pub excluded_folder_ids: Option<Vec<i64>>,
    #[serde(alias = "folderMatchMode")]
    pub folder_match_mode: Option<String>,
    /// Collection entity scope filter — restricts grid to members of this collection.
    #[serde(alias = "collectionEntityId")]
    pub collection_entity_id: Option<i64>,
    /// Minimum rating filter (1-5)
    #[serde(alias = "ratingMin")]
    pub rating_min: Option<i64>,
    /// MIME prefix filters (e.g. ["image/", "video/"])
    #[serde(alias = "mimePrefixes")]
    pub mime_prefixes: Option<Vec<String>>,
    /// Dominant color hex filter
    #[serde(alias = "colorHex")]
    pub color_hex: Option<String>,
    /// Color tolerance / max distance (1-30, lower = stricter). Default 20.
    #[serde(alias = "colorAccuracy")]
    pub color_accuracy: Option<f64>,
    /// Free-text search query (FTS5 on name + notes)
    #[serde(alias = "searchText")]
    pub search_text: Option<String>,
    /// Seed for deterministic random ordering (Random view)
    #[serde(alias = "randomSeed")]
    pub random_seed: Option<i64>,
}

#[derive(Debug, Serialize)]
pub struct GridPageSlimResponse {
    pub items: Vec<EntitySlim>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    /// Total count of items matching the current filter (for scroll height estimation).
    /// Populated from bitmap length (free) or COUNT query. None when unavailable.
    pub total_count: Option<i64>,
}

#[cfg(test)]
mod grid_query_tests {
    use super::GridPageSlimQuery;

    #[test]
    fn grid_query_deserializes_camel_case_fields() {
        let raw = serde_json::json!({
            "limit": 100,
            "cursor": null,
            "sortField": "imported_at",
            "sortOrder": "desc",
            "smartFolderPredicate": { "groups": [] },
            "searchTags": ["series:test"],
            "searchExcludedTags": ["artist:foo"],
            "tagMatchMode": "any",
            "folderIds": [42],
            "excludedFolderIds": [99],
            "folderMatchMode": "all",
            "collectionEntityId": 7,
            "ratingMin": 3,
            "mimePrefixes": ["image/"],
            "colorHex": "#ffffff",
            "colorAccuracy": 12.0,
            "searchText": "cat"
        });
        let parsed: GridPageSlimQuery =
            serde_json::from_value(raw).expect("query should deserialize");
        assert_eq!(parsed.sort_field.as_deref(), Some("imported_at"));
        assert_eq!(parsed.sort_order.as_deref(), Some("desc"));
        assert_eq!(parsed.folder_ids, Some(vec![42]));
        assert_eq!(parsed.excluded_folder_ids, Some(vec![99]));
        assert_eq!(parsed.folder_match_mode.as_deref(), Some("all"));
        assert_eq!(parsed.collection_entity_id, Some(7));
        assert_eq!(parsed.search_tags, Some(vec!["series:test".to_string()]));
        assert_eq!(
            parsed.search_excluded_tags,
            Some(vec!["artist:foo".to_string()])
        );
        assert_eq!(parsed.tag_match_mode.as_deref(), Some("any"));
        assert_eq!(parsed.search_text.as_deref(), Some("cat"));
    }
}

#[derive(Debug, Serialize)]
pub struct TagInfo {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub display: String,
    pub file_count: i64,
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
pub struct StorageStats {
    pub file_count: i64,
}

#[derive(Debug, Serialize)]
pub struct FileAllMetadata {
    pub file: EntityDetails,
    pub tags: Vec<ResolvedTagInfo>,
    pub parent_tags: Vec<TagInfo>,
}

#[derive(Debug, Serialize)]
pub struct EntityMetadataBatchResponse {
    pub items: HashMap<String, FileAllMetadata>,
    pub missing: Vec<String>,
    pub generated_at: String,
}

// Temporary migration aliases while TS/front-end callsites are moved to entity-centric naming.
pub type FileInfo = EntityDetails;
pub type FileInfoSlim = EntitySlim;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectionMode {
    ExplicitHashes,
    AllResults,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SelectionQuerySpec {
    pub mode: SelectionMode,
    pub hashes: Option<Vec<String>>,
    pub search_tags: Option<Vec<String>>,
    pub search_excluded_tags: Option<Vec<String>>,
    pub tag_match_mode: Option<String>,
    pub smart_folder_predicate: Option<SmartFolderPredicate>,
    pub smart_folder_sort_field: Option<String>,
    pub smart_folder_sort_order: Option<String>,
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub excluded_hashes: Option<Vec<String>>,
    pub included_hashes: Option<Vec<String>>,
    pub status: Option<String>,
    pub folder_ids: Option<Vec<i64>>,
    pub excluded_folder_ids: Option<Vec<i64>>,
    pub folder_match_mode: Option<String>,
}

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
pub struct SelectionSummary {
    pub total_count: i64,
    pub selected_count: i64,
    pub sample_hashes: Vec<String>,
    pub shared_tags: Vec<SelectionTagCount>,
    pub top_tags: Vec<SelectionTagCount>,
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
}

#[derive(Debug, Clone, Deserialize)]
pub struct ViewPrefsPatch {
    pub sort_field: Option<String>,
    pub sort_order: Option<String>,
    pub view_mode: Option<String>,
    pub target_size: Option<i64>,
    pub show_name: Option<bool>,
    pub show_resolution: Option<bool>,
    pub show_extension: Option<bool>,
    pub show_label: Option<bool>,
    pub thumbnail_fit: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FolderReorderMove {
    pub hash: String,
    pub before_hash: Option<String>,
    pub after_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DuplicateInfo {
    pub other_hash: String,
    pub distance: f64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct DuplicatePairResponse {
    pub hash_a: String,
    pub hash_b: String,
    pub distance: f64,
}

#[derive(Debug, Serialize)]
pub struct ScanDuplicatesResponse {
    pub candidates_found: usize,
    pub pairs_inserted: usize,
    pub reviewable_detected_total: usize,
    pub reviewable_detected_new: usize,
    pub total_files: usize,
    pub files_with_phash: usize,
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
pub struct ColorSearchResult {
    pub hash: String,
    pub distance: f64,
}

#[derive(Debug, Serialize)]
pub struct FlowInfo {
    pub id: String,
    pub name: String,
    pub schedule: String,
    pub created_at: String,
    pub total_files: u64,
    pub subscriptions: Vec<SubscriptionInfo>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionInfo {
    pub id: String,
    pub name: String,
    pub site_id: String,
    pub paused: bool,
    pub flow_id: Option<String>,
    pub initial_file_limit: u32,
    pub periodic_file_limit: u32,
    pub created_at: String,
    pub total_files: u64,
    pub queries: Vec<SubscriptionQueryInfo>,
}

#[derive(Debug, Serialize)]
pub struct SubscriptionQueryInfo {
    pub id: String,
    pub query_text: String,
    pub display_name: Option<String>,
    pub paused: bool,
    pub last_check_time: Option<String>,
    pub files_found: u64,
    pub completed_initial_run: bool,
    pub resume_cursor: Option<String>,
    pub resume_strategy: Option<String>,
}

/// Running subscriptions tracker. Key = subscription ID string.
pub type RunningSubscriptions =
    std::sync::Arc<tokio::sync::Mutex<HashMap<String, tokio_util::sync::CancellationToken>>>;

/// Terminal status map for finished subscriptions. Key = subscription ID, Value = terminal status string.
/// Written by subscription tasks on exit, read by flow monitor to aggregate final flow status.
pub type SubTerminalStatuses = std::sync::Arc<tokio::sync::Mutex<HashMap<String, String>>>;
