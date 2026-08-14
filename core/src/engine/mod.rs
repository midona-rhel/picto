//! Application engine — the single backend behavior entry point.
//!
//! `ApplicationEngine` sits above `LibraryDatabase` and owns:
//! - target resolution (EntityTarget → concrete ids or DB-backed bulk target)
//! - expansion rules (collection tags apply to member singles; status/folders include collections)
//! - projection rebuilding after writes
//! - state-change emission after projections settle
//!
//! Transport adapters call engine methods. They do not own behavior.
//! The engine calls LibraryDatabase. It does not access storage directly.

pub mod collections;
pub mod duplicates;
pub mod folders;
pub mod ingest;
pub mod media_io;
pub mod reads;
pub mod selection;
pub mod smart_folders;
pub mod system;
pub mod tags;
pub mod target;
pub mod views;
pub mod writes;

use std::sync::Arc;

use crate::db::types::*;
use crate::db::LibraryDatabase;
use crate::runtime_contract::change_builder::ChangeImpact;
use crate::runtime_contract::state_change::Domain;

/// The single application behavior boundary.
///
/// All app-level operations go through here. Transport code deserializes
/// input, calls an engine method, and serializes the result. Nothing else.
pub struct ApplicationEngine {
    db: Arc<LibraryDatabase>,
}

impl ApplicationEngine {
    pub fn new(db: Arc<LibraryDatabase>) -> Self {
        Self { db }
    }

    pub(crate) fn db(&self) -> &LibraryDatabase {
        &self.db
    }

    pub(crate) fn db_arc(&self) -> Arc<LibraryDatabase> {
        self.db.clone()
    }

    fn settled_sidebar_counts(
        &self,
    ) -> Option<crate::runtime_contract::state_change::SidebarCounts> {
        let counts = self.db.get_scope_counts().ok()?;
        Some(crate::runtime_contract::state_change::SidebarCounts {
            active: counts.active,
            inbox: counts.inbox,
            trash: counts.trash,
            uncategorized: counts.uncategorized,
            untagged: counts.untagged,
            duplicates: self.db.get_duplicate_count().unwrap_or(-1),
        })
    }

