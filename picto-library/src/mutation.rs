use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use roaring::RoaringBitmap;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::WorkPriority;
use crate::history::{HistoryEntry, SemanticChange, SessionHistory, TagDefinitionState};
use crate::ingest;
use crate::model::{
    FolderId, GroupRequest, Lifecycle, MediaFactsUpdate, MediaId, PreparedImport, Rating, RootId,
    SmartFolderId,
};
use crate::ordering::{self, OrderOwnerKind};
use crate::projection::{ProjectionSnapshot, ProjectionStore};
use crate::publication::{MutationReceipt, PublicationCoordinator};
use crate::query::{LibraryCounts, PageRequest, RootPage, RootQuery};
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

    pub fn counts(&self) -> Result<LibraryCounts> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |_, snapshot| Ok(crate::query::counts(&snapshot)),
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
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &settled,
                    crate::predicate::DependencyChange::RootText,
                )?;
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

    pub fn update_media_facts(
        &self,
        media_id: MediaId,
        update: &MediaFactsUpdate,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        if update == &MediaFactsUpdate::default() {
            return Err(LibraryError::InvalidInput(
                "media facts update contains no changes".into(),
            ));
        }
        let update = update.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (receipt, _, ()) = self.database.published_write(
            WorkPriority::Maintenance,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let (
                    file_id,
                    mut mime,
                    mut width,
                    mut height,
                    mut duration_ms,
                    mut frame_count,
                    mut perceptual_hash,
                    mut palette,
                ) = transaction
                    .query_row(
                        "SELECT file.file_id, file.mime, file.width, file.height,
                                file.duration_ms, file.frame_count, file.perceptual_hash,
                                file.palette_json
                         FROM media_item media
                         JOIN media_file file ON file.file_id = media.file_id
                         WHERE media.media_id = ?1",
                        [media_id.0],
                        |row| {
                            Ok((
                                row.get::<_, u32>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, Option<u32>>(2)?,
                                row.get::<_, Option<u32>>(3)?,
                                row.get::<_, Option<i64>>(4)?.map(|value| value as u64),
                                row.get::<_, Option<u32>>(5)?,
                                row.get::<_, Option<String>>(6)?,
                                serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                            ))
                        },
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => {
                            LibraryError::NotFound(format!("media {media_id}"))
                        }
                        error => error.into(),
                    })?;
                if let Some(value) = update.mime {
                    if value.trim().is_empty() {
                        return Err(LibraryError::InvalidInput("MIME is empty".into()));
                    }
                    mime = value;
                }
                if let Some(value) = update.width {
                    width = value;
                }
                if let Some(value) = update.height {
                    height = value;
                }
                if let Some(value) = update.duration_ms {
                    duration_ms = value;
                }
                if let Some(value) = update.frame_count {
                    frame_count = value;
                }
                if let Some(value) = update.perceptual_hash {
                    perceptual_hash = value;
                }
                if let Some(value) = update.palette {
                    palette = value;
                }
                transaction.execute(
                    "UPDATE media_file
                     SET mime = ?2, width = ?3, height = ?4, duration_ms = ?5,
                         frame_count = ?6, perceptual_hash = ?7, palette_json = ?8
                     WHERE file_id = ?1",
                    rusqlite::params![
                        file_id,
                        mime,
                        width,
                        height,
                        duration_ms.map(i64::try_from).transpose().map_err(|_| {
                            LibraryError::InvalidInput("duration exceeds SQLite range".into())
                        })?,
                        frame_count,
                        perceptual_hash,
                        serde_json::to_string(&palette)?
                    ],
                )?;
                let affected = {
                    let mut statement = transaction.prepare_cached(
                        "SELECT root.root_id
                         FROM library_root root
                         JOIN media_item media ON media.media_id = root.cover_media_id
                         WHERE media.file_id = ?1",
                    )?;
                    let roots = statement
                        .query_map([file_id], |row| row.get::<_, u32>(0))?
                        .collect::<std::result::Result<RoaringBitmap, _>>()?;
                    roots
                };
                let mut next = (*snapshot).clone();
                for root_id in &affected {
                    crate::group::refresh_cover_projection(
                        transaction,
                        &mut next,
                        RootId(root_id),
                    )?;
                }
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &affected,
                    crate::predicate::DependencyChange::CoverFacts,
                )?;
                transaction.execute(
                    "INSERT INTO cloud_journal
                         (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                     VALUES (?1, 'media.facts', ?2, ?3, ?4)",
                    rusqlite::params![
                        revision as i64,
                        crate::bitmap::encode(&affected)?,
                        serde_json::json!({"media_id": media_id.0, "file_id": file_id}).to_string(),
                        changed_at_ms
                    ],
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["media".into(), "roots".into(), "smart-folders".into()],
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

    pub fn rename_tag(&self, tag_id: crate::TagId, name: &str) -> Result<MutationReceipt> {
        let name = required_name("tag", name)?;
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let old_name = snapshot
                    .tag_ids_by_name
                    .iter()
                    .find_map(|(name, id)| (*id == tag_id).then_some(name.clone()))
                    .ok_or_else(|| LibraryError::NotFound(format!("tag {}", tag_id.0)))?;
                if snapshot
                    .tag_ids_by_name
                    .get(&name)
                    .is_some_and(|existing| *existing != tag_id)
                {
                    return Err(LibraryError::InvalidInput(format!(
                        "tag {name} already exists"
                    )));
                }
                let mut next = (*snapshot).clone();
                rename_tag_definition(transaction, &mut next, tag_id, &name)?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["tags".into(), "navigation".into(), "smart-folders".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (old_name != name).then(|| {
                            HistoryEntry::new(
                                "Rename tag",
                                SemanticChange::TagName {
                                    tag_id,
                                    before: old_name,
                                    after: name.clone(),
                                },
                            )
                        }),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    pub fn delete_tag(&self, tag_id: crate::TagId, changed_at_ms: i64) -> Result<MutationReceipt> {
        self.remove_tag_definition(tag_id, None, changed_at_ms)
    }

    pub fn merge_tags(
        &self,
        source: crate::TagId,
        destination: crate::TagId,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        if source == destination {
            return Err(LibraryError::InvalidInput(
                "source and destination tags are identical".into(),
            ));
        }
        self.remove_tag_definition(source, Some(destination), changed_at_ms)
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

    pub fn set_folder_auto_tags(
        &self,
        folder_id: FolderId,
        tag_ids: Vec<crate::TagId>,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let after = tag_ids
            .into_iter()
            .map(|tag_id| tag_id.0)
            .collect::<RoaringBitmap>();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let before = ingest::folder_auto_tags(transaction, folder_id)?;
                for tag_id in &after {
                    let exists = transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM tag_definition WHERE tag_id = ?1)",
                        [tag_id],
                        |row| row.get::<_, bool>(0),
                    )?;
                    if !exists {
                        return Err(LibraryError::NotFound(format!("tag {tag_id}")));
                    }
                }
                transaction.execute(
                    "UPDATE folder_definition SET auto_tag_ids = ?2 WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, ingest::encode_folder_auto_tags(&after)?],
                )?;
                transaction.execute(
                    "INSERT INTO cloud_journal
                         (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                     VALUES (?1, 'folder.auto_tags', NULL, ?2, ?3)",
                    rusqlite::params![
                        revision as i64,
                        serde_json::json!({"folder_id": folder_id.0}).to_string(),
                        changed_at_ms
                    ],
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt =
                    PublicationCoordinator::receipt(revision, vec!["folders".into()], Vec::new());
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (before != after).then(|| {
                            HistoryEntry::new(
                                "Change folder auto-tags",
                                SemanticChange::FolderAutoTags {
                                    folder_id,
                                    before: Arc::new(before),
                                    after: Arc::new(after.clone()),
                                },
                            )
                        }),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
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

    pub fn detach_collection_member(
        &self,
        collection_id: RootId,
        media_id: MediaId,
        modified_at_ms: i64,
    ) -> Result<(RootId, MutationReceipt)> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let ((root_id, receipt), _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let output = crate::group::detach(
                    transaction,
                    revision,
                    (*snapshot).clone(),
                    collection_id,
                    media_id,
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
                        "search".into(),
                    ],
                    output.affected.iter().map(RootId),
                );
                Ok((
                    (output.root_id, receipt.clone()),
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
        Ok((root_id, receipt))
    }

    pub fn reorder_collection(
        &self,
        collection_id: RootId,
        ordered_media_ids: Vec<MediaId>,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let before = snapshot
                    .collection_orders
                    .get(&collection_id)
                    .ok_or_else(|| {
                        LibraryError::InvalidInput(format!(
                            "root {collection_id} is not a collection"
                        ))
                    })?
                    .iter()
                    .map(|media| media.0)
                    .collect::<Vec<_>>();
                let after = ordered_media_ids
                    .iter()
                    .map(|media| media.0)
                    .collect::<Vec<_>>();
                let before_members = before.iter().copied().collect::<RoaringBitmap>();
                let after_members = after.iter().copied().collect::<RoaringBitmap>();
                if before.len() != after.len() || before_members != after_members {
                    return Err(LibraryError::InvalidInput(
                        "collection order must contain every member exactly once".into(),
                    ));
                }
                let mut next = (*snapshot).clone();
                let history_entry = if before == after {
                    None
                } else {
                    ordering::replace(
                        transaction,
                        revision,
                        OrderOwnerKind::Collection,
                        collection_id.0,
                        &after,
                    )?;
                    Arc::make_mut(&mut next.collection_orders)
                        .insert(collection_id, Arc::new(ordered_media_ids.clone()));
                    transaction.execute(
                        "INSERT INTO cloud_journal
                             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                         VALUES (?1, 'collection.reorder', ?2, '{}', ?3)",
                        rusqlite::params![
                            revision as i64,
                            crate::bitmap::encode(&[collection_id.0].into_iter().collect())?,
                            changed_at_ms
                        ],
                    )?;
                    Some(HistoryEntry::new(
                        "Reorder collection",
                        SemanticChange::Order {
                            owner_kind: OrderOwnerKind::Collection,
                            owner_id: collection_id.0,
                            before: Arc::new(before),
                            after: Arc::new(after),
                        },
                    ))
                };
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "collections".into()],
                    [collection_id],
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: history_entry,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    pub fn set_collection_cover(
        &self,
        collection_id: RootId,
        cover_media_id: MediaId,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let before = transaction
                    .query_row(
                        "SELECT cover_media_id FROM library_root WHERE root_id = ?1",
                        [collection_id.0],
                        |row| row.get::<_, u32>(0).map(MediaId),
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => {
                            LibraryError::NotFound(format!("collection {collection_id}"))
                        }
                        error => error.into(),
                    })?;
                let mut next = (*snapshot).clone();
                let history_entry = if before == cover_media_id {
                    None
                } else {
                    crate::group::set_cover(transaction, &mut next, collection_id, cover_media_id)?;
                    let affected = [collection_id.0].into_iter().collect();
                    crate::smart::settle_affected_for(
                        transaction,
                        &mut next,
                        &affected,
                        crate::predicate::DependencyChange::CoverFacts,
                    )?;
                    transaction.execute(
                        "INSERT INTO cloud_journal
                             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                         VALUES (?1, 'collection.cover', ?2, ?3, ?4)",
                        rusqlite::params![
                            revision as i64,
                            crate::bitmap::encode(&affected)?,
                            serde_json::json!({"cover_media_id": cover_media_id.0}).to_string(),
                            changed_at_ms
                        ],
                    )?;
                    Some(HistoryEntry::new(
                        "Set collection cover",
                        SemanticChange::CollectionCover {
                            root_id: collection_id,
                            before,
                            after: cover_media_id,
                        },
                    ))
                };
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "collections".into(), "smart-folders".into()],
                    [collection_id],
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: history_entry,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    pub fn permanently_delete(
        &self,
        target: &SelectionTarget,
        deleted_at_ms: i64,
    ) -> Result<(MutationReceipt, Vec<crate::PendingBlobCleanup>)> {
        let target = target.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let ((receipt, cleanup), _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let roots = crate::selection::resolve(transaction, &snapshot, &target)?;
                if roots.is_empty() {
                    return Err(LibraryError::InvalidInput(
                        "permanent deletion requires at least one root".into(),
                    ));
                }
                if !roots.is_subset(snapshot.lifecycle(Lifecycle::Trash)) {
                    return Err(LibraryError::InvalidInput(
                        "only Trash roots can be permanently deleted".into(),
                    ));
                }

                let mut media_ids = RoaringBitmap::new();
                let mut collection_ids = Vec::new();
                for root_id in &roots {
                    let root_id = RootId(root_id);
                    if let Some(members) = snapshot.collection_orders.get(&root_id) {
                        collection_ids.push(root_id);
                        media_ids.extend(members.iter().map(|media| media.0));
                    } else {
                        media_ids.insert(root_id.0);
                    }
                }
                let mut item_ids = roots.clone();
                item_ids |= &media_ids;
                stage_delete_ids(transaction, "delete_root", &roots)?;
                stage_delete_ids(transaction, "delete_media", &media_ids)?;
                stage_delete_ids(transaction, "delete_item", &item_ids)?;

                let mut files = std::collections::HashMap::new();
                {
                    let mut statement = transaction.prepare_cached(
                        "SELECT file.file_id, file.file_path
                         FROM media_item media
                         JOIN media_file file ON file.file_id = media.file_id
                         WHERE media.media_id = ?1",
                    )?;
                    for media_id in &media_ids {
                        let (file_id, file_path) = statement.query_row([media_id], |row| {
                            Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
                        })?;
                        files.entry(file_id).or_insert(file_path);
                    }
                }
                transaction.execute(
                    "INSERT INTO deletion_tombstone(stable_key, revision, deleted_at_ms)
                     SELECT item.stable_key, ?1, ?2
                     FROM library_item item JOIN temp.delete_item selected
                         ON selected.local_id = item.local_id
                     ON CONFLICT(stable_key) DO UPDATE SET
                         revision = excluded.revision,
                         deleted_at_ms = excluded.deleted_at_ms",
                    rusqlite::params![revision as i64, deleted_at_ms],
                )?;
                transaction.execute(
                    "DELETE FROM root_fts WHERE CAST(root_id AS INTEGER) IN
                         (SELECT local_id FROM temp.delete_root)",
                    [],
                )?;
                transaction.execute(
                    "DELETE FROM library_root WHERE root_id IN
                         (SELECT local_id FROM temp.delete_root)",
                    [],
                )?;
                for collection_id in &collection_ids {
                    ordering::delete(transaction, OrderOwnerKind::Collection, collection_id.0)?;
                }
                transaction.execute(
                    "DELETE FROM media_item WHERE media_id IN
                         (SELECT local_id FROM temp.delete_media)",
                    [],
                )?;
                transaction.execute(
                    "DELETE FROM library_item WHERE local_id IN
                         (SELECT local_id FROM temp.delete_item)",
                    [],
                )?;

                let mut cleanup = Vec::new();
                for (file_id, file_path) in files {
                    let referenced = transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM media_item WHERE file_id = ?1)",
                        [file_id],
                        |row| row.get::<_, bool>(0),
                    )?;
                    if !referenced {
                        transaction.execute(
                            "INSERT INTO blob_cleanup_queue(file_id, file_path, enqueued_revision)
                             VALUES (?1, ?2, ?3)
                             ON CONFLICT(file_id) DO NOTHING",
                            rusqlite::params![file_id, file_path, revision as i64],
                        )?;
                        cleanup.push(crate::PendingBlobCleanup {
                            file_id: crate::FileId(file_id),
                            file_path,
                        });
                    }
                }

                let mut next = (*snapshot).clone();
                for lifecycle in Lifecycle::ALL {
                    let values = Arc::make_mut(&mut next.lifecycle)
                        .entry(lifecycle)
                        .or_default();
                    if !(&*values & &roots).is_empty() {
                        *values -= &roots;
                        bitmap::replace(
                            transaction,
                            revision,
                            BitmapKey {
                                domain: BitmapDomain::Lifecycle,
                                key_id: lifecycle.bitmap_key(),
                            },
                            values,
                        )?;
                    }
                }
                for rating in Rating::ALL {
                    let values = Arc::make_mut(&mut next.ratings).entry(rating).or_default();
                    if !(&*values & &roots).is_empty() {
                        *values -= &roots;
                        bitmap::replace(
                            transaction,
                            revision,
                            BitmapKey {
                                domain: BitmapDomain::Rating,
                                key_id: rating.bitmap_key(),
                            },
                            values,
                        )?;
                    }
                }
                let changed_tags = next
                    .tags
                    .iter()
                    .filter_map(|(tag_id, members)| {
                        (!((members & &roots).is_empty())).then_some(*tag_id)
                    })
                    .collect::<Vec<_>>();
                for tag_id in changed_tags {
                    let members = Arc::make_mut(&mut next.tags).get_mut(&tag_id).unwrap();
                    *members -= &roots;
                    bitmap::replace(
                        transaction,
                        revision,
                        BitmapKey {
                            domain: BitmapDomain::Tag,
                            key_id: tag_id.0,
                        },
                        members,
                    )?;
                }
                let changed_folders = next
                    .folder_orders
                    .iter()
                    .filter_map(|(folder_id, order)| {
                        order
                            .iter()
                            .any(|root| roots.contains(root.0))
                            .then_some(*folder_id)
                    })
                    .collect::<Vec<_>>();
                for folder_id in changed_folders {
                    let mut order = next.folder_orders[&folder_id].as_ref().clone();
                    order.retain(|root| !roots.contains(root.0));
                    ordering::replace(
                        transaction,
                        revision,
                        OrderOwnerKind::Folder,
                        folder_id.0,
                        &order.iter().map(|root| root.0).collect::<Vec<_>>(),
                    )?;
                    Arc::make_mut(&mut next.folder_orders)
                        .insert(folder_id, Arc::new(order.clone()));
                    Arc::make_mut(&mut next.folders)
                        .insert(folder_id, order.iter().map(|root| root.0).collect());
                }
                for collection_id in &collection_ids {
                    Arc::make_mut(&mut next.collection_orders).remove(collection_id);
                }
                for media_id in &media_ids {
                    if let Some(owner) =
                        Arc::make_mut(&mut next.media_owner).get_mut(media_id as usize)
                    {
                        *owner = None;
                    }
                }
                crate::group::remove_root_projections(&mut next, &roots);
                crate::smart::settle_affected(transaction, &mut next, &roots)?;
                transaction.execute(
                    "INSERT INTO cloud_journal
                         (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                     VALUES (?1, 'root.permanent_delete', ?2, '{}', ?3)",
                    rusqlite::params![
                        revision as i64,
                        crate::bitmap::encode(&roots)?,
                        deleted_at_ms
                    ],
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec![
                        "roots".into(),
                        "sidebar".into(),
                        "tags".into(),
                        "folders".into(),
                        "collections".into(),
                        "search".into(),
                    ],
                    roots.iter().map(RootId),
                );
                Ok((
                    (receipt.clone(), cleanup),
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
        Ok((receipt, cleanup))
    }

    fn remove_tag_definition(
        &self,
        source: crate::TagId,
        destination: Option<crate::TagId>,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let source_state = load_tag_definition(transaction, source)?
                    .ok_or_else(|| LibraryError::NotFound(format!("tag {}", source.0)))?;
                if let Some(destination) = destination {
                    if load_tag_definition(transaction, destination)?.is_none() {
                        return Err(LibraryError::NotFound(format!("tag {}", destination.0)));
                    }
                }
                let source_members = snapshot.tags.get(&source).cloned().unwrap_or_default();
                let mut next = (*snapshot).clone();

                let mut history_changes = Vec::new();
                let folder_ids = {
                    let mut statement =
                        transaction.prepare("SELECT folder_id FROM folder_definition")?;
                    let ids = statement
                        .query_map([], |row| row.get::<_, u32>(0))?
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    ids
                };
                for folder_id in folder_ids.into_iter().map(FolderId) {
                    let before = ingest::folder_auto_tags(transaction, folder_id)?;
                    if !before.contains(source.0) {
                        continue;
                    }
                    let mut after = before.clone();
                    after.remove(source.0);
                    if let Some(destination) = destination {
                        after.insert(destination.0);
                    }
                    transaction.execute(
                        "UPDATE folder_definition SET auto_tag_ids = ?2 WHERE folder_id = ?1",
                        rusqlite::params![folder_id.0, ingest::encode_folder_auto_tags(&after)?],
                    )?;
                    history_changes.push(SemanticChange::FolderAutoTags {
                        folder_id,
                        before: Arc::new(before),
                        after: Arc::new(after),
                    });
                }
                if let Some(destination) = destination {
                    let before = next.tags.get(&destination).cloned().unwrap_or_default();
                    let mut after = before.clone();
                    after |= &source_members;
                    if before != after {
                        bitmap::replace(
                            transaction,
                            revision,
                            BitmapKey {
                                domain: BitmapDomain::Tag,
                                key_id: destination.0,
                            },
                            &after,
                        )?;
                        Arc::make_mut(&mut next.tags).insert(destination, after.clone());
                        history_changes.push(SemanticChange::Bitmap {
                            key: BitmapKey {
                                domain: BitmapDomain::Tag,
                                key_id: destination.0,
                            },
                            before: Arc::new(before),
                            after: Arc::new(after),
                        });
                    }
                }

                bitmap::replace(
                    transaction,
                    revision,
                    BitmapKey {
                        domain: BitmapDomain::Tag,
                        key_id: source.0,
                    },
                    &RoaringBitmap::new(),
                )?;
                transaction.execute("DELETE FROM tag_definition WHERE tag_id = ?1", [source.0])?;
                Arc::make_mut(&mut next.tags).remove(&source);
                Arc::make_mut(&mut next.tag_ids_by_name).remove(&source_state.full_name);

                let destination_members = destination
                    .and_then(|tag_id| next.tags.get(&tag_id))
                    .cloned()
                    .unwrap_or_default();
                let counts = Arc::make_mut(&mut next.tag_count);
                for root_id in &source_members {
                    let loses_assignment = destination.is_none()
                        || destination_members.contains(root_id)
                            && snapshot
                                .tags
                                .get(&destination.unwrap())
                                .is_some_and(|members| members.contains(root_id));
                    if loses_assignment {
                        let current = counts.value(root_id).unwrap_or(0);
                        counts.insert(root_id, current.saturating_sub(1));
                    }
                }
                let query_changes = crate::smart::rewrite_tag_references(
                    transaction,
                    &mut next,
                    source,
                    destination,
                )?;

                history_changes.insert(
                    0,
                    SemanticChange::TagDefinition {
                        before: Some(source_state),
                        after: None,
                        before_members: Arc::new(source_members.clone()),
                        after_members: Arc::new(RoaringBitmap::new()),
                        queries: Arc::new(query_changes),
                    },
                );
                transaction.execute(
                    "INSERT INTO cloud_journal
                         (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        revision as i64,
                        if destination.is_some() {
                            "tag.merge"
                        } else {
                            "tag.delete"
                        },
                        crate::bitmap::encode(&source_members)?,
                        serde_json::json!({
                            "source_tag_id": source.0,
                            "destination_tag_id": destination.map(|tag| tag.0),
                        })
                        .to_string(),
                        changed_at_ms
                    ],
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec![
                        "roots".into(),
                        "tags".into(),
                        "navigation".into(),
                        "smart-folders".into(),
                    ],
                    source_members.iter().map(RootId),
                );
                let label = if destination.is_some() {
                    "Merge tags"
                } else {
                    "Delete tag"
                };
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::new(
                            label,
                            SemanticChange::Compound(history_changes),
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
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
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &changed,
                    crate::predicate::DependencyChange::Tag(tag_id),
                )?;
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
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &selection,
                    crate::predicate::DependencyChange::RootText,
                )?;
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
                let added = &after_bitmap - &before_bitmap;
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
                let mut history_changes = vec![SemanticChange::Order {
                    owner_kind: OrderOwnerKind::Folder,
                    owner_id: folder_id.0,
                    before: Arc::new(before.iter().map(|root| root.0).collect()),
                    after: Arc::new(after.iter().map(|root| root.0).collect()),
                }];
                let mut auto_tagged = false;
                if add && !added.is_empty() {
                    for tag_id in ingest::folder_auto_tags(transaction, folder_id)? {
                        let tag_id = crate::TagId(tag_id);
                        let before = next.tags.get(&tag_id).cloned().unwrap_or_default();
                        let mut after = before.clone();
                        after |= &added;
                        let tag_changed = &before ^ &after;
                        if tag_changed.is_empty() {
                            continue;
                        }
                        bitmap::replace(
                            transaction,
                            revision,
                            BitmapKey {
                                domain: BitmapDomain::Tag,
                                key_id: tag_id.0,
                            },
                            &after,
                        )?;
                        Arc::make_mut(&mut next.tags).insert(tag_id, after.clone());
                        let counts = Arc::make_mut(&mut next.tag_count);
                        for root_id in &tag_changed {
                            let current = counts.value(root_id).unwrap_or(0);
                            counts.insert(root_id, current + 1);
                        }
                        history_changes.push(SemanticChange::Bitmap {
                            key: BitmapKey {
                                domain: BitmapDomain::Tag,
                                key_id: tag_id.0,
                            },
                            before: Arc::new(before),
                            after: Arc::new(after),
                        });
                        auto_tagged = true;
                    }
                }
                if auto_tagged {
                    crate::smart::settle_affected(transaction, &mut next, &changed)?;
                } else {
                    crate::smart::settle_affected_for(
                        transaction,
                        &mut next,
                        &changed,
                        crate::predicate::DependencyChange::Folder(folder_id),
                    )?;
                }
                next.revision = revision;
                let mut resources = vec!["roots".into(), "folders".into(), "sidebar".into()];
                if auto_tagged {
                    resources.push("tags".into());
                }
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    changed.iter().map(RootId),
                );
                let history = HistoryEntry::new(
                    if add {
                        "Add to folder"
                    } else {
                        "Remove from folder"
                    },
                    SemanticChange::Compound(history_changes),
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
                let smart_change = match domain {
                    BitmapDomain::Lifecycle => crate::predicate::DependencyChange::Lifecycle,
                    BitmapDomain::Rating => crate::predicate::DependencyChange::Rating,
                    BitmapDomain::Tag => {
                        crate::predicate::DependencyChange::Tag(crate::TagId(destination))
                    }
                };
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &selection,
                    smart_change,
                )?;
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
        SemanticChange::CollectionCover {
            root_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (*before, *after)
            } else {
                (*after, *before)
            };
            let current = transaction
                .query_row(
                    "SELECT cover_media_id FROM library_root WHERE root_id = ?1",
                    [root_id.0],
                    |row| row.get::<_, u32>(0).map(MediaId),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::NotFound(format!("collection {root_id}"))
                    }
                    error => error.into(),
                })?;
            if current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because collection {root_id} cover changed"
                )));
            }
            crate::group::set_cover(transaction, snapshot, *root_id, replacement)?;
            affected.insert(root_id.0);
            resources.insert("roots".into());
            resources.insert("collections".into());
        }
        SemanticChange::TagName {
            tag_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = snapshot
                .tag_ids_by_name
                .iter()
                .find_map(|(name, id)| (*id == *tag_id).then_some(name.as_str()))
                .ok_or_else(|| LibraryError::NotFound(format!("tag {}", tag_id.0)))?;
            if current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag {} was renamed",
                    tag_id.0
                )));
            }
            rename_tag_definition(transaction, snapshot, *tag_id, replacement)?;
            resources.insert("tags".into());
            resources.insert("navigation".into());
            resources.insert("smart-folders".into());
        }
        SemanticChange::TagDefinition {
            before,
            after,
            before_members,
            after_members,
            queries,
        } => {
            let (expected, replacement, expected_members, replacement_members) = if use_after {
                (
                    before,
                    after,
                    before_members.as_ref(),
                    after_members.as_ref(),
                )
            } else {
                (
                    after,
                    before,
                    after_members.as_ref(),
                    before_members.as_ref(),
                )
            };
            let tag_id = expected
                .as_ref()
                .or(replacement.as_ref())
                .expect("tag definition history has one state")
                .tag_id;
            if load_tag_definition(transaction, tag_id)? != *expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag {} definition changed",
                    tag_id.0
                )));
            }
            let current_members = snapshot.tags.get(&tag_id).cloned().unwrap_or_default();
            if &current_members != expected_members {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag {} membership changed",
                    tag_id.0
                )));
            }
            for query in queries.iter() {
                let expected_query = if use_after {
                    &query.before
                } else {
                    &query.after
                };
                if snapshot.smart_queries.get(&query.smart_folder_id.0) != Some(expected_query) {
                    return Err(LibraryError::InvalidState(format!(
                        "cannot replay history because smart folder {} changed",
                        query.smart_folder_id.0
                    )));
                }
            }

            if let Some(state) = replacement {
                transaction.execute(
                    "INSERT INTO tag_definition
                         (tag_id, stable_key, namespace_id, subname)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT(tag_id) DO UPDATE SET
                         stable_key = excluded.stable_key,
                         namespace_id = excluded.namespace_id,
                         subname = excluded.subname",
                    rusqlite::params![
                        state.tag_id.0,
                        state.stable_key,
                        state.namespace_id,
                        tag_subname(&state.full_name)
                    ],
                )?;
            } else {
                transaction.execute("DELETE FROM tag_definition WHERE tag_id = ?1", [tag_id.0])?;
            }
            bitmap::replace(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                },
                replacement_members,
            )?;
            let names = Arc::make_mut(&mut snapshot.tag_ids_by_name);
            names.retain(|_, id| *id != tag_id);
            if let Some(state) = replacement {
                names.insert(state.full_name.clone(), tag_id);
                Arc::make_mut(&mut snapshot.tags).insert(tag_id, replacement_members.clone());
            } else {
                Arc::make_mut(&mut snapshot.tags).remove(&tag_id);
            }
            let changed = expected_members ^ replacement_members;
            let counts = Arc::make_mut(&mut snapshot.tag_count);
            for root_id in &changed {
                let current = counts.value(root_id).unwrap_or(0);
                counts.insert(
                    root_id,
                    if replacement_members.contains(root_id) {
                        current + 1
                    } else {
                        current.saturating_sub(1)
                    },
                );
            }
            for query in queries.iter() {
                let replacement_query = if use_after {
                    &query.after
                } else {
                    &query.before
                };
                transaction.execute(
                    "UPDATE smart_folder_definition SET view_query_json = ?2
                     WHERE smart_folder_id = ?1",
                    rusqlite::params![
                        query.smart_folder_id.0,
                        serde_json::to_string(replacement_query)?
                    ],
                )?;
                Arc::make_mut(&mut snapshot.smart_queries)
                    .insert(query.smart_folder_id.0, replacement_query.clone());
            }
            *affected |= &changed;
            if !queries.is_empty() {
                *affected |= snapshot.active();
            }
            resources.insert("roots".into());
            resources.insert("tags".into());
            resources.insert("navigation".into());
            resources.insert("smart-folders".into());
        }
        SemanticChange::FolderAutoTags {
            folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before.as_ref(), after.as_ref())
            } else {
                (after.as_ref(), before.as_ref())
            };
            let current = ingest::folder_auto_tags(transaction, *folder_id)?;
            if &current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because folder {} auto-tags changed",
                    folder_id.0
                )));
            }
            transaction.execute(
                "UPDATE folder_definition SET auto_tag_ids = ?2 WHERE folder_id = ?1",
                rusqlite::params![folder_id.0, ingest::encode_folder_auto_tags(replacement)?],
            )?;
            resources.insert("folders".into());
        }
        SemanticChange::Compound(changes) => {
            if use_after {
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
            } else {
                for change in changes.iter().rev() {
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

fn rename_tag_definition(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    tag_id: crate::TagId,
    name: &str,
) -> Result<()> {
    let (namespace, subname) = name.split_once(':').unwrap_or(("", name));
    if subname.trim().is_empty() {
        return Err(LibraryError::InvalidInput("tag name is empty".into()));
    }
    if snapshot
        .tag_ids_by_name
        .get(name)
        .is_some_and(|existing| *existing != tag_id)
    {
        return Err(LibraryError::InvalidInput(format!(
            "tag {name} already exists"
        )));
    }
    let old_name = snapshot
        .tag_ids_by_name
        .iter()
        .find_map(|(name, id)| (*id == tag_id).then_some(name.clone()))
        .ok_or_else(|| LibraryError::NotFound(format!("tag {}", tag_id.0)))?;
    let namespace_id = ingest::ensure_namespace(transaction, namespace)?;
    transaction.execute(
        "UPDATE tag_definition SET namespace_id = ?2, subname = ?3 WHERE tag_id = ?1",
        rusqlite::params![tag_id.0, namespace_id, subname],
    )?;
    let names = Arc::make_mut(&mut snapshot.tag_ids_by_name);
    names.remove(&old_name);
    names.insert(name.to_owned(), tag_id);
    Ok(())
}

fn load_tag_definition(
    transaction: &rusqlite::Transaction<'_>,
    tag_id: crate::TagId,
) -> Result<Option<TagDefinitionState>> {
    use rusqlite::OptionalExtension;

    transaction
        .query_row(
            "SELECT tag.stable_key, tag.namespace_id, namespace.display_name, tag.subname
             FROM tag_definition tag
             JOIN tag_namespace namespace ON namespace.namespace_id = tag.namespace_id
             WHERE tag.tag_id = ?1",
            [tag_id.0],
            |row| {
                let namespace = row.get::<_, String>(2)?;
                let subname = row.get::<_, String>(3)?;
                Ok(TagDefinitionState {
                    tag_id,
                    stable_key: row.get(0)?,
                    namespace_id: row.get(1)?,
                    full_name: if namespace.is_empty() {
                        subname
                    } else {
                        format!("{namespace}:{subname}")
                    },
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn tag_subname(name: &str) -> &str {
    name.split_once(':').map_or(name, |(_, subname)| subname)
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

fn stage_delete_ids(
    transaction: &rusqlite::Transaction<'_>,
    table: &str,
    ids: &RoaringBitmap,
) -> Result<()> {
    let table = match table {
        "delete_root" | "delete_media" | "delete_item" => table,
        _ => {
            return Err(LibraryError::InvalidInput(format!(
                "unsupported deletion staging table {table}"
            )))
        }
    };
    transaction.execute_batch(&format!(
        "CREATE TEMP TABLE IF NOT EXISTS {table} (
             local_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM {table};"
    ))?;
    let mut insert =
        transaction.prepare_cached(&format!("INSERT INTO temp.{table}(local_id) VALUES (?1)"))?;
    for id in ids {
        insert.execute([id])?;
    }
    Ok(())
}
