//! Mutation contract types — emitted via `runtime/mutation_committed` events.
//!
//! `MutationReceipt` is the primary event the frontend subscribes to.
//! It carries sequencing metadata, what changed (`MutationFacts`), and
//! optional O(1) sidebar counts.

use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
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

/// The primary mutation description emitted via `runtime/mutation_committed`.
///
/// The frontend derives stale resources from `facts` directly.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct MutationReceipt {
    #[ts(type = "number")]
    pub seq: u64,
    pub ts: String,
    pub origin_command: String,
    pub facts: MutationFacts,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sidebar_counts: Option<SidebarCounts>,
}

/// What actually changed — domain flags, affected entity IDs, and change descriptors.
///
/// Change descriptors (`status_changed`, `tags_changed`, etc.) tell the system
/// *what kind* of mutation happened. The frontend derives stale resources
/// from these facts directly.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct MutationFacts {
    pub domains: Vec<Domain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub file_hashes: Option<Vec<String>>,
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
    /// Grid scopes not derivable from other fact fields (e.g. `collection:{id}`).
    /// The frontend includes these when deriving stale grid resources.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub extra_grid_scopes: Option<Vec<String>>,
}

/// O(1) bitmap-derived sidebar counts.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct SidebarCounts {
    #[ts(type = "number")]
    pub all_images: i64,
    #[ts(type = "number")]
    pub inbox: i64,
    #[ts(type = "number")]
    pub trash: i64,
}
