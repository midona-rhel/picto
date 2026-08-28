use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use roaring::RoaringBitmap;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::WorkPriority;
use crate::history::{HistoryEntry, SemanticChange, SessionHistory};
use crate::ingest;
use crate::model::{
    FolderId, GroupRequest, Lifecycle, PreparedImport, Rating, RootId, SmartFolderId,
};
use crate::ordering::{self, OrderOwnerKind};
use crate::projection::{ProjectionSnapshot, ProjectionStore};
use crate::publication::{MutationReceipt, PublicationCoordinator};
use crate::query::{PageRequest, RootPage, RootQuery};
use crate::selection::{SelectionSummary, SelectionTarget};
use crate::smart::SmartFolderRecord;
use crate::{LibraryDatabase, LibraryError, Result};

struct PublishedDelta {
    snapshot: ProjectionSnapshot,
    receipt: MutationReceipt,
    history: Option<HistoryEntry>,
}

enum TextMutation {
    Rename(String),
    Notes(Option<String>),
    SourceUrls(Vec<String>),
}

pub struct Library {
    database: Arc<LibraryDatabase>,
    projections: Arc<ProjectionStore>,
    publication: Arc<PublicationCoordinator>,
    history: Arc<SessionHistory>,
}

impl Library {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_database(LibraryDatabase::create(path)?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_database(LibraryDatabase::open(path)?)
    }

    fn from_database(database: LibraryDatabase) -> Result<Self> {
        let database = Arc::new(database);
        let projections = Arc::new(ProjectionStore::load(&database)?);
        Ok(Self {
            database,
            projections,
            publication: Arc::new(PublicationCoordinator::default()),
            history: Arc::new(SessionHistory::default()),
        })
    }

    pub fn database(&self) -> &Arc<LibraryDatabase> {
        &self.database
    }

    pub fn projections(&self) -> &Arc<ProjectionStore> {
        &self.projections
    }

    pub fn publication(&self) -> &Arc<PublicationCoordinator> {
        &self.publication
    }

    pub fn history(&self) -> &Arc<SessionHistory> {
        &self.history
    }

    pub fn undo(&self) -> Result<Option<MutationReceipt>> {
        let Some(entry) = self.history.take_undo() else {
            return Ok(None);
        };
        match self.replay_history(&entry, false) {
            Ok(receipt) => {
                self.history.complete_undo(entry);
                Ok(Some(receipt))
            }
            Err(error) => {
                self.history.restore_undo(entry);
                Err(error)
            }
        }
    }

    pub fn redo(&self) -> Result<Option<MutationReceipt>> {
        let Some(entry) = self.history.take_redo() else {
            return Ok(None);
        };
        match self.replay_history(&entry, true) {
            Ok(receipt) => {
                self.history.complete_redo(entry);
                Ok(Some(receipt))
            }
            Err(error) => {
                self.history.restore_redo(entry);
                Err(error)
            }
        }
    }

