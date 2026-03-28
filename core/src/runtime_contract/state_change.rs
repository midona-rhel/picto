//! State-change contract types — emitted via `runtime/state_changed` events.
//!
//! `StateChangedEvent` is the primary event the frontend subscribes to.
//! It carries sequencing metadata, what changed (`StateChanges`), and
//! optional O(1) sidebar counts.

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Files,
    Folders,
    SmartFolders,
    Tags,
    Sidebar,
    Selection,
    ViewPrefs,
    Subscriptions,
}

/// The primary state-change description emitted via `runtime/state_changed`.
///
/// The frontend derives stale resources from `changes` directly.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct StateChangedEvent {
    #[ts(type = "number")]
    pub seq: u64,
    pub ts: String,
    pub origin: String,
    pub changes: StateChanges,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sidebar_counts: Option<SidebarCounts>,
}

/// What actually changed — domain flags, affected entity IDs, and change descriptors.
///
/// Change descriptors (`status_changed`, `tags_changed`, etc.) tell the system
/// *what kind* of state change happened. The frontend derives stale resources
/// from these changes directly.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct StateChanges {
    pub domains: Vec<Domain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub entity_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub member_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<number>")]
    pub folder_ids: Option<Vec<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<number>")]
    pub smart_folder_ids: Option<Vec<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compiler_batch_done: Option<bool>,
    /// Entity status transitions (inbox/active/trash).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub status_changed: Option<bool>,
    /// Tags added/removed on specific entities.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tags_changed: Option<bool>,
    /// Exact tag additions/removals when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tag_changes: Option<TagChangeDetails>,
    /// Tag hierarchy, aliases, merges, or renames changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tag_structure_changed: Option<bool>,
    /// Folder IDs where membership changed (files added/removed).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<number>")]
    pub folder_membership_changed: Option<Vec<i64>>,
    /// View preferences changed (zoom, sort, display mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub view_prefs_changed: Option<bool>,
    /// Generic media metadata changed (name, rating, notes, urls, etc.).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub media_metadata_changed: Option<bool>,
    /// Exact media metadata fields that changed when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub media_fields_changed: Option<Vec<MediaMetadataField>>,
    /// Deferred media derivatives changed (thumbnail, colors, phash, analysis).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub media_derivatives_changed: Option<bool>,
    /// Exact derived media fields that changed when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub derivative_fields_changed: Option<Vec<MediaDerivativeField>>,
    /// Grid scopes not derivable from other fact fields (e.g. `collection:{id}`).
    /// The frontend includes these when deriving stale grid resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extra_grid_scopes: Option<Vec<String>>,
    /// Subscription group IDs that changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<number>")]
    pub group_ids: Option<Vec<i64>>,
    /// Subscription IDs that changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<number>")]
    pub subscription_ids: Option<Vec<i64>>,
    /// Subscription query IDs that changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<number>")]
    pub query_ids: Option<Vec<i64>>,
    /// Credential site categories that changed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub credential_categories: Option<Vec<String>>,
    /// Folder tree parent changes: [[folder_id, new_parent_id | null], ...]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<[number, number | null]>")]
    pub folder_parent_changes: Option<Vec<(i64, Option<i64>)>>,
    /// Folder tree order changes: [[folder_id, new_sort_order], ...]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<[number, number]>")]
    pub folder_order_changes: Option<Vec<(i64, i64)>>,
    /// Smart folder tree parent changes: [[sf_id, new_parent_id | null], ...]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<[number, number | null]>")]
    pub smart_folder_parent_changes: Option<Vec<(i64, Option<i64>)>>,
    /// Smart folder tree order changes: [[sf_id, new_sort_order], ...]
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<[number, number]>")]
    pub smart_folder_order_changes: Option<Vec<(i64, i64)>>,
    /// Sidebar node patches — partial updates, creates, or deletes for exact settle.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sidebar_node_patches: Option<Vec<SidebarNodePatch>>,
    /// Smart folder count deltas: [[sf_id, new_count], ...]
    /// Emitted from compiler publish when smart folder bitmaps are recomputed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "Array<[number, number]>")]
    pub smart_folder_counts: Option<Vec<(i64, i64)>>,
}

/// Sidebar node patch — carries partial updates, removals, or rendering-sufficient
/// upserts for sidebar nodes. The frontend can apply these as exact deltas
/// without fetching the whole tree.
///
/// Note: upserts are rendering-sufficient, not full SidebarNodeDto clones.
/// They carry enough data to render the node and its current context inspector
/// summary (kind, parent, name, icon, color, sort_order, count, selectable,
/// freshness, meta_json) but omit `expanded_by_default`.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct SidebarNodePatch {
    pub node_id: String,
    /// If true, the node was removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub removed: Option<bool>,
    /// If true, this is a full node upsert (insert-or-replace). All fields are set.
    /// If false or absent, this is a partial patch on an existing node.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub upsert: Option<bool>,
    /// Node kind — required for upserts (folder, smart_folder, system, section).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kind: Option<String>,
    /// Parent node ID — required for upserts. Absent = not patching parent.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string | null")]
    pub parent_id: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub name: Option<String>,
    /// Icon: None = not in patch, Some(None) = clear, Some(Some(v)) = set.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string | null")]
    pub icon: Option<Option<String>>,
    /// Color: None = not in patch, Some(None) = clear, Some(Some(v)) = set.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string | null")]
    pub color: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number | null")]
    pub sort_order: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "number | null")]
    pub count: Option<Option<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub selectable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub freshness: Option<String>,
    /// Meta JSON: None = not in patch, Some(None) = clear, Some(Some(v)) = set.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional, type = "string | null")]
    pub meta_json: Option<Option<String>>,
}

/// Sidebar system scope counts — bitmap-derived where possible.
/// active/inbox/trash/untagged are O(1) bitmap reads.
/// uncategorized and duplicates require SQL but are still fast.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct SidebarCounts {
    #[ts(type = "number")]
    pub active: i64,
    #[ts(type = "number")]
    pub inbox: i64,
    #[ts(type = "number")]
    pub trash: i64,
    #[ts(type = "number")]
    pub uncategorized: i64,
    #[ts(type = "number")]
    pub untagged: i64,
    #[ts(type = "number")]
    pub duplicates: i64,
}

#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct TagChangeDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub added: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub removed: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
#[serde(rename_all = "snake_case")]
pub enum MediaMetadataField {
    Name,
    Rating,
    Notes,
    SourceUrls,
    CreatedAt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
#[serde(rename_all = "snake_case")]
pub enum MediaDerivativeField {
    Thumbnail,
    DominantColorHex,
    Phash,
    Analysis,
}