    /// Commit a write result: rebuild projections, then emit their settled state.
    /// Every engine write method calls this after the db write succeeds.
    fn commit_write(&self, change: &WriteChange) {
        if change.entities_deleted {
            self.db.bitmaps.remove_entities(&change.entity_ids);
        }

        let mut plan = crate::db::projection::compiler::CompilerPlan::default();
        if change.status_changed || change.entities_deleted {
            plan.rebuild_status = true;
            plan.rebuild_sidebar = true;
        }
        if change.entities_deleted {
            plan.rebuild_folder_sizes = true;
        }
        if change.tags_changed || change.tag_structure_changed {
            plan.dirty_tag_ids = change.dirty_tag_ids.clone();
            plan.rebuild_all_smart_folders = true;
            plan.rebuild_sidebar = true;
        }
        if change.metadata_changed {
            plan.rebuild_all_smart_folders = true;
            plan.rebuild_sidebar = true;
        }
        if change.tags_changed || change.tag_structure_changed {
            plan.rebuild_tag_derivatives = true;
        }
        if !change.dirty_folder_ids.is_empty() {
            plan.rebuild_sidebar = true;
            plan.rebuild_folder_sizes = true;
        }

        let smart_folders_rebuilt = plan.rebuild_status || plan.rebuild_all_smart_folders;
        if !plan.is_empty() {
            self.db.run_compiler(plan);
        }

        let mut impact = ChangeImpact::new();

        if !change.entity_hashes.is_empty() {
            impact.entity_hashes = Some(change.entity_hashes.clone());
        }
        if change.status_changed {
            impact.status_changed = Some(true);
            impact = impact.add_domain(Domain::Sidebar);
        }
        if change.tags_changed {
            impact.tags_changed = Some(true);
            impact = impact.add_domain(Domain::Sidebar);
            impact = impact.add_domain(Domain::SmartFolders);
        }
        if change.metadata_changed {
            impact = impact.all_smart_folder_scopes_changed();
        }
        if change.tag_structure_changed {
            impact.tag_structure_changed = Some(true);
            impact = impact.add_domain(Domain::Sidebar);
            impact = impact.add_domain(Domain::SmartFolders);
        }
        if !change.dirty_folder_ids.is_empty() {
            impact.folder_membership_changed = Some(change.dirty_folder_ids.clone());
            impact = impact.add_domain(Domain::Sidebar);
        }
        if change.entities_deleted {
            impact.status_changed = Some(true);
            impact = impact.add_domain(Domain::Sidebar);
        }
        if change.metadata_changed {
            impact.media_metadata_changed = Some(true);
        }
        if !change.extra_grid_scopes.is_empty() {
            impact.extra_grid_scopes = Some(change.extra_grid_scopes.clone());
        }

        if smart_folders_rebuilt {
            if let Ok(counts) = self.all_smart_folder_counts() {
                if !counts.is_empty() {
                    impact = impact
                        .add_domain(Domain::SmartFolders)
                        .smart_folder_ids(counts.iter().map(|(id, _)| *id).collect())
                        .smart_folder_counts(counts);
                }
            }
        }

        if change.status_changed || !change.dirty_folder_ids.is_empty() {
            if let Ok(nodes) = self.db.get_sidebar_tree() {
                let dirty_nodes: std::collections::HashSet<String> = change
                    .dirty_folder_ids
                    .iter()
                    .map(|id| format!("folder:{id}"))
                    .collect();
                let patches: Vec<_> = nodes
                    .into_iter()
                    .filter(|node| {
                        node.kind == "folder"
                            && (change.status_changed || dirty_nodes.contains(&node.node_id))
                    })
                    .map(
                        |node| crate::runtime_contract::state_change::SidebarNodePatch {
                            node_id: node.node_id,
                            count: Some(node.count),
                            freshness: Some("exact".into()),
                            ..Default::default()
                        },
                    )
                    .collect();
                if !patches.is_empty() {
                    impact.sidebar_node_patches = Some(patches);
                }
            }
        }

        // Scope counts are authoritative after the compiler settles. Duplicate
        // visibility depends on entity status, so status transitions must
        // publish the exact duplicate count just like deletions do.
        if let Some(mut counts) = self.settled_sidebar_counts() {
            if !change.status_changed && !change.entities_deleted {
                counts.duplicates = -1;
            }
            impact.sidebar_counts = Some(counts);
        }

        crate::events::emit_state_changed(&change.origin, impact);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::types::{EntityTarget, EntityTargetKind};
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[test]
    fn status_changes_emit_settled_duplicate_counts() {
        let temp = TempDir::new().expect("create test library");
        let db = Arc::new(LibraryDatabase::open(temp.path()).expect("open test library"));
        db.with_write(|conn| {
            conn.execute(
                "INSERT INTO media_entity
                    (entity_id, entity_hash, entity_kind, status, name, date_created, date_added, date_modified)
                 VALUES
                    (1, 'inbox-entity', 'single', 0, 'Inbox', '2026-08-14', '2026-08-14', '2026-08-14'),
                    (2, 'active-entity', 'single', 1, 'Active', '2026-08-14', '2026-08-14', '2026-08-14')",
                [],
            )?;
            conn.execute(
                "INSERT INTO media_file
                    (file_id, file_hash, mime_type, size_bytes, pixel_width, pixel_height, frame_count,
                     has_audio, perceptual_hash, date_added)
                 VALUES
                    (1, 'inbox-file', 'image/png', 1, 1, 1, 1, 0, 'inbox-phash', '2026-08-14'),
                    (2, 'active-file', 'image/png', 1, 1, 1, 1, 0, 'active-phash', '2026-08-14')",
                [],
            )?;
            conn.execute(
                "INSERT INTO single_media_entity (entity_id, file_id) VALUES (1, 1), (2, 2)",
                [],
            )?;
            conn.execute(
                "INSERT INTO duplicate (file_id_a, file_id_b, distance) VALUES (1, 2, 1)",
                [],
            )?;
            Ok(())
        })
        .expect("seed duplicate pair");

        let events = Arc::new(Mutex::new(Vec::<serde_json::Value>::new()));
        let captured_events = Arc::clone(&events);
        crate::events::set_event_callback(move |name, payload| {
            if name == crate::events::event_names::RUNTIME_STATE_CHANGED {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(payload) {
                    captured_events.lock().unwrap().push(event);
                }
            }
        });

        let engine = ApplicationEngine::new(db);
        let target = || EntityTarget {
            kind: EntityTargetKind::EntityHashes,
            entity_hashes: Some(vec!["inbox-entity".to_string()]),
            query: None,
            excluded_entity_hashes: None,
        };

        engine
            .set_entity_status(target(), 2)
            .expect("move duplicate candidate to trash");
        engine
            .set_entity_status(target(), 0)
            .expect("restore duplicate candidate to inbox");
        engine
            .resolve_duplicate_pair("keep_both", "inbox-entity", "active-entity", None)
            .expect("resolve duplicate pair");

        let events = events.lock().unwrap();
        let status_events: Vec<_> = events
            .iter()
            .filter(|event| event["origin"] == "set_entity_status")
            .collect();
        assert_eq!(status_events.len(), 2);
        assert_eq!(status_events[0]["sidebar_counts"]["duplicates"], 0);
        assert_eq!(status_events[1]["sidebar_counts"]["duplicates"], 1);
        let resolution_event = events
            .iter()
            .find(|event| event["origin"] == "resolve_duplicate_pair")
            .expect("duplicate resolution event");
        assert_eq!(resolution_event["sidebar_counts"]["duplicates"], 0);

        // Do not leave a test callback installed for later tests in this process.
        crate::events::set_event_callback(|_, _| {});
    }
}

/// What a write operation changed. Built from typed db change results,
/// consumed by `commit_write` to rebuild projections and emit their settled state.
#[derive(Debug, Clone)]
pub struct WriteChange {
    /// Origin label for the state-change event (e.g. "set_entity_status").
    pub origin: String,
    /// Top-level entity hashes affected.
    pub entity_hashes: Vec<String>,
    /// Entity ids affected (including descendants if expanded).
    pub entity_ids: Vec<i64>,
    /// Status changed (triggers status bitmap + sidebar rebuild).
    pub status_changed: bool,
    /// Tag ids that changed (triggers tag bitmap rebuild for these).
    pub dirty_tag_ids: Vec<i64>,
    /// Folder ids where membership changed.
    pub dirty_folder_ids: Vec<i64>,
    /// Tags were added or removed.
    pub tags_changed: bool,
    /// Tag structure changed (rename, delete, merge, alias, implication).
    pub tag_structure_changed: bool,
    /// Entities were deleted.
    pub entities_deleted: bool,
    /// Non-structural metadata changed (name, rating, notes, urls).
    pub metadata_changed: bool,
    /// Explicit grid scopes that should receive eager insertions.
    pub extra_grid_scopes: Vec<String>,
}

impl Default for WriteChange {
    fn default() -> Self {
        Self {
            origin: "engine".to_string(),
            entity_hashes: Vec::new(),
            entity_ids: Vec::new(),
            status_changed: false,
            dirty_tag_ids: Vec::new(),
            dirty_folder_ids: Vec::new(),
            tags_changed: false,
            tag_structure_changed: false,
            entities_deleted: false,
            metadata_changed: false,
            extra_grid_scopes: Vec::new(),
        }
    }
}

impl WriteChange {
    pub fn from_status(sc: &StatusChange) -> Self {
        Self {
            origin: "set_entity_status".to_string(),
            entity_hashes: sc.entity_hashes.clone(),
            entity_ids: sc.entity_ids.clone(),
            status_changed: true,
            ..Default::default()
        }
    }

    pub fn from_entity(ec: &EntityChange) -> Self {
        Self {
            origin: "patch_media_entities".to_string(),
            entity_hashes: ec.entity_hashes.clone(),
            entity_ids: ec.entity_ids.clone(),
            metadata_changed: true,
            ..Default::default()
        }
    }

    pub fn from_entity_delete(ec: &EntityChange) -> Self {
        Self {
            origin: "delete_entities".to_string(),
            entity_hashes: ec.entity_hashes.clone(),
            entity_ids: ec.entity_ids.clone(),
            entities_deleted: true,
            status_changed: true,
            ..Default::default()
        }
    }

    pub fn from_tag(tc: &TagChange) -> Self {
        Self {
            origin: "apply_entity_tags".to_string(),
            entity_ids: tc.entity_ids.clone(),
            dirty_tag_ids: tc.tag_ids.clone(),
            tags_changed: true,
            ..Default::default()
        }
    }

    pub fn from_folder(fc: &FolderMembershipChange) -> Self {
        Self {
            origin: "update_folder_membership".to_string(),
            entity_ids: fc.entity_ids.clone(),
            dirty_folder_ids: vec![fc.folder_id],
            ..Default::default()
        }
    }
}
