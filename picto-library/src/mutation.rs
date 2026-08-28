use std::path::Path;
use std::sync::Arc;

use roaring::RoaringBitmap;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::WorkPriority;
use crate::history::{HistoryEntry, SemanticChange, SessionHistory};
use crate::ingest;
use crate::model::{FolderId, GroupRequest, Lifecycle, PreparedImport, Rating, RootId};
use crate::ordering::{self, OrderOwnerKind};
use crate::projection::{ProjectionSnapshot, ProjectionStore};
use crate::publication::{MutationReceipt, PublicationCoordinator};
use crate::query::{PageRequest, RootPage, RootQuery};
use crate::selection::{SelectionSummary, SelectionTarget};
use crate::{LibraryDatabase, LibraryError, Result};

struct PublishedDelta {
    snapshot: ProjectionSnapshot,
    receipt: MutationReceipt,
    history: Option<HistoryEntry>,
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

    pub fn ingest(&self, input: &PreparedImport) -> Result<(RootId, MutationReceipt)> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((root_id, receipt), _, history_entry) = self.database.published_write(
            WorkPriority::CanonicalIngest,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let output = ingest::insert_one(transaction, revision, (*snapshot).clone(), input)?;
                let receipt =
                    PublicationCoordinator::receipt(revision, output.resources, [output.root_id]);
                Ok((
                    (output.root_id, receipt.clone()),
                    PublishedDelta {
                        snapshot: output.snapshot,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((root_id, receipt))
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
                        snapshot: output.snapshot,
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
                        snapshot: output.snapshot,
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
                let tag_id = ingest::ensure_tag(transaction, &name)?;
                let mut next = (*snapshot).clone();
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
