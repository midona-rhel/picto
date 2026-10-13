use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use parking_lot::Mutex;
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};

use crate::bitmap::BitmapKey;
use crate::model::{FileId, FolderId, MediaId, RootId, RootKind, SmartFolderId};
use crate::ordering::OrderOwnerKind;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub watch_lifecycle: crate::Lifecycle,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralMediaNoteState {
    pub media_id: MediaId,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralState {
    pub roots: Arc<Vec<StructuralRootState>>,
    pub media_notes: Arc<Vec<StructuralMediaNoteState>>,
    pub memberships: HashMap<BitmapKey, RoaringBitmap>,
    pub folders: BTreeMap<FolderId, Vec<StructuralFolderPosition>>,
    pub collections: BTreeMap<RootId, Vec<MediaId>>,
}

/// Only affected members are retained, anchored to their original neighbours.
/// Later imports into the same folder are not replaced when structure is undone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructuralFolderPosition {
    pub root_id: RootId,
    pub previous: Option<RootId>,
    pub next_unaffected: Option<RootId>,
    pub ordinal: usize,
}

impl StructuralState {
    fn membership_bytes(&self) -> usize {
        self.memberships
            .values()
            .map(|ids| ids.serialized_size() + 32)
            .sum::<usize>()
            + self
                .folders
                .values()
                .map(|positions| {
                    positions.capacity() * std::mem::size_of::<StructuralFolderPosition>() + 32
                })
                .sum::<usize>()
            + self
                .collections
                .values()
                .map(|ids| ids.capacity() * std::mem::size_of::<MediaId>() + 32)
                .sum::<usize>()
    }
}

/// A `media_item` row: id, name, notes, file.
pub type DeletedMediaRow = (u32, String, Option<String>, u32);
/// A `source_provenance` row: source key, source item key, media, text.
pub type DeletedProvenanceRow = (String, String, u32, Option<String>);

/// Everything a permanent delete removed, so undo can put it back exactly:
/// the roots' structure (rows, memberships, folder positions, collection
/// orders) and the rows that pointed at them. The files stay on disk while
/// the entry is in history (`protected_cleanup_files`).
/// Rows a merge pointed from the merged-away item (or a collection it
/// emptied) at the item that stays, so undo can point them back. Each move
/// names the row, where it pointed before and where it points after.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelinkState {
    /// Source posts: post, root before, root after.
    pub source_posts: Vec<(i64, u32, u32)>,
    /// Source items: item, media before, media after.
    pub source_items: Vec<(i64, u32, u32)>,
    /// Provenance rows moved to another media item.
    pub provenance: Vec<RelinkedProvenance>,
    /// Folder covers: folder, root before, root after.
    pub folder_covers: Vec<(u32, u32, u32)>,
    /// Stable-key aliases: key, the item it named before (none for a new
    /// alias), the item it names after.
    pub aliases: Vec<(String, Option<u32>, u32)>,
}

/// A `source_provenance` row moved from one media item to another. When the
/// target already had the same source, the moved row is dropped instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelinkedProvenance {
    pub source_key: String,
    pub source_item_key: String,
    pub source_text: Option<String>,
    pub from: u32,
    pub to: u32,
    pub target_had: bool,
}

impl RelinkState {
    pub fn is_empty(&self) -> bool {
        self.source_posts.is_empty()
            && self.source_items.is_empty()
            && self.provenance.is_empty()
            && self.folder_covers.is_empty()
            && self.aliases.is_empty()
    }

    fn estimated_bytes(&self) -> usize {
        (self.source_posts.len() + self.source_items.len() + self.folder_covers.len()) * 16
            + self
                .provenance
                .iter()
                .map(|row| {
                    row.source_key.len()
                        + row.source_item_key.len()
                        + row.source_text.as_deref().map_or(0, str::len)
                        + 24
                })
                .sum::<usize>()
            + self.aliases.iter().map(|(key, _, _)| key.len() + 16).sum::<usize>()
    }
}

