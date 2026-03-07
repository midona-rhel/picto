use serde::Serialize;

use crate::events::Domain;

/// The primary mutation description emitted via `runtime/mutation_committed`.
///
/// Combines sequencing metadata with what changed (`facts`), what the frontend
/// should refresh (`invalidate`), and optional O(1) sidebar counts.
#[derive(Debug, Clone, Serialize)]
pub struct MutationReceipt {
    pub seq: u64,
    pub ts: String,
    pub origin_command: String,
    pub facts: MutationFacts,
    pub invalidate: DerivedInvalidation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_counts: Option<SidebarCounts>,
}

/// What actually changed — domain flags + affected entity IDs.
#[derive(Debug, Clone, Default, Serialize)]
pub struct MutationFacts {
    pub domains: Vec<Domain>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder_ids: Option<Vec<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub smart_folder_ids: Option<Vec<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compiler_batch_done: Option<bool>,
}

/// What the frontend should refresh — mirrors the legacy `Invalidate` struct.
#[derive(Debug, Clone, Default, Serialize)]
pub struct DerivedInvalidation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_tree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_prefs: Option<bool>,
}

/// O(1) bitmap-derived sidebar counts.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SidebarCounts {
    pub all_images: i64,
    pub inbox: i64,
    pub trash: i64,
}
