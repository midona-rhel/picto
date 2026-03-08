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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::Domain;

    fn facts(f: impl FnOnce(&mut MutationFacts)) -> MutationFacts {
        let mut facts = MutationFacts::default();
        f(&mut facts);
        facts
    }

    // --- status_changed ---

    #[test]
    fn status_changed_derives_sidebar_tree_and_selection() {
        let inv = derive_invalidation(&facts(|f| {
            f.status_changed = Some(true);
        }));
        assert_eq!(inv.sidebar_tree, Some(true));
        assert_eq!(inv.selection_summary, Some(true));
        let scopes = inv.grid_scopes.unwrap();
        assert!(scopes.contains(&"system:all".to_string()));
        assert!(scopes.contains(&"system:inbox".to_string()));
        assert!(scopes.contains(&"system:trash".to_string()));
        assert!(scopes.contains(&"system:recently_viewed".to_string()));
        assert!(scopes.contains(&"smart:all".to_string()));
    }

    #[test]
    fn status_changed_with_folder_ids_includes_folder_scopes() {
        let inv = derive_invalidation(&facts(|f| {
            f.status_changed = Some(true);
            f.folder_ids = Some(vec![10, 20]);
        }));
        let scopes = inv.grid_scopes.unwrap();
        assert!(scopes.contains(&"folder:10".to_string()));
        assert!(scopes.contains(&"folder:20".to_string()));
    }

    // --- tags_changed ---

    #[test]
    fn tags_changed_with_file_hashes_derives_selection_and_metadata() {
        let inv = derive_invalidation(&facts(|f| {
            f.tags_changed = Some(true);
            f.file_hashes = Some(vec!["abc".into()]);
        }));
        assert_eq!(inv.selection_summary, Some(true));
        assert_eq!(inv.metadata_hashes, Some(vec!["abc".to_string()]));
        // No grid/system:all when file_hashes present
        assert!(inv.grid_scopes.is_none());
    }

    #[test]
    fn tags_changed_without_file_hashes_derives_grid_system_all() {
        let inv = derive_invalidation(&facts(|f| {
            f.tags_changed = Some(true);
        }));
        assert_eq!(inv.selection_summary, Some(true));
        let scopes = inv.grid_scopes.unwrap();
        assert!(scopes.contains(&"system:all".to_string()));
    }

    // --- tag_structure_changed ---

    #[test]
    fn tag_structure_changed_derives_sidebar_and_all_grids() {
        let inv = derive_invalidation(&facts(|f| {
            f.tag_structure_changed = Some(true);
        }));
        assert_eq!(inv.sidebar_tree, Some(true));
        assert_eq!(inv.selection_summary, Some(true));
        let scopes = inv.grid_scopes.unwrap();
        assert!(scopes.contains(&"system:all".to_string()));
        assert!(scopes.contains(&"smart:all".to_string()));
    }

    // --- folder_membership_changed ---

    #[test]
    fn folder_membership_changed_derives_folder_scopes() {
        let inv = derive_invalidation(&facts(|f| {
            f.folder_membership_changed = Some(vec![5, 15]);
        }));
        assert_eq!(inv.sidebar_tree, Some(true));
        assert_eq!(inv.selection_summary, Some(true));
        let scopes = inv.grid_scopes.unwrap();
        assert!(scopes.contains(&"folder:5".to_string()));
        assert!(scopes.contains(&"folder:15".to_string()));
    }

    // --- view_prefs_changed ---

    #[test]
    fn view_prefs_changed_derives_view_prefs() {
        let inv = derive_invalidation(&facts(|f| {
            f.view_prefs_changed = Some(true);
        }));
        assert_eq!(inv.view_prefs, Some(true));
        assert!(inv.sidebar_tree.is_none());
        assert!(inv.selection_summary.is_none());
        assert!(inv.grid_scopes.is_none());
    }

    // --- Domain fallbacks ---

    #[test]
    fn domain_sidebar_fallback() {
        let inv = derive_invalidation(&facts(|f| {
            f.domains = vec![Domain::Sidebar];
        }));
        assert_eq!(inv.sidebar_tree, Some(true));
    }

    #[test]
    fn domain_selection_fallback() {
        let inv = derive_invalidation(&facts(|f| {
            f.domains = vec![Domain::Selection];
        }));
        assert_eq!(inv.selection_summary, Some(true));
    }

    #[test]
    fn domain_sidebar_fallback_skipped_when_facts_set_it() {
        let inv = derive_invalidation(&facts(|f| {
            f.domains = vec![Domain::Sidebar];
            f.tag_structure_changed = Some(true);
        }));
        // sidebar_tree set by tag_structure_changed, domain fallback doesn't fire
        assert_eq!(inv.sidebar_tree, Some(true));
    }

    // --- Entity-reference rules ---

    #[test]
    fn file_hashes_derive_metadata_hashes() {
        let inv = derive_invalidation(&facts(|f| {
            f.domains = vec![Domain::Files];
            f.file_hashes = Some(vec!["h1".into(), "h2".into()]);
        }));
        assert_eq!(
            inv.metadata_hashes,
            Some(vec!["h1".to_string(), "h2".to_string()])
        );
    }

    #[test]
    fn folder_ids_without_membership_change_derive_grid_scopes() {
        let inv = derive_invalidation(&facts(|f| {
            f.folder_ids = Some(vec![7, 8]);
        }));
        let scopes = inv.grid_scopes.unwrap();
        assert!(scopes.contains(&"folder:7".to_string()));
        assert!(scopes.contains(&"folder:8".to_string()));
        // No sidebar_tree or selection_summary from folder_ids alone
        assert!(inv.sidebar_tree.is_none());
        assert!(inv.selection_summary.is_none());
    }

    #[test]
    fn folder_ids_suppressed_when_membership_changed_present() {
        let inv = derive_invalidation(&facts(|f| {
            f.folder_membership_changed = Some(vec![7]);
            f.folder_ids = Some(vec![7]);
        }));
        let scopes = inv.grid_scopes.unwrap();
        // folder:7 appears once from folder_membership_changed, not duplicated
        assert_eq!(scopes.iter().filter(|s| *s == "folder:7").count(), 1);
    }

    #[test]
    fn smart_folder_ids_derive_grid_scopes() {
        let inv = derive_invalidation(&facts(|f| {
            f.smart_folder_ids = Some(vec![3, 9]);
        }));
        assert_eq!(inv.selection_summary, Some(true));
        let scopes = inv.grid_scopes.unwrap();
        assert!(scopes.contains(&"smart:3".to_string()));
        assert!(scopes.contains(&"smart:9".to_string()));
    }

    // --- Empty facts ---

    #[test]
    fn empty_facts_derive_nothing() {
        let inv = derive_invalidation(&MutationFacts::default());
        assert!(inv.sidebar_tree.is_none());
        assert!(inv.selection_summary.is_none());
        assert!(inv.grid_scopes.is_none());
        assert!(inv.metadata_hashes.is_none());
        assert!(inv.view_prefs.is_none());
    }
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
