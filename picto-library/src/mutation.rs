use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use roaring::RoaringBitmap;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::WorkPriority;
use crate::history::{
    FolderDefinitionState, HistoryEntry, SemanticChange, SessionHistory,
    SmartFolderDefinitionState, StructuralRootState, StructuralState, TagDefinitionState,
};
use crate::ingest;
use crate::model::{
    DuplicatePair, DuplicateResolutionChoice, DuplicateResolutionResult, DuplicateStatus, FileId,
    FolderDeleteResult, FolderId, FolderRecord, GroupRequest, Lifecycle, MediaFactsUpdate, MediaId,
    PendingBlobCleanup, PreparedCollectionImport, PreparedImport, Rating, RootId, RootKind,
    RootTagAssignment, SmartFolderDeleteResult, SmartFolderId, SmartFolderInput, TagNamespaceId,
    TagRecord,
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

    pub fn details(&self, root_id: RootId) -> Result<crate::RootDetails> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::query::details(connection, &snapshot, root_id),
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
                crate::selection::summarize(connection, &snapshot, target, &selection)
            },
        )
    }

    pub fn ordered_selection(&self, target: &SelectionTarget) -> Result<Vec<RootId>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::selection::resolve_ordered(connection, &snapshot, target),
        )
    }

    pub fn record_recent_view(
        &self,
        root_id: RootId,
        viewed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (receipt, _, _) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let exists = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM library_root WHERE root_id = ?1)",
                    [root_id.0],
                    |row| row.get::<_, bool>(0),
                )?;
                if !exists {
                    return Err(LibraryError::NotFound(format!("root {root_id}")));
                }
                transaction.execute(
                    "INSERT INTO recent_view (root_id, viewed_at_ms) VALUES (?1, ?2)
                     ON CONFLICT(root_id) DO UPDATE SET viewed_at_ms = excluded.viewed_at_ms",
                    rusqlite::params![root_id.0, viewed_at_ms],
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["recently-viewed".into()],
                    vec![root_id],
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
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        Ok(receipt)
    }

    pub fn clear_recent_views(&self) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let before = load_recent_views(transaction)?;
                transaction.execute("DELETE FROM recent_view", [])?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["recently-viewed".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (!before.is_empty()).then(|| {
                            HistoryEntry::for_command(
                                "items.clear_recent_views",
                                "Clear recently viewed",
                                SemanticChange::RecentViews {
                                    before: Arc::new(before),
                                    after: Arc::new(Vec::new()),
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

    pub fn smart_folders(&self) -> Result<Vec<SmartFolderRecord>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::smart::list(connection, &snapshot),
        )
    }

    pub fn folders(&self) -> Result<Vec<FolderRecord>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let mut statement = connection.prepare(
                    "SELECT folder_id, stable_key, parent_id, name, icon, color, notes,
                            display_order
                     FROM folder_definition
                     ORDER BY parent_id, display_order, folder_id",
                )?;
                let rows = statement.query_map([], |row| {
                    let folder_id = FolderId(row.get(0)?);
                    Ok(FolderRecord {
                        folder_id,
                        stable_key: row.get(1)?,
                        parent_id: row.get::<_, Option<u32>>(2)?.map(FolderId),
                        name: row.get(3)?,
                        icon: row.get(4)?,
                        color: row.get(5)?,
                        notes: row.get(6)?,
                        display_order: row.get(7)?,
                        count: snapshot
                            .folders
                            .get(&folder_id)
                            .map_or(0, |roots| (roots & snapshot.active()).len()),
                    })
                })?;
                rows.collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(Into::into)
            },
        )
    }

    pub fn folder_auto_tags(&self, folder_id: FolderId) -> Result<Vec<String>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                require_folder(connection, folder_id)?;
                let ids = ingest::folder_auto_tags(connection, folder_id)?;
                let names_by_id = snapshot
                    .tag_ids_by_name
                    .iter()
                    .map(|(name, tag_id)| (*tag_id, name.as_str()))
                    .collect::<HashMap<_, _>>();
                ids.iter()
                    .map(|tag_id| {
                        names_by_id.get(&crate::TagId(tag_id)).map_or_else(
                            || {
                                Err(LibraryError::InvalidState(format!(
                                    "folder {} references missing auto-tag {tag_id}",
                                    folder_id.0
                                )))
                            },
                            |name| Ok((*name).to_owned()),
                        )
                    })
                    .collect()
            },
        )
    }

    pub fn tags(&self) -> Result<Vec<TagRecord>> {
        self.tags_with_revision().map(|(tags, _)| tags)
    }

    pub fn tags_with_revision(&self) -> Result<(Vec<TagRecord>, u64)> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let mut statement = connection.prepare(
                    "SELECT tag.tag_id, namespace.namespace_id,
                            namespace.display_name, tag.subname
                     FROM tag_definition tag
                     JOIN tag_namespace namespace
                       ON namespace.namespace_id = tag.namespace_id
                     ORDER BY namespace.display_name COLLATE NOCASE,
                              tag.subname COLLATE NOCASE, tag.tag_id",
                )?;
                let rows = statement.query_map([], |row| {
                    let tag_id = crate::TagId(row.get(0)?);
                    let members = snapshot.tags.get(&tag_id);
                    Ok(TagRecord {
                        tag_id,
                        namespace_id: TagNamespaceId(row.get(1)?),
                        namespace: row.get(2)?,
                        subname: row.get(3)?,
                        active_count: members.map_or(0, |roots| (roots & snapshot.active()).len()),
                        assignment_count: members.map_or(0, |roots| roots.len()),
                    })
                })?;
                let tags = rows
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .map_err(LibraryError::from)?;
                Ok((tags, snapshot.revision))
            },
        )
    }

    pub fn pending_cloud_journal(&self, limit: usize) -> Result<Vec<crate::CloudJournalRecord>> {
        if limit == 0 {
            return Ok(Vec::new());
        }
        self.database.read(WorkPriority::Cloud, |connection| {
            let mut statement = connection.prepare(
                "SELECT journal_id, revision, operation_kind, target_bitmap,
                        payload_json, created_at_ms
                 FROM cloud_journal
                 WHERE expanded_at_ms IS NULL
                 ORDER BY journal_id
                 LIMIT ?1",
            )?;
            let rows = statement
                .query_map([limit.min(1000) as i64], |row| {
                    Ok((
                        row.get::<_, i64>(0)? as u64,
                        row.get::<_, i64>(1)? as u64,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<Vec<u8>>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows.into_iter()
                .map(
                    |(journal_id, revision, operation_kind, targets, payload, created_at_ms)| {
                        let target_root_ids = targets
                            .map(|payload| {
                                RoaringBitmap::deserialize_from(&mut std::io::Cursor::new(payload))
                            })
                            .transpose()?;
                        Ok(crate::CloudJournalRecord {
                            journal_id,
                            revision,
                            operation_kind,
                            target_root_ids,
                            payload: serde_json::from_str(&payload)?,
                            created_at_ms,
                        })
                    },
                )
                .collect()
        })
    }

    pub fn mark_cloud_journal_expanded(
        &self,
        journal_ids: &[u64],
        expanded_at_ms: i64,
    ) -> Result<()> {
        if journal_ids.is_empty() {
            return Ok(());
        }
        self.database
            .maintenance_write(WorkPriority::Cloud, |transaction| {
                let mut statement = transaction.prepare_cached(
                    "UPDATE cloud_journal SET expanded_at_ms = ?2
                     WHERE journal_id = ?1 AND expanded_at_ms IS NULL",
                )?;
                for journal_id in journal_ids {
                    let journal_id = i64::try_from(*journal_id).map_err(|_| {
                        LibraryError::InvalidInput("cloud journal ID exceeds SQLite range".into())
                    })?;
                    statement.execute(rusqlite::params![journal_id, expanded_at_ms])?;
                }
                Ok(())
            })
    }

    pub fn create_smart_folder(
        &self,
        input: SmartFolderInput,
    ) -> Result<(SmartFolderId, MutationReceipt)> {
        let name = required_name("smart folder", &input.name)?;
        let parent_id = input.parent_id;
        let icon = normalized_optional(input.icon.as_deref());
        let color = normalized_optional(input.color.as_deref());
        let notes = normalized_optional(input.notes.as_deref());
        let view = input.view;
        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((smart_folder_id, receipt), _, history_entry) = self.database.published_write(
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
                         (smart_folder_id, stable_key, parent_id, name, icon, color, notes,
                          view_query_json, display_order)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    rusqlite::params![
                        smart_folder_id.0,
                        uuid::Uuid::new_v4().to_string(),
                        parent_id.map(|id| id.0),
                        name,
                        icon,
                        color,
                        notes,
                        serde_json::to_string(&view)?,
                        display_order
                    ],
                )?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "smart_folder.create",
                    None,
                    serde_json::json!({"smart_folder_id": smart_folder_id.0}),
                    changed_at_ms,
                )?;
                let mut next = (*snapshot).clone();
                crate::smart::replace_query(transaction, &mut next, smart_folder_id, view.clone())?;
                next.revision = revision;
                let after = load_smart_folder_definition(transaction, smart_folder_id)?
                    .ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "new smart folder {} disappeared before publication",
                            smart_folder_id.0
                        ))
                    })?;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["smart-folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    (smart_folder_id, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "smart_folders.create",
                            "Create smart folder",
                            SemanticChange::SmartFolderDefinition {
                                smart_folder_id,
                                before: None,
                                after: Some(Box::new(after)),
                            },
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((smart_folder_id, receipt))
    }

    pub fn update_smart_folder(
        &self,
        smart_folder_id: SmartFolderId,
        input: SmartFolderInput,
    ) -> Result<MutationReceipt> {
        let name = required_name("smart folder", &input.name)?;
        let parent_id = input.parent_id;
        let icon = normalized_optional(input.icon.as_deref());
        let color = normalized_optional(input.color.as_deref());
        let notes = normalized_optional(input.notes.as_deref());
        let view = input.view;
        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                validate_smart_parent(transaction, parent_id, Some(smart_folder_id))?;
                let before = load_smart_folder_definition(transaction, smart_folder_id)?
                    .ok_or_else(|| {
                        LibraryError::NotFound(format!("smart folder {smart_folder_id}"))
                    })?;
                if transaction.execute(
                    "UPDATE smart_folder_definition
                     SET parent_id = ?2, name = ?3, icon = ?4, color = ?5, notes = ?6,
                         view_query_json = ?7
                     WHERE smart_folder_id = ?1",
                    rusqlite::params![
                        smart_folder_id.0,
                        parent_id.map(|id| id.0),
                        name,
                        icon,
                        color,
                        notes,
                        serde_json::to_string(&view)?
                    ],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!(
                        "smart folder {}",
                        smart_folder_id.0
                    )));
                }
                insert_cloud_journal(
                    transaction,
                    revision,
                    "smart_folder.update",
                    None,
                    serde_json::json!({"smart_folder_id": smart_folder_id.0}),
                    changed_at_ms,
                )?;
                let mut next = (*snapshot).clone();
                crate::smart::replace_query(transaction, &mut next, smart_folder_id, view.clone())?;
                next.revision = revision;
                let after = load_smart_folder_definition(transaction, smart_folder_id)?
                    .ok_or_else(|| {
                        LibraryError::InvalidState(format!(
                            "smart folder {} disappeared during update",
                            smart_folder_id.0
                        ))
                    })?;
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
                        history: (before != after).then(|| {
                            HistoryEntry::for_command(
                                "smart_folders.update",
                                "Update smart folder",
                                SemanticChange::SmartFolderDefinition {
                                    smart_folder_id,
                                    before: Some(Box::new(before)),
                                    after: Some(Box::new(after)),
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

    pub fn delete_smart_folder(
        &self,
        smart_folder_id: SmartFolderId,
    ) -> Result<SmartFolderDeleteResult> {
        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (result, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut definitions = load_smart_folder_subtree(transaction, smart_folder_id)?;
                let fallback_smart_folder_id = definitions
                    .iter()
                    .find(|(definition, _)| definition.smart_folder_id == smart_folder_id)
                    .and_then(|(definition, _)| definition.parent_id);
                definitions.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
                let mut next = (*snapshot).clone();
                let mut changes = Vec::with_capacity(definitions.len());
                for (definition, _) in &definitions {
                    transaction.execute(
                        "DELETE FROM smart_folder_definition WHERE smart_folder_id = ?1",
                        [definition.smart_folder_id.0],
                    )?;
                    crate::smart::remove(&mut next, definition.smart_folder_id);
                    changes.push(SemanticChange::SmartFolderDefinition {
                        smart_folder_id: definition.smart_folder_id,
                        before: Some(Box::new(definition.clone())),
                        after: None,
                    });
                }
                insert_cloud_journal(
                    transaction,
                    revision,
                    "smart_folder.delete",
                    None,
                    serde_json::json!({
                        "smart_folder_ids": definitions.iter()
                            .map(|(definition, _)| definition.smart_folder_id.0)
                            .collect::<Vec<_>>()
                    }),
                    changed_at_ms,
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["smart-folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    SmartFolderDeleteResult {
                        deleted_smart_folder_ids: definitions
                            .iter()
                            .map(|(definition, _)| definition.smart_folder_id)
                            .collect(),
                        fallback_smart_folder_id,
                        receipt: receipt.clone(),
                    },
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "smart_folders.delete",
                            "Delete smart folder",
                            SemanticChange::Compound(changes),
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(result)
    }

    pub fn reorder_smart_folder_children(
        &self,
        parent_id: Option<SmartFolderId>,
        smart_folder_ids: &[SmartFolderId],
    ) -> Result<MutationReceipt> {
        let requested = smart_folder_ids.iter().map(|id| id.0).collect::<Vec<_>>();
        if requested.iter().copied().collect::<HashSet<_>>().len() != requested.len() {
            return Err(LibraryError::InvalidInput(
                "smart-folder reorder contains duplicate IDs".into(),
            ));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                validate_smart_parent(transaction, parent_id, None)?;
                let mut statement = transaction.prepare(
                    "SELECT smart_folder_id FROM smart_folder_definition
                     WHERE parent_id IS ?1 ORDER BY display_order, smart_folder_id",
                )?;
                let current = statement
                    .query_map([parent_id.map(|id| id.0)], |row| {
                        row.get::<_, u32>(0).map(SmartFolderId)
                    })?
                    .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
                if current.iter().map(|id| id.0).collect::<BTreeSet<_>>()
                    != requested.iter().copied().collect::<BTreeSet<_>>()
                    || current.len() != requested.len()
                {
                    return Err(LibraryError::InvalidInput(
                        "smart-folder reorder must contain every sibling exactly once".into(),
                    ));
                }
                let before = current
                    .iter()
                    .map(|id| {
                        load_smart_folder_definition(transaction, *id)?.ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "smart folder {} disappeared during reorder",
                                id.0
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut update = transaction.prepare_cached(
                    "UPDATE smart_folder_definition SET display_order = ?2
                     WHERE smart_folder_id = ?1",
                )?;
                for (display_order, id) in smart_folder_ids.iter().enumerate() {
                    update.execute(rusqlite::params![id.0, display_order as i64])?;
                }
                let mut after_by_id = HashMap::new();
                for id in smart_folder_ids {
                    let after =
                        load_smart_folder_definition(transaction, *id)?.ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "smart folder {} disappeared during reorder",
                                id.0
                            ))
                        })?;
                    after_by_id.insert(*id, after);
                }
                let changes = before
                    .into_iter()
                    .filter_map(|before| {
                        let after = after_by_id.remove(&before.smart_folder_id)?;
                        (before != after).then_some(SemanticChange::SmartFolderDefinition {
                            smart_folder_id: before.smart_folder_id,
                            before: Some(Box::new(before)),
                            after: Some(Box::new(after)),
                        })
                    })
                    .collect::<Vec<_>>();
                insert_cloud_journal(
                    transaction,
                    revision,
                    "smart_folder.reorder",
                    None,
                    serde_json::json!({"parent_id": parent_id.map(|id| id.0)}),
                    now_ms(),
                )?;
                let mut next = (*snapshot).clone();
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
                        history: (!changes.is_empty()).then(|| {
                            HistoryEntry::for_command(
                                "smart_folders.reorder",
                                "Reorder smart folders",
                                SemanticChange::Compound(changes),
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
                ingest::persist_touched(
                    transaction,
                    revision,
                    &next,
                    bitmap_keys,
                    folder_ids,
                    root_ids.iter().copied(),
                )?;
                let affected = root_ids.iter().map(|root| root.0).collect();
                crate::smart::settle_affected(transaction, &mut next, &affected)?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "root.ingest",
                    Some(&affected),
                    serde_json::json!({"count": root_ids.len()}),
                    inputs
                        .iter()
                        .map(|input| input.imported_at_ms)
                        .max()
                        .unwrap_or_else(now_ms),
                )?;
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

    pub fn ingest_collection(
        &self,
        input: &PreparedCollectionImport,
    ) -> Result<(RootId, MutationReceipt)> {
        if input.members.len() < 2 {
            return Err(LibraryError::InvalidInput(
                "a collection import requires at least two media members".into(),
            ));
        }
        if input.cover_index >= input.members.len() {
            return Err(LibraryError::InvalidInput(
                "collection cover index is outside the member list".into(),
            ));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let ((collection_id, receipt), _, ()) = self.database.published_write(
            WorkPriority::CanonicalIngest,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut next = (*snapshot).clone();
                let mut root_ids = Vec::with_capacity(input.members.len());
                let mut resources = BTreeSet::new();
                let mut bitmap_keys = HashSet::new();
                let mut folder_ids = HashSet::new();
                for member in &input.members {
                    let output = ingest::insert_one(transaction, revision, next, member)?;
                    next = output.snapshot;
                    root_ids.push(output.root_id);
                    resources.extend(output.resources);
                    bitmap_keys.extend(output.bitmap_keys);
                    folder_ids.extend(output.folder_ids);
                }
                let output = crate::group::organize(
                    transaction,
                    revision,
                    next,
                    &GroupRequest {
                        target: SelectionTarget::Explicit {
                            root_ids: root_ids.clone(),
                        },
                        cover_root_id: root_ids[input.cover_index],
                        winning_collection_id: None,
                        name: input.name.clone(),
                        modified_at_ms: input.modified_at_ms,
                    },
                )?;
                let mut next = output.snapshot;
                ingest::persist_touched(
                    transaction,
                    revision,
                    &next,
                    bitmap_keys,
                    folder_ids,
                    root_ids.iter().copied(),
                )?;
                crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
                resources.extend([
                    "collections".to_owned(),
                    "tags".to_owned(),
                    "folders".to_owned(),
                ]);
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    std::iter::once(output.collection_id),
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
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok((collection_id, receipt))
    }

    pub fn settle_fts(&self, limit: usize) -> Result<Option<MutationReceipt>> {
        if limit == 0 {
            return Ok(None);
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let published = self.database.published_write_if_changed(
            WorkPriority::Fts,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let settled = crate::fts::settle_batch(transaction, limit)?;
                if settled.is_empty() {
                    return Ok(None);
                }
                let mut next = (*snapshot).clone();
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &settled,
                    crate::predicate::DependencyChange::RootText,
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["search".into(), "smart-folders".into()],
                    settled.iter().map(RootId),
                );
                Ok(Some((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                )))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok(published.map(|(receipt, _, ())| receipt))
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

    pub fn record_duplicate_pair(
        &self,
        file_id_a: FileId,
        file_id_b: FileId,
        distance: u32,
        detected_at_ms: i64,
    ) -> Result<Option<MutationReceipt>> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let published = self.database.published_write_if_changed(
            WorkPriority::Maintenance,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let Some(pair) = crate::duplicate::record_detected(
                    transaction,
                    file_id_a,
                    file_id_b,
                    distance,
                    detected_at_ms,
                )?
                else {
                    return Ok(None);
                };
                let affected = crate::duplicate::affected_roots(
                    transaction,
                    &snapshot,
                    pair.file_id_a,
                    pair.file_id_b,
                )?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "duplicate.detect",
                    (!affected.is_empty()).then_some(&affected),
                    serde_json::json!({
                        "file_id_a": pair.file_id_a.0,
                        "file_id_b": pair.file_id_b.0,
                        "distance": pair.distance,
                    }),
                    detected_at_ms,
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["duplicates".into()],
                    affected.iter().map(RootId),
                );
                Ok(Some((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                )))
            },
            move |_, delta| {
                publish_delta(&projections, &publication, delta);
            },
        )?;
        Ok(published.map(|(receipt, _, ())| receipt))
    }

    pub fn duplicate_pairs(
        &self,
        status: Option<DuplicateStatus>,
        limit: usize,
    ) -> Result<Vec<DuplicatePair>> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            crate::duplicate::list_pairs(connection, status, limit)
        })
    }

    pub fn resolve_duplicate(
        &self,
        file_id_a: FileId,
        file_id_b: FileId,
        choice: DuplicateResolutionChoice,
        decided_at_ms: i64,
    ) -> Result<DuplicateResolutionResult> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (result, _, ()) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut output = crate::duplicate::resolve(
                    transaction,
                    revision,
                    (*snapshot).clone(),
                    file_id_a,
                    file_id_b,
                    choice,
                    decided_at_ms,
                )?;
                if matches!(choice, DuplicateResolutionChoice::KeepFile { .. }) {
                    crate::smart::settle_affected(
                        transaction,
                        &mut output.snapshot,
                        &output.affected_roots,
                    )?;
                }
                insert_cloud_journal(
                    transaction,
                    revision,
                    "duplicate.resolve",
                    (!output.affected_roots.is_empty()).then_some(&output.affected_roots),
                    serde_json::json!({
                        "file_id_a": output.history.file_id_a().0,
                        "file_id_b": output.history.file_id_b().0,
                        "choice": choice,
                    }),
                    decided_at_ms,
                )?;
                output.snapshot.revision = revision;
                let resources = if matches!(choice, DuplicateResolutionChoice::KeepFile { .. }) {
                    vec![
                        "duplicates".into(),
                        "media".into(),
                        "roots".into(),
                        "smart-folders".into(),
                    ]
                } else {
                    vec!["duplicates".into()]
                };
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    output.affected_roots.iter().map(RootId),
                );
                let result = DuplicateResolutionResult {
                    choice,
                    affected_root_ids: receipt.item_ids.clone(),
                    receipt: receipt.clone(),
                };
                Ok((
                    result,
                    PublishedDelta {
                        snapshot: output.snapshot,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "duplicates.resolve",
                            "Resolve duplicate",
                            SemanticChange::DuplicateResolution(output.history),
                        )),
                    },
                ))
            },
            move |_, delta| {
                let history_entry = publish_delta(&projections, &publication, delta);
                push_history(&history, history_entry);
            },
        )?;
        Ok(result)
    }

    pub fn pending_blob_cleanup(&self, limit: usize) -> Result<Vec<PendingBlobCleanup>> {
        self.database.read_consistent(
            WorkPriority::Maintenance,
            |_| Ok(self.history.protected_cleanup_files()),
            |connection, protected| crate::duplicate::ready_cleanup(connection, &protected, limit),
        )
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
                let mut next = (*snapshot).clone();
                let affected = {
                    let media_ids = transaction
                        .prepare_cached("SELECT media_id FROM media_item WHERE file_id = ?1")?
                        .query_map([file_id], |row| row.get::<_, u32>(0))?
                        .collect::<std::result::Result<Vec<_>, _>>()?;
                    let image_media = Arc::make_mut(&mut next.image_media);
                    let mut roots = RoaringBitmap::new();
                    for media_id in media_ids {
                        if mime.starts_with("image/") {
                            image_media.insert(media_id);
                        } else {
                            image_media.remove(media_id);
                        }
                        if let Some(owner) = snapshot.media_owner.get(media_id) {
                            roots.insert(owner.0);
                        }
                    }
                    roots
                };
                for root_id in &affected {
                    let has_image = next.collection_orders.get(&RootId(root_id)).map_or_else(
                        || next.image_media.contains(root_id),
                        |members| {
                            members
                                .iter()
                                .any(|media_id| next.image_media.contains(media_id.0))
                        },
                    );
                    if has_image {
                        Arc::make_mut(&mut next.roots_with_images).insert(root_id);
                    } else {
                        Arc::make_mut(&mut next.roots_with_images).remove(root_id);
                    }
                    crate::group::refresh_cover_projection(
                        transaction,
                        &mut next,
                        RootId(root_id),
                    )?;
                    crate::group::refresh_root_mime_projection(
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
            "items.set_lifecycle",
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
            "items.patch_metadata",
            "Change rating",
            BitmapDomain::Rating,
            rating.bitmap_key(),
            Rating::ALL.iter().map(|value| value.bitmap_key()).collect(),
            target,
            vec!["roots".into(), "ratings".into()],
        )
    }

    pub fn patch_metadata(
        &self,
        target: &SelectionTarget,
        rating: Option<Rating>,
        notes: Option<Option<String>>,
        source_urls: Option<Vec<String>>,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        if rating.is_none() && notes.is_none() && source_urls.is_none() {
            return Err(LibraryError::InvalidInput(
                "metadata patch does not change a supported field".into(),
            ));
        }
        let target = target.clone();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
                let mut next = (*snapshot).clone();
                let mut changes = Vec::new();
                let mut resources = BTreeSet::from(["roots".to_string()]);

                if let Some(rating) = rating {
                    for value in Rating::ALL {
                        let key = BitmapKey {
                            domain: BitmapDomain::Rating,
                            key_id: value.bitmap_key(),
                        };
                        let before = projection_bitmap(&next, key);
                        let mut after = before.clone();
                        if value == rating {
                            after |= &selection;
                        } else {
                            after -= &selection;
                        }
                        if before == after {
                            continue;
                        }
                        set_projection_bitmap(&mut next, key, after.clone());
                        bitmap::replace(transaction, revision, key, &after)?;
                        changes.push(SemanticChange::Bitmap {
                            key,
                            before: Arc::new(before),
                            after: Arc::new(after),
                        });
                    }
                    resources.insert("ratings".into());
                }

                if notes.is_some() || source_urls.is_some() {
                    let before = load_root_text(transaction, &selection)?;
                    let mut after = before.clone();
                    for state in &mut after {
                        if let Some(notes) = &notes {
                            state.notes.clone_from(notes);
                        }
                        if let Some(source_urls) = &source_urls {
                            state.source_urls.clone_from(source_urls);
                        }
                        state.modified_at_ms = modified_at_ms;
                    }
                    if before != after {
                        let mut update = transaction.prepare_cached(
                            "UPDATE library_root
                             SET notes = ?2, source_urls_json = ?3, modified_at_ms = ?4
                             WHERE root_id = ?1",
                        )?;
                        for state in &after {
                            update.execute(rusqlite::params![
                                state.root_id.0,
                                state.notes,
                                serde_json::to_string(&state.source_urls)?,
                                state.modified_at_ms,
                            ])?;
                            Arc::make_mut(&mut next.modified_at)
                                .insert(state.root_id.0, state.modified_at_ms.max(0) as u64);
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
                        drop(update);
                        crate::fts::mark_dirty(transaction, &selection, 1, modified_at_ms)?;
                        changes.push(SemanticChange::RootText {
                            before: Arc::new(before),
                            after: Arc::new(after),
                        });
                        resources.insert("search".into());
                    }
                }

                if changes.is_empty() {
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
                crate::smart::settle_affected(transaction, &mut next, &selection)?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "root.metadata.patch",
                    Some(&selection),
                    serde_json::json!({
                        "rating": rating.map(|value| value.bitmap_key()),
                        "notes": notes.is_some(),
                        "source_urls": source_urls.is_some(),
                    }),
                    modified_at_ms,
                )?;
                resources.insert("smart-folders".into());
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    selection.iter().map(RootId),
                );
                let history = HistoryEntry::for_command(
                    "items.patch_metadata",
                    "Change metadata",
                    SemanticChange::Compound(changes),
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

    pub fn rename_root(
        &self,
        root_id: RootId,
        name: &str,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        self.rename_roots(&[(root_id, name.to_owned())], modified_at_ms)
    }

    pub fn rename_roots(
        &self,
        renames: &[(RootId, String)],
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        if renames.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one root rename is required".into(),
            ));
        }
        let mut names = HashMap::new();
        for (root_id, name) in renames {
            let name = required_name("root", name)?;
            if names.insert(*root_id, name).is_some() {
                return Err(LibraryError::InvalidInput(format!(
                    "root {} appears more than once in the rename request",
                    root_id.0
                )));
            }
        }
        let requested = names
            .keys()
            .map(|root_id| root_id.0)
            .collect::<RoaringBitmap>();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let loaded = load_root_text(transaction, &requested)?;
                if loaded.len() != names.len() {
                    return Err(LibraryError::NotFound(
                        "one or more roots to rename do not exist".into(),
                    ));
                }
                let mut before = Vec::new();
                let mut after = Vec::new();
                let mut changed = RoaringBitmap::new();
                for state in loaded {
                    let name = names
                        .get(&state.root_id)
                        .expect("loaded roots are present in the rename map");
                    if state.name == *name {
                        continue;
                    }
                    let mut next_state = state.clone();
                    next_state.name.clone_from(name);
                    next_state.modified_at_ms = modified_at_ms;
                    changed.insert(state.root_id.0);
                    before.push(state);
                    after.push(next_state);
                }
                let mut next = (*snapshot).clone();
                if changed.is_empty() {
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
                let mut root_update = transaction.prepare_cached(
                    "UPDATE library_root SET name = ?2, modified_at_ms = ?3 WHERE root_id = ?1",
                )?;
                let mut media_update = transaction.prepare_cached(
                    "UPDATE media_item SET media_name = ?2
                     WHERE media_id = ?1 AND EXISTS(
                         SELECT 1 FROM library_item WHERE local_id = ?1 AND item_kind = 1
                     )",
                )?;
                for state in &after {
                    root_update.execute(rusqlite::params![
                        state.root_id.0,
                        state.name,
                        state.modified_at_ms
                    ])?;
                    media_update.execute(rusqlite::params![state.root_id.0, state.name])?;
                    Arc::make_mut(&mut next.modified_at)
                        .insert(state.root_id.0, state.modified_at_ms.max(0) as u64);
                }
                drop(root_update);
                drop(media_update);
                crate::fts::mark_dirty(transaction, &changed, 1, modified_at_ms)?;
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &changed,
                    crate::predicate::DependencyChange::RootText,
                )?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "root.rename",
                    Some(&changed),
                    serde_json::json!({"count": changed.len()}),
                    modified_at_ms,
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "search".into(), "smart-folders".into()],
                    changed.iter().map(RootId),
                );
                let history = HistoryEntry::for_command(
                    if changed.len() == 1 {
                        "items.rename"
                    } else {
                        "items.rename_many"
                    },
                    if changed.len() == 1 {
                        "Rename item"
                    } else {
                        "Rename items"
                    },
                    SemanticChange::RootText {
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

    pub fn set_notes(
        &self,
        target: &SelectionTarget,
        notes: Option<String>,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        self.text_mutation(
            target,
            "items.patch_metadata",
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
            "items.patch_metadata",
            "Change source URLs",
            TextMutation::SourceUrls(source_urls),
            modified_at_ms,
        )
    }

    pub fn add_tag(&self, target: &SelectionTarget, name: &str) -> Result<MutationReceipt> {
        self.apply_tags(target, &[name.to_owned()], true)
    }

    /// Apply independently predicted tag sets to visible roots in one exact
    /// publication. Collections are one root, so callers union all member
    /// predictions into one assignment before entering the library kernel.
    pub fn add_tag_assignments(
        &self,
        assignments: &[RootTagAssignment],
    ) -> Result<MutationReceipt> {
        if assignments.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one root tag assignment is required".into(),
            ));
        }
        let mut roots_by_tag = HashMap::<String, RoaringBitmap>::new();
        let mut requested_roots = RoaringBitmap::new();
        for assignment in assignments {
            requested_roots.insert(assignment.root_id.0);
            for tag in &assignment.tags {
                roots_by_tag
                    .entry(required_name("tag", tag)?)
                    .or_default()
                    .insert(assignment.root_id.0);
            }
        }
        if roots_by_tag.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one tag is required".into(),
            ));
        }

        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let every_root = snapshot.root_kinds.values().fold(
                    RoaringBitmap::new(),
                    |mut roots, members| {
                        roots |= members;
                        roots
                    },
                );
                let missing = &requested_roots - &every_root;
                if let Some(root_id) = missing.min() {
                    return Err(LibraryError::NotFound(format!("root {root_id}")));
                }

                let mut next = (*snapshot).clone();
                let mut changes = Vec::with_capacity(roots_by_tag.len());
                let mut affected = RoaringBitmap::new();
                for (name, requested) in &roots_by_tag {
                    let tag_id = if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                        tag_id
                    } else {
                        let tag_id = ingest::ensure_tag(transaction, name)?;
                        Arc::make_mut(&mut next.tag_ids_by_name).insert(name.clone(), tag_id);
                        tag_id
                    };
                    let before = next
                        .tags
                        .get(&tag_id)
                        .map(|members| members.to_bitmap())
                        .unwrap_or_default();
                    let mut after = before.clone();
                    after |= requested;
                    let changed = &after - &before;
                    if changed.is_empty() {
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
                    Arc::make_mut(&mut next.tags).insert(tag_id, after.clone().into());
                    let counts = Arc::make_mut(&mut next.tag_count);
                    for root_id in &changed {
                        counts.insert(root_id, counts.value(root_id).unwrap_or(0) + 1);
                    }
                    affected |= &changed;
                    changes.push(SemanticChange::Bitmap {
                        key: BitmapKey {
                            domain: BitmapDomain::Tag,
                            key_id: tag_id.0,
                        },
                        before: Arc::new(before),
                        after: Arc::new(after),
                    });
                }

                if !affected.is_empty() {
                    crate::smart::settle_affected(transaction, &mut next, &affected)?;
                    insert_cloud_journal(
                        transaction,
                        revision,
                        "tag.ai_batch",
                        Some(&affected),
                        serde_json::json!({"assignment_count": assignments.len()}),
                        changed_at_ms,
                    )?;
                }
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    if affected.is_empty() {
                        Vec::new()
                    } else {
                        vec![
                            "roots".into(),
                            "tags".into(),
                            "sidebar".into(),
                            "smart-folders".into(),
                        ]
                    },
                    affected.iter().map(RootId),
                );
                let history = (!changes.is_empty()).then(|| {
                    HistoryEntry::for_command(
                        "items.apply_ai_tags",
                        "Apply AI tags",
                        SemanticChange::Compound(changes),
                    )
                });
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

    pub fn rename_tag(&self, tag_id: crate::TagId, name: &str) -> Result<MutationReceipt> {
        let name = required_name("tag", name)?;
        let changed_at_ms = now_ms();
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
                insert_cloud_journal(
                    transaction,
                    revision,
                    "tag.rename",
                    None,
                    serde_json::json!({"tag_id": tag_id.0, "name": name}),
                    changed_at_ms,
                )?;
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
                            HistoryEntry::for_command(
                                "tags.rename_or_merge",
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

    pub fn rename_tag_namespace(
        &self,
        namespace_id: TagNamespaceId,
        name: &str,
    ) -> Result<MutationReceipt> {
        let name = required_name("tag namespace", name)?;
        if name.contains(':') {
            return Err(LibraryError::InvalidInput(
                "tag namespace cannot contain a colon".into(),
            ));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let before = load_namespace_name(transaction, namespace_id)?;
                let mut next = (*snapshot).clone();
                apply_namespace_name(transaction, &mut next, namespace_id, &name)?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "tag.namespace.rename",
                    None,
                    serde_json::json!({"namespace_id": namespace_id.0, "name": name}),
                    now_ms(),
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["tags".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (before != name).then(|| {
                            HistoryEntry::for_command(
                                "tags.group.rename",
                                "Rename tag namespace",
                                SemanticChange::TagNamespaceName {
                                    namespace_id,
                                    before,
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
        let changed_at_ms = now_ms();
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
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.create",
                    None,
                    serde_json::json!({
                        "folder_id": folder_id.0,
                        "parent_id": parent_id.map(|id| id.0),
                        "name": name,
                    }),
                    changed_at_ms,
                )?;
                let mut next = (*snapshot).clone();
                Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(Vec::new()));
                Arc::make_mut(&mut next.folders).insert(folder_id, RoaringBitmap::new().into());
                next.revision = revision;
                let after = load_folder_definition(transaction, folder_id)?.ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "new folder {} disappeared before publication",
                        folder_id.0
                    ))
                })?;
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
                        history: Some(HistoryEntry::for_command(
                            "folders.create",
                            "Create folder",
                            SemanticChange::FolderDefinition {
                                folder_id,
                                before: None,
                                after: Some(Box::new(after)),
                            },
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((folder_id, receipt))
    }

    pub fn set_folder_metadata(
        &self,
        folder_id: FolderId,
        icon: Option<&str>,
        color: Option<&str>,
        notes: Option<&str>,
    ) -> Result<MutationReceipt> {
        let icon = normalized_optional(icon);
        let color = normalized_optional(color);
        let notes = normalized_optional(notes);
        self.update_folder_definition(
            "folders.set_metadata",
            "Edit folder",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition SET icon = ?2, color = ?3, notes = ?4
                 WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, icon, color, notes],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!("folder {folder_id}")));
                }
                Ok(())
            },
        )
    }

    pub fn move_folder(
        &self,
        folder_id: FolderId,
        parent_id: Option<FolderId>,
    ) -> Result<MutationReceipt> {
        self.update_folder_definition(
            "folders.move",
            "Move folder",
            folder_id,
            move |transaction| {
                validate_folder_parent(transaction, parent_id, Some(folder_id))?;
                let name = transaction
                    .query_row(
                        "SELECT name FROM folder_definition WHERE folder_id = ?1",
                        [folder_id.0],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => {
                            LibraryError::NotFound(format!("folder {folder_id}"))
                        }
                        error => error.into(),
                    })?;
                let duplicate = transaction.query_row(
                    "SELECT EXISTS(
                     SELECT 1 FROM folder_definition
                     WHERE parent_id IS ?1 AND name = ?2 AND folder_id != ?3
                 )",
                    rusqlite::params![parent_id.map(|id| id.0), name, folder_id.0],
                    |row| row.get::<_, bool>(0),
                )?;
                if duplicate {
                    return Err(LibraryError::InvalidInput(format!(
                        "a sibling folder named {name} already exists"
                    )));
                }
                let display_order = transaction.query_row(
                    "SELECT COALESCE(MAX(display_order) + 1, 0)
                 FROM folder_definition WHERE parent_id IS ?1 AND folder_id != ?2",
                    rusqlite::params![parent_id.map(|id| id.0), folder_id.0],
                    |row| row.get::<_, i64>(0),
                )?;
                transaction.execute(
                    "UPDATE folder_definition SET parent_id = ?2, display_order = ?3
                 WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, parent_id.map(|id| id.0), display_order],
                )?;
                Ok(())
            },
        )
    }

    pub fn reorder_folder_children(
        &self,
        parent_id: Option<FolderId>,
        folder_ids: &[FolderId],
    ) -> Result<MutationReceipt> {
        let requested = folder_ids
            .iter()
            .map(|folder_id| folder_id.0)
            .collect::<Vec<_>>();
        if requested.iter().copied().collect::<HashSet<_>>().len() != requested.len() {
            return Err(LibraryError::InvalidInput(
                "folder reorder contains duplicate IDs".into(),
            ));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                validate_folder_parent(transaction, parent_id, None)?;
                let current = load_folder_children(transaction, parent_id)?;
                let current_set = current
                    .iter()
                    .map(|folder| folder.0)
                    .collect::<BTreeSet<_>>();
                let requested_set = requested.iter().copied().collect::<BTreeSet<_>>();
                if current_set != requested_set || current.len() != requested.len() {
                    return Err(LibraryError::InvalidInput(
                        "folder reorder must contain every sibling exactly once".into(),
                    ));
                }
                let before = current
                    .iter()
                    .map(|folder_id| {
                        load_folder_definition(transaction, *folder_id)?.ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "folder {} disappeared during reorder",
                                folder_id.0
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut update = transaction.prepare_cached(
                    "UPDATE folder_definition SET display_order = ?2 WHERE folder_id = ?1",
                )?;
                for (display_order, folder_id) in folder_ids.iter().enumerate() {
                    update.execute(rusqlite::params![folder_id.0, display_order as i64])?;
                }
                let after = folder_ids
                    .iter()
                    .map(|folder_id| {
                        load_folder_definition(transaction, *folder_id)?.ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "folder {} disappeared during reorder",
                                folder_id.0
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let mut after_by_id = after
                    .into_iter()
                    .map(|state| (state.folder_id, state))
                    .collect::<HashMap<_, _>>();
                let changes = before
                    .into_iter()
                    .filter_map(|before| {
                        let after = after_by_id.remove(&before.folder_id)?;
                        (before != after).then_some(SemanticChange::FolderDefinition {
                            folder_id: before.folder_id,
                            before: Some(Box::new(before)),
                            after: Some(Box::new(after)),
                        })
                    })
                    .collect::<Vec<_>>();
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.reorder",
                    None,
                    serde_json::json!({"parent_id": parent_id.map(|id| id.0)}),
                    now_ms(),
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (!changes.is_empty()).then(|| {
                            HistoryEntry::for_command(
                                "folders.reorder",
                                "Reorder folders",
                                SemanticChange::Compound(changes),
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

    pub fn reorder_folder_items(
        &self,
        folder_id: FolderId,
        root_ids: &[RootId],
    ) -> Result<MutationReceipt> {
        let values = root_ids.iter().map(|root_id| root_id.0).collect::<Vec<_>>();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                require_folder(transaction, folder_id)?;
                let before = snapshot
                    .folder_orders
                    .get(&folder_id)
                    .map(|order| order.iter().map(|root| root.0).collect::<Vec<_>>())
                    .unwrap_or_default();
                if before.iter().copied().collect::<BTreeSet<_>>()
                    != values.iter().copied().collect::<BTreeSet<_>>()
                    || before.len() != values.len()
                {
                    return Err(LibraryError::InvalidInput(
                        "folder item reorder must contain every member exactly once".into(),
                    ));
                }
                ordering::replace(
                    transaction,
                    revision,
                    OrderOwnerKind::Folder,
                    folder_id.0,
                    &values,
                )?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.items.reorder",
                    None,
                    serde_json::json!({"folder_id": folder_id.0}),
                    now_ms(),
                )?;
                let mut next = (*snapshot).clone();
                Arc::make_mut(&mut next.folder_orders).insert(
                    folder_id,
                    Arc::new(values.iter().copied().map(RootId).collect()),
                );
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["folders".into(), format!("folder:{}", folder_id.0)],
                    if values.len() <= 256 {
                        values.iter().copied().map(RootId).collect()
                    } else {
                        Vec::new()
                    },
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (before != values).then(|| {
                            HistoryEntry::for_command(
                                "folders.sort_items",
                                "Reorder folder items",
                                SemanticChange::Order {
                                    owner_kind: OrderOwnerKind::Folder,
                                    owner_id: folder_id.0,
                                    before: Arc::new(before),
                                    after: Arc::new(values.clone()),
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

    pub fn sort_folder_items_by_name(&self, folder_id: FolderId) -> Result<MutationReceipt> {
        let ordered = self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                require_folder(connection, folder_id)?;
                let roots = snapshot
                    .folder_orders
                    .get(&folder_id)
                    .map(AsRef::as_ref)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let mut statement = connection
                    .prepare_cached("SELECT name FROM library_root WHERE root_id = ?1")?;
                let mut values = roots
                    .iter()
                    .map(|root_id| {
                        statement
                            .query_row([root_id.0], |row| row.get::<_, String>(0))
                            .map(|name| (name.to_lowercase(), *root_id))
                            .map_err(Into::into)
                    })
                    .collect::<Result<Vec<_>>>()?;
                values.sort();
                Ok(values
                    .into_iter()
                    .map(|(_, root_id)| root_id)
                    .collect::<Vec<_>>())
            },
        )?;
        self.reorder_folder_items(folder_id, &ordered)
    }

    pub fn delete_folders(&self, folder_ids: &[FolderId]) -> Result<FolderDeleteResult> {
        if folder_ids.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one folder is required".into(),
            ));
        }
        let requested = folder_ids
            .iter()
            .copied()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (result, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let subtree = load_folder_subtree(transaction, &requested)?;
                let present = subtree
                    .iter()
                    .map(|(folder_id, _)| *folder_id)
                    .collect::<BTreeSet<_>>();
                if requested
                    .iter()
                    .any(|folder_id| !present.contains(folder_id))
                {
                    return Err(LibraryError::NotFound(
                        "one or more selected folders do not exist".into(),
                    ));
                }
                let definitions = subtree
                    .iter()
                    .map(|(folder_id, depth)| {
                        load_folder_definition(transaction, *folder_id)?
                            .map(|definition| (definition, *depth))
                            .ok_or_else(|| {
                                LibraryError::InvalidState(format!(
                                    "folder {} disappeared during deletion",
                                    folder_id.0
                                ))
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let fallback_folder_id = if requested.len() == 1 {
                    let mut parent = definitions
                        .iter()
                        .find(|(definition, _)| definition.folder_id == requested[0])
                        .and_then(|(definition, _)| definition.parent_id);
                    while parent.is_some_and(|folder_id| present.contains(&folder_id)) {
                        parent = definitions
                            .iter()
                            .find(|(definition, _)| Some(definition.folder_id) == parent)
                            .and_then(|(definition, _)| definition.parent_id);
                    }
                    parent
                } else {
                    None
                };

                let mut next = (*snapshot).clone();
                let mut affected = RoaringBitmap::new();
                let mut history_changes = Vec::new();
                for (definition, _) in &definitions {
                    let before = next
                        .folder_orders
                        .get(&definition.folder_id)
                        .map(|order| order.iter().map(|root| root.0).collect::<Vec<_>>())
                        .unwrap_or_default();
                    if !before.is_empty() {
                        history_changes.push(SemanticChange::Order {
                            owner_kind: OrderOwnerKind::Folder,
                            owner_id: definition.folder_id.0,
                            before: Arc::new(before.clone()),
                            after: Arc::new(Vec::new()),
                        });
                    }
                    let members = before.iter().copied().collect::<RoaringBitmap>();
                    affected |= &members;
                    let counts = Arc::make_mut(&mut next.folder_count);
                    for root_id in &members {
                        let current = counts.value(root_id).unwrap_or(0);
                        counts.insert(root_id, current.saturating_sub(1));
                    }
                    ordering::delete(transaction, OrderOwnerKind::Folder, definition.folder_id.0)?;
                    Arc::make_mut(&mut next.folder_orders).remove(&definition.folder_id);
                    Arc::make_mut(&mut next.folders).remove(&definition.folder_id);
                }

                let mut child_first = definitions.clone();
                child_first.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
                for (definition, _) in &child_first {
                    transaction.execute(
                        "DELETE FROM folder_definition WHERE folder_id = ?1",
                        [definition.folder_id.0],
                    )?;
                    history_changes.push(SemanticChange::FolderDefinition {
                        folder_id: definition.folder_id,
                        before: Some(Box::new(definition.clone())),
                        after: None,
                    });
                }
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &affected,
                    crate::predicate::DependencyChange::Folders,
                )?;
                next.revision = revision;
                let deleted_folder_ids = definitions
                    .iter()
                    .map(|(definition, _)| definition.folder_id)
                    .collect::<Vec<_>>();
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.delete",
                    None,
                    serde_json::json!({
                        "folder_ids": deleted_folder_ids.iter().map(|id| id.0).collect::<Vec<_>>()
                    }),
                    now_ms(),
                )?;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec![
                        "folders".into(),
                        "navigation".into(),
                        "sidebar".into(),
                        "smart-folders".into(),
                    ],
                    if affected.len() <= 256 {
                        affected.iter().map(RootId).collect()
                    } else {
                        Vec::new()
                    },
                );
                Ok((
                    FolderDeleteResult {
                        deleted_folder_ids,
                        fallback_folder_id,
                        receipt: receipt.clone(),
                    },
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "folders.delete",
                            "Delete folders",
                            SemanticChange::Compound(history_changes),
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(result)
    }

    fn update_folder_definition<F>(
        &self,
        command: &'static str,
        label: &'static str,
        folder_id: FolderId,
        update: F,
    ) -> Result<MutationReceipt>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<()>,
    {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let before = load_folder_definition(transaction, folder_id)?
                    .ok_or_else(|| LibraryError::NotFound(format!("folder {folder_id}")))?;
                update(transaction)?;
                let after = load_folder_definition(transaction, folder_id)?.ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "folder {} disappeared during update",
                        folder_id.0
                    ))
                })?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.update",
                    None,
                    serde_json::json!({"folder_id": folder_id.0}),
                    now_ms(),
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (before != after).then(|| {
                            HistoryEntry::for_command(
                                command,
                                label,
                                SemanticChange::FolderDefinition {
                                    folder_id,
                                    before: Some(Box::new(before)),
                                    after: Some(Box::new(after)),
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

    pub fn rename_folder(&self, folder_id: FolderId, name: &str) -> Result<MutationReceipt> {
        let name = required_name("folder", name)?;
        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let before = transaction
                    .query_row(
                        "SELECT name FROM folder_definition WHERE folder_id = ?1",
                        [folder_id.0],
                        |row| row.get::<_, String>(0),
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => {
                            LibraryError::NotFound(format!("folder {}", folder_id.0))
                        }
                        error => error.into(),
                    })?;
                rename_folder_definition(transaction, folder_id, &name)?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.rename",
                    None,
                    serde_json::json!({"folder_id": folder_id.0, "name": name}),
                    changed_at_ms,
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: (before != name).then(|| {
                            HistoryEntry::for_command(
                                "folders.rename",
                                "Rename folder",
                                SemanticChange::FolderName {
                                    folder_id,
                                    before,
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
                            HistoryEntry::for_command(
                                "folders.set_auto_tags",
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
        self.apply_tags(target, &[name.to_owned()], false)
    }

    pub fn apply_tags(
        &self,
        target: &SelectionTarget,
        names: &[String],
        add: bool,
    ) -> Result<MutationReceipt> {
        let names = names
            .iter()
            .map(|name| name.trim())
            .filter(|name| !name.is_empty())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one non-empty tag is required".into(),
            ));
        }
        self.tag_mutation(target, names, add)
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
                let selected = crate::selection::resolve(transaction, &snapshot, &request.target)?;
                let before = capture_structure(transaction, &snapshot, &selected)?;
                let output =
                    crate::group::organize(transaction, revision, (*snapshot).clone(), &request)?;
                let mut next = output.snapshot;
                crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
                let after = capture_structure(transaction, &next, &output.affected)?;
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
                        history: Some(HistoryEntry::for_command(
                            "collections.organize",
                            "Organize collection",
                            SemanticChange::Structure {
                                affected: Arc::new(output.affected),
                                before,
                                after,
                            },
                        )),
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
                let mut affected = snapshot
                    .collection_orders
                    .get(&collection_id)
                    .ok_or_else(|| {
                        LibraryError::InvalidInput(format!(
                            "root {collection_id} is not a collection"
                        ))
                    })?
                    .iter()
                    .map(|media| media.0)
                    .collect::<RoaringBitmap>();
                affected.insert(collection_id.0);
                let before = capture_structure(transaction, &snapshot, &affected)?;
                let output = crate::group::ungroup(
                    transaction,
                    revision,
                    (*snapshot).clone(),
                    collection_id,
                    modified_at_ms,
                )?;
                let mut next = output.snapshot;
                crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
                let after = capture_structure(transaction, &next, &output.affected)?;
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
                        history: Some(HistoryEntry::for_command(
                            "collections.ungroup",
                            "Ungroup collection",
                            SemanticChange::Structure {
                                affected: Arc::new(output.affected),
                                before,
                                after,
                            },
                        )),
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
        let (mut roots, receipt) =
            self.detach_collection_members(collection_id, vec![media_id], None, modified_at_ms)?;
        let root_id = roots
            .pop()
            .ok_or_else(|| LibraryError::InvalidState("detach produced no root".into()))?;
        Ok((root_id, receipt))
    }

    pub fn detach_collection_members(
        &self,
        collection_id: RootId,
        media_ids: Vec<MediaId>,
        target_lifecycle: Option<Lifecycle>,
        modified_at_ms: i64,
    ) -> Result<(Vec<RootId>, MutationReceipt)> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((root_ids, receipt), _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut affected = media_ids
                    .iter()
                    .map(|media_id| media_id.0)
                    .collect::<RoaringBitmap>();
                affected.insert(collection_id.0);
                let before = capture_structure(transaction, &snapshot, &affected)?;
                let output = crate::group::detach_many(
                    transaction,
                    revision,
                    (*snapshot).clone(),
                    collection_id,
                    &media_ids,
                    target_lifecycle,
                    modified_at_ms,
                )?;
                let mut next = output.snapshot;
                crate::smart::settle_affected(transaction, &mut next, &output.affected)?;
                let after = capture_structure(transaction, &next, &output.affected)?;
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
                    (output.root_ids, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "collections.detach",
                            "Detach collection member",
                            SemanticChange::Structure {
                                affected: Arc::new(output.affected),
                                before,
                                after,
                            },
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((root_ids, receipt))
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
                    Some(HistoryEntry::for_command(
                        "collections.reorder",
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
                    Some(HistoryEntry::for_command(
                        "collections.set_cover",
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
        let protected_cleanup_files = self.history.protected_cleanup_files();
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
                        let file_id = crate::FileId(file_id);
                        if !protected_cleanup_files.contains(&file_id) {
                            cleanup.push(crate::PendingBlobCleanup { file_id, file_path });
                        }
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
                    Arc::make_mut(&mut next.media_owner).remove(media_id);
                    Arc::make_mut(&mut next.image_media).remove(media_id);
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
                let source_members = snapshot
                    .tags
                    .get(&source)
                    .map(|members| members.to_bitmap())
                    .unwrap_or_default();
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
                    let before = next
                        .tags
                        .get(&destination)
                        .map(|members| members.to_bitmap())
                        .unwrap_or_default();
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
                        Arc::make_mut(&mut next.tags).insert(destination, after.clone().into());
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
                    .map(|members| members.to_bitmap())
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
                        history: Some(HistoryEntry::for_command(
                            if destination.is_some() {
                                "tags.rename_or_merge"
                            } else {
                                "tags.delete"
                            },
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
        names: Vec<String>,
        add: bool,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
                let mut next = (*snapshot).clone();
                if selection.is_empty() {
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
                let mut affected = RoaringBitmap::new();
                let mut changes = Vec::new();
                let mut tag_ids = Vec::new();
                for name in &names {
                    let tag_id = if add {
                        if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                            tag_id
                        } else {
                            let tag_id = ingest::ensure_tag(transaction, name)?;
                            Arc::make_mut(&mut next.tag_ids_by_name).insert(name.clone(), tag_id);
                            tag_id
                        }
                    } else if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                        tag_id
                    } else {
                        continue;
                    };
                    let before = next
                        .tags
                        .get(&tag_id)
                        .map(|members| members.to_bitmap())
                        .unwrap_or_default();
                    let mut after = before.clone();
                    if add {
                        after |= &selection;
                    } else {
                        after -= &selection;
                    }
                    let changed = &before ^ &after;
                    if changed.is_empty() {
                        continue;
                    }
                    Arc::make_mut(&mut next.tags).insert(tag_id, after.clone().into());
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
                    affected |= &changed;
                    tag_ids.push(tag_id.0);
                    changes.push(SemanticChange::Bitmap {
                        key: BitmapKey {
                            domain: BitmapDomain::Tag,
                            key_id: tag_id.0,
                        },
                        before: Arc::new(before),
                        after: Arc::new(after),
                    });
                }
                if changes.is_empty() {
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
                crate::smart::settle_affected(transaction, &mut next, &affected)?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    if add { "tag.add" } else { "tag.remove" },
                    Some(&affected),
                    serde_json::json!({"tag_ids": tag_ids}),
                    changed_at_ms,
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "tags".into(), "sidebar".into()],
                    affected.iter().map(RootId),
                );
                let history = HistoryEntry::for_command(
                    "items.apply_tags",
                    if add { "Add tags" } else { "Remove tags" },
                    SemanticChange::Compound(changes),
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
        command: &'static str,
        label: &'static str,
        mutation: TextMutation,
        modified_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let operation_kind = match &mutation {
            TextMutation::Notes(_) => "root.notes",
            TextMutation::SourceUrls(_) => "root.source_urls",
        };
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let selection = crate::selection::resolve(transaction, &snapshot, &target)?;
                let mut before = load_root_text(transaction, &selection)?;
                let mut after = before.clone();
                for state in &mut after {
                    match &mutation {
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
                insert_cloud_journal(
                    transaction,
                    revision,
                    operation_kind,
                    Some(&selection),
                    serde_json::json!({}),
                    modified_at_ms,
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["roots".into(), "search".into(), "smart-folders".into()],
                    selection.iter().map(RootId),
                );
                let history = HistoryEntry::for_command(
                    command,
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
        let changed_at_ms = now_ms();
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
                Arc::make_mut(&mut next.folders).insert(folder_id, after_bitmap.clone().into());
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
                        let before = next
                            .tags
                            .get(&tag_id)
                            .map(|members| members.to_bitmap())
                            .unwrap_or_default();
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
                        Arc::make_mut(&mut next.tags).insert(tag_id, after.clone().into());
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
                insert_cloud_journal(
                    transaction,
                    revision,
                    if add { "folder.add" } else { "folder.remove" },
                    Some(&changed),
                    serde_json::json!({"folder_id": folder_id.0}),
                    changed_at_ms,
                )?;
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
                let history = HistoryEntry::for_command(
                    "items.set_folder",
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
        command: &'static str,
        label: &'static str,
        domain: BitmapDomain,
        destination: u32,
        partition_keys: Vec<u32>,
        target: &SelectionTarget,
        resources: Vec<String>,
    ) -> Result<MutationReceipt> {
        let target = target.clone();
        let changed_at_ms = now_ms();
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
                if !changes.is_empty() {
                    insert_cloud_journal(
                        transaction,
                        revision,
                        match domain {
                            BitmapDomain::Lifecycle => "root.lifecycle",
                            BitmapDomain::Rating => "root.rating",
                            BitmapDomain::Tag => "tag.partition",
                        },
                        Some(&selection),
                        serde_json::json!({"destination": destination}),
                        changed_at_ms,
                    )?;
                }
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources.clone(),
                    selection.iter().map(RootId),
                );
                let history = (!changes.is_empty()).then(|| {
                    HistoryEntry::for_command(command, label, SemanticChange::Compound(changes))
                });
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
        let changed_at_ms = now_ms();
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
                insert_cloud_journal(
                    transaction,
                    revision,
                    if use_after {
                        "history.redo"
                    } else {
                        "history.undo"
                    },
                    (!affected.is_empty()).then_some(&affected),
                    serde_json::json!({"label": entry.label}),
                    changed_at_ms,
                )?;
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
                    Arc::make_mut(&mut snapshot.folders)
                        .insert(folder_id, new_bitmap.clone().into());
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
        SemanticChange::TagNamespaceName {
            namespace_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            if load_namespace_name(transaction, *namespace_id)? != *expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag namespace {} was renamed",
                    namespace_id.0
                )));
            }
            apply_namespace_name(transaction, snapshot, *namespace_id, replacement)?;
            resources.insert("tags".into());
            resources.insert("navigation".into());
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
            let current_members = snapshot
                .tags
                .get(&tag_id)
                .map(|members| members.to_bitmap())
                .unwrap_or_default();
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
                Arc::make_mut(&mut snapshot.tags)
                    .insert(tag_id, replacement_members.clone().into());
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
        SemanticChange::FolderName {
            folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = transaction
                .query_row(
                    "SELECT name FROM folder_definition WHERE folder_id = ?1",
                    [folder_id.0],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::NotFound(format!("folder {}", folder_id.0))
                    }
                    error => error.into(),
                })?;
            if &current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because folder {} was renamed",
                    folder_id.0
                )));
            }
            rename_folder_definition(transaction, *folder_id, replacement)?;
            resources.insert("folders".into());
            resources.insert("navigation".into());
        }
        SemanticChange::FolderDefinition {
            folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = load_folder_definition(transaction, *folder_id)?;
            if current.as_ref() != expected.as_deref() {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because folder {} changed",
                    folder_id.0
                )));
            }
            match replacement {
                Some(folder) => {
                    transaction.execute(
                        "INSERT INTO folder_definition
                             (folder_id, stable_key, parent_id, name, icon, color, notes,
                              auto_tag_ids, display_order)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                         ON CONFLICT(folder_id) DO UPDATE SET
                             stable_key = excluded.stable_key,
                             parent_id = excluded.parent_id,
                             name = excluded.name,
                             icon = excluded.icon,
                             color = excluded.color,
                             notes = excluded.notes,
                             auto_tag_ids = excluded.auto_tag_ids,
                             display_order = excluded.display_order",
                        rusqlite::params![
                            folder.folder_id.0,
                            folder.stable_key,
                            folder.parent_id.map(|id| id.0),
                            folder.name,
                            folder.icon,
                            folder.color,
                            folder.notes,
                            folder.auto_tag_ids,
                            folder.display_order
                        ],
                    )?;
                    Arc::make_mut(&mut snapshot.folders)
                        .entry(*folder_id)
                        .or_default();
                    Arc::make_mut(&mut snapshot.folder_orders)
                        .entry(*folder_id)
                        .or_insert_with(|| Arc::new(Vec::new()));
                }
                None => {
                    let members = snapshot
                        .folder_orders
                        .get(folder_id)
                        .map_or(0, |order| order.len());
                    if members != 0 {
                        return Err(LibraryError::InvalidState(format!(
                            "cannot remove folder {} with members while replaying history",
                            folder_id.0
                        )));
                    }
                    ordering::delete(transaction, OrderOwnerKind::Folder, folder_id.0)?;
                    transaction.execute(
                        "DELETE FROM folder_definition WHERE folder_id = ?1",
                        [folder_id.0],
                    )?;
                    Arc::make_mut(&mut snapshot.folders).remove(folder_id);
                    Arc::make_mut(&mut snapshot.folder_orders).remove(folder_id);
                }
            }
            resources.insert("folders".into());
            resources.insert("navigation".into());
        }
        SemanticChange::SmartFolderDefinition {
            smart_folder_id,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = load_smart_folder_definition(transaction, *smart_folder_id)?;
            if current.as_ref() != expected.as_deref() {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because smart folder {} changed",
                    smart_folder_id.0
                )));
            }
            match replacement {
                Some(folder) => {
                    transaction.execute(
                        "INSERT INTO smart_folder_definition
                             (smart_folder_id, stable_key, parent_id, name, icon, color, notes,
                              view_query_json, display_order)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                         ON CONFLICT(smart_folder_id) DO UPDATE SET
                             stable_key = excluded.stable_key,
                             parent_id = excluded.parent_id,
                             name = excluded.name,
                             icon = excluded.icon,
                             color = excluded.color,
                             notes = excluded.notes,
                             view_query_json = excluded.view_query_json,
                             display_order = excluded.display_order",
                        rusqlite::params![
                            folder.smart_folder_id.0,
                            folder.stable_key,
                            folder.parent_id.map(|id| id.0),
                            folder.name,
                            folder.icon,
                            folder.color,
                            folder.notes,
                            serde_json::to_string(&folder.view)?,
                            folder.display_order
                        ],
                    )?;
                    crate::smart::replace_query(
                        transaction,
                        snapshot,
                        *smart_folder_id,
                        folder.view.clone(),
                    )?;
                }
                None => {
                    transaction.execute(
                        "DELETE FROM smart_folder_definition WHERE smart_folder_id = ?1",
                        [smart_folder_id.0],
                    )?;
                    crate::smart::remove(snapshot, *smart_folder_id);
                }
            }
            resources.insert("smart-folders".into());
            resources.insert("navigation".into());
        }
        SemanticChange::RecentViews { before, after } => {
            let replacement = if use_after { after } else { before };
            transaction.execute("DELETE FROM recent_view", [])?;
            let mut statement = transaction.prepare_cached(
                "INSERT INTO recent_view (root_id, viewed_at_ms) VALUES (?1, ?2)",
            )?;
            for (root_id, viewed_at_ms) in replacement.iter() {
                statement.execute(rusqlite::params![root_id.0, viewed_at_ms])?;
            }
            resources.insert("recently-viewed".into());
            resources.insert("navigation".into());
        }
        SemanticChange::Structure {
            affected: changed_roots,
            before,
            after,
        } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            restore_structure(
                transaction,
                revision,
                snapshot,
                changed_roots,
                expected,
                replacement,
            )?;
            *affected |= changed_roots.as_ref();
            resources.extend([
                "roots".into(),
                "collections".into(),
                "tags".into(),
                "folders".into(),
                "sidebar".into(),
                "search".into(),
            ]);
        }
        SemanticChange::DuplicateResolution(state) => {
            let changed =
                crate::duplicate::replay(transaction, revision, snapshot, state, use_after)?;
            *affected |= &changed;
            resources.insert("duplicates".into());
            if state.rewires_file() {
                resources.insert("roots".into());
                resources.insert("media".into());
                resources.insert("smart-folders".into());
            }
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

fn load_recent_views(connection: &rusqlite::Connection) -> Result<Vec<(RootId, i64)>> {
    let mut statement = connection.prepare(
        "SELECT root_id, viewed_at_ms FROM recent_view
         ORDER BY viewed_at_ms DESC, root_id DESC",
    )?;
    let values = statement
        .query_map([], |row| Ok((RootId(row.get(0)?), row.get(1)?)))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

fn restore_structure(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    snapshot: &mut ProjectionSnapshot,
    affected: &RoaringBitmap,
    expected: &StructuralState,
    replacement: &StructuralState,
) -> Result<()> {
    if load_structural_roots(transaction, affected)? != *expected.roots {
        return Err(LibraryError::InvalidState(
            "cannot replay history because collection structure changed".into(),
        ));
    }

    for root_id in affected {
        if snapshot.collection_orders.contains_key(&RootId(root_id)) {
            ordering::delete(transaction, OrderOwnerKind::Collection, root_id)?;
        }
        transaction.execute("DELETE FROM root_fts WHERE root_id = ?1", [root_id])?;
        transaction.execute("DELETE FROM library_root WHERE root_id = ?1", [root_id])?;
        transaction.execute(
            "DELETE FROM library_item WHERE local_id = ?1 AND item_kind = 2",
            [root_id],
        )?;
    }

    for root in replacement.roots.iter() {
        match root.kind {
            RootKind::Media => {
                let valid = transaction.query_row(
                    "SELECT EXISTS(
                         SELECT 1 FROM library_item
                         WHERE local_id = ?1 AND item_kind = 1
                     )",
                    [root.root_id.0],
                    |row| row.get::<_, bool>(0),
                )?;
                if !valid {
                    return Err(LibraryError::InvalidState(format!(
                        "media {} lost its canonical item",
                        root.root_id.0
                    )));
                }
            }
            RootKind::Collection => {
                transaction.execute(
                    "INSERT INTO library_item(local_id, stable_key, item_kind)
                     VALUES (?1, ?2, 2)",
                    rusqlite::params![root.root_id.0, root.stable_key],
                )?;
            }
        }
        transaction.execute(
            "INSERT INTO library_root
                 (root_id, name, notes, source_urls_json, cover_media_id, imported_at_ms,
                  captured_at_ms, modified_at_ms, media_count, total_size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                root.root_id.0,
                root.name,
                root.notes,
                root.source_urls_json,
                root.cover_media_id.0,
                root.imported_at_ms,
                root.captured_at_ms,
                root.modified_at_ms,
                root.media_count,
                i64::try_from(root.total_size_bytes).map_err(|_| {
                    LibraryError::InvalidState("root size exceeds SQLite range".into())
                })?
            ],
        )?;
        crate::fts::mark_one(transaction, root.root_id, root.modified_at_ms)?;
    }

    restore_structure_bitmaps(transaction, revision, snapshot, &replacement.projection)?;
    restore_structure_orders(transaction, revision, snapshot, &replacement.projection)?;
    *snapshot = (*replacement.projection).clone();
    snapshot.revision = revision;
    Ok(())
}

fn restore_structure_bitmaps(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    current: &ProjectionSnapshot,
    replacement: &ProjectionSnapshot,
) -> Result<()> {
    for lifecycle in Lifecycle::ALL {
        if current.lifecycle(lifecycle) != replacement.lifecycle(lifecycle) {
            bitmap::replace(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Lifecycle,
                    key_id: lifecycle.bitmap_key(),
                },
                replacement.lifecycle(lifecycle),
            )?;
        }
    }
    for rating in Rating::ALL {
        if current.rating(rating) != replacement.rating(rating) {
            bitmap::replace(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Rating,
                    key_id: rating.bitmap_key(),
                },
                replacement.rating(rating),
            )?;
        }
    }
    let tag_ids = current
        .tags
        .keys()
        .chain(replacement.tags.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for tag_id in tag_ids {
        let before = current.tags.get(&tag_id).map(|values| &**values);
        let after = replacement.tags.get(&tag_id).map(|values| &**values);
        if before != after {
            bitmap::replace(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: tag_id.0,
                },
                after.unwrap_or(&RoaringBitmap::new()),
            )?;
        }
    }
    Ok(())
}

fn restore_structure_orders(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    current: &ProjectionSnapshot,
    replacement: &ProjectionSnapshot,
) -> Result<()> {
    let folders = current
        .folder_orders
        .keys()
        .chain(replacement.folder_orders.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for folder_id in folders {
        let before = current.folder_orders.get(&folder_id);
        let after = replacement.folder_orders.get(&folder_id);
        if before == after {
            continue;
        }
        if let Some(after) = after {
            ordering::replace(
                transaction,
                revision,
                OrderOwnerKind::Folder,
                folder_id.0,
                &after.iter().map(|root| root.0).collect::<Vec<_>>(),
            )?;
        } else {
            ordering::delete(transaction, OrderOwnerKind::Folder, folder_id.0)?;
        }
    }

    let collections = current
        .collection_orders
        .keys()
        .chain(replacement.collection_orders.keys())
        .copied()
        .collect::<BTreeSet<_>>();
    for root_id in collections {
        let before = current.collection_orders.get(&root_id);
        let after = replacement.collection_orders.get(&root_id);
        if before == after {
            continue;
        }
        if let Some(after) = after {
            ordering::replace(
                transaction,
                revision,
                OrderOwnerKind::Collection,
                root_id.0,
                &after.iter().map(|media| media.0).collect::<Vec<_>>(),
            )?;
        } else {
            ordering::delete(transaction, OrderOwnerKind::Collection, root_id.0)?;
        }
    }
    Ok(())
}

fn projection_bitmap(snapshot: &ProjectionSnapshot, key: BitmapKey) -> RoaringBitmap {
    match key.domain {
        BitmapDomain::Lifecycle => snapshot
            .lifecycle
            .iter()
            .find_map(|(value, bitmap)| {
                (value.bitmap_key() == key.key_id).then(|| bitmap.to_bitmap())
            })
            .unwrap_or_default(),
        BitmapDomain::Rating => snapshot
            .ratings
            .iter()
            .find_map(|(value, bitmap)| {
                (value.bitmap_key() == key.key_id).then(|| bitmap.to_bitmap())
            })
            .unwrap_or_default(),
        BitmapDomain::Tag => snapshot
            .tags
            .get(&crate::TagId(key.key_id))
            .map(|bitmap| bitmap.to_bitmap())
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
                Arc::make_mut(&mut snapshot.lifecycle).insert(value, bitmap.into());
            }
        }
        BitmapDomain::Rating => {
            if let Some(value) = Rating::ALL
                .into_iter()
                .find(|value| value.bitmap_key() == key.key_id)
            {
                Arc::make_mut(&mut snapshot.ratings).insert(value, bitmap.into());
            }
        }
        BitmapDomain::Tag => {
            Arc::make_mut(&mut snapshot.tags).insert(crate::TagId(key.key_id), bitmap.into());
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

fn load_namespace_name(
    connection: &rusqlite::Connection,
    namespace_id: TagNamespaceId,
) -> Result<String> {
    connection
        .query_row(
            "SELECT display_name FROM tag_namespace WHERE namespace_id = ?1",
            [namespace_id.0],
            |row| row.get(0),
        )
        .map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => {
                LibraryError::NotFound(format!("tag namespace {namespace_id}"))
            }
            error => error.into(),
        })
}

fn apply_namespace_name(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    namespace_id: TagNamespaceId,
    replacement: &str,
) -> Result<()> {
    let before = load_namespace_name(transaction, namespace_id)?;
    let duplicate = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM tag_namespace
             WHERE display_name = ?1 AND namespace_id != ?2
         )",
        rusqlite::params![replacement, namespace_id.0],
        |row| row.get::<_, bool>(0),
    )?;
    if duplicate {
        return Err(LibraryError::InvalidInput(format!(
            "tag namespace {replacement} already exists"
        )));
    }
    let mut statement = transaction
        .prepare("SELECT tag_id, subname FROM tag_definition WHERE namespace_id = ?1")?;
    let tags = statement
        .query_map([namespace_id.0], |row| {
            Ok((crate::TagId(row.get(0)?), row.get::<_, String>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    transaction.execute(
        "UPDATE tag_namespace SET display_name = ?2 WHERE namespace_id = ?1",
        rusqlite::params![namespace_id.0, replacement],
    )?;
    let names = Arc::make_mut(&mut snapshot.tag_ids_by_name);
    for (tag_id, subname) in tags {
        let old_name = if before.is_empty() {
            subname.clone()
        } else {
            format!("{before}:{subname}")
        };
        let new_name = if replacement.is_empty() {
            subname
        } else {
            format!("{replacement}:{subname}")
        };
        names.remove(&old_name);
        names.insert(new_name, tag_id);
    }
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

fn rename_folder_definition(
    transaction: &rusqlite::Transaction<'_>,
    folder_id: FolderId,
    name: &str,
) -> Result<()> {
    let duplicate = transaction.query_row(
        "SELECT EXISTS(
             SELECT 1
             FROM folder_definition candidate
             JOIN folder_definition current ON current.folder_id = ?1
             WHERE candidate.parent_id IS current.parent_id
               AND candidate.name = ?2
               AND candidate.folder_id != current.folder_id
         )",
        rusqlite::params![folder_id.0, name],
        |row| row.get::<_, bool>(0),
    )?;
    if duplicate {
        return Err(LibraryError::InvalidInput(format!(
            "a sibling folder named {name} already exists"
        )));
    }
    if transaction.execute(
        "UPDATE folder_definition SET name = ?2 WHERE folder_id = ?1",
        rusqlite::params![folder_id.0, name],
    )? == 0
    {
        return Err(LibraryError::NotFound(format!("folder {}", folder_id.0)));
    }
    Ok(())
}

fn load_folder_definition(
    connection: &rusqlite::Connection,
    folder_id: FolderId,
) -> Result<Option<FolderDefinitionState>> {
    use rusqlite::OptionalExtension;

    connection
        .query_row(
            "SELECT stable_key, parent_id, name, icon, color, notes, auto_tag_ids,
                    display_order
             FROM folder_definition WHERE folder_id = ?1",
            [folder_id.0],
            |row| {
                Ok(FolderDefinitionState {
                    folder_id,
                    stable_key: row.get(0)?,
                    parent_id: row.get::<_, Option<u32>>(1)?.map(FolderId),
                    name: row.get(2)?,
                    icon: row.get(3)?,
                    color: row.get(4)?,
                    notes: row.get(5)?,
                    auto_tag_ids: row.get(6)?,
                    display_order: row.get(7)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn require_folder(connection: &rusqlite::Connection, folder_id: FolderId) -> Result<()> {
    if load_folder_definition(connection, folder_id)?.is_none() {
        return Err(LibraryError::NotFound(format!("folder {folder_id}")));
    }
    Ok(())
}

fn load_folder_children(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
) -> Result<Vec<FolderId>> {
    let mut statement = connection.prepare(
        "SELECT folder_id FROM folder_definition
         WHERE parent_id IS ?1 ORDER BY display_order, folder_id",
    )?;
    let values = statement
        .query_map([parent_id.map(|id| id.0)], |row| {
            row.get::<_, u32>(0).map(FolderId)
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

fn load_folder_subtree(
    transaction: &rusqlite::Transaction<'_>,
    requested: &[FolderId],
) -> Result<Vec<(FolderId, u32)>> {
    transaction.execute_batch(
        "CREATE TEMP TABLE IF NOT EXISTS picto_selected_folder (
             folder_id INTEGER PRIMARY KEY
         ) WITHOUT ROWID;
         DELETE FROM picto_selected_folder;",
    )?;
    {
        let mut insert = transaction
            .prepare_cached("INSERT INTO picto_selected_folder(folder_id) VALUES (?1)")?;
        for folder_id in requested {
            insert.execute([folder_id.0])?;
        }
    }
    let mut statement = transaction.prepare(
        "WITH RECURSIVE subtree(folder_id, depth) AS (
             SELECT folder.folder_id, 0
             FROM folder_definition folder
             JOIN picto_selected_folder selected
               ON selected.folder_id = folder.folder_id
             UNION
             SELECT child.folder_id, parent.depth + 1
             FROM folder_definition child
             JOIN subtree parent ON child.parent_id = parent.folder_id
         )
         SELECT folder_id, MAX(depth) FROM subtree
         GROUP BY folder_id ORDER BY folder_id",
    )?;
    let values = statement
        .query_map([], |row| Ok((FolderId(row.get(0)?), row.get::<_, u32>(1)?)))?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    Ok(values)
}

fn validate_folder_parent(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
    child_id: Option<FolderId>,
) -> Result<()> {
    if parent_id.is_some() && parent_id == child_id {
        return Err(LibraryError::InvalidInput(
            "a folder cannot be its own parent".into(),
        ));
    }
    let Some(parent_id) = parent_id else {
        return Ok(());
    };
    require_folder(connection, parent_id)?;
    if let Some(child_id) = child_id {
        let creates_cycle = connection.query_row(
            "WITH RECURSIVE descendants(folder_id) AS (
                 SELECT folder_id FROM folder_definition WHERE parent_id = ?1
                 UNION ALL
                 SELECT child.folder_id
                 FROM folder_definition child
                 JOIN descendants parent ON child.parent_id = parent.folder_id
             )
             SELECT EXISTS(SELECT 1 FROM descendants WHERE folder_id = ?2)",
            rusqlite::params![child_id.0, parent_id.0],
            |row| row.get::<_, bool>(0),
        )?;
        if creates_cycle {
            return Err(LibraryError::InvalidInput(
                "a folder cannot move below its descendant".into(),
            ));
        }
    }
    Ok(())
}

fn normalized_optional(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
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
        if let Some(child_id) = child_id {
            let creates_cycle = transaction.query_row(
                "WITH RECURSIVE descendants(smart_folder_id) AS (
                     SELECT smart_folder_id FROM smart_folder_definition WHERE parent_id = ?1
                     UNION ALL
                     SELECT child.smart_folder_id
                     FROM smart_folder_definition child
                     JOIN descendants parent ON child.parent_id = parent.smart_folder_id
                 )
                 SELECT EXISTS(
                     SELECT 1 FROM descendants WHERE smart_folder_id = ?2
                 )",
                rusqlite::params![child_id.0, parent_id.0],
                |row| row.get::<_, bool>(0),
            )?;
            if creates_cycle {
                return Err(LibraryError::InvalidInput(
                    "a smart folder cannot move below its descendant".into(),
                ));
            }
        }
    }
    Ok(())
}

fn load_smart_folder_definition(
    connection: &rusqlite::Connection,
    smart_folder_id: SmartFolderId,
) -> Result<Option<SmartFolderDefinitionState>> {
    use rusqlite::OptionalExtension;

    let row = connection
        .query_row(
            "SELECT stable_key, parent_id, name, icon, color, notes, view_query_json,
                    display_order
             FROM smart_folder_definition WHERE smart_folder_id = ?1",
            [smart_folder_id.0],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<u32>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()?;
    row.map(
        |(stable_key, parent_id, name, icon, color, notes, view_json, display_order)| {
            Ok(SmartFolderDefinitionState {
                smart_folder_id,
                stable_key,
                parent_id: parent_id.map(SmartFolderId),
                name,
                icon,
                color,
                notes,
                view: serde_json::from_str(&view_json)?,
                display_order,
            })
        },
    )
    .transpose()
}

fn load_smart_folder_subtree(
    connection: &rusqlite::Connection,
    smart_folder_id: SmartFolderId,
) -> Result<Vec<(SmartFolderDefinitionState, u32)>> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE subtree(smart_folder_id, depth) AS (
             SELECT smart_folder_id, 0 FROM smart_folder_definition
             WHERE smart_folder_id = ?1
             UNION ALL
             SELECT child.smart_folder_id, parent.depth + 1
             FROM smart_folder_definition child
             JOIN subtree parent ON child.parent_id = parent.smart_folder_id
         )
         SELECT smart_folder_id, depth FROM subtree ORDER BY smart_folder_id",
    )?;
    let ids = statement
        .query_map([smart_folder_id.0], |row| {
            Ok((SmartFolderId(row.get(0)?), row.get::<_, u32>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    if ids.is_empty() {
        return Err(LibraryError::NotFound(format!(
            "smart folder {smart_folder_id}"
        )));
    }
    ids.into_iter()
        .map(|(id, depth)| {
            load_smart_folder_definition(connection, id)?
                .map(|definition| (definition, depth))
                .ok_or_else(|| {
                    LibraryError::InvalidState(format!(
                        "smart folder {} disappeared during subtree load",
                        id.0
                    ))
                })
        })
        .collect()
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

fn capture_structure(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    affected: &RoaringBitmap,
) -> Result<StructuralState> {
    Ok(StructuralState {
        roots: Arc::new(load_structural_roots(transaction, affected)?),
        projection: Arc::new(snapshot.clone()),
    })
}

fn load_structural_roots(
    transaction: &rusqlite::Transaction<'_>,
    affected: &RoaringBitmap,
) -> Result<Vec<StructuralRootState>> {
    let mut statement = transaction.prepare_cached(
        "SELECT item.stable_key, item.item_kind, root.name, root.notes,
                root.source_urls_json, root.cover_media_id, root.imported_at_ms,
                root.captured_at_ms, root.modified_at_ms, root.media_count,
                root.total_size_bytes
         FROM library_root root
         JOIN library_item item ON item.local_id = root.root_id
         WHERE root.root_id = ?1",
    )?;
    let mut roots = Vec::new();
    for root_id in affected {
        let state = statement.query_row([root_id], |row| {
            Ok(StructuralRootState {
                root_id: RootId(root_id),
                stable_key: row.get(0)?,
                kind: match row.get::<_, u8>(1)? {
                    1 => RootKind::Media,
                    2 => RootKind::Collection,
                    _ => unreachable!("schema constrains root kinds"),
                },
                name: row.get(2)?,
                notes: row.get(3)?,
                source_urls_json: row.get(4)?,
                cover_media_id: MediaId(row.get(5)?),
                imported_at_ms: row.get(6)?,
                captured_at_ms: row.get(7)?,
                modified_at_ms: row.get(8)?,
                media_count: row.get(9)?,
                total_size_bytes: row.get::<_, i64>(10)? as u64,
            })
        });
        match state {
            Ok(state) => roots.push(state),
            Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(roots)
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

fn insert_cloud_journal(
    transaction: &rusqlite::Transaction<'_>,
    revision: u64,
    operation_kind: &str,
    targets: Option<&RoaringBitmap>,
    payload: serde_json::Value,
    created_at_ms: i64,
) -> Result<()> {
    let target_bitmap = targets.map(crate::bitmap::encode).transpose()?;
    transaction.execute(
        "INSERT INTO cloud_journal
             (revision, operation_kind, target_bitmap, payload_json, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            revision as i64,
            operation_kind,
            target_bitmap,
            payload.to_string(),
            created_at_ms
        ],
    )?;
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis() as i64)
}
