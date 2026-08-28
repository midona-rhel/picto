use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};

use crate::bitmap::BitmapKey;
use crate::model::{MediaId, RootId};
use crate::ordering::OrderOwnerKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagDefinitionState {
    pub tag_id: crate::TagId,
    pub stable_key: String,
    pub namespace_id: u32,
    pub full_name: String,
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

#[derive(Debug, Clone)]
pub enum SemanticChange {
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
    Compound(Vec<SemanticChange>),
}

impl SemanticChange {
    pub fn estimated_bytes(&self) -> usize {
        match self {
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
            Self::Compound(changes) => changes.iter().map(Self::estimated_bytes).sum(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub label: String,
    pub change: SemanticChange,
    pub estimated_bytes: usize,
}

impl HistoryEntry {
    pub fn new(label: impl Into<String>, change: SemanticChange) -> Self {
        let estimated_bytes = change.estimated_bytes();
        Self {
            label: label.into(),
            change,
            estimated_bytes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_label: Option<String>,
    pub redo_label: Option<String>,
    pub entries: usize,
    pub bytes: usize,
}

#[derive(Default)]
struct Stacks {
    undo: VecDeque<HistoryEntry>,
    redo: VecDeque<HistoryEntry>,
    bytes: usize,
}

#[derive(Default)]
pub struct SessionHistory {
    stacks: Mutex<Stacks>,
}

impl SessionHistory {
    pub fn push(&self, entry: HistoryEntry) -> bool {
        if entry.estimated_bytes > HISTORY_BYTE_LIMIT {
            return false;
        }
        let mut stacks = self.stacks.lock();
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

    pub fn state(&self) -> HistoryState {
        let stacks = self.stacks.lock();
        HistoryState {
            can_undo: !stacks.undo.is_empty(),
            can_redo: !stacks.redo.is_empty(),
            undo_label: stacks.undo.back().map(|entry| entry.label.clone()),
            redo_label: stacks.redo.back().map(|entry| entry.label.clone()),
            entries: stacks.undo.len() + stacks.redo.len(),
            bytes: stacks.bytes,
        }
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
