//! Shared types for the database boundary.

use serde::{Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityKind {
    Single,
    Collection,
}

impl EntityKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            EntityKind::Single => "single",
            EntityKind::Collection => "collection",
        }
    }

    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "single" => Ok(EntityKind::Single),
            "collection" => Ok(EntityKind::Collection),
            other => Err(format!("Invalid entity_kind: {other}")),
        }
    }
}

/// How a command expands entity targets to include collection members.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpansionMode {
    EntityOnly,
    DescendantsOnly,
    EntityAndDescendants,
}

#[derive(Debug, Default, Serialize)]
pub struct EntityChange {
    pub entity_ids: Vec<i64>,
    pub entity_hashes: Vec<String>,
    /// Content hashes whose last media_file reference was deleted — their
    /// blobs are safe to reclaim once the transaction commits.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub freed_file_hashes: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct StatusChange {
    pub entity_ids: Vec<i64>,
    pub entity_hashes: Vec<String>,
    pub new_status: i64,
}

#[derive(Debug, Default, Serialize)]
pub struct TagChange {
    pub entity_ids: Vec<i64>,
    pub tag_ids: Vec<i64>,
    pub tags_added: Vec<String>,
    pub tags_removed: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
pub struct FolderMembershipChange {
    pub folder_id: i64,
    pub entity_ids: Vec<i64>,
}

/// The folders removed by one recursive delete, ordered from leaves to root.
/// UUIDs are captured before the cascade so every deleted folder can receive
/// its own sync tombstone in the same transaction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FolderDeleteResult {
    pub deleted_folders: Vec<DeletedFolder>,
}

impl FolderDeleteResult {
    pub fn is_empty(&self) -> bool {
        self.deleted_folders.is_empty()
    }