    pub fn query(&self, query: &RootQuery, page: &PageRequest) -> Result<RootPage> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::query::page(connection, &snapshot, query, page),
        )
    }

    pub fn selection_summary(&self, target: &SelectionTarget) -> Result<SelectionSummary> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let selection = crate::selection::resolve(connection, &snapshot, target)?;
                crate::selection::summarize(connection, &snapshot, &selection)
            },
        )
    }

    pub fn smart_folders(&self) -> Result<Vec<SmartFolderRecord>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::smart::list(connection, &snapshot),
        )
    }

    pub fn create_smart_folder(
        &self,
        name: &str,
        parent_id: Option<SmartFolderId>,
        view: crate::predicate::ViewQuerySpec,
    ) -> Result<(SmartFolderId, MutationReceipt)> {
        let name = required_name("smart folder", name)?;
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let ((smart_folder_id, receipt), _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                validate_smart_parent(transaction, parent_id, None)?;
                let smart_folder_id = SmartFolderId(LibraryDatabase::allocate_id(transaction)?);
                let display_order = transaction.query_row(
                    "SELECT COALESCE(MAX(display_order) + 1, 0)
                     FROM smart_folder_definition WHERE parent_id IS ?1",
                    [parent_id.map(|id| id.0)],
                    |row| row.get::<_, i64>(0),
                )?;
                transaction.execute(
                    "INSERT INTO smart_folder_definition
                         (smart_folder_id, stable_key, parent_id, name, view_query_json, display_order)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![
                        smart_folder_id.0,
                        uuid::Uuid::new_v4().to_string(),
                        parent_id.map(|id| id.0),
                        name,
                        serde_json::to_string(&view)?,
                        display_order
                    ],
                )?;
                let mut next = (*snapshot).clone();
                crate::smart::replace_query(transaction, &mut next, smart_folder_id, view.clone())?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["smart-folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok(((smart_folder_id, receipt.clone()), PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: None,
                }))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok((smart_folder_id, receipt))
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: SmartFolderId,
        name: &str,
        parent_id: Option<SmartFolderId>,
        view: crate::predicate::ViewQuerySpec,
    ) -> Result<MutationReceipt> {
        let name = required_name("smart folder", name)?;
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (receipt, _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                validate_smart_parent(transaction, parent_id, Some(smart_folder_id))?;
                if transaction.execute(
                    "UPDATE smart_folder_definition
                     SET parent_id = ?2, name = ?3, view_query_json = ?4
                     WHERE smart_folder_id = ?1",
                    rusqlite::params![
                        smart_folder_id.0,
                        parent_id.map(|id| id.0),
                        name,
                        serde_json::to_string(&view)?
                    ],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!(
                        "smart folder {}",
                        smart_folder_id.0
                    )));
                }
                let mut next = (*snapshot).clone();
                crate::smart::replace_query(transaction, &mut next, smart_folder_id, view.clone())?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["smart-folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok(receipt)
    }

    pub fn delete_smart_folder(&self, smart_folder_id: SmartFolderId) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (receipt, _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let child_count = transaction.query_row(
                    "SELECT COUNT(*) FROM smart_folder_definition WHERE parent_id = ?1",
                    [smart_folder_id.0],
                    |row| row.get::<_, i64>(0),
                )?;
                if child_count > 0 {
                    return Err(LibraryError::InvalidInput(
                        "move or delete child smart folders first".into(),
                    ));
                }
                if transaction.execute(
                    "DELETE FROM smart_folder_definition WHERE smart_folder_id = ?1",
                    [smart_folder_id.0],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!(
                        "smart folder {}",
                        smart_folder_id.0
                    )));
                }
                let mut next = (*snapshot).clone();
                crate::smart::remove(&mut next, smart_folder_id);
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["smart-folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok(receipt)
    }

    pub fn ingest(&self, input: &PreparedImport) -> Result<(RootId, MutationReceipt)> {
        let mut outputs = self.ingest_batch(std::slice::from_ref(input))?;
        Ok(outputs.remove(0))
    }

    pub fn ingest_batch(
        &self,
        inputs: &[PreparedImport],
    ) -> Result<Vec<(RootId, MutationReceipt)>> {
        if inputs.is_empty() {
            return Ok(Vec::new());
        }
        if inputs.len() > ingest::MAX_INGEST_BATCH {
            return Err(LibraryError::InvalidInput(format!(
                "one canonical ingest batch may contain at most {} items",
                ingest::MAX_INGEST_BATCH
            )));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (outputs, _, _) = self.database.published_write(
            WorkPriority::CanonicalIngest,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut next = (*snapshot).clone();
                let mut root_ids = Vec::with_capacity(inputs.len());
                let mut resources = BTreeSet::new();
                let mut bitmap_keys = HashSet::new();
                let mut folder_ids = HashSet::new();
                for input in inputs {
                    let output = ingest::insert_one(transaction, revision, next, input)?;
                    next = output.snapshot;
                    root_ids.push(output.root_id);
                    resources.extend(output.resources);
                    bitmap_keys.extend(output.bitmap_keys);
                    folder_ids.extend(output.folder_ids);
                }
                ingest::persist_touched(transaction, revision, &next, bitmap_keys, folder_ids)?;
                let affected = root_ids.iter().map(|root| root.0).collect();
                crate::smart::settle_affected(transaction, &mut next, &affected)?;
                let receipt =
                    PublicationCoordinator::receipt(revision, resources, root_ids.iter().copied());
                let outputs = root_ids
                    .into_iter()
                    .map(|root_id| (root_id, receipt.clone()))
                    .collect::<Vec<_>>();
                Ok((
                    outputs,
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        Ok(outputs)
    }

    pub fn settle_fts(&self, limit: usize) -> Result<Option<MutationReceipt>> {
        if limit == 0 {
            return Ok(None);
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (receipt, _, ()) = self.database.published_write(
            WorkPriority::Fts,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let settled = crate::fts::settle_batch(transaction, limit)?;
                let mut next = (*snapshot).clone();
                crate::smart::settle_affected(transaction, &mut next, &settled)?;
                next.revision = revision;
                let resources = if settled.is_empty() {
                    Vec::new()
                } else {
                    vec!["search".into(), "smart-folders".into()]
                };
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    settled.iter().map(RootId),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok((!receipt.resources.is_empty()).then_some(receipt))
    }

    pub fn write_projection_checkpoint(&self) -> Result<usize> {
        let snapshot = self.projections.snapshot();
        let revision = self.database.revision()?;
        if snapshot.revision != revision {
            return Err(LibraryError::InvalidState(format!(
                "cannot checkpoint database revision {revision} with projection revision {}",
                snapshot.revision
            )));
        }
        let payload = crate::checkpoint::encode(&snapshot)?;
        let size = payload.len();
        self.database
            .maintenance_write(WorkPriority::Maintenance, |transaction| {
                crate::checkpoint::write(transaction, revision, &payload)
            })?;
        Ok(size)
    }

    pub fn set_lifecycle(
        &self,
        target: &SelectionTarget,
        lifecycle: Lifecycle,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        self.bitmap_partition_mutation(
            WorkPriority::ForegroundMutation,
            "Change lifecycle",
            BitmapDomain::Lifecycle,
            lifecycle.bitmap_key(),
            Lifecycle::ALL
                .iter()
                .map(|value| value.bitmap_key())
                .collect(),
            &target,
            vec![
                "roots".into(),
                "sidebar".into(),
                "tags".into(),
                "folders".into(),
            ],
        )
    }

    pub fn set_rating(&self, target: &SelectionTarget, rating: Rating) -> Result<MutationReceipt> {
        self.bitmap_partition_mutation(
            WorkPriority::ForegroundMutation,
            "Change rating",
            BitmapDomain::Rating,
            rating.bitmap_key(),
            Rating::ALL.iter().map(|value| value.bitmap_key()).collect(),
            target,
            vec!["roots".into(), "ratings".into()],
        )
    }

    pub fn rename_root(
        &self,
        root_id: RootId,
        name: &str,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let name = required_name("root", name)?;
        self.text_mutation(
            &SelectionTarget::Explicit {
                root_ids: vec![root_id],
            },
            "Rename",
            TextMutation::Rename(name),
            modified_at_ms,
        )
    }

    pub fn set_notes(
        &self,
        target: &SelectionTarget,
        notes: Option<String>,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        self.text_mutation(
            target,
            "Change notes",
            TextMutation::Notes(notes.filter(|value| !value.is_empty())),
            modified_at_ms,
        )
    }

    pub fn set_source_urls(
        &self,
        target: &SelectionTarget,
        source_urls: Vec<String>,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        self.text_mutation(
            target,
            "Change source URLs",
            TextMutation::SourceUrls(source_urls),
            modified_at_ms,
        )
    }

    pub fn add_tag(&self, target: &SelectionTarget, name: &str) -> Result<MutationReceipt> {
        self.tag_mutation(target, name, true)
    }

    pub fn create_folder(
        &self,
        name: &str,
        parent_id: Option<FolderId>,
    ) -> Result<(FolderId, MutationReceipt)> {
        if name.trim().is_empty() {
            return Err(LibraryError::InvalidInput("folder name is empty".into()));
        }
        let name = name.trim().to_owned();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((folder_id, receipt), _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                if let Some(parent_id) = parent_id {
                    let exists = transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM folder_definition WHERE folder_id = ?1)",
                        [parent_id.0],
                        |row| row.get::<_, bool>(0),
                    )?;
                    if !exists {
                        return Err(LibraryError::InvalidInput(format!(
                            "parent folder {} does not exist",
                            parent_id.0
                        )));
                    }
                }
                let folder_id = FolderId(LibraryDatabase::allocate_id(transaction)?);
                let display_order = transaction.query_row(
                    "SELECT COALESCE(MAX(display_order) + 1, 0)
                     FROM folder_definition WHERE parent_id IS ?1",
                    [parent_id.map(|id| id.0)],
                    |row| row.get::<_, i64>(0),
                )?;
                transaction.execute(
                    "INSERT INTO folder_definition
                         (folder_id, stable_key, parent_id, name, display_order)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        folder_id.0,
                        uuid::Uuid::new_v4().to_string(),
                        parent_id.map(|id| id.0),
                        name,
                        display_order
                    ],
                )?;
                let mut next = (*snapshot).clone();
                Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(Vec::new()));
                Arc::make_mut(&mut next.folders).insert(folder_id, RoaringBitmap::new());
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    (folder_id, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((folder_id, receipt))
    }

    pub fn add_to_folder(
        &self,
        target: &SelectionTarget,
        folder_id: FolderId,
    ) -> Result<MutationReceipt> {
        self.folder_membership_mutation(target, folder_id, true)
    }

    pub fn remove_from_folder(
        &self,
        target: &SelectionTarget,
        folder_id: FolderId,
    ) -> Result<MutationReceipt> {
        self.folder_membership_mutation(target, folder_id, false)
    }

    pub fn remove_tag(&self, target: &SelectionTarget, name: &str) -> Result<MutationReceipt> {
        self.tag_mutation(target, name, false)
    }

    pub fn organize_into_collection(
        &self,
        request: &GroupRequest,
    ) -> Result<(RootId, MutationReceipt)> {
        let request = request.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((collection_id, receipt), _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let output =
                    crate::group::organize(transaction, revision, (*snapshot).clone(), &request)?;
                let mut next = output.snapshot;
                crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec![
                        "roots".into(),
                        "collections".into(),
                        "tags".into(),
                        "folders".into(),
                        "sidebar".into(),
                    ],
                    output.affected.iter().map(RootId),
                );
                Ok((
                    (output.collection_id, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((collection_id, receipt))
    }

    pub fn ungroup_collection(
        &self,
        collection_id: RootId,
        modified_at_ms: i64,
    ) -> Result<(Vec<RootId>, MutationReceipt)> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((roots, receipt), _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let output = crate::group::ungroup(
                    transaction,
                    revision,
                    (*snapshot).clone(),
                    collection_id,
                    modified_at_ms,
                )?;
                let mut next = output.snapshot;
                crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec![
                        "roots".into(),
                        "collections".into(),
                        "tags".into(),
                        "folders".into(),
                        "sidebar".into(),
                    ],
                    output.affected.iter().map(RootId),
                );
                Ok((
                    (output.roots, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((roots, receipt))
    }

    fn tag_mutation(
        &self,
        target: &SelectionTarget,
        name: &str,
        add: bool,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let name = name.to_owned();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
                let mut next = (*snapshot).clone();
                let tag_id = if add {
                    if let Some(tag_id) = next.tag_ids_by_name.get(&name).copied() {
                        tag_id
                    } else {
                        let tag_id = ingest::ensure_tag(transaction, &name)?;
                        Arc::make_mut(&mut next.tag_ids_by_name).insert(name.clone(), tag_id);
                        tag_id
                    }
                } else if let Some(tag_id) = next.tag_ids_by_name.get(&name).copied() {
                    tag_id
                } else {
                    next.revision = revision;
                    let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
                    return Ok((
                        receipt.clone(),
                        PublishedDelta {
                            snapshot: next,
                            receipt,
                            history: None,
                        },
                    ));
                };
                let tags = Arc::make_mut(&mut next.tags);
                let before = tags.get(&tag_id).cloned().unwrap_or_default();
                let mut after = before.clone();
                if add {
                    after |= &selection;
                } else {
                    after -= &selection;
                }
                let changed = &before ^ &after;
                if changed.is_empty() {
                    let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
                    next.revision = revision;
                    return Ok((
                        receipt.clone(),
                        PublishedDelta {
                            snapshot: next,
                            receipt,
                            history: None,
                        },
                    ));
                }
                tags.insert(tag_id, after.clone());
                bitmap::replace(
                    transaction,
                    revision,
                    BitmapKey {
                        domain: BitmapDomain::Tag,
                        key_id: tag_id.0,
                    },
                    &after,
                )?;
                let counts = Arc::make_mut(&mut next.tag_count);
                for root_id in &changed {
                    let current = counts.value(root_id).unwrap_or(0);
                    counts.insert(
                        root_id,
                        if after.contains(root_id) {
                            current + 1
                        } else {
                            current.saturating_sub(1)
                        },
                    );
                }
                crate::smart::settle_affected(transaction, &mut next, &changed)?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "tags".into(), "sidebar".into()],
                    changed.iter().map(RootId),
                );
                let history = HistoryEntry::new(
                    if add { "Add tag" } else { "Remove tag" },
                    SemanticChange::Bitmap {
                        key: BitmapKey {
                            domain: BitmapDomain::Tag,
                            key_id: tag_id.0,
                        },
                        before: Arc::new(before),
                        after: Arc::new(after),
                    },
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(history),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    fn text_mutation(
        &self,
        target: &SelectionTarget,
        label: &'static str,
        mutation: TextMutation,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
                if matches!(&mutation, TextMutation::Rename(_)) && selection.len() != 1 {
                    return Err(LibraryError::InvalidInput(
                        "rename requires exactly one root".into(),
                    ));
                }
                let mut before = load_root_text(transaction, &selection)?;
                let mut after = before.clone();
                for state in &mut after {
                    match &mutation {
                        TextMutation::Rename(name) => state.name.clone_from(name),
                        TextMutation::Notes(notes) => state.notes.clone_from(notes),
                        TextMutation::SourceUrls(urls) => state.source_urls.clone_from(urls),
                    }
                    state.modified_at_ms = modified_at_ms;
                }
                if before == after {
                    let mut next = (*snapshot).clone();
                    next.revision = revision;
                    let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
                    return Ok((
                        receipt.clone(),
                        PublishedDelta {
                            snapshot: next,
                            receipt,
                            history: None,
                        },
                    ));
                }
                let mut update = transaction.prepare_cached(
                    "UPDATE library_root
                     SET name = ?2, notes = ?3, source_urls_json = ?4, modified_at_ms = ?5
                     WHERE root_id = ?1",
                )?;
                for state in &after {
                    update.execute(rusqlite::params![
                        state.root_id.0,
                        state.name,
                        state.notes,
                        serde_json::to_string(&state.source_urls)?,
                        state.modified_at_ms
                    ])?;
                    if matches!(&mutation, TextMutation::Rename(_)) {
                        transaction.execute(
                            "UPDATE media_item SET media_name = ?2
                             WHERE media_id = ?1 AND EXISTS(
                                 SELECT 1 FROM library_item
                                 WHERE local_id = ?1 AND item_kind = 1
                             )",
                            rusqlite::params![state.root_id.0, state.name],
                        )?;
                    }
                }
                drop(update);
                crate::fts::mark_dirty(transaction, &selection, 1, modified_at_ms)?;
                let mut next = (*snapshot).clone();
                {
                    let index = Arc::make_mut(&mut next.modified_at);
                    for state in &after {
                        index.insert(state.root_id.0, state.modified_at_ms.max(0) as u64);
                    }
                }
                for state in &after {
                    if state.notes.is_some() {
                        Arc::make_mut(&mut next.notes_present).insert(state.root_id.0);
                    } else {
                        Arc::make_mut(&mut next.notes_present).remove(state.root_id.0);
                    }
                    if state.source_urls.is_empty() {
                        Arc::make_mut(&mut next.urls_present).remove(state.root_id.0);
                    } else {
                        Arc::make_mut(&mut next.urls_present).insert(state.root_id.0);
                    }
                }
                crate::smart::settle_affected(transaction, &mut next, &selection)?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "search".into(), "smart-folders".into()],
                    selection.iter().map(RootId),
                );
                let history = HistoryEntry::new(
                    label,
                    SemanticChange::RootText {
                        before: Arc::new(std::mem::take(&mut before)),
                        after: Arc::new(after),
                    },
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(history),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    fn folder_membership_mutation(
        &self,
        target: &SelectionTarget,
        folder_id: FolderId,
        add: bool,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let exists = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM folder_definition WHERE folder_id = ?1)",
                    [folder_id.0],
                    |row| row.get::<_, bool>(0),
                )?;
                if !exists {
                    return Err(LibraryError::NotFound(format!("folder {}", folder_id.0)));
                }
                let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
                let mut next = (*snapshot).clone();
                let before = next
                    .folder_orders
                    .get(&folder_id)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(Vec::new()));
                let mut after = before.as_ref().clone();
                if add {
                    let existing = after.iter().map(|root| root.0).collect::<RoaringBitmap>();
                    after.extend((&selection - &existing).iter().map(RootId));
                } else {
                    after.retain(|root| !selection.contains(root.0));
                }
                if before.as_ref() == &after {
                    let receipt = PublicationCoordinator::receipt(revision, Vec::new(), Vec::new());
                    next.revision = revision;
                    return Ok((
                        receipt.clone(),
                        PublishedDelta {
                            snapshot: next,
                            receipt,
                            history: None,
                        },
                    ));
                }
                ordering::replace(
                    transaction,
                    revision,
                    OrderOwnerKind::Folder,
                    folder_id.0,
                    &after.iter().map(|root| root.0).collect::<Vec<_>>(),
                )?;
                let after_bitmap = after.iter().map(|root| root.0).collect::<RoaringBitmap>();
                let before_bitmap = before.iter().map(|root| root.0).collect::<RoaringBitmap>();
                let changed = &before_bitmap ^ &after_bitmap;
                Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(after.clone()));
                Arc::make_mut(&mut next.folders).insert(folder_id, after_bitmap.clone());
                let counts = Arc::make_mut(&mut next.folder_count);
                for root_id in &changed {
                    let current = counts.value(root_id).unwrap_or(0);
                    counts.insert(
                        root_id,
                        if after_bitmap.contains(root_id) {
                            current + 1
                        } else {
                            current.saturating_sub(1)
                        },
                    );
                }
                crate::smart::settle_affected(transaction, &mut next, &changed)?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "folders".into(), "sidebar".into()],
                    changed.iter().map(RootId),
                );
                let history = HistoryEntry::new(
                    if add {
                        "Add to folder"
                    } else {
                        "Remove from folder"
                    },
                    SemanticChange::Order {
                        owner_kind: OrderOwnerKind::Folder,
                        owner_id: folder_id.0,
                        before: Arc::new(before.iter().map(|root| root.0).collect()),
                        after: Arc::new(after.iter().map(|root| root.0).collect()),
                    },
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(history),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    #[allow(clippy::too_many_arguments)]
    fn bitmap_partition_mutation(
        &self,
        priority: WorkPriority,
        label: &'static str,
        domain: BitmapDomain,
        destination: u32,
        partition_keys: Vec<u32>,
        target: &SelectionTarget,
        resources: Vec<String>,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            priority,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
                let mut next = (*snapshot).clone();
                let mut changes = Vec::new();
                for key_id in &partition_keys {
                    let key = BitmapKey {
                        domain,
                        key_id: *key_id,
                    };
                    let before = projection_bitmap(&next, key);
                    let mut after = before.clone();
                    if *key_id == destination {
                        after |= &selection;
                    } else {
                        after -= &selection;
                    }
                    if before != after {
                        set_projection_bitmap(&mut next, key, after.clone());
                        bitmap::replace(transaction, revision, key, &after)?;
                        changes.push(SemanticChange::Bitmap {
                            key,
                            before: Arc::new(before),
                            after: Arc::new(after),
                        });
                    }
                }
                crate::smart::settle_affected(transaction, &mut next, &selection)?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources.clone(),
                    selection.iter().map(RootId),
                );
                let history = (!changes.is_empty())
                    .then(|| HistoryEntry::new(label, SemanticChange::Compound(changes)));
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    fn replay_history(&self, entry: &HistoryEntry, use_after: bool) -> Result<MutationReceipt> {
        let change = entry.change.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (receipt, _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut next = (*snapshot).clone();
                let mut affected = RoaringBitmap::new();
                let mut resources = BTreeSet::new();
                apply_semantic_change(
                    transaction,
                    revision,
                    &mut next,
                    &change,
                    use_after,
                    &mut affected,
                    &mut resources,
                )?;
                crate::smart::settle_affected(transaction, &mut next, &affected)?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    affected.iter().map(RootId),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok(receipt)
    }

    fn capture_revision(&self, revision: u64) -> Result<Arc<ProjectionSnapshot>> {
        let snapshot = self.projections.snapshot();
        if snapshot.revision != revision {
            return Err(LibraryError::InvalidState(format!(
                "database revision {revision} does not match projection revision {}",
                snapshot.revision
            )));
        }
        Ok(snapshot)
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_semantic_change(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    change: &SemanticChange,
    use_after: bool,
    affected: &mut RoaringBitmap,
    resources: &mut BTreeSet<String>,
) -> Result<()> {
    match change {
        SemanticChange::Bitmap { key, before, after } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            let current = projection_bitmap(snapshot, *key);
            if &current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because {:?}/{} changed",
                    key.domain, key.key_id
                )));
            }
            bitmap::replace(transaction, revision, *key, replacement)?;
            set_projection_bitmap(snapshot, *key, replacement.clone());
            let changed = expected ^ replacement;
            *affected |= &changed;
            resources.insert("roots".into());
            match key.domain {
                BitmapDomain::Tag => {
                    let counts = Arc::make_mut(&mut snapshot.tag_count);
                    for root_id in &changed {
                        let current = counts.value(root_id).unwrap_or(0);
                        counts.insert(
                            root_id,
                            if replacement.contains(root_id) {
                                current + 1
                            } else {
                                current.saturating_sub(1)
                            },
                        );
                    }
                    resources.insert("tags".into());
                }
                BitmapDomain::Lifecycle => {
                    resources.insert("sidebar".into());
                    resources.insert("folders".into());
                    resources.insert("tags".into());
                }
                BitmapDomain::Rating => {
                    resources.insert("ratings".into());
                }
            }
        }
        SemanticChange::Order {
            owner_kind,
            owner_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            match owner_kind {
                OrderOwnerKind::Folder => {
                    let folder_id = FolderId(*owner_id);
                    let current = snapshot
                        .folder_orders
                        .get(&folder_id)
                        .map(|roots| roots.iter().map(|root| root.0).collect::<Vec<_>>())
                        .unwrap_or_default();
                    if &current != expected {
                        return Err(LibraryError::InvalidState(format!(
                            "cannot replay history because folder {owner_id} changed"
                        )));
                    }
                    ordering::replace(transaction, revision, *owner_kind, *owner_id, replacement)?;
                    let old_bitmap = expected.iter().copied().collect::<RoaringBitmap>();
                    let new_bitmap = replacement.iter().copied().collect::<RoaringBitmap>();
                    let changed = &old_bitmap ^ &new_bitmap;
                    Arc::make_mut(&mut snapshot.folder_orders).insert(
                        folder_id,
                        Arc::new(replacement.iter().copied().map(RootId).collect()),
                    );
                    Arc::make_mut(&mut snapshot.folders).insert(folder_id, new_bitmap.clone());
                    let counts = Arc::make_mut(&mut snapshot.folder_count);
                    for root_id in &changed {
                        let current = counts.value(root_id).unwrap_or(0);
                        counts.insert(
                            root_id,
                            if new_bitmap.contains(root_id) {
                                current + 1
                            } else {
                                current.saturating_sub(1)
                            },
                        );
                    }
                    *affected |= &changed;
                    resources.insert("roots".into());
                    resources.insert("folders".into());
                    resources.insert("sidebar".into());
                }
                OrderOwnerKind::Collection => {
                    let root_id = RootId(*owner_id);
                    let current = snapshot
                        .collection_orders
                        .get(&root_id)
                        .map(|members| members.iter().map(|media| media.0).collect::<Vec<_>>())
                        .unwrap_or_default();
                    if &current != expected {
                        return Err(LibraryError::InvalidState(format!(
                            "cannot replay history because collection {owner_id} changed"
                        )));
                    }
                    let old_members = expected.iter().copied().collect::<RoaringBitmap>();
                    let new_members = replacement.iter().copied().collect::<RoaringBitmap>();
                    if old_members != new_members {
                        return Err(LibraryError::InvalidState(
                            "structural collection history requires a structural snapshot".into(),
                        ));
                    }
                    ordering::replace(transaction, revision, *owner_kind, *owner_id, replacement)?;
                    Arc::make_mut(&mut snapshot.collection_orders).insert(
                        root_id,
                        Arc::new(replacement.iter().copied().map(crate::MediaId).collect()),
                    );
                    affected.insert(root_id.0);
                    resources.insert("roots".into());
                    resources.insert("collections".into());
                }
            }
        }
        SemanticChange::RootText { before, after } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            if expected.len() != replacement.len() {
                return Err(LibraryError::InvalidState(
                    "text history root sets do not match".into(),
                ));
            }
            for (expected, replacement) in expected.iter().zip(replacement.iter()) {
                if expected.root_id != replacement.root_id {
                    return Err(LibraryError::InvalidState(
                        "text history root order does not match".into(),
                    ));
                }
                let current = transaction.query_row(
                    "SELECT name, notes, source_urls_json, modified_at_ms
                     FROM library_root WHERE root_id = ?1",
                    [expected.root_id.0],
                    |row| {
                        Ok(crate::history::RootTextState {
                            root_id: expected.root_id,
                            name: row.get(0)?,
                            notes: row.get(1)?,
                            source_urls: serde_json::from_str(&row.get::<_, String>(2)?)
                                .unwrap_or_default(),
                            modified_at_ms: row.get(3)?,
                        })
                    },
                )?;
                if current != *expected {
                    return Err(LibraryError::InvalidState(format!(
                        "cannot replay history because root {} text changed",
                        expected.root_id.0
                    )));
                }
                transaction.execute(
                    "UPDATE library_root
                     SET name = ?2, notes = ?3, source_urls_json = ?4, modified_at_ms = ?5
                     WHERE root_id = ?1",
                    rusqlite::params![
                        replacement.root_id.0,
                        replacement.name,
                        replacement.notes,
                        serde_json::to_string(&replacement.source_urls)?,
                        replacement.modified_at_ms
                    ],
                )?;
                transaction.execute(
                    "UPDATE media_item SET media_name = ?2
                     WHERE media_id = ?1 AND EXISTS(
                         SELECT 1 FROM library_item
                         WHERE local_id = ?1 AND item_kind = 1
                     )",
                    rusqlite::params![replacement.root_id.0, replacement.name],
                )?;
                crate::fts::mark_one(transaction, replacement.root_id, revision as i64)?;
                Arc::make_mut(&mut snapshot.modified_at).insert(
                    replacement.root_id.0,
                    replacement.modified_at_ms.max(0) as u64,
                );
                if replacement.notes.is_some() {
                    Arc::make_mut(&mut snapshot.notes_present).insert(replacement.root_id.0);
                } else {
                    Arc::make_mut(&mut snapshot.notes_present).remove(replacement.root_id.0);
                }
                if replacement.source_urls.is_empty() {
                    Arc::make_mut(&mut snapshot.urls_present).remove(replacement.root_id.0);
                } else {
                    Arc::make_mut(&mut snapshot.urls_present).insert(replacement.root_id.0);
                }
                affected.insert(replacement.root_id.0);
            }
            resources.insert("roots".into());
            resources.insert("search".into());
        }
        SemanticChange::Compound(changes) => {
            for change in changes {
                apply_semantic_change(
                    transaction,
                    revision,
                    snapshot,
                    change,
                    use_after,
                    affected,
                    resources,
                )?;
            }
        }
    }
    Ok(())
}

fn projection_bitmap(snapshot: &ProjectionSnapshot, key: BitmapKey) -> RoaringBitmap {
    match key.domain {
        BitmapDomain::Lifecycle => snapshot
            .lifecycle
            .iter()
            .find_map(|(value, bitmap)| (value.bitmap_key() == key.key_id).then(|| bitmap.clone()))
            .unwrap_or_default(),
        BitmapDomain::Rating => snapshot
            .ratings
            .iter()
            .find_map(|(value, bitmap)| (value.bitmap_key() == key.key_id).then(|| bitmap.clone()))
            .unwrap_or_default(),
        BitmapDomain::Tag => snapshot
            .tags
            .get(&crate::TagId(key.key_id))
            .cloned()
            .unwrap_or_default(),
    }
}

fn set_projection_bitmap(snapshot: &mut ProjectionSnapshot, key: BitmapKey, bitmap: RoaringBitmap) {
    match key.domain {
        BitmapDomain::Lifecycle => {
            if let Some(value) = Lifecycle::ALL
                .into_iter()
                .find(|value| value.bitmap_key() == key.key_id)
            {
                Arc::make_mut(&mut snapshot.lifecycle).insert(value, bitmap);
            }
        }
        BitmapDomain::Rating => {
            if let Some(value) = Rating::ALL
                .into_iter()
                .find(|value| value.bitmap_key() == key.key_id)
            {
                Arc::make_mut(&mut snapshot.ratings).insert(value, bitmap);
            }
        }
        BitmapDomain::Tag => {
            Arc::make_mut(&mut snapshot.tags).insert(crate::TagId(key.key_id), bitmap);
        }
    }
}

fn publish_delta(
    projections: &ProjectionStore,
    publication: &PublicationCoordinator,
    delta: PublishedDelta,
) -> Option<HistoryEntry> {
    projections.publish(delta.snapshot);
    publication.register(&delta.receipt);
    delta.history
}

fn push_history(history: &SessionHistory, entry: Option<HistoryEntry>) {
    if let Some(entry) = entry {
        history.push(entry);
    }
}

fn required_name(kind: &str, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(LibraryError::InvalidInput(format!("{kind} name is empty")));
    }
    Ok(name.to_owned())
}

