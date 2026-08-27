use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use roaring::RoaringBitmap;
use rusqlite::session::{invert_strm, ConflictAction, ConflictType, Session};
use rusqlite::{params, Transaction};
use serde::{Deserialize, Serialize};

use crate::app::{ItemKind, Lifecycle};

use super::{schema, Store, WritePriority};

const HISTORY_LIMIT: usize = 100;
const HISTORY_BYTE_LIMIT: usize = 256 * 1024 * 1024;

// Only canonical user-owned state belongs in application history. Runtime,
// subscription, credential, queue, revision, FTS, and history tables are
// deliberately absent.
const UNDOABLE_TABLES: &[&str] = &[
    "media_file",
    "library_item",
    "library_root",
    "root_metadata",
    "media_asset",
    "media_view",
    "tag",
    "folder",
    "smart_folder",
    "duplicate",
    "file_color",
    "view_pref",
    "setting",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryDescriptor {
    pub command: String,
    pub label: String,
    pub resources: Vec<String>,
    pub item_ids: Vec<i64>,
}

impl HistoryDescriptor {
    pub fn new(
        command: impl Into<String>,
        label: impl Into<String>,
        resources: Vec<String>,
        item_ids: Vec<i64>,
    ) -> Self {
        Self {
            command: command.into(),
            label: label.into(),
            resources,
            item_ids,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntrySummary {
    pub entry_id: i64,
    pub command: String,
    pub label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryState {
    pub undo: Option<HistoryEntrySummary>,
    pub redo: Option<HistoryEntrySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryMutation {
    pub entry: HistoryEntrySummary,
    pub resources: Vec<String>,
    pub item_ids: Vec<i64>,
    pub revision: u64,
    pub state: HistoryState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryDirection {
    Undo,
    Redo,
}

/// One relationship mutation represented by the roots whose membership
/// changes. The payload stores only actual changes, never the full selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticMembershipDelta {
    pub relation_id: i64,
    pub add: RoaringBitmap,
    pub remove: RoaringBitmap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticFolderOrder {
    pub folder_id: i64,
    pub item_ids: Vec<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticLifecycleDelta {
    pub inbox: RoaringBitmap,
    pub active: RoaringBitmap,
    pub trash: RoaringBitmap,
}

impl SemanticLifecycleDelta {
    pub fn roots(&self) -> RoaringBitmap {
        &(&self.inbox | &self.active) | &self.trash
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticRatingDelta {
    /// Roots whose rating becomes NULL.
    pub unrated: RoaringBitmap,
    /// Roots grouped by their exact rating.
    pub rated: Vec<(i64, RoaringBitmap)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticTagIdentityState {
    pub tag_id: i64,
    pub namespace: String,
    pub subtag: String,
    pub present: bool,
}

/// Exact directional inverse for tag identity, graph, and root ownership.
/// Dense root sets are grouped by their row values and Roaring-compressed.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticTagGraphDelta {
    pub identities: Vec<SemanticTagIdentityState>,
    pub projection_tags: Vec<SemanticMembershipDelta>,
    pub removed_tag_ids: Vec<i64>,
    pub affected_roots: RoaringBitmap,
    pub affected_tag_ids: Vec<i64>,
    pub dependency_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticGroupRoot {
    pub item_id: i64,
    pub item_key: String,
    pub kind: ItemKind,
    pub cover_media_item_id: Option<i64>,
    pub lifecycle: Lifecycle,
    pub sort_rank: Option<i64>,
    pub name: Option<String>,
    pub rating: Option<i64>,
    pub notes: Option<String>,
    pub source_urls_json: String,
    pub created_at: String,
    pub updated_at: String,
    pub folders: Vec<SemanticGroupFolder>,
    pub tags: Vec<SemanticGroupTag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticGroupFolder {
    pub folder_id: i64,
    pub position_rank: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticGroupTag {
    pub tag_id: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SemanticGroupMember {
    pub collection_id: i64,
    pub media_item_id: i64,
    pub position_rank: i64,
    pub present: bool,
}

/// Structural state required to restore group membership and the organization
/// inherited by roots created by detach/ungroup. Rows are staged into TEMP
/// tables and applied with set-based statements.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SemanticGroupDelta {
    pub remove_root_ids: RoaringBitmap,
    pub remove_item_ids: RoaringBitmap,
    pub roots: Vec<SemanticGroupRoot>,
    pub members: Vec<SemanticGroupMember>,
    pub folder_changes: Vec<SemanticMembershipDelta>,
    pub tag_changes: Vec<SemanticMembershipDelta>,
    pub rating_changes: SemanticRatingDelta,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SemanticHistoryPayload {
    Lifecycle(SemanticLifecycleDelta),
    Tags(Vec<SemanticMembershipDelta>),
    Folders(Vec<SemanticMembershipDelta>),
    FolderOrders(Vec<SemanticFolderOrder>),
    Ratings(SemanticRatingDelta),
    TagGraph(SemanticTagGraphDelta),
    Group(SemanticGroupDelta),
    Composite(Vec<SemanticHistoryPayload>),
}

impl SemanticHistoryPayload {
    pub(crate) fn lifecycle_target(&self) -> Option<(RoaringBitmap, RoaringBitmap)> {
        fn collect(
            payload: &SemanticHistoryPayload,
            roots: &mut RoaringBitmap,
            active: &mut RoaringBitmap,
        ) {
            match payload {
                SemanticHistoryPayload::Lifecycle(delta) => {
                    let changed = delta.roots();
                    *active -= &changed;
                    *active |= &delta.active;
                    *roots |= changed;
                }
                SemanticHistoryPayload::Composite(payloads) => {
                    for payload in payloads {
                        collect(payload, roots, active);
                    }
                }
                _ => {}
            }
        }

        let mut roots = RoaringBitmap::new();
        let mut active = RoaringBitmap::new();
        collect(self, &mut roots, &mut active);
        (!roots.is_empty()).then_some((roots, active))
    }

    pub fn estimated_bytes(&self) -> usize {
        match self {
            Self::Lifecycle(delta) => {
                bitmap_bytes(&delta.inbox)
                    + bitmap_bytes(&delta.active)
                    + bitmap_bytes(&delta.trash)
            }
            Self::Tags(changes) | Self::Folders(changes) => changes
                .iter()
                .map(|change| {
                    std::mem::size_of::<i64>()
                        + bitmap_bytes(&change.add)
                        + bitmap_bytes(&change.remove)
                })
                .sum(),
            Self::FolderOrders(changes) => changes
                .iter()
                .map(|change| {
                    std::mem::size_of::<i64>() + change.item_ids.len() * std::mem::size_of::<i64>()
                })
                .sum(),
            Self::Ratings(delta) => {
                bitmap_bytes(&delta.unrated)
                    + delta
                        .rated
                        .iter()
                        .map(|(_, roots)| std::mem::size_of::<i64>() + bitmap_bytes(roots))
                        .sum::<usize>()
            }
            Self::TagGraph(delta) => {
                delta
                    .identities
                    .iter()
                    .map(|identity| {
                        std::mem::size_of::<i64>()
                            + identity.namespace.len()
                            + identity.subtag.len()
                    })
                    .sum::<usize>()
                    + delta
                        .projection_tags
                        .iter()
                        .map(membership_bytes)
                        .sum::<usize>()
                    + delta.removed_tag_ids.len() * std::mem::size_of::<i64>()
                    + bitmap_bytes(&delta.affected_roots)
                    + delta.affected_tag_ids.len() * std::mem::size_of::<i64>()
                    + delta.dependency_keys.iter().map(String::len).sum::<usize>()
            }
            Self::Group(delta) => {
                bitmap_bytes(&delta.remove_root_ids)
                    + bitmap_bytes(&delta.remove_item_ids)
                    + delta.roots.iter().map(group_root_bytes).sum::<usize>()
                    + delta.members.len() * std::mem::size_of::<SemanticGroupMember>()
                    + delta
                        .folder_changes
                        .iter()
                        .map(membership_bytes)
                        .sum::<usize>()
                    + delta
                        .tag_changes
                        .iter()
                        .map(membership_bytes)
                        .sum::<usize>()
                    + bitmap_bytes(&delta.rating_changes.unrated)
                    + delta
                        .rating_changes
                        .rated
                        .iter()
                        .map(|(_, roots)| std::mem::size_of::<i64>() + bitmap_bytes(roots))
                        .sum::<usize>()
            }
            Self::Composite(payloads) => payloads.iter().map(Self::estimated_bytes).sum(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticHistoryRecord {
    pub undo: SemanticHistoryPayload,
    pub redo: SemanticHistoryPayload,
}

impl SemanticHistoryRecord {
    pub fn new(undo: SemanticHistoryPayload, redo: SemanticHistoryPayload) -> Self {
        Self { undo, redo }
    }

    fn payload(&self, direction: HistoryDirection) -> &SemanticHistoryPayload {
        match direction {
            HistoryDirection::Undo => &self.undo,
            HistoryDirection::Redo => &self.redo,
        }
    }

    pub fn estimated_bytes(&self) -> usize {
        self.undo.estimated_bytes() + self.redo.estimated_bytes()
    }
}

fn collect_group_replay_roots(payload: &SemanticHistoryPayload, roots: &mut Vec<i64>) {
    match payload {
        SemanticHistoryPayload::Group(delta) => {
            roots.extend(delta.roots.iter().map(|root| root.item_id));
        }
        SemanticHistoryPayload::Composite(payloads) => {
            for payload in payloads {
                collect_group_replay_roots(payload, roots);
            }
        }
        _ => {}
    }
}

pub enum HistoryProjectionRequest<'a> {
    Changeset,
    Semantic {
        payload: &'a SemanticHistoryPayload,
        /// Numeric root-summary projection changes for every root a Group
        /// replay re-created, read back from the canonical transaction so
        /// the in-memory numeric slices are restored exactly.
        group_summaries: Vec<crate::projection_v2::RootSummaryProjectionChange>,
    },
}

#[derive(Clone)]
enum StoredChange {
    Changeset(Arc<[u8]>),
    Semantic(Arc<SemanticHistoryRecord>),
}

impl StoredChange {
    fn estimated_bytes(&self) -> usize {
        match self {
            Self::Changeset(changeset) => changeset.len(),
            Self::Semantic(record) => record.estimated_bytes(),
        }
    }
}

#[derive(Clone)]
struct StoredEntry {
    summary: HistoryEntrySummary,
    change: StoredChange,
    resources: Vec<String>,
    item_ids: Vec<i64>,
}

pub(super) struct HistoryBuffer {
    entries: Vec<StoredEntry>,
    cursor: usize,
    next_entry_id: i64,
    byte_size: usize,
}

impl Default for HistoryBuffer {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            cursor: 0,
            next_entry_id: 1,
            byte_size: 0,
        }
    }
}

impl HistoryBuffer {
    fn push_changeset(
        &mut self,
        descriptor: HistoryDescriptor,
        changeset: Vec<u8>,
    ) -> HistoryEntrySummary {
        self.push(descriptor, StoredChange::Changeset(changeset.into()))
    }

    fn push_semantic(
        &mut self,
        descriptor: HistoryDescriptor,
        record: SemanticHistoryRecord,
    ) -> HistoryEntrySummary {
        self.push(descriptor, StoredChange::Semantic(Arc::new(record)))
    }

    fn push(&mut self, descriptor: HistoryDescriptor, change: StoredChange) -> HistoryEntrySummary {
        for entry in self.entries.drain(self.cursor..) {
            self.byte_size = self
                .byte_size
                .saturating_sub(entry.change.estimated_bytes());
        }

        let summary = HistoryEntrySummary {
            entry_id: self.next_entry_id,
            command: descriptor.command,
            label: descriptor.label,
        };
        self.next_entry_id = self.next_entry_id.saturating_add(1);
        self.byte_size = self.byte_size.saturating_add(change.estimated_bytes());
        self.entries.push(StoredEntry {
            summary: summary.clone(),
            change,
            resources: descriptor.resources,
            item_ids: descriptor.item_ids,
        });
        self.cursor = self.entries.len();

        while self.entries.len() > HISTORY_LIMIT || self.byte_size > HISTORY_BYTE_LIMIT {
            let removed = self.entries.remove(0);
            self.byte_size = self
                .byte_size
                .saturating_sub(removed.change.estimated_bytes());
            self.cursor = self.cursor.saturating_sub(1);
        }
        summary
    }

    fn entry(&self, direction: HistoryDirection) -> Option<StoredEntry> {
        match direction {
            HistoryDirection::Undo => self.cursor.checked_sub(1),
            HistoryDirection::Redo => (self.cursor < self.entries.len()).then_some(self.cursor),
        }
        .and_then(|index| self.entries.get(index).cloned())
    }

    fn finish(&mut self, direction: HistoryDirection, entry_id: i64) -> Result<(), String> {
        let expected = self.entry(direction).map(|entry| entry.summary.entry_id);
        if expected != Some(entry_id) {
            return Err("History changed while the operation was running".to_string());
        }
        match direction {
            HistoryDirection::Undo => self.cursor = self.cursor.saturating_sub(1),
            HistoryDirection::Redo => self.cursor += 1,
        }
        Ok(())
    }

    fn state(&self) -> HistoryState {
        HistoryState {
            undo: self
                .cursor
                .checked_sub(1)
                .and_then(|index| self.entries.get(index))
                .map(|entry| entry.summary.clone()),
            redo: self
                .entries
                .get(self.cursor)
                .map(|entry| entry.summary.clone()),
        }
    }
}

impl Store {
    pub(crate) fn undoable_transaction_settled<T, D, P: super::PreparedSettlement>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, Option<HistoryEntrySummary>), String> {
        let (value, revision, history, _) = self.undoable_transaction_inner(
            descriptor,
            |transaction| operation(transaction).map(|(value, delta)| (value, delta, true)),
            prepare,
            publish,
        )?;
        Ok((value, revision, history))
    }

    pub(crate) fn undoable_transaction_if_changed_settled<T, D, P: super::PreparedSettlement>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        self.undoable_transaction_inner(descriptor, operation, prepare, publish)
    }

    fn undoable_transaction_inner<T, D, P: super::PreparedSettlement>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        let total_started = Instant::now();
        let command = descriptor.command.clone();
        let _permit = self.writer_admission.acquire(WritePriority::Foreground)?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        crate::cloud::capture::begin_explicit_capture(&transaction)
            .map_err(|error| error.to_string())?;

        let mut session = Session::new(&transaction).map_err(|error| error.to_string())?;
        for table in UNDOABLE_TABLES {
            session
                .attach(Some(*table))
                .map_err(|error| error.to_string())?;
        }

        let operation_started = Instant::now();
        let (value, delta, changed) = operation(&transaction).map_err(|error| error.to_string())?;
        let operation_elapsed = operation_started.elapsed();
        let read_models_started = Instant::now();
        schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
        let read_models_elapsed = read_models_started.elapsed();
        let revision = if changed {
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?
        } else {
            schema::revision(&transaction).map_err(|error| error.to_string())?
        };

        let changeset_started = Instant::now();
        let changeset = if !changed || session.is_empty() {
            None
        } else {
            let mut changeset = Vec::new();
            session
                .changeset_strm(&mut changeset)
                .map_err(|error| error.to_string())?;
            Some(changeset)
        };
        drop(session);
        let changeset_elapsed = changeset_started.elapsed();

        let cloud_started = Instant::now();
        if changed {
            crate::cloud::capture::finish_explicit_capture(&transaction, changeset.as_deref())
                .map_err(|error| error.to_string())?;
        }
        let cloud_elapsed = cloud_started.elapsed();

        // History is process-local by contract. Keep the captured change in
        // memory and publish it only after the canonical transaction settles.
        let history_started = Instant::now();
        let pending_history = changeset.map(|changeset| (descriptor, changeset));
        let history_elapsed = history_started.elapsed();

        let prepare_started = Instant::now();
        let mut prepared = changed.then(|| prepare(delta)).transpose()?;
        if let Some(prepared) = prepared.as_mut() {
            prepared.persist(&transaction, revision)?;
        }
        let prepare_elapsed = prepare_started.elapsed();
        let publication_started = Instant::now();
        let _publication = self.consistency_write(std::panic::Location::caller())?;
        transaction.commit().map_err(|error| error.to_string())?;
        let commit_elapsed = publication_started.elapsed();
        let publish_started = Instant::now();
        if let Some(prepared) = prepared {
            publish(prepared);
        }
        let publish_elapsed = publish_started.elapsed();
        let history = if let Some((descriptor, changeset)) = pending_history {
            let mut history = self
                .history
                .lock()
                .map_err(|_| "Store history lock poisoned".to_string())?;
            Some(history.push_changeset(descriptor, changeset))
        } else {
            None
        };
        if total_started.elapsed() >= Duration::from_millis(100) {
            if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some() {
                eprintln!(
                    "store_stages command={} total_ms={:.2} operation_ms={:.2} read_models_ms={:.2} changeset_ms={:.2} cloud_ms={:.2} history_ms={:.2} prepare_ms={:.2} commit_ms={:.2} publish_ms={:.2}",
                    command,
                    total_started.elapsed().as_secs_f64() * 1_000.0,
                    operation_elapsed.as_secs_f64() * 1_000.0,
                    read_models_elapsed.as_secs_f64() * 1_000.0,
                    changeset_elapsed.as_secs_f64() * 1_000.0,
                    cloud_elapsed.as_secs_f64() * 1_000.0,
                    history_elapsed.as_secs_f64() * 1_000.0,
                    prepare_elapsed.as_secs_f64() * 1_000.0,
                    commit_elapsed.as_secs_f64() * 1_000.0,
                    publish_elapsed.as_secs_f64() * 1_000.0,
                );
            }
            tracing::warn!(
                target: "picto::store",
                command = %command,
                total_ms = total_started.elapsed().as_secs_f64() * 1_000.0,
                operation_ms = operation_elapsed.as_secs_f64() * 1_000.0,
                read_models_ms = read_models_elapsed.as_secs_f64() * 1_000.0,
                changeset_ms = changeset_elapsed.as_secs_f64() * 1_000.0,
                cloud_ms = cloud_elapsed.as_secs_f64() * 1_000.0,
                history_ms = history_elapsed.as_secs_f64() * 1_000.0,
                prepare_ms = prepare_elapsed.as_secs_f64() * 1_000.0,
                commit_ms = commit_elapsed.as_secs_f64() * 1_000.0,
                publish_ms = publish_elapsed.as_secs_f64() * 1_000.0,
                "Slow undoable transaction stages"
            );
        }
        Ok((value, revision, history, changed))
    }

    /// Run a broad undoable operation without constructing a row-level SQLite
    /// Session changeset. The operation returns compact directional semantic
    /// payloads alongside its already-prepared projection delta.
    pub(crate) fn semantic_undoable_transaction_if_changed_settled<
        T,
        D,
        P: super::PreparedSettlement,
    >(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(
            &Transaction<'_>,
        ) -> rusqlite::Result<(T, D, Option<SemanticHistoryRecord>, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        self.semantic_undoable_transaction_if_changed_settled_captured(
            descriptor,
            || (),
            |transaction, ()| operation(transaction),
            prepare,
            publish,
        )
    }

    /// Capture immutable caller state after writer admission so a broad SQL
    /// mutation can prepare exact deltas from the matching published view.
    pub(crate) fn semantic_undoable_transaction_if_changed_settled_captured<
        T,
        D,
        P: super::PreparedSettlement,
        C,
    >(
        &self,
        descriptor: HistoryDescriptor,
        capture: impl FnOnce() -> C,
        operation: impl FnOnce(
            &Transaction<'_>,
            C,
        ) -> rusqlite::Result<(T, D, Option<SemanticHistoryRecord>, bool)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        let total_started = Instant::now();
        let command = descriptor.command.clone();
        let _permit = self.writer_admission.acquire(WritePriority::Foreground)?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let cloud_capture = crate::cloud::capture::SemanticCapture::start(&transaction)
            .map_err(|error| error.to_string())?;
        let captured = capture();
        let operation_started = Instant::now();
        let (value, delta, semantic, changed) =
            operation(&transaction, captured).map_err(|error| error.to_string())?;
        let operation_elapsed = operation_started.elapsed();
        if changed && semantic.is_none() {
            return Err("A changed semantic transaction must supply undo and redo payloads".into());
        }
        if !changed && semantic.is_some() {
            return Err("An unchanged semantic transaction cannot create history".into());
        }
        let settlement_started = Instant::now();
        if changed {
            if let Some(record) = semantic.as_ref() {
                refresh_smart_for_semantic_payload(
                    &transaction,
                    record.payload(HistoryDirection::Redo),
                )?;
            }
            schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
        }
        let settlement_elapsed = settlement_started.elapsed();
        let cloud_started = Instant::now();
        let captured_cloud_operations = cloud_capture
            .finish(&transaction)
            .map_err(|error| error.to_string())?;
        let cloud_elapsed = cloud_started.elapsed();
        if !changed && captured_cloud_operations != 0 {
            return Err("An unchanged semantic transaction captured cloud mutations".into());
        }
        let revision = if changed {
            schema::increment_revision(&transaction).map_err(|error| error.to_string())?
        } else {
            schema::revision(&transaction).map_err(|error| error.to_string())?
        };

        let prepare_started = Instant::now();
        let mut prepared = changed.then(|| prepare(delta)).transpose()?;
        if let Some(prepared) = prepared.as_mut() {
            prepared.persist(&transaction, revision)?;
        }
        let prepare_elapsed = prepare_started.elapsed();
        let commit_started = Instant::now();
        let _publication = self.consistency_write(std::panic::Location::caller())?;
        transaction.commit().map_err(|error| error.to_string())?;
        let commit_elapsed = commit_started.elapsed();
        let publish_started = Instant::now();
        if let Some(prepared) = prepared {
            publish(prepared);
        }
        let publish_elapsed = publish_started.elapsed();
        let history_started = Instant::now();
        let entry = if let Some(semantic) = semantic {
            let mut history = self
                .history
                .lock()
                .map_err(|_| "Store history lock poisoned".to_string())?;
            Some(history.push_semantic(descriptor, semantic))
        } else {
            None
        };
        let history_elapsed = history_started.elapsed();
        if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some()
            && total_started.elapsed() >= Duration::from_millis(100)
        {
            eprintln!(
                "semantic_store_stages command={} total_ms={:.2} operation_ms={:.2} settlement_ms={:.2} cloud_ms={:.2} prepare_ms={:.2} commit_ms={:.2} publish_ms={:.2} history_ms={:.2}",
                command,
                total_started.elapsed().as_secs_f64() * 1_000.0,
                operation_elapsed.as_secs_f64() * 1_000.0,
                settlement_elapsed.as_secs_f64() * 1_000.0,
                cloud_elapsed.as_secs_f64() * 1_000.0,
                prepare_elapsed.as_secs_f64() * 1_000.0,
                commit_elapsed.as_secs_f64() * 1_000.0,
                publish_elapsed.as_secs_f64() * 1_000.0,
                history_elapsed.as_secs_f64() * 1_000.0,
            );
        }
        Ok((value, revision, entry, changed))
    }

    pub(crate) fn semantic_undoable_transaction_settled<T, D, P: super::PreparedSettlement>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&Transaction<'_>) -> rusqlite::Result<(T, D, SemanticHistoryRecord)>,
        prepare: impl FnOnce(D) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<(T, u64, Option<HistoryEntrySummary>), String> {
        let (value, revision, entry, _) = self.semantic_undoable_transaction_if_changed_settled(
            descriptor,
            |transaction| {
                let (value, delta, semantic) = operation(transaction)?;
                Ok((value, delta, Some(semantic), true))
            },
            prepare,
            publish,
        )?;
        Ok((value, revision, entry))
    }

    pub fn history_state(&self) -> Result<HistoryState, String> {
        let history = self
            .history
            .lock()
            .map_err(|_| "Store history lock poisoned".to_string())?;
        Ok(history.state())
    }

    pub(crate) fn apply_history_prepared<P: super::PreparedSettlement>(
        &self,
        direction: HistoryDirection,
        prepare_semantic: impl FnOnce(&Transaction<'_>, &SemanticHistoryPayload) -> Result<(), String>,
        prepare: impl FnOnce(HistoryProjectionRequest<'_>) -> Result<P, String>,
        publish: impl FnOnce(P),
    ) -> Result<HistoryMutation, String> {
        let total_started = Instant::now();
        let _permit = self.writer_admission.acquire(WritePriority::Foreground)?;
        let entry = self
            .history
            .lock()
            .map_err(|_| "Store history lock poisoned".to_string())?
            .entry(direction)
            .ok_or_else(|| match direction {
                HistoryDirection::Undo => "Nothing to undo".to_string(),
                HistoryDirection::Redo => "Nothing to redo".to_string(),
            })?;
        let mut connection = self
            .writer
            .lock()
            .map_err(|_| "Store writer lock poisoned".to_string())?;
        let is_changeset = matches!(&entry.change, StoredChange::Changeset(_));
        // Only the temporary changeset fallback needs foreign keys disabled.
        // Semantic history respects normal constraints and never runs a global
        // post-operation foreign-key scan.
        let foreign_keys_enabled: bool = is_changeset
            && connection
                .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
                .map_err(|error| error.to_string())?;
        if foreign_keys_enabled {
            connection
                .pragma_update(None, "foreign_keys", false)
                .map_err(|error| error.to_string())?;
        }
        let applied = (|| -> Result<u64, String> {
            let transaction = connection
                .transaction()
                .map_err(|error| error.to_string())?;
            let cloud_capture = crate::cloud::capture::SemanticCapture::start(&transaction)
                .map_err(|error| error.to_string())?;

            let payload_started = Instant::now();
            let projection_request = match &entry.change {
                StoredChange::Changeset(changeset) => {
                    let mut input = Cursor::new(changeset.as_ref());
                    let apply_result = match direction {
                        HistoryDirection::Undo => {
                            let mut inverse = Vec::new();
                            invert_strm(&mut input, &mut inverse)
                                .map_err(|error| error.to_string())?;
                            transaction.apply_strm(
                                &mut Cursor::new(inverse),
                                None::<fn(&str) -> bool>,
                                history_conflict_action,
                            )
                        }
                        HistoryDirection::Redo => transaction.apply_strm(
                            &mut input,
                            None::<fn(&str) -> bool>,
                            history_conflict_action,
                        ),
                    };
                    apply_result.map_err(|error| {
                        format!("History conflicts with newer library data: {error}")
                    })?;
                    // Smart-folder generation rows are rebuildable and are
                    // intentionally excluded from the changeset. Definition
                    // triggers create only targeted building generations;
                    // finish those generations before projection preparation
                    // so undo/redo never publishes stale membership.
                    settle_changeset_smart_generations(&transaction)?;
                    schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
                    HistoryProjectionRequest::Changeset
                }
                StoredChange::Semantic(record) => {
                    let payload = record.payload(direction);
                    transaction
                        .execute_batch("DROP TABLE IF EXISTS picto_prepared_lifecycle_delta;")
                        .map_err(|error| error.to_string())?;
                    prepare_semantic(&transaction, payload)?;
                    apply_semantic_payload(&transaction, payload)?;
                    refresh_smart_for_semantic_payload(&transaction, payload)?;
                    schema::refresh_read_models(&transaction).map_err(|error| error.to_string())?;
                    let mut replayed_roots = Vec::new();
                    collect_group_replay_roots(payload, &mut replayed_roots);
                    let group_summaries = crate::operations_v2::root_summary_changes_for_roots(
                        &transaction,
                        &replayed_roots,
                    )
                    .map_err(|error| error.to_string())?;
                    HistoryProjectionRequest::Semantic {
                        payload,
                        group_summaries,
                    }
                }
            };
            let payload_elapsed = payload_started.elapsed();

            let cloud_started = Instant::now();
            cloud_capture
                .finish(&transaction)
                .map_err(|error| error.to_string())?;
            let cloud_elapsed = cloud_started.elapsed();
            let revision =
                schema::increment_revision(&transaction).map_err(|error| error.to_string())?;
            let prepare_started = Instant::now();
            let mut prepared = prepare(projection_request)?;
            prepared.persist(&transaction, revision)?;
            let prepare_elapsed = prepare_started.elapsed();
            let commit_started = Instant::now();
            let _publication = self.consistency_write(std::panic::Location::caller())?;
            transaction.commit().map_err(|error| error.to_string())?;
            let commit_elapsed = commit_started.elapsed();
            let publish_started = Instant::now();
            publish(prepared);
            let publish_elapsed = publish_started.elapsed();
            if std::env::var_os("PICTO_TRACE_STORE_STAGES").is_some()
                && total_started.elapsed() >= Duration::from_millis(100)
            {
                eprintln!(
                    "history_store_stages direction={:?} total_ms={:.2} payload_ms={:.2} cloud_ms={:.2} prepare_ms={:.2} commit_ms={:.2} publish_ms={:.2}",
                    direction,
                    total_started.elapsed().as_secs_f64() * 1_000.0,
                    payload_elapsed.as_secs_f64() * 1_000.0,
                    cloud_elapsed.as_secs_f64() * 1_000.0,
                    prepare_elapsed.as_secs_f64() * 1_000.0,
                    commit_elapsed.as_secs_f64() * 1_000.0,
                    publish_elapsed.as_secs_f64() * 1_000.0,
                );
            }
            Ok(revision)
        })();
        let restore_foreign_keys = if foreign_keys_enabled {
            connection
                .pragma_update(None, "foreign_keys", true)
                .map_err(|error| error.to_string())
        } else {
            Ok(())
        };
        let revision = applied?;
        restore_foreign_keys?;
        let state = {
            let mut history = self
                .history
                .lock()
                .map_err(|_| "Store history lock poisoned".to_string())?;
            history.finish(direction, entry.summary.entry_id)?;
            history.state()
        };

        Ok(HistoryMutation {
            entry: entry.summary,
            resources: entry.resources,
            item_ids: entry.item_ids,
            revision,
            state,
        })
    }
}

fn settle_changeset_smart_generations(transaction: &Transaction<'_>) -> Result<(), String> {
    // Changesets are applied with foreign keys temporarily disabled so their
    // captured parent/child rows can be restored in dependency order. Smart
    // generations and dependencies are intentionally not captured, so remove
    // only read-model rows whose definition disappeared before completing the
    // targeted building generations created by definition/tag triggers.
    transaction
        .execute_batch(
            "DELETE FROM smart_folder_membership
             WHERE generation_id IN (
                 SELECT generation.generation_id
                 FROM smart_folder_generation generation
                 LEFT JOIN smart_folder folder
                   ON folder.smart_folder_id = generation.smart_folder_id
                 WHERE folder.smart_folder_id IS NULL
             );
             DELETE FROM smart_folder_generation
             WHERE NOT EXISTS (
                 SELECT 1 FROM smart_folder folder
                 WHERE folder.smart_folder_id = smart_folder_generation.smart_folder_id
             );
             DELETE FROM smart_folder_dependency
             WHERE NOT EXISTS (
                 SELECT 1 FROM smart_folder folder
                 WHERE folder.smart_folder_id = smart_folder_dependency.smart_folder_id
             );",
        )
        .map_err(|error| error.to_string())?;
    crate::smart_v2::refresh_materialized(transaction).map_err(|error| error.to_string())
}

fn apply_semantic_payload(
    transaction: &Transaction<'_>,
    payload: &SemanticHistoryPayload,
) -> Result<(), String> {
    match payload {
        SemanticHistoryPayload::Lifecycle(delta) => apply_lifecycle(transaction, delta),
        // Tag ownership is canonical bitmap state. The in-memory semantic
        // payload is applied to the private projection candidate and persisted
        // as one checksummed bitmap during prepared publication.
        SemanticHistoryPayload::Tags(_) => Ok(()),
        // Folder ownership is canonical bitmap state. The in-memory semantic
        // payload is applied to the private projection candidate and persisted
        // as one checksummed bitmap during prepared publication.
        SemanticHistoryPayload::Folders(changes) => validate_membership_changes(changes),
        SemanticHistoryPayload::FolderOrders(changes) => {
            for change in changes {
                let mut unique = std::collections::BTreeSet::new();
                if change.folder_id <= 0
                    || change.item_ids.iter().any(|item_id| *item_id <= 0)
                    || !change
                        .item_ids
                        .iter()
                        .all(|item_id| unique.insert(*item_id))
                {
                    return Err("Folder order history contains invalid item IDs".to_string());
                }
            }
            Ok(())
        }
        SemanticHistoryPayload::Ratings(delta) => apply_ratings(transaction, delta),
        SemanticHistoryPayload::TagGraph(delta) => apply_tag_graph(transaction, delta),
        SemanticHistoryPayload::Group(delta) => apply_group(transaction, delta),
        SemanticHistoryPayload::Composite(payloads) => {
            for payload in payloads {
                apply_semantic_payload(transaction, payload)?;
            }
            Ok(())
        }
    }
}

fn refresh_smart_for_semantic_payload(
    transaction: &Transaction<'_>,
    payload: &SemanticHistoryPayload,
) -> Result<(), String> {
    let mut roots = RoaringBitmap::new();
    let mut fields = std::collections::BTreeSet::<&'static str>::new();
    let mut tag_ids = std::collections::BTreeSet::<i64>::new();
    collect_semantic_smart_impact(payload, &mut roots, &mut fields, &mut tag_ids);
    if roots.is_empty() || !semantic_impact_has_smart_targets(transaction, &fields, &tag_ids)? {
        return Ok(());
    }
    crate::smart_v2::refresh_impacted_roots(
        transaction,
        &roots,
        &fields.into_iter().collect::<Vec<_>>(),
        &tag_ids.into_iter().collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string())
}

fn semantic_impact_has_smart_targets(
    transaction: &Transaction<'_>,
    fields: &std::collections::BTreeSet<&'static str>,
    tag_ids: &std::collections::BTreeSet<i64>,
) -> Result<bool, String> {
    if fields.contains("lifecycle") {
        return transaction
            .query_row("SELECT EXISTS(SELECT 1 FROM smart_folder)", [], |row| {
                row.get(0)
            })
            .map_err(|error| error.to_string());
    }
    for field in fields.iter().filter(|field| **field != "tags") {
        let dependency_kind = if ["rating", "name", "notes", "url"].contains(field) {
            "root_field"
        } else {
            "media_field"
        };
        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM smart_folder_dependency
                     WHERE dependency_kind = ?1 AND dependency_key = ?2
                 )",
                params![dependency_kind, *field],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if exists {
            return Ok(true);
        }
    }
    for tag_id in tag_ids {
        if crate::smart_v2::tag_affects_any_smart_folder(transaction, *tag_id)
            .map_err(|error| error.to_string())?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn collect_semantic_smart_impact(
    payload: &SemanticHistoryPayload,
    roots: &mut RoaringBitmap,
    fields: &mut std::collections::BTreeSet<&'static str>,
    tag_ids: &mut std::collections::BTreeSet<i64>,
) {
    match payload {
        SemanticHistoryPayload::Lifecycle(delta) => {
            *roots |= delta.roots();
            fields.insert("lifecycle");
        }
        // Prepared projection settlement re-evaluates tag-dependent smart
        // folders from the candidate bitmap. SQL read models are not involved.
        SemanticHistoryPayload::Tags(_) => {}
        SemanticHistoryPayload::Folders(_) => {}
        SemanticHistoryPayload::FolderOrders(_) => {}
        SemanticHistoryPayload::Ratings(delta) => {
            fields.insert("rating");
            *roots |= &delta.unrated;
            for (_, changed) in &delta.rated {
                *roots |= changed;
            }
        }
        SemanticHistoryPayload::TagGraph(delta) => {
            fields.insert("tags");
            *roots |= &delta.affected_roots;
            tag_ids.extend(delta.affected_tag_ids.iter().copied());
        }
        SemanticHistoryPayload::Group(delta) => {
            // Structural ownership changes can affect every media predicate,
            // so lifecycle targets every definition for only these roots.
            fields.insert("lifecycle");
            *roots |= &delta.remove_root_ids;
            for root in &delta.roots {
                if let Ok(item_id) = u32::try_from(root.item_id) {
                    roots.insert(item_id);
                }
            }
            for change in &delta.tag_changes {
                tag_ids.insert(change.relation_id);
            }
        }
        SemanticHistoryPayload::Composite(payloads) => {
            for payload in payloads {
                collect_semantic_smart_impact(payload, roots, fields, tag_ids);
            }
        }
    }
}

fn apply_tag_graph(
    transaction: &Transaction<'_>,
    delta: &SemanticTagGraphDelta,
) -> Result<(), String> {
    for identity in delta.identities.iter().filter(|identity| identity.present) {
        transaction
            .execute(
                "INSERT INTO tag(tag_id, namespace, subtag)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(tag_id) DO UPDATE SET
                   namespace = excluded.namespace,
                   subtag = excluded.subtag",
                params![identity.tag_id, identity.namespace, identity.subtag],
            )
            .map_err(|error| error.to_string())?;
    }

    for identity in delta.identities.iter().filter(|identity| !identity.present) {
        transaction
            .execute("DELETE FROM tag WHERE tag_id = ?1", [identity.tag_id])
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS picto_changed_tag_dependency_key (
                 dependency_key TEXT PRIMARY KEY
             ) WITHOUT ROWID;
             DELETE FROM picto_changed_tag_dependency_key;",
        )
        .map_err(|error| error.to_string())?;
    if !delta.dependency_keys.is_empty() {
        let json =
            serde_json::to_string(&delta.dependency_keys).map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO picto_changed_tag_dependency_key(dependency_key)
                 SELECT CAST(value AS TEXT) FROM json_each(?1)
                 WHERE TRUE
                 ON CONFLICT DO NOTHING",
                [json],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn apply_lifecycle(
    transaction: &Transaction<'_>,
    delta: &SemanticLifecycleDelta,
) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS picto_history_lifecycle (
                 root_item_id INTEGER PRIMARY KEY,
                 lifecycle TEXT NOT NULL
             ) WITHOUT ROWID;
             DELETE FROM picto_history_lifecycle;",
        )
        .map_err(|error| error.to_string())?;
    for (lifecycle, roots) in [
        ("inbox", &delta.inbox),
        ("active", &delta.active),
        ("trash", &delta.trash),
    ] {
        insert_bitmap_rows(
            transaction,
            "INSERT INTO picto_history_lifecycle(root_item_id, lifecycle)
             SELECT CAST(value AS INTEGER), ?1 FROM json_each(?2)",
            &[rusqlite::types::Value::Text(lifecycle.to_string())],
            roots,
        )?;
    }
    transaction
        .execute_batch(
            "UPDATE projection_write_control
             SET suppress_root_summary = 1,
                 suppress_folder_summary = 1,
                 suppress_tag_summary = 1
             WHERE singleton = 1;",
        )
        .map_err(|error| error.to_string())?;
    let changed = transaction
        .execute(
            "UPDATE library_root
             SET lifecycle = (
                 SELECT staged.lifecycle
                 FROM picto_history_lifecycle staged
                 WHERE staged.root_item_id = library_root.item_id
             )
             WHERE item_id IN (SELECT root_item_id FROM picto_history_lifecycle)",
            [],
        )
        .map_err(|error| error.to_string())?;
    let expected = delta.roots().len() as usize;
    if changed != expected {
        return Err(format!(
            "Semantic lifecycle history expected {expected} roots but changed {changed}"
        ));
    }
    transaction
        .execute_batch(
            "UPDATE lifecycle_summary
             SET root_count = root_count
                     - COALESCE((
                         SELECT COUNT(*)
                         FROM picto_history_lifecycle staged
                         JOIN root_summary current
                           ON current.root_item_id = staged.root_item_id
                         WHERE current.lifecycle = lifecycle_summary.lifecycle
                     ), 0)
                     + COALESCE((
                         SELECT COUNT(*)
                         FROM picto_history_lifecycle staged
                         WHERE staged.lifecycle = lifecycle_summary.lifecycle
                     ), 0),
                 media_count = media_count
                     - COALESCE((
                         SELECT SUM(current.media_count)
                         FROM picto_history_lifecycle staged
                         JOIN root_summary current
                           ON current.root_item_id = staged.root_item_id
                         WHERE current.lifecycle = lifecycle_summary.lifecycle
                     ), 0)
                     + COALESCE((
                         SELECT SUM(current.media_count)
                         FROM picto_history_lifecycle staged
                         JOIN root_summary current
                           ON current.root_item_id = staged.root_item_id
                         WHERE staged.lifecycle = lifecycle_summary.lifecycle
                     ), 0),
                 total_size_bytes = total_size_bytes
                     - COALESCE((
                         SELECT SUM(current.total_size_bytes)
                         FROM picto_history_lifecycle staged
                         JOIN root_summary current
                           ON current.root_item_id = staged.root_item_id
                         WHERE current.lifecycle = lifecycle_summary.lifecycle
                     ), 0)
                     + COALESCE((
                         SELECT SUM(current.total_size_bytes)
                         FROM picto_history_lifecycle staged
                         JOIN root_summary current
                           ON current.root_item_id = staged.root_item_id
                         WHERE staged.lifecycle = lifecycle_summary.lifecycle
                     ), 0);",
        )
        .map_err(|error| error.to_string())?;
    let has_prepared_summary = transaction
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM sqlite_temp_master
                 WHERE type = 'table' AND name = 'picto_prepared_lifecycle_delta'
             )",
            [],
            |row| row.get::<_, bool>(0),
        )
        .map_err(|error| error.to_string())?;
    if has_prepared_summary {
        transaction
            .execute_batch(
                "UPDATE folder_summary
                 SET visible_root_count = visible_root_count + (
                         SELECT delta.root_count FROM picto_lifecycle_folder_delta delta
                         WHERE delta.folder_id = folder_summary.folder_id
                     ),
                     media_count = media_count + (
                         SELECT delta.media_count FROM picto_lifecycle_folder_delta delta
                         WHERE delta.folder_id = folder_summary.folder_id
                     ),
                     total_size_bytes = total_size_bytes + (
                         SELECT delta.total_size_bytes FROM picto_lifecycle_folder_delta delta
                         WHERE delta.folder_id = folder_summary.folder_id
                     )
                 WHERE folder_id IN (SELECT folder_id FROM picto_lifecycle_folder_delta);
                 UPDATE tag_summary
                 SET visible_root_count = visible_root_count + (
                     SELECT delta.visible_root_count FROM picto_lifecycle_tag_delta delta
                     WHERE delta.tag_id = tag_summary.tag_id
                 )
                 WHERE tag_id IN (SELECT tag_id FROM picto_lifecycle_tag_delta);",
            )
            .map_err(|error| error.to_string())?;
    }
    transaction
        .execute(
            "UPDATE root_summary
             SET lifecycle = (
                 SELECT staged.lifecycle
                 FROM picto_history_lifecycle staged
                 WHERE staged.root_item_id = root_summary.root_item_id
             )
             WHERE root_item_id IN (SELECT root_item_id FROM picto_history_lifecycle)",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE projection_write_control
             SET suppress_root_summary = 0,
                 suppress_folder_summary = 0,
                 suppress_tag_summary = 0
             WHERE singleton = 1",
            [],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_membership_changes(changes: &[SemanticMembershipDelta]) -> Result<(), String> {
    for change in changes {
        if !(&change.add & &change.remove).is_empty() {
            return Err(format!(
                "Semantic membership {} adds and removes the same root",
                change.relation_id
            ));
        }
    }
    Ok(())
}

fn apply_ratings(transaction: &Transaction<'_>, delta: &SemanticRatingDelta) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS picto_history_rating (
                 root_item_id INTEGER PRIMARY KEY,
                 rating INTEGER
             ) WITHOUT ROWID;
             DELETE FROM picto_history_rating;",
        )
        .map_err(|error| error.to_string())?;
    insert_bitmap_rows(
        transaction,
        "INSERT INTO picto_history_rating(root_item_id, rating)
         SELECT CAST(value AS INTEGER), NULL FROM json_each(?1)",
        &[],
        &delta.unrated,
    )?;
    for (rating, roots) in &delta.rated {
        insert_bitmap_rows(
            transaction,
            "INSERT INTO picto_history_rating(root_item_id, rating)
             SELECT CAST(value AS INTEGER), ?1 FROM json_each(?2)",
            &[rusqlite::types::Value::Integer(*rating)],
            roots,
        )?;
    }
    transaction
        .execute(
            "UPDATE projection_write_control SET suppress_root_summary = 1 WHERE singleton = 1",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE root_metadata
             SET rating = (
                 SELECT staged.rating FROM picto_history_rating staged
                 WHERE staged.root_item_id = root_metadata.root_item_id
             )
             WHERE root_item_id IN (SELECT root_item_id FROM picto_history_rating)",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE root_summary
             SET sort_rating = (
                 SELECT staged.rating FROM picto_history_rating staged
                 WHERE staged.root_item_id = root_summary.root_item_id
             )
             WHERE root_item_id IN (SELECT root_item_id FROM picto_history_rating)",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "UPDATE projection_write_control SET suppress_root_summary = 0 WHERE singleton = 1",
            [],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn insert_bitmap_rows(
    transaction: &Transaction<'_>,
    sql: &str,
    prefix: &[rusqlite::types::Value],
    bitmap: &RoaringBitmap,
) -> Result<(), String> {
    if bitmap.is_empty() {
        return Ok(());
    }
    let json = i64_json(bitmap.iter().map(i64::from));
    let parameters = prefix
        .iter()
        .cloned()
        .chain(std::iter::once(rusqlite::types::Value::Text(json)));
    transaction
        .execute(sql, rusqlite::params_from_iter(parameters))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn i64_json(values: impl IntoIterator<Item = i64>) -> String {
    let iterator = values.into_iter();
    let (lower, _) = iterator.size_hint();
    let mut json = String::with_capacity(lower.saturating_mul(8).saturating_add(2));
    json.push('[');
    for (index, value) in iterator.enumerate() {
        if index != 0 {
            json.push(',');
        }
        json.push_str(&value.to_string());
    }
    json.push(']');
    json
}

fn apply_group(transaction: &Transaction<'_>, delta: &SemanticGroupDelta) -> Result<(), String> {
    transaction
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS picto_history_remove_root (
                 item_id INTEGER PRIMARY KEY
             ) WITHOUT ROWID;
             CREATE TEMP TABLE IF NOT EXISTS picto_history_remove_item (
                 item_id INTEGER PRIMARY KEY
             ) WITHOUT ROWID;
             CREATE TEMP TABLE IF NOT EXISTS picto_history_group_root (
                 item_id INTEGER PRIMARY KEY,
                 item_key TEXT NOT NULL,
                 kind TEXT NOT NULL,
                 cover_media_item_id INTEGER,
                 lifecycle TEXT NOT NULL,
                 sort_rank INTEGER,
                 name TEXT,
                 rating INTEGER,
                 notes TEXT,
                 source_urls_json TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             ) WITHOUT ROWID;
             CREATE TEMP TABLE IF NOT EXISTS picto_history_group_member (
                 collection_id INTEGER NOT NULL,
                 media_item_id INTEGER NOT NULL,
                 position_rank INTEGER NOT NULL,
                 present INTEGER NOT NULL,
                 PRIMARY KEY (collection_id, media_item_id)
             ) WITHOUT ROWID;
             DELETE FROM picto_history_remove_root;
             DELETE FROM picto_history_remove_item;
             DELETE FROM picto_history_group_root;
             DELETE FROM picto_history_group_member;",
        )
        .map_err(|error| error.to_string())?;
    stage_group_payload(transaction, delta)?;
    transaction
        .execute(
            "DELETE FROM library_root
             WHERE item_id IN (SELECT item_id FROM picto_history_remove_root)",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute(
            "DELETE FROM library_item
             WHERE item_id IN (SELECT item_id FROM picto_history_remove_item)",
            [],
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(
            "INSERT INTO library_item
                 (item_id, item_key, kind, cover_media_item_id, created_at, updated_at)
             SELECT item_id, item_key, kind, cover_media_item_id, created_at, updated_at
             FROM picto_history_group_root WHERE 1
             ON CONFLICT(item_id) DO UPDATE SET
                 item_key = excluded.item_key,
                 kind = excluded.kind,
                 cover_media_item_id = excluded.cover_media_item_id,
                 updated_at = excluded.updated_at;
             INSERT INTO library_root(item_id, lifecycle, sort_rank)
             SELECT item_id, lifecycle, sort_rank FROM picto_history_group_root WHERE 1
             ON CONFLICT(item_id) DO UPDATE SET
                 lifecycle = excluded.lifecycle,
                 sort_rank = excluded.sort_rank;
             INSERT INTO root_metadata
                 (root_item_id, name, rating, notes, source_urls_json, updated_at)
             SELECT item_id, name, rating, notes, source_urls_json, updated_at
             FROM picto_history_group_root WHERE 1
             ON CONFLICT(root_item_id) DO UPDATE SET
                 name = excluded.name,
                 rating = excluded.rating,
                 notes = excluded.notes,
                 source_urls_json = excluded.source_urls_json,
                 updated_at = excluded.updated_at;",
        )
        .map_err(|error| error.to_string())?;
    transaction
        .execute_batch(
            "INSERT INTO root_summary (
                 root_item_id, lifecycle, kind, cover_media_item_id, media_count,
                 total_size_bytes, imported_at, captured_at, sort_rating,
                 sort_name, updated_at
             )
             SELECT root.item_id, root.lifecycle, root.kind,
                    root.cover_media_item_id, COUNT(member.media_item_id),
                    COALESCE(SUM(file.size_bytes), 0), MAX(asset.imported_at),
                    MAX(asset.captured_at), root.rating, root.name, root.updated_at
             FROM picto_history_group_root root
             JOIN picto_history_group_member member
               ON member.collection_id = root.item_id AND member.present = 1
             JOIN media_asset asset ON asset.item_id = member.media_item_id
             JOIN media_file file ON file.file_id = asset.file_id
             WHERE root.kind = 'collection'
             GROUP BY root.item_id
             ON CONFLICT(root_item_id) DO UPDATE SET
                 lifecycle = excluded.lifecycle,
                 kind = excluded.kind,
                 cover_media_item_id = excluded.cover_media_item_id,
                 media_count = excluded.media_count,
                 total_size_bytes = excluded.total_size_bytes,
                 imported_at = excluded.imported_at,
                 captured_at = excluded.captured_at,
                 sort_rating = excluded.sort_rating,
                 sort_name = excluded.sort_name,
                 updated_at = excluded.updated_at;
             INSERT INTO root_summary (
                 root_item_id, lifecycle, kind, cover_media_item_id, media_count,
                 total_size_bytes, imported_at, captured_at, sort_rating,
                 sort_name, updated_at
             )
             SELECT root.item_id, root.lifecycle, root.kind,
                    COALESCE(root.cover_media_item_id, root.item_id), 1,
                    COALESCE(file.size_bytes, 0), asset.imported_at,
                    asset.captured_at, root.rating,
                    COALESCE(root.name, asset.name), root.updated_at
             FROM picto_history_group_root root
             LEFT JOIN media_asset asset ON asset.item_id = COALESCE(
                 root.cover_media_item_id, root.item_id
             )
             LEFT JOIN media_file file ON file.file_id = asset.file_id
             WHERE root.kind = 'media'
             ON CONFLICT(root_item_id) DO UPDATE SET
                 lifecycle = excluded.lifecycle,
                 kind = excluded.kind,
                 cover_media_item_id = excluded.cover_media_item_id,
                 media_count = excluded.media_count,
                 total_size_bytes = excluded.total_size_bytes,
                 imported_at = excluded.imported_at,
                 captured_at = excluded.captured_at,
                 sort_rating = excluded.sort_rating,
                 sort_name = excluded.sort_name,
                 updated_at = excluded.updated_at;",
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn stage_group_payload(
    transaction: &Transaction<'_>,
    delta: &SemanticGroupDelta,
) -> Result<(), String> {
    {
        let mut remove_root = transaction
            .prepare_cached("INSERT INTO picto_history_remove_root(item_id) VALUES (?1)")
            .map_err(|error| error.to_string())?;
        for item_id in &delta.remove_root_ids {
            remove_root
                .execute([i64::from(item_id)])
                .map_err(|error| error.to_string())?;
        }
        let mut remove_item = transaction
            .prepare_cached("INSERT INTO picto_history_remove_item(item_id) VALUES (?1)")
            .map_err(|error| error.to_string())?;
        for item_id in &delta.remove_item_ids {
            remove_item
                .execute([i64::from(item_id)])
                .map_err(|error| error.to_string())?;
        }
    }
    let mut insert_root = transaction
        .prepare_cached(
            "INSERT INTO picto_history_group_root
                 (item_id, item_key, kind, cover_media_item_id, lifecycle, sort_rank,
                  name, rating, notes, source_urls_json, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        )
        .map_err(|error| error.to_string())?;
    for root in &delta.roots {
        insert_root
            .execute(params![
                root.item_id,
                root.item_key,
                match root.kind {
                    ItemKind::Media => "media",
                    ItemKind::Collection => "collection",
                },
                root.cover_media_item_id,
                root.lifecycle.as_str(),
                root.sort_rank,
                root.name,
                root.rating,
                root.notes,
                root.source_urls_json,
                root.created_at,
                root.updated_at,
            ])
            .map_err(|error| error.to_string())?;
    }
    let mut insert_member = transaction
        .prepare_cached(
            "INSERT INTO picto_history_group_member
                 (collection_id, media_item_id, position_rank, present)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(|error| error.to_string())?;
    for member in &delta.members {
        insert_member
            .execute(params![
                member.collection_id,
                member.media_item_id,
                member.position_rank,
                member.present
            ])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn bitmap_bytes(bitmap: &RoaringBitmap) -> usize {
    bitmap.serialized_size()
}

fn membership_bytes(change: &SemanticMembershipDelta) -> usize {
    std::mem::size_of::<SemanticMembershipDelta>()
        + bitmap_bytes(&change.add)
        + bitmap_bytes(&change.remove)
}

fn group_root_bytes(root: &SemanticGroupRoot) -> usize {
    std::mem::size_of::<SemanticGroupRoot>()
        + root.item_key.len()
        + root.name.as_ref().map_or(0, String::len)
        + root.notes.as_ref().map_or(0, String::len)
        + root.source_urls_json.len()
        + root.created_at.len()
        + root.updated_at.len()
        + root.folders.len() * std::mem::size_of::<SemanticGroupFolder>()
        + root.tags.len() * std::mem::size_of::<SemanticGroupTag>()
}

fn history_conflict_action(
    _conflict: ConflictType,
    _: rusqlite::session::ChangesetItem,
) -> ConflictAction {
    ConflictAction::SQLITE_CHANGESET_ABORT
}

#[cfg(test)]
mod tests {
    use roaring::RoaringBitmap;

    use super::{
        HistoryDescriptor, HistoryDirection, SemanticHistoryPayload, SemanticHistoryRecord,
        SemanticLifecycleDelta, SemanticMembershipDelta, SemanticRatingDelta,
        SemanticTagGraphDelta,
    };
    use crate::store::Store;

    fn rename_descriptor(item_id: i64) -> HistoryDescriptor {
        HistoryDescriptor::new(
            "items.rename",
            "Rename item",
            vec!["library".to_string(), format!("item:{item_id}")],
            vec![item_id],
        )
    }

    fn media_fixture(store: &Store) -> i64 {
        store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO media_file
                         (file_hash, mime_type, size_bytes, created_at)
                     VALUES ('history-hash', 'image/png', 10, 'now')",
                    [],
                )?;
                let file_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO library_item
                         (item_key, kind, created_at, updated_at)
                     VALUES ('history-item', 'media', 'now', 'now')",
                    [],
                )?;
                let item_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO media_asset
                         (item_id, file_id, name, imported_at, updated_at)
                     VALUES (?1, ?2, 'Before', 'now', 'now')",
                    rusqlite::params![item_id, file_id],
                )?;
                transaction.execute(
                    "INSERT INTO library_root (item_id, lifecycle) VALUES (?1, 'active')",
                    [item_id],
                )?;
                Ok(item_id)
            })
            .unwrap()
            .0
    }

    fn name(store: &Store, item_id: i64) -> String {
        store
            .read(|connection| {
                connection.query_row(
                    "SELECT name FROM media_asset WHERE item_id = ?1",
                    [item_id],
                    |row| row.get(0),
                )
            })
            .unwrap()
    }

    #[test]
    fn captures_undoes_and_redoes_one_transaction() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);

        let (_, _, entry) = store
            .undoable_transaction_settled(
                rename_descriptor(item_id),
                |transaction| {
                    transaction.execute(
                        "UPDATE media_asset SET name = 'After' WHERE item_id = ?1",
                        [item_id],
                    )?;
                    Ok(((), ()))
                },
                |()| Ok(()),
                |()| {},
            )
            .unwrap();
        assert_eq!(entry.unwrap().label, "Rename item");
        assert_eq!(name(&store, item_id), "After");
        assert_eq!(
            store.history_state().unwrap().undo.unwrap().label,
            "Rename item"
        );

        let undone = store
            .apply_history_prepared(HistoryDirection::Undo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_eq!(undone.item_ids, vec![item_id]);
        assert_eq!(name(&store, item_id), "Before");
        assert!(undone.state.undo.is_none());
        assert_eq!(undone.state.redo.unwrap().label, "Rename item");

        let redone = store
            .apply_history_prepared(HistoryDirection::Redo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_eq!(name(&store, item_id), "After");
        assert!(redone.state.redo.is_none());
    }

    #[test]
    fn aborts_conflicting_undo_without_moving_history() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);
        store
            .undoable_transaction_settled(
                rename_descriptor(item_id),
                |transaction| {
                    transaction.execute(
                        "UPDATE media_asset SET name = 'After' WHERE item_id = ?1",
                        [item_id],
                    )?;
                    Ok(((), ()))
                },
                |()| Ok(()),
                |()| {},
            )
            .unwrap();
        store
            .transaction(|transaction| {
                transaction.execute(
                    "UPDATE media_asset SET name = 'Background' WHERE item_id = ?1",
                    [item_id],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(store
            .apply_history_prepared(HistoryDirection::Undo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .is_err());
        assert_eq!(name(&store, item_id), "Background");
        assert!(store.history_state().unwrap().undo.is_some());
        assert!(store.history_state().unwrap().redo.is_none());
    }

    #[test]
    fn restores_cascaded_folder_children() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let (parent_id, child_id) = store
            .transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO folder
                         (folder_key, name, created_at, updated_at)
                     VALUES ('parent', 'Parent', 'now', 'now')",
                    [],
                )?;
                let parent_id = transaction.last_insert_rowid();
                transaction.execute(
                    "INSERT INTO folder
                         (folder_key, name, parent_id, created_at, updated_at)
                     VALUES ('child', 'Child', ?1, 'now', 'now')",
                    [parent_id],
                )?;
                Ok((parent_id, transaction.last_insert_rowid()))
            })
            .unwrap()
            .0;

        store
            .undoable_transaction_settled(
                HistoryDescriptor::new(
                    "folders.delete",
                    "Delete folder",
                    vec!["folders".to_string()],
                    vec![],
                ),
                |transaction| {
                    transaction.execute("DELETE FROM folder WHERE folder_id = ?1", [parent_id])?;
                    Ok(((), ()))
                },
                |()| Ok(()),
                |()| {},
            )
            .unwrap();
        let remaining = store
            .read(|connection| {
                connection.query_row("SELECT COUNT(*) FROM folder", [], |row| {
                    row.get::<_, i64>(0)
                })
            })
            .unwrap();
        assert_eq!(remaining, 0);

        store
            .apply_history_prepared(HistoryDirection::Undo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        let restored = store
            .read(|connection| {
                connection.query_row(
                    "SELECT parent_id FROM folder WHERE folder_id = ?1",
                    [child_id],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap();
        assert_eq!(restored, parent_id);
    }

    #[test]
    fn no_op_writes_do_not_create_history() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);

        let (_, _, entry, changed) = store
            .undoable_transaction_if_changed_settled(
                rename_descriptor(item_id),
                |_| Ok(((), (), false)),
                |()| Ok(()),
                |()| {},
            )
            .unwrap();

        assert!(!changed);
        assert!(entry.is_none());
        assert_eq!(
            store.history_state().unwrap(),
            super::HistoryState::default()
        );
    }

    #[test]
    fn a_new_write_discards_the_redo_branch() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);

        for value in ["First", "Second"] {
            store
                .undoable_transaction_settled(
                    rename_descriptor(item_id),
                    |transaction| {
                        transaction.execute(
                            "UPDATE media_asset SET name = ?1 WHERE item_id = ?2",
                            rusqlite::params![value, item_id],
                        )?;
                        Ok(((), ()))
                    },
                    |()| Ok(()),
                    |()| {},
                )
                .unwrap();
        }
        store
            .apply_history_prepared(HistoryDirection::Undo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert!(store.history_state().unwrap().redo.is_some());

        store
            .undoable_transaction_settled(
                rename_descriptor(item_id),
                |transaction| {
                    transaction.execute(
                        "UPDATE media_asset SET name = 'Third' WHERE item_id = ?1",
                        [item_id],
                    )?;
                    Ok(((), ()))
                },
                |()| Ok(()),
                |()| {},
            )
            .unwrap();

        assert_eq!(name(&store, item_id), "Third");
        assert!(store.history_state().unwrap().redo.is_none());
    }

    #[test]
    fn semantic_payload_for_one_hundred_thousand_roots_is_compact() {
        let roots = RoaringBitmap::from_iter(1..=100_000);
        let record = SemanticHistoryRecord::new(
            SemanticHistoryPayload::Composite(vec![
                SemanticHistoryPayload::Lifecycle(SemanticLifecycleDelta {
                    active: roots.clone(),
                    ..SemanticLifecycleDelta::default()
                }),
                SemanticHistoryPayload::Tags(vec![SemanticMembershipDelta {
                    relation_id: 7,
                    remove: roots.clone(),
                    ..SemanticMembershipDelta::default()
                }]),
                SemanticHistoryPayload::Folders(vec![SemanticMembershipDelta {
                    relation_id: 11,
                    remove: roots.clone(),
                    ..SemanticMembershipDelta::default()
                }]),
                SemanticHistoryPayload::Ratings(SemanticRatingDelta {
                    unrated: roots.clone(),
                    ..SemanticRatingDelta::default()
                }),
                SemanticHistoryPayload::TagGraph(SemanticTagGraphDelta {
                    projection_tags: vec![SemanticMembershipDelta {
                        relation_id: 7,
                        add: roots.clone(),
                        remove: RoaringBitmap::new(),
                    }],
                    affected_roots: roots.clone(),
                    affected_tag_ids: vec![7],
                    ..SemanticTagGraphDelta::default()
                }),
            ]),
            SemanticHistoryPayload::Lifecycle(SemanticLifecycleDelta {
                trash: roots,
                ..SemanticLifecycleDelta::default()
            }),
        );

        let estimated_bytes = record.estimated_bytes();
        assert!(
            estimated_bytes < 256 * 1024,
            "100k-root semantic history used {estimated_bytes} bytes"
        );
    }

    #[test]
    fn semantic_lifecycle_and_rating_undo_and_redo_are_exact() {
        const ROOT_COUNT: u32 = 1_000;
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        store
            .transaction(|transaction| {
                let mut insert_file = transaction.prepare_cached(
                    "INSERT INTO media_file
                         (file_hash, mime_type, size_bytes, created_at)
                     VALUES (?1, 'image/png', 1, 'now')",
                )?;
                let mut insert_item = transaction.prepare_cached(
                    "INSERT INTO library_item
                         (item_id, item_key, kind, created_at, updated_at)
                     VALUES (?1, ?2, 'media', 'now', 'now')",
                )?;
                let mut insert_asset = transaction.prepare_cached(
                    "INSERT INTO media_asset
                         (item_id, file_id, name, imported_at, updated_at)
                     VALUES (?1, ?2, ?3, 'now', 'now')",
                )?;
                let mut insert_root = transaction.prepare_cached(
                    "INSERT INTO library_root(item_id, lifecycle) VALUES (?1, 'active')",
                )?;
                let mut insert_metadata = transaction.prepare_cached(
                    "INSERT INTO root_metadata
                         (root_item_id, source_urls_json, updated_at)
                     VALUES (?1, '[]', 'now')",
                )?;
                for root_id in 1..=ROOT_COUNT {
                    insert_file.execute([format!("semantic-{root_id}")])?;
                    let file_id = transaction.last_insert_rowid();
                    insert_item.execute(rusqlite::params![
                        i64::from(root_id),
                        format!("semantic-item-{root_id}")
                    ])?;
                    insert_asset.execute(rusqlite::params![
                        i64::from(root_id),
                        file_id,
                        format!("Item {root_id}")
                    ])?;
                    insert_root.execute([i64::from(root_id)])?;
                    insert_metadata.execute([i64::from(root_id)])?;
                }
                Ok(())
            })
            .unwrap();
        let roots = RoaringBitmap::from_iter(1..=ROOT_COUNT);
        let undo = SemanticHistoryPayload::Composite(vec![
            SemanticHistoryPayload::Lifecycle(SemanticLifecycleDelta {
                active: roots.clone(),
                ..SemanticLifecycleDelta::default()
            }),
            SemanticHistoryPayload::Ratings(SemanticRatingDelta {
                unrated: roots.clone(),
                ..SemanticRatingDelta::default()
            }),
        ]);
        let redo = SemanticHistoryPayload::Composite(vec![
            SemanticHistoryPayload::Lifecycle(SemanticLifecycleDelta {
                trash: roots.clone(),
                ..SemanticLifecycleDelta::default()
            }),
            SemanticHistoryPayload::Ratings(SemanticRatingDelta {
                rated: vec![(5, roots.clone())],
                ..SemanticRatingDelta::default()
            }),
        ]);

        store
            .semantic_undoable_transaction_settled(
                HistoryDescriptor::new(
                    "items.semantic",
                    "Broad semantic edit",
                    vec!["library".into(), "folders".into(), "tags".into()],
                    Vec::new(),
                ),
                |transaction| {
                    transaction.execute("UPDATE library_root SET lifecycle = 'trash'", [])?;
                    transaction.execute("UPDATE root_metadata SET rating = 5", [])?;
                    Ok(((), (), SemanticHistoryRecord::new(undo, redo)))
                },
                |()| Ok(()),
                |()| {},
            )
            .unwrap();

        assert_semantic_state(&store, ROOT_COUNT, "trash", 5);
        store
            .apply_history_prepared(HistoryDirection::Undo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_semantic_state(&store, ROOT_COUNT, "active", 0);
        store
            .apply_history_prepared(HistoryDirection::Redo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_semantic_state(&store, ROOT_COUNT, "trash", 5);
    }

    fn assert_semantic_state(store: &Store, root_count: u32, lifecycle: &str, rating: i64) {
        store
            .read(|connection| {
                let lifecycle_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM library_root WHERE lifecycle = ?1",
                    [lifecycle],
                    |row| row.get(0),
                )?;
                let rating_count: i64 = if rating == 0 {
                    connection.query_row(
                        "SELECT COUNT(*) FROM root_metadata WHERE rating IS NULL",
                        [],
                        |row| row.get(0),
                    )?
                } else {
                    connection.query_row(
                        "SELECT COUNT(*) FROM root_metadata WHERE rating = ?1",
                        [rating],
                        |row| row.get(0),
                    )?
                };
                let expected = i64::from(root_count);
                assert_eq!(lifecycle_count, expected);
                assert_eq!(rating_count, expected);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn changeset_smart_folder_undo_and_redo_materialize_exact_generations() {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(directory.path()).unwrap();
        let item_id = media_fixture(&store);
        let matching = r#"{"groups":[{"match_mode":"all","negate":false,"rules":[{"field":"name","op":"contains","value":"Before"}]}]}"#;
        let missing = r#"{"groups":[{"match_mode":"all","negate":false,"rules":[{"field":"name","op":"contains","value":"Missing"}]}]}"#;

        store
            .undoable_transaction_settled(
                HistoryDescriptor::new(
                    "smart_folders.create",
                    "Create smart folder",
                    vec!["smart_folders".into()],
                    Vec::new(),
                ),
                |transaction| {
                    transaction.execute(
                        "INSERT INTO smart_folder (
                             smart_folder_key, name, predicate_json, created_at, updated_at
                         ) VALUES ('history-smart', 'History smart', ?1, 'now', 'now')",
                        [matching],
                    )?;
                    crate::smart_v2::refresh_materialized(transaction)?;
                    Ok((transaction.last_insert_rowid(), ()))
                },
                |()| Ok(()),
                |()| {},
            )
            .unwrap();
        assert_smart_generation_state(&store, item_id, 1, 1);

        store
            .apply_history_prepared(HistoryDirection::Undo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_smart_generation_state(&store, item_id, 0, 0);

        store
            .apply_history_prepared(HistoryDirection::Redo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_smart_generation_state(&store, item_id, 1, 1);

        store
            .undoable_transaction_settled(
                HistoryDescriptor::new(
                    "smart_folders.rules.update",
                    "Edit smart folder rules",
                    vec!["smart_folders".into()],
                    Vec::new(),
                ),
                |transaction| {
                    transaction.execute(
                        "UPDATE smart_folder
                         SET predicate_json = ?1, updated_at = 'later'
                         WHERE smart_folder_key = 'history-smart'",
                        [missing],
                    )?;
                    crate::smart_v2::refresh_materialized(transaction)?;
                    Ok(((), ()))
                },
                |()| Ok(()),
                |()| {},
            )
            .unwrap();
        assert_smart_generation_state(&store, item_id, 1, 0);

        store
            .apply_history_prepared(HistoryDirection::Undo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_smart_generation_state(&store, item_id, 1, 1);

        store
            .apply_history_prepared(HistoryDirection::Redo, |_, _| Ok(()), |_| Ok(()), |()| {})
            .unwrap();
        assert_smart_generation_state(&store, item_id, 1, 0);
    }

    fn assert_smart_generation_state(
        store: &Store,
        item_id: i64,
        expected_folder_count: i64,
        expected_member_count: i64,
    ) {
        store
            .read(|connection| {
                let folder_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM smart_folder
                     WHERE smart_folder_key = 'history-smart'",
                    [],
                    |row| row.get(0),
                )?;
                let active_count: i64 = connection.query_row(
                    "SELECT COUNT(*)
                     FROM smart_folder_generation generation
                     JOIN smart_folder folder
                       ON folder.smart_folder_id = generation.smart_folder_id
                     WHERE folder.smart_folder_key = 'history-smart'
                       AND generation.state = 'active'",
                    [],
                    |row| row.get(0),
                )?;
                let building_count: i64 = connection.query_row(
                    "SELECT COUNT(*) FROM smart_folder_generation
                     WHERE state = 'building'",
                    [],
                    |row| row.get(0),
                )?;
                let stored_member_count: i64 = connection.query_row(
                    "SELECT COALESCE(SUM(generation.member_count), 0)
                     FROM smart_folder_generation generation
                     JOIN smart_folder folder
                       ON folder.smart_folder_id = generation.smart_folder_id
                     WHERE folder.smart_folder_key = 'history-smart'
                       AND generation.state = 'active'",
                    [],
                    |row| row.get(0),
                )?;
                let membership_count: i64 = connection.query_row(
                    "SELECT COUNT(*)
                     FROM smart_folder_membership membership
                     JOIN smart_folder_generation generation
                       ON generation.generation_id = membership.generation_id
                     JOIN smart_folder folder
                       ON folder.smart_folder_id = generation.smart_folder_id
                     WHERE folder.smart_folder_key = 'history-smart'
                       AND generation.state = 'active'
                       AND membership.root_item_id = ?1",
                    [item_id],
                    |row| row.get(0),
                )?;
                let orphan_count: i64 = connection.query_row(
                    "SELECT
                         (SELECT COUNT(*)
                          FROM smart_folder_generation generation
                          LEFT JOIN smart_folder folder
                            ON folder.smart_folder_id = generation.smart_folder_id
                          WHERE folder.smart_folder_id IS NULL)
                       + (SELECT COUNT(*)
                          FROM smart_folder_dependency dependency
                          LEFT JOIN smart_folder folder
                            ON folder.smart_folder_id = dependency.smart_folder_id
                          WHERE folder.smart_folder_id IS NULL)",
                    [],
                    |row| row.get(0),
                )?;

                assert_eq!(folder_count, expected_folder_count);
                assert_eq!(active_count, expected_folder_count);
                assert_eq!(building_count, 0);
                assert_eq!(stored_member_count, expected_member_count);
                assert_eq!(membership_count, expected_member_count);
                assert_eq!(orphan_count, 0);
                Ok(())
            })
            .unwrap();
    }
}