    pub fn folder_ids(&self) -> Vec<i64> {
        self.deleted_folders
            .iter()
            .map(|folder| folder.folder_id)
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletedFolder {
    pub folder_id: i64,
    pub uuid: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FolderMembership {
    pub folder_id: i64,
    pub folder_name: String,
}

#[derive(Debug, Default, Serialize)]
pub struct CollectionMembershipChange {
    pub collection_id: i64,
    pub added: Vec<i64>,
    pub removed: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatePairRecord {
    pub hash_a: String,
    pub hash_b: String,
    pub distance: f64,
    pub similarity_pct: f64,
    pub status: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateScanSummary {
    pub candidates_found: usize,
    pub pairs_inserted: usize,
    pub reviewable_detected_total: usize,
    pub reviewable_detected_new: usize,
    pub total_files: usize,
    pub files_with_phash: usize,
    pub files_scanned: usize,
    pub closest_distance: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FileStats {
    pub total: i64,
    pub inbox: i64,
    pub active: i64,
    pub trash: i64,
    pub total_size: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaTypeBreakdown {
    pub images: i64,
    pub images_size: i64,
    pub videos: i64,
    pub videos_size: i64,
    pub audio: i64,
    pub audio_size: i64,
    pub other: i64,
    pub other_size: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicatePairPage {
    pub items: Vec<DuplicatePairRecord>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DuplicateResolveStatus {
    Resolved,
    Conflict,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateCollectionConflict {
    pub winner_hash: String,
    pub loser_hash: String,
    pub winner_collection_id: Option<i64>,
    pub loser_collection_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DuplicateResolutionResult {
    pub status: DuplicateResolveStatus,
    pub winner_hash: Option<String>,
    pub loser_hash: Option<String>,
    pub action: String,
    pub affected_folder_ids: Vec<i64>,
    pub affected_collection_ids: Vec<i64>,
    pub tags_merged: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conflict: Option<DuplicateCollectionConflict>,
}

#[derive(Debug, Clone)]
pub struct PerceptualHashCandidate {
    pub file_id: i64,
    pub entity_id: i64,
    pub entity_hash: String,
    pub file_hash: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub frame_count: Option<i64>,
    pub perceptual_hash: String,
    pub distance: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngestDuplicateAction {
    None,
    ReuseExisting { entity_hash: String },
    PreferNewOverExisting { existing_entity_hash: String },
}

#[derive(Debug, Clone)]
pub struct IngestDuplicatePlan {
    pub action: IngestDuplicateAction,
    pub review_candidates: Vec<PerceptualHashCandidate>,
}

impl Default for IngestDuplicatePlan {
    fn default() -> Self {
        Self {
            action: IngestDuplicateAction::None,
            review_candidates: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionRecord {
    pub id: i64,
    pub name: String,
    pub tags: Vec<String>,
    pub image_count: i64,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionMimeCount {
    pub mime: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollectionSummary {
    pub id: i64,
    pub name: String,
    pub tags: Vec<String>,
    pub image_count: i64,
    pub total_size_bytes: i64,
    pub mime_breakdown: Vec<CollectionMimeCount>,
    pub source_urls: Vec<String>,
    pub rating: Option<i64>,
    #[serde(rename = "date_created")]
    pub created_at: Option<String>,
    #[serde(rename = "date_modified")]
    pub updated_at: Option<String>,
    #[serde(rename = "date_added")]
    pub imported_at: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone)]
pub struct IngestPreparedSingle {
    pub entity_hash: String,
    pub name: Option<String>,
    pub size_bytes: i64,
    pub mime_type: String,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub status: i64,
    pub date_created: String,
    pub date_added: String,
    pub has_thumbnail: bool,
    pub skip_thumbnail: bool,
    pub notes: Option<String>,
    pub source_urls: Vec<String>,
    pub tag_strings: Vec<String>,
    pub tag_provenance_mask: u64,
    pub perceptual_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClaimedDeferredWorkItem {
    pub work_id: i64,
    pub entity_hash: String,
    pub work_type: String,
    pub attempt_count: i64,
}

/// Partial patch for folder metadata.
/// Each field is None = "not included in this patch".
#[derive(Debug, Clone, Default)]
pub struct FolderPatch {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub auto_tags: Option<String>,
    pub watch_path: Option<String>,
    pub watch_enabled: Option<bool>,
    pub watch_subfolders: Option<bool>,
    pub watch_import_status_mode: Option<String>,
}

// ── Query/projection types (public boundary) ────────────────────

/// Grid tile payload.
#[derive(Debug, Clone, Serialize)]
pub struct EntityGridItem {
    pub entity_id: i64,
    pub entity_hash: String,
    /// Hash used to load the thumbnail. For singles == entity_hash.
    /// For collections == primary member's hash.
    pub thumbnail_hash: String,
    pub entity_kind: EntityKind,
    pub name: Option<String>,
    pub mime_type: String,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub status: i64,
    pub rating: Option<i64>,
    pub date_added: String,
    pub date_created: String,
    pub date_modified: String,
    pub has_thumbnail: bool,
    pub member_count: Option<i64>,
    pub duration_ms: Option<i64>,
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub dominant_color_hex: Option<String>,
    pub size_bytes: i64,
}

/// A page of grid results.
#[derive(Debug, Clone, Serialize)]
pub struct EntityViewPage {
    pub items: Vec<EntityGridItem>,
    pub next_cursor: Option<String>,
    pub total_count: Option<i64>,
    pub total_size_bytes: Option<i64>,
}

/// Scope kind for grid queries.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    System,
    Folder,
    SmartFolder,
    Collection,
    Similar,
    Search,
    Tag,
}

/// Grid query model.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct EntityViewQuery {
    pub base_scope: BaseScope,
    #[serde(default)]
    pub filters: QueryFilters,
    #[serde(default)]
    pub sort: QuerySort,
    #[serde(default)]
    pub page: QueryPage,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct BaseScope {
    pub kind: ScopeKind,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub id: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
pub struct QueryFilters {
    pub rating: Option<RatingFilter>,
    pub colors: Option<Vec<String>>,
    pub mime_types: Option<Vec<String>>,
    pub entity_types: Option<Vec<String>>, // "image", "video", "audio", "collection"
    pub tags: Option<Vec<TagFilter>>,
    pub date_created: Option<DateRange>,
    pub date_added: Option<DateRange>,
    pub date_modified: Option<DateRange>,
    pub search_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct RatingFilter {
    pub value: i64,
    #[serde(default = "default_filter_op")]
    pub op: FilterOp,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct TagFilter {
    pub tag: String,
    #[serde(default = "default_tag_match")]
    pub match_mode: TagMatchMode,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TagMatchMode {
    Include,
    Exclude,
}

fn default_tag_match() -> TagMatchMode {
    TagMatchMode::Include
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Gte,
    Lte,
    Gt,
    Lt,
}

fn default_filter_op() -> FilterOp {
    FilterOp::Gte
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct DateRange {
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct QuerySort {
    #[serde(default = "default_sort_field")]
    pub field: String,
    #[serde(default = "default_sort_dir")]
    pub direction: String,
}

impl Default for QuerySort {
    fn default() -> Self {
        Self {
            field: "date_added".into(),
            direction: "desc".into(),
        }
    }
}

fn default_sort_field() -> String {
    "date_added".into()
}
fn default_sort_dir() -> String {
    "desc".into()
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct QueryPage {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

impl Default for QueryPage {
    fn default() -> Self {
        Self {
            limit: 100,
            cursor: None,
        }
    }
}

fn default_limit() -> i64 {
    100
}

/// Bulk entity target — replaces SelectionQuerySpec.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct EntityTarget {
    pub kind: EntityTargetKind,
    pub entity_hashes: Option<Vec<String>>,
    pub query: Option<EntityViewQuery>,
    pub excluded_entity_hashes: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityTargetKind {
    EntityHashes,
    QueryResults,
}

/// Inspector/detail panel payload. Fully independent from EntityGridItem.
#[derive(Debug, Clone, Serialize)]
pub struct EntityDetails {
    pub entity_hash: String,
    pub thumbnail_hash: String,
    pub entity_kind: EntityKind,
    pub name: Option<String>,
    pub mime_type: String,
    pub size_bytes: i64,
    pub pixel_width: Option<i64>,
    pub pixel_height: Option<i64>,
    pub duration_ms: Option<i64>,
    pub frame_count: Option<i64>,
    pub has_audio: bool,
    pub status: i64,
    pub rating: Option<i64>,
    pub notes: Option<String>,
    pub source_urls: Option<Vec<String>>,
    pub date_created: String,
    pub date_added: String,
    pub date_modified: String,
    pub dominant_color_hex: Option<String>,
    pub dominant_colors: Option<Vec<crate::types::DominantColorDto>>,
    pub perceptual_hash: Option<String>,
    pub tags: Vec<TagInfo>,
    pub folders: Vec<FolderInfo>,
    pub member_count: Option<i64>,
    pub total_size_bytes: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagInfo {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    /// Concept-level site support mask. Curated metadata, not derived from assignment rows.
    #[serde(serialize_with = "serialize_mask_as_decimal")]
    pub site_mask: u64,
    /// Assignment provenance mask for this entity-tag relation.
    #[serde(serialize_with = "serialize_mask_as_decimal")]
    pub provenance_mask: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagRecord {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub file_count: i64,
    #[serde(serialize_with = "serialize_mask_as_decimal")]
    pub site_mask: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TagRelation {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub relation: String,
    #[serde(serialize_with = "serialize_mask_as_decimal")]
    pub site_mask: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NamespaceSummary {
    pub namespace: String,
    pub count: i64,
}

#[derive(Debug, Default, Clone, Serialize)]
pub struct TagStructureChange {
    pub entity_ids: Vec<i64>,
    pub dirty_tag_ids: Vec<i64>,
    pub merged_into_tag_id: Option<i64>,
}

/// Low-bit provenance flags for tag assignment masks.
pub const TAG_PROVENANCE_MANUAL: u64 = 1_u64 << 0;
pub const TAG_PROVENANCE_AI: u64 = 1_u64 << 1;
pub const TAG_PROVENANCE_UNKNOWN: u64 = 1_u64 << 2;
pub const TAG_PROVENANCE_LOCAL_TOOL: u64 = 1_u64 << 3;

/// High-bit site flags reserved by PBI-598.
pub const TAG_SITE_E621: u64 = 1_u64 << 63;
pub const TAG_SITE_GELBOORU: u64 = 1_u64 << 62;
pub const TAG_SITE_DANBOORU: u64 = 1_u64 << 61;
pub const TAG_SITE_RULE34: u64 = 1_u64 << 60;

pub fn mask_to_db_bits(mask: u64) -> i64 {
    mask as i64
}

pub fn mask_from_db_bits(bits: i64) -> u64 {
    bits as u64
}

pub fn parse_mask_decimal(mask: &str) -> Result<u64, String> {
    mask.parse::<u64>()
        .map_err(|e| format!("Invalid mask decimal '{mask}': {e}"))
}

fn serialize_mask_as_decimal<S>(mask: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&mask.to_string())
}

#[cfg(test)]
mod tests {
    use super::{TagInfo, TAG_PROVENANCE_MANUAL, TAG_SITE_E621};

    #[test]
    fn tag_info_serializes_masks_as_decimal_strings() {
        let tag = TagInfo {
            tag_id: 1,
            namespace: String::new(),
            subtag: "test".to_string(),
            site_mask: TAG_SITE_E621,
            provenance_mask: TAG_PROVENANCE_MANUAL,
            source: "local".to_string(),
        };

        let json = serde_json::to_value(&tag).expect("serialize tag info");
        assert_eq!(json["site_mask"], TAG_SITE_E621.to_string());
        assert_eq!(json["provenance_mask"], TAG_PROVENANCE_MANUAL.to_string());
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FolderInfo {
    pub folder_id: i64,
    pub name: String,
}

// ── Grid reconcile types ─────────────────────────────────────────

/// Request from the frontend to reconcile the current grid view.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EntityViewReconcileRequest {
    /// The query that produced the current visible grid.
    pub query: EntityViewQuery,
    /// Entity hashes currently visible in the frontend grid.
    pub visible_hashes: Vec<String>,
    /// If true, the frontend asserts that only metadata/derivative fields changed
    /// (no membership or ordering change). The backend can safely return PatchRows
    /// if all visible hashes are still present.
    /// If false, membership may have changed — backend must prove the window is
    /// unchanged before returning PatchRows.
    #[serde(default)]
    pub metadata_only: bool,
}

/// What the backend determined about the current view after a change.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityViewReconcileResult {
    /// Nothing visible changed — frontend can keep its current state.
    NoChange,
    /// Some visible rows have updated metadata/derivatives.
    /// Frontend should patch these rows in place.
    PatchRows { items: Vec<EntityGridItem> },
    /// Membership or order changed. The backend re-ran the query for
    /// the loaded window size and returns the correct replacement page.
    /// Frontend should swap items, next_cursor, and total_count.
    ReplaceWindow { page: EntityViewPage },
    /// Truly unsupported case — frontend should call loadFirstPage.
    FullRefreshRequired,
}

/// Partial metadata patch for entities.
/// Each field is None = "not included in this patch", Some = "set to this value".
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct MediaEntityPatch {
    pub name: Option<String>,
    /// Plain-text notes. Absent = unchanged, null = clear, string = set.
    #[serde(
        default,
        deserialize_with = "crate::dispatch::common::deserialize_some"
    )]
    pub notes: Option<Option<String>>,
    pub rating: Option<i64>,
    pub source_urls: Option<Vec<String>>,
}
