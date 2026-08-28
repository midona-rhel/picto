use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};

use crate::bitmap::BitmapKey;
use crate::model::{FileId, FolderId, MediaId, RootId, RootKind, SmartFolderId};
use crate::ordering::OrderOwnerKind;
use crate::projection::ProjectionSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagDefinitionState {
    pub tag_id: crate::TagId,
    pub stable_key: String,
    pub namespace_id: u32,
    pub full_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagNamespaceDefinitionState {
    pub namespace_id: crate::TagNamespaceId,
    pub stable_key: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedQueryChange {
    pub smart_folder_id: crate::SmartFolderId,
    pub before: crate::predicate::ViewQuerySpec,
    pub after: crate::predicate::ViewQuerySpec,
}

pub const HISTORY_ENTRY_LIMIT: usize = 100;
pub const HISTORY_BYTE_LIMIT: usize = 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RootTextState {
    pub root_id: RootId,
    pub name: String,
    pub notes: Option<String>,
    pub source_urls: Vec<String>,
    pub modified_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderDefinitionState {
    pub folder_id: FolderId,
    pub stable_key: String,
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub auto_tag_ids: Vec<u8>,
    pub cover_root_id: Option<RootId>,
    pub watch_path: Option<String>,
    pub watch_enabled: bool,
    pub watch_subfolders: bool,
    pub display_order: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SmartFolderDefinitionState {
    pub smart_folder_id: SmartFolderId,
    pub stable_key: String,
    pub parent_id: Option<SmartFolderId>,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub notes: Option<String>,
    pub view: crate::predicate::ViewQuerySpec,
    pub display_order: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuralRootState {
    pub root_id: RootId,
    pub stable_key: String,
    pub kind: RootKind,
    pub name: String,
    pub notes: Option<String>,
    pub source_urls_json: String,
    pub cover_media_id: MediaId,
    pub imported_at_ms: i64,
    pub captured_at_ms: Option<i64>,
    pub modified_at_ms: i64,
    pub media_count: u32,
    pub total_size_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct StructuralState {
    pub roots: Arc<Vec<StructuralRootState>>,
    pub projection: Arc<ProjectionSnapshot>,
}

#[derive(Debug, Clone)]
pub enum SemanticChange {
    AuxiliaryJson {
        table: &'static str,
        key: String,
        before: Option<String>,
        after: Option<String>,
        resource: &'static str,
    },
    Bitmap {
        key: BitmapKey,
        before: Arc<RoaringBitmap>,
        after: Arc<RoaringBitmap>,
    },
    Order {
        owner_kind: OrderOwnerKind,
        owner_id: u32,
        before: Arc<Vec<u32>>,
        after: Arc<Vec<u32>>,
    },
    RootText {
        before: Arc<Vec<RootTextState>>,
        after: Arc<Vec<RootTextState>>,
    },
    CollectionCover {
        root_id: RootId,
        before: MediaId,
        after: MediaId,
    },
    TagName {
        tag_id: crate::TagId,
        before: String,
        after: String,
    },
    TagNamespaceName {
        namespace_id: crate::TagNamespaceId,
        before: String,
        after: String,
    },
    TagNamespaceDefinition {
        before: Option<TagNamespaceDefinitionState>,
        after: Option<TagNamespaceDefinitionState>,
    },
    TagDefinition {
        before: Option<TagDefinitionState>,
        after: Option<TagDefinitionState>,
        before_members: Arc<RoaringBitmap>,
        after_members: Arc<RoaringBitmap>,
        queries: Arc<Vec<SavedQueryChange>>,
    },
    FolderAutoTags {
        folder_id: crate::FolderId,
        before: Arc<RoaringBitmap>,
        after: Arc<RoaringBitmap>,
    },
    FolderName {
        folder_id: crate::FolderId,
        before: String,
        after: String,
    },
    FolderDefinition {
        folder_id: FolderId,
        before: Option<Box<FolderDefinitionState>>,
        after: Option<Box<FolderDefinitionState>>,
    },
    SmartFolderDefinition {
        smart_folder_id: SmartFolderId,
        before: Option<Box<SmartFolderDefinitionState>>,
        after: Option<Box<SmartFolderDefinitionState>>,
    },
    RecentViews {
        before: Arc<Vec<(RootId, i64)>>,
        after: Arc<Vec<(RootId, i64)>>,
    },
    Structure {
        affected: Arc<RoaringBitmap>,
        before: StructuralState,
        after: StructuralState,
    },
    DuplicateResolution(crate::duplicate::DuplicateHistoryState),
    Compound(Vec<SemanticChange>),
}

impl SemanticChange {
    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::AuxiliaryJson {
                key, before, after, ..
            } => key.len()
                + before.as_deref().map(str::len).unwrap_or(0)
                + after.as_deref().map(str::len).unwrap_or(0)
                + 64,
            Self::Bitmap { before, after, .. } => {
                before.serialized_size() + after.serialized_size() + 64
            }
            Self::Order { before, after, .. } => (before.len() + after.len()) * 4 + 64,
            Self::RootText { before, after } => before
                .iter()
                .chain(after.iter())
                .map(|state| {
                    state.name.len()
                        + state.notes.as_deref().map(str::len).unwrap_or(0)
                        + state.source_urls.iter().map(String::len).sum::<usize>()
                        + 40
                })
                .sum(),
            Self::CollectionCover { .. } => 64,
            Self::TagName { before, after, .. } => before.len() + after.len() + 32,
            Self::TagNamespaceName { before, after, .. } => before.len() + after.len() + 32,
            Self::TagNamespaceDefinition { before, after } => before
                .iter()
                .chain(after.iter())
                .map(|state| state.stable_key.len() + state.display_name.len() + 32)
                .sum(),
            Self::TagDefinition {
                before,
                after,
                before_members,
                after_members,
                queries,
            } => {
                let definitions = before
                    .iter()
                    .chain(after.iter())
                    .map(|state| state.stable_key.len() + state.full_name.len() + 32)
                    .sum::<usize>();
                definitions
                    + before_members.serialized_size()
                    + after_members.serialized_size()
                    + queries
                        .iter()
                        .map(|change| {
                            serde_json::to_vec(&change.before).map_or(0, |value| value.len())
                                + serde_json::to_vec(&change.after).map_or(0, |value| value.len())
                                + 16
                        })
                        .sum::<usize>()
            }
            Self::FolderAutoTags { before, after, .. } => {
                before.serialized_size() + after.serialized_size() + 32
            }
            Self::FolderName { before, after, .. } => before.len() + after.len() + 32,
            Self::FolderDefinition { before, after, .. } => before
                .iter()
                .chain(after.iter())
                .map(|state| {
                    state.stable_key.len()
                        + state.name.len()
                        + state.icon.as_deref().map(str::len).unwrap_or(0)
                        + state.color.as_deref().map(str::len).unwrap_or(0)
                        + state.notes.as_deref().map(str::len).unwrap_or(0)
                        + state.auto_tag_ids.len()
                        + state.watch_path.as_deref().map(str::len).unwrap_or(0)
                        + 64
                })
                .sum(),
            Self::SmartFolderDefinition { before, after, .. } => before
                .iter()
                .chain(after.iter())
                .map(|state| {
                    state.stable_key.len()
                        + state.name.len()
                        + state.icon.as_deref().map(str::len).unwrap_or(0)
                        + state.color.as_deref().map(str::len).unwrap_or(0)
                        + state.notes.as_deref().map(str::len).unwrap_or(0)
                        + serde_json::to_vec(&state.view).map_or(0, |value| value.len())
                        + 64
                })
                .sum(),
            Self::RecentViews { before, after } => (before.len() + after.len()) * 16 + 32,
            Self::Structure {
                affected,
                before,
                after,
            } => {
                let roots = before
                    .roots
                    .iter()
                    .chain(after.roots.iter())
                    .map(|root| {
                        root.stable_key.len()
                            + root.name.len()
                            + root.notes.as_deref().map(str::len).unwrap_or(0)
                            + root.source_urls_json.len()
                            + 96
                    })
                    .sum::<usize>();
                roots
                    + affected.serialized_size()
                    + before.projection.estimated_bytes()
                    + after.projection.estimated_bytes()
            }
            Self::DuplicateResolution(state) => state.estimated_bytes(),
            Self::Compound(changes) => changes.iter().map(Self::estimated_bytes).sum(),
        }
    }

    fn protected_cleanup_files(&self, protected: &mut BTreeSet<FileId>) {
        match self {
            Self::DuplicateResolution(state) => {
                if let Some(file_id) = state.protected_file_id() {
                    protected.insert(file_id);
                }
            }
            Self::Compound(changes) => {
                for change in changes {
                    change.protected_cleanup_files(protected);
                }
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub entry_id: u64,
    pub command: String,
    pub label: String,
    pub change: SemanticChange,
    pub estimated_bytes: usize,
}

impl HistoryEntry {
    pub fn new(label: impl Into<String>, change: SemanticChange) -> Self {
        Self::for_command("library.change", label, change)
    }

    pub fn for_command(
        command: impl Into<String>,
        label: impl Into<String>,
        change: SemanticChange,
    ) -> Self {
        let estimated_bytes = change.estimated_bytes();
        Self {
            entry_id: 0,
            command: command.into(),
            label: label.into(),
            change,
            estimated_bytes,
        }
    }

    fn summary(&self) -> HistoryEntrySummary {
        HistoryEntrySummary {
            entry_id: self.entry_id,
            command: self.command.clone(),
            label: self.label.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntrySummary {
    pub entry_id: u64,
    pub command: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryState {
    pub undo: Option<HistoryEntrySummary>,
    pub redo: Option<HistoryEntrySummary>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub entries: usize,
    pub bytes: usize,
}

struct Stacks {
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    bytes: usize,
    protected_cleanup_files: BTreeSet<FileId>,
    next_entry_id: u64,
}

impl Default for Stacks {
    fn default() -> Self {
        Self {
            undo: VecDeque::new(),
            redo: VecDeque::new(),
            bytes: 0,
            protected_cleanup_files: BTreeSet::new(),
            next_entry_id: 1,
        }
    }
}

#[derive(Default)]
pub struct SessionHistory {
    stacks: Mutex<Stacks>,
}

impl SessionHistory {
    pub fn push(&self, mut entry: HistoryEntry) -> bool {
        if entry.estimated_bytes > HISTORY_BYTE_LIMIT {
            return false;
        }
        let mut stacks = self.stacks.lock();
        entry.entry_id = stacks.next_entry_id;
        stacks.next_entry_id = stacks.next_entry_id.saturating_add(1);
        let redo_bytes = stacks
            .redo
            .drain(..)
            .map(|entry| entry.estimated_bytes)
            .sum::<usize>();
        stacks.bytes = stacks.bytes.saturating_sub(redo_bytes);
        stacks.bytes += entry.estimated_bytes;
        stacks.undo.push_back(entry);
        while stacks.undo.len() > HISTORY_ENTRY_LIMIT || stacks.bytes > HISTORY_BYTE_LIMIT {
            if let Some(removed) = stacks.undo.pop_front() {
                stacks.bytes = stacks.bytes.saturating_sub(removed.estimated_bytes);
            }
        }
        stacks.refresh_cleanup_protections();
        true
    }

    pub fn take_undo(&self) -> Option<HistoryEntry> {
        self.stacks.lock().undo.pop_back()
    }

    pub fn complete_undo(&self, entry: HistoryEntry) {
        self.stacks.lock().redo.push_back(entry);
    }

    pub fn take_redo(&self) -> Option<HistoryEntry> {
        self.stacks.lock().redo.pop_back()
    }

    pub fn complete_redo(&self, entry: HistoryEntry) {
        self.stacks.lock().undo.push_back(entry);
    }

    pub fn restore_undo(&self, entry: HistoryEntry) {
        self.stacks.lock().undo.push_back(entry);
    }

    pub fn restore_redo(&self, entry: HistoryEntry) {
        self.stacks.lock().redo.push_back(entry);
    }

    pub fn clear(&self) {
        *self.stacks.lock() = Stacks::default();
    }

    pub fn protected_cleanup_files(&self) -> BTreeSet<FileId> {
        self.stacks.lock().protected_cleanup_files.clone()
    }

    pub fn state(&self) -> HistoryState {
        let stacks = self.stacks.lock();
        HistoryState {
            undo: stacks.undo.back().map(HistoryEntry::summary),
            redo: stacks.redo.back().map(HistoryEntry::summary),
            can_undo: !stacks.undo.is_empty(),
            can_redo: !stacks.redo.is_empty(),
            undo_label: stacks.undo.back().map(|entry| entry.label.clone()),
            redo_label: stacks.redo.back().map(|entry| entry.label.clone()),
            entries: stacks.undo.len() + stacks.redo.len(),
            bytes: stacks.bytes,
        }
    }
}

impl Stacks {
    fn refresh_cleanup_protections(&mut self) {
        let mut protected = BTreeSet::new();
        for entry in self.undo.iter().chain(self.redo.iter()) {
            entry.change.protected_cleanup_files(&mut protected);
        }
        self.protected_cleanup_files = protected;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::{BitmapDomain, BitmapKey};

    fn entry(label: &str, value: u32) -> HistoryEntry {
        HistoryEntry::new(
            label,
            SemanticChange::Bitmap {
                key: BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: value,
                },
                before: Arc::new(RoaringBitmap::new()),
                after: Arc::new([value].into_iter().collect()),
            },
        )
    }

    #[test]
    fn new_action_discards_redo_memory() {
        let history = SessionHistory::default();
        history.push(entry("one", 1));
        let first = history.take_undo().unwrap();
        history.complete_undo(first);
        let redo_bytes = history.state().bytes;
        assert!(redo_bytes > 0);

        history.push(entry("two", 2));
        let state = history.state();
        assert!(!state.can_redo);
        assert_eq!(state.entries, 1);
        assert!(state.bytes <= redo_bytes);
    }
}