#[derive(Debug, Clone)]
pub struct DeletedRootsState {
    pub roots: Arc<RoaringBitmap>,
    pub structure: StructuralState,
    /// `library_item` rows of the roots and their media: id, stable key, kind.
    pub items: Arc<Vec<(u32, String, u8)>>,
    /// Stable identities retained from earlier item or collection merges.
    pub aliases: Arc<Vec<(String, u32)>>,
    /// `media_item` rows: id, name, notes, file.
    pub media: Arc<Vec<DeletedMediaRow>>,
    /// The media that are images.
    pub image_media: Arc<RoaringBitmap>,
    /// `source_provenance` rows: source key, source item key, media, text.
    pub provenance: Arc<Vec<DeletedProvenanceRow>>,
    /// Source posts that pointed at a deleted root, with that root.
    pub source_posts: Arc<Vec<(i64, u32)>>,
    /// Source items that pointed at a deleted media item, with that media.
    pub source_items: Arc<Vec<(i64, u32)>>,
    /// Folders whose cover was a deleted root.
    pub folder_covers: Arc<Vec<(u32, u32)>>,
    pub recent_views: Arc<Vec<(u32, i64)>>,
    /// Cascaded queue rows, serialized for set-based restoration.
    pub work_items_json: Arc<String>,
    pub files: Arc<Vec<FileId>>,
}

impl DeletedRootsState {
    fn estimated_bytes(&self) -> usize {
        let roots = self
            .structure
            .roots
            .iter()
            .map(|root| root.stable_key.len() + root.name.len() + root.source_urls_json.len() + 96)
            .sum::<usize>();
        let items = self.items.iter().map(|(_, key, _)| key.len() + 16).sum::<usize>();
        let aliases = self.aliases.iter().map(|(key, _)| key.len() + 8).sum::<usize>();
        let media = self
            .media
            .iter()
            .map(|(_, name, notes, _)| name.len() + notes.as_deref().map_or(0, str::len) + 24)
            .sum::<usize>();
        let provenance = self
            .provenance
            .iter()
            .map(|(key, item, _, text)| key.len() + item.len() + text.as_deref().map_or(0, str::len) + 24)
            .sum::<usize>();
        roots
            + items
            + aliases
            + media
            + provenance
            + self.structure.membership_bytes()
            + (self.source_posts.len() + self.source_items.len() + self.folder_covers.len()) * 16
            + self.recent_views.len() * 16
            + self.work_items_json.len()
            + self.files.len() * 4
            + self.roots.serialized_size()
            + self.image_media.serialized_size()
    }
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
    MediaText {
        media_id: MediaId,
        before: (String, Option<String>),
        after: (String, Option<String>),
    },
    CollectionCover {
        root_id: RootId,
        before: MediaId,
        after: MediaId,
        before_notes: Option<String>,
        after_notes: Option<String>,
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
    /// Undo restores the deleted roots; redo deletes them again.
    PermanentDelete(Arc<DeletedRootsState>),
    /// Rows a merge moved onto the item that stays.
    Relink(Arc<RelinkState>),
    Compound(Vec<SemanticChange>),
}

impl SemanticChange {
    /// Undo replays only the members an entry changed, so a bitmap entry keeps
    /// just those: `before` holds the removed ids, `after` the added ones.
    /// Tagging 10 items with a 200k-member tag stores 10 ids, not 400k.
    pub(crate) fn keep_only_changed_members(&mut self) {
        match self {
            Self::Bitmap { before, after, .. } => {
                let changed = before.as_ref() ^ after.as_ref();
                if changed.len() != before.len() + after.len() {
                    *before = Arc::new(before.as_ref() & &changed);
                    *after = Arc::new(after.as_ref() & &changed);
                }
            }
            Self::Compound(changes) => {
                changes.iter_mut().for_each(Self::keep_only_changed_members);
            }
            _ => {}
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::AuxiliaryJson {
                key, before, after, ..
            } => {
                key.len()
                    + before.as_deref().map(str::len).unwrap_or(0)
                    + after.as_deref().map(str::len).unwrap_or(0)
                    + 64
            }
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
            Self::MediaText { before, after, .. } => before.0.len() + after.0.len() + before.1.as_deref().map_or(0, str::len) + after.1.as_deref().map_or(0, str::len) + 64,
            Self::CollectionCover {
                before_notes,
                after_notes,
                ..
            } => {
                before_notes.as_deref().map(str::len).unwrap_or(0)
                    + after_notes.as_deref().map(str::len).unwrap_or(0)
                    + 64
            }
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
                let media_notes = before
                    .media_notes
                    .iter()
                    .chain(after.media_notes.iter())
                    .map(|media| media.notes.as_deref().map(str::len).unwrap_or(0) + 16)
                    .sum::<usize>();
                roots
                    + media_notes
                    + affected.serialized_size()
                    + before.membership_bytes()
                    + after.membership_bytes()
            }
            Self::DuplicateResolution(state) => state.estimated_bytes(),
            Self::PermanentDelete(state) => state.estimated_bytes(),
            Self::Relink(state) => state.estimated_bytes(),
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
            Self::PermanentDelete(state) => protected.extend(state.files.iter().copied()),
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
    /// How many items the change touched (0 when it is not about items), so
    /// the renderer can describe it in plain words.
    pub count: u64,
    /// What the change set, where the command alone does not say: the status
    /// moved to, the rating given, or whether something was added or removed.
    pub detail: Option<String>,
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
        mut change: SemanticChange,
    ) -> Self {
        change.keep_only_changed_members();
        let estimated_bytes = change.estimated_bytes();
        Self {
            entry_id: 0,
            command: command.into(),
            label: label.into(),
            count: 0,
            detail: None,
            change,
            estimated_bytes,
        }
    }