fn validate_smart_parent(
    transaction: &rusqlite::Transaction<'_>,
    parent_id: Option<SmartFolderId>,
    child_id: Option<SmartFolderId>,
) -> Result<()> {
    if parent_id.is_some() && parent_id == child_id {
        return Err(LibraryError::InvalidInput(
            "a smart folder cannot be its own parent".into(),
        ));
    }
    if let Some(parent_id) = parent_id {
        let exists = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM smart_folder_definition WHERE smart_folder_id = ?1
             )",
            [parent_id.0],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Err(LibraryError::NotFound(format!(
                "smart folder {}",
                parent_id.0
            )));
        }
    }
    Ok(())
}

fn load_root_text(
    transaction: &rusqlite::Transaction<'_>,
    roots: &RoaringBitmap,
) -> Result<Vec<crate::history::RootTextState>> {
    let mut statement = transaction.prepare_cached(
        "SELECT name, notes, source_urls_json, modified_at_ms
         FROM library_root WHERE root_id = ?1",
    )?;
    roots
        .iter()
        .map(|root_id| {
            statement
                .query_row([root_id], |row| {
                    Ok(crate::history::RootTextState {
                        root_id: RootId(root_id),
                        name: row.get(0)?,
                        notes: row.get(1)?,
                        source_urls: serde_json::from_str(&row.get::<_, String>(2)?)
                            .unwrap_or_default(),
                        modified_at_ms: row.get(3)?,
                    })
                })
                .map_err(Into::into)
        })
        .collect()
}
