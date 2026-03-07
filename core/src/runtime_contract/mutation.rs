//! Mutation contract types — emitted via `runtime/mutation_committed` events.
//!
//! `MutationReceipt` is the primary event the frontend subscribes to.
//! It carries sequencing metadata, what changed (`MutationFacts`), and
//! optional O(1) sidebar counts.
//!
//! The `invalidate` field (`DerivedInvalidation`) is transitional — the
//! frontend derives stale resources from `facts` directly. Once all
//! consumers confirm independence from `invalidate`, it can be removed.

use serde::Serialize;
use ts_rs::TS;

use crate::events::Domain;

/// The primary mutation description emitted via `runtime/mutation_committed`.
///
/// The frontend derives stale resources from `facts` directly.
/// `invalidate` is transitional and will be removed once confirmed unused.
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

/// What actually changed — domain flags, affected entity IDs, and change descriptors.
///
/// Change descriptors (`status_changed`, `tags_changed`, etc.) tell the system
/// *what kind* of mutation happened. `derive_invalidation()` converts these
/// facts into the concrete `DerivedInvalidation` the frontend needs.
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

/// Transitional: backend-derived invalidation hints.
///
/// The frontend now derives stale resources from `MutationFacts` directly.
/// This struct is retained for backward compatibility and debugging; it will
/// be removed once all consumers confirm they no longer read it.
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

/// Derive what the frontend should refresh from what actually changed.
///
/// This is the single source of truth for the facts → invalidation mapping.
/// Handlers set change descriptors (facts), and this function determines
/// which UI resources need refreshing.
pub fn derive_invalidation(facts: &MutationFacts) -> DerivedInvalidation {
    let mut inv = DerivedInvalidation::default();
    let mut scopes: Vec<String> = Vec::new();

    // --- Fact-driven rules ---

    if facts.status_changed == Some(true) {
        inv.sidebar_tree = Some(true);
        inv.selection_summary = Some(true);
        scopes.extend([
            "system:all".into(),
            "system:inbox".into(),
            "system:trash".into(),
            "system:recently_viewed".into(),
            "smart:all".into(),
        ]);
        if let Some(ref ids) = facts.folder_ids {
            for id in ids {
                scopes.push(format!("folder:{}", id));
            }
        }
    }

    if facts.tags_changed == Some(true) {
        inv.selection_summary = Some(true);
        if facts.file_hashes.is_none() {
            scopes.push("system:all".into());
        }
    }

    if facts.tag_structure_changed == Some(true) {
        inv.sidebar_tree = Some(true);
        inv.selection_summary = Some(true);
        scopes.extend(["system:all".into(), "smart:all".into()]);
    }

    if let Some(ref ids) = facts.folder_membership_changed {
        inv.sidebar_tree = Some(true);
        inv.selection_summary = Some(true);
        for id in ids {
            scopes.push(format!("folder:{}", id));
        }
    }

    if facts.view_prefs_changed == Some(true) {
        inv.view_prefs = Some(true);
    }

    // compiler_batch_done does NOT unconditionally set sidebar_tree — that is
    // determined by whether Domain::Sidebar is present (sidebar_affected flag).

    // --- Domain-driven rules (fallback for patterns without fact flags) ---

    if inv.sidebar_tree.is_none()
        && facts
            .domains
            .iter()
            .any(|d| matches!(d, Domain::Sidebar))
    {
        inv.sidebar_tree = Some(true);
    }

    if inv.selection_summary.is_none()
        && facts
            .domains
            .iter()
            .any(|d| matches!(d, Domain::Selection))
    {
        inv.selection_summary = Some(true);
    }

    // --- Entity-reference rules ---

    if let Some(ref hashes) = facts.file_hashes {
        inv.metadata_hashes = Some(hashes.clone());
    }

    // Folder IDs without folder_membership_changed → grid refresh for those
    // folder scopes only (e.g., reorder within a folder).
    if facts.folder_membership_changed.is_none() {
        if let Some(ref ids) = facts.folder_ids {
            for id in ids {
                scopes.push(format!("folder:{}", id));
            }
        }
    }

    if let Some(ref ids) = facts.smart_folder_ids {
        inv.selection_summary = Some(true);
        for id in ids {
            scopes.push(format!("smart:{}", id));
        }
    }

    // Deduplicate and set grid_scopes
    if !scopes.is_empty() {
        scopes.sort();
        scopes.dedup();
        inv.grid_scopes = Some(scopes);
    }

    inv
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
