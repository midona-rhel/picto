//! Mutation contract types — emitted via `runtime/mutation_committed` events.
//!
//! `MutationReceipt` is the primary event the frontend subscribes to.
//! It carries sequencing metadata, what changed (`MutationFacts`), what
//! to refresh (`DerivedInvalidation`), and optional O(1) sidebar counts.

use serde::Serialize;
use ts_rs::TS;

use crate::events::Domain;

/// The primary mutation description emitted via `runtime/mutation_committed`.
///
/// Combines sequencing metadata with what changed (`facts`), what the frontend
/// should refresh (`invalidate`), and optional O(1) sidebar counts.
#[derive(Debug, Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct MutationReceipt {
    #[ts(type = "number")]
    pub seq: u64,
    pub ts: String,
    pub origin_command: String,
    pub facts: MutationFacts,
    pub invalidate: DerivedInvalidation,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sidebar_counts: Option<SidebarCounts>,
}

/// What actually changed — domain flags + affected entity IDs.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/runtime-contract/")]
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
}

/// What the frontend should refresh — mirrors the legacy `Invalidate` struct.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct DerivedInvalidation {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sidebar_tree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub grid_scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub selection_summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub metadata_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub view_prefs: Option<bool>,
}

/// O(1) bitmap-derived sidebar counts.
#[derive(Debug, Clone, Default, Serialize, TS)]
#[ts(export, export_to = "../../src/shared/types/generated/runtime-contract/")]
pub struct SidebarCounts {
    #[ts(type = "number")]
    pub all_images: i64,
    #[ts(type = "number")]
    pub inbox: i64,
    #[ts(type = "number")]
    pub trash: i64,
}