    /// Say how many items the change touched and, optionally, what it set.
    pub fn about(mut self, count: u64, detail: Option<&str>) -> Self {
        self.count = count;
        self.detail = detail.map(str::to_owned);
        self
    }

    fn summary(&self) -> HistoryEntrySummary {
        HistoryEntrySummary {
            entry_id: self.entry_id,
            command: self.command.clone(),
            label: self.label.clone(),
            count: self.count,
            detail: self.detail.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntrySummary {
    pub entry_id: u64,
    pub command: String,
    /// A developer-facing fallback; the renderer words the change itself from
    /// `command`, `count` and `detail`.
    pub label: String,
    #[serde(default)]
    pub count: u64,
    #[serde(default)]
    pub detail: Option<String>,
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
    suppressed_threads: Mutex<HashMap<std::thread::ThreadId, usize>>,
}

impl SessionHistory {
    pub fn push(&self, mut entry: HistoryEntry) -> bool {
        if self
            .suppressed_threads
            .lock()
            .contains_key(&std::thread::current().id())
        {
            return false;
        }
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

    pub fn without_recording<T>(&self, operation: impl FnOnce() -> T) -> T {
        let thread_id = std::thread::current().id();
        *self.suppressed_threads.lock().entry(thread_id).or_default() += 1;
        let _suppression = RecordingSuppression {
            history: self,
            thread_id,
        };
        operation()
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

struct RecordingSuppression<'a> {
    history: &'a SessionHistory,
    thread_id: std::thread::ThreadId,
}

impl Drop for RecordingSuppression<'_> {
    fn drop(&mut self) {
        let mut threads = self.history.suppressed_threads.lock();
        let Some(depth) = threads.get_mut(&self.thread_id) else {
            return;
        };
        *depth -= 1;
        if *depth == 0 {
            threads.remove(&self.thread_id);
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

    #[test]
    fn recording_suppression_preserves_existing_undo_and_redo() {
        let history = SessionHistory::default();
        history.push(entry("one", 1));
        history.push(entry("two", 2));
        let second = history.take_undo().unwrap();
        history.complete_undo(second);
        let before = history.state();

        assert!(!history.without_recording(|| history.push(entry("automatic", 3))));

        assert_eq!(history.state(), before);
    }

    #[test]
    fn recording_suppression_is_nested_and_restored_after_unwind() {
        let history = SessionHistory::default();
        history.without_recording(|| {
            assert!(!history.without_recording(|| history.push(entry("nested", 1))));
            assert!(!history.push(entry("outer", 2)));
        });
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            history.without_recording(|| panic!("stop"));
        }));

        assert!(panic.is_err());
        assert!(history.push(entry("after", 3)));
        assert_eq!(history.state().undo.unwrap().label, "after");
    }

    #[test]
    fn recording_suppression_does_not_block_other_threads() {
        let history = Arc::new(SessionHistory::default());
        history.without_recording(|| {
            let other = Arc::clone(&history);
            assert!(std::thread::spawn(move || other.push(entry("other", 1)))
                .join()
                .unwrap());
            assert!(!history.push(entry("current", 2)));
        });

        assert_eq!(history.state().undo.unwrap().label, "other");
    }
}
