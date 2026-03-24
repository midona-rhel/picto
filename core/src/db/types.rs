//! Shared types for the database boundary.

use serde::Serialize;

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

#[derive(Debug, Default, Serialize)]
pub struct CollectionMembershipChange {
    pub collection_id: i64,
    pub added: Vec<i64>,
    pub removed: Vec<i64>,
}

// ── Query/projection types (public boundary) ────────────────────

/// Grid tile payload.
#[derive(Debug, Clone, Serialize)]
pub struct EntityGridItem {
    pub entity_hash: String,
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
    pub entity_types: Option<Vec<String>>,  // "image", "video", "audio", "collection"
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

fn default_tag_match() -> TagMatchMode { TagMatchMode::Include }

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FilterOp {
    Eq,
    Gte,
    Lte,
    Gt,
    Lt,
}

fn default_filter_op() -> FilterOp { FilterOp::Gte }

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
        Self { field: "date_added".into(), direction: "desc".into() }
    }
}

fn default_sort_field() -> String { "date_added".into() }
fn default_sort_dir() -> String { "desc".into() }

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct QueryPage {
    #[serde(default = "default_limit")]
    pub limit: i64,
    #[serde(default)]
    pub cursor: Option<String>,
}

impl Default for QueryPage {
    fn default() -> Self {
        Self { limit: 100, cursor: None }
    }
}

fn default_limit() -> i64 { 100 }

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
    pub source: String,
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
    PatchRows {
        items: Vec<EntityGridItem>,
    },
    /// Membership or order changed. The backend re-ran the query for
    /// the loaded window size and returns the correct replacement page.
    /// Frontend should swap items, next_cursor, and total_count.
    ReplaceWindow {
        page: EntityViewPage,
    },
    /// Truly unsupported case — frontend should call loadFirstPage.
    FullRefreshRequired,
}

/// Partial metadata patch for entities.
/// Each field is None = "not included in this patch", Some = "set to this value".
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct MediaEntityPatch {
    pub name: Option<String>,
    /// Notes as a JSON object (Record<string, string> from the frontend).
    /// Stored as JSON text in the database.
    pub notes: Option<serde_json::Value>,
    pub rating: Option<i64>,
    pub source_urls: Option<Vec<String>>,
}
