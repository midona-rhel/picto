use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

use roaring::RoaringBitmap;

use crate::bitmap::{self, BitmapDomain, BitmapKey};
use crate::database::WorkPriority;
use crate::history::{
    FolderDefinitionState, HistoryEntry, SemanticChange, SessionHistory,
    SmartFolderDefinitionState, StructuralMediaNoteState, StructuralRootState, StructuralState,
    TagDefinitionState, TagNamespaceDefinitionState,
};
use crate::ingest;
use crate::model::{
    DuplicatePair, DuplicateResolutionChoice, DuplicateResolutionResult, DuplicateStatus, FileId,
    FolderDeleteResult, FolderId, FolderRecord, GroupRequest, LibraryStatistics, Lifecycle,
    MediaFactsUpdate, MediaId, PendingBlobCleanup, PreparedCollectionImport, PreparedImport,
    Rating, RootId, RootKind, RootTagAssignment, SmartFolderDeleteResult, SmartFolderId,
    SmartFolderInput, TagNamespaceId, TagRecord,
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
    match_cache: Arc<crate::query::MatchCache>,
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
        database.maintenance_write(WorkPriority::CorrectnessRecovery, |transaction| {
            transaction.execute(
                "DELETE FROM view_pref
                 WHERE (scope LIKE 'folder:%' AND NOT EXISTS (
                            SELECT 1 FROM folder_definition
                            WHERE folder_id = CAST(substr(view_pref.scope, 8) AS INTEGER)
                        ))
                    OR (scope LIKE 'smart:%' AND NOT EXISTS (
                            SELECT 1 FROM smart_folder_definition
                            WHERE smart_folder_id = CAST(substr(view_pref.scope, 7) AS INTEGER)
                        ))",
                [],
            )?;
            Ok(())
        })?;
        let projections = Arc::new(ProjectionStore::load(&database)?);
        Ok(Self {
            database,
            projections,
            publication: Arc::new(PublicationCoordinator::default()),
            history: Arc::new(SessionHistory::default()),
            match_cache: Arc::new(crate::query::MatchCache::default()),
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

    pub fn auxiliary_read<T>(
        &self,
        priority: WorkPriority,
        operation: impl FnOnce(&rusqlite::Connection) -> Result<T>,
    ) -> Result<T> {
        self.database.read(priority, operation)
    }

    pub fn auxiliary_read_consistent<T>(
        &self,
        priority: WorkPriority,
        operation: impl FnOnce(&rusqlite::Transaction<'_>, &ProjectionSnapshot) -> Result<T>,
    ) -> Result<T> {
        self.database.read_consistent(
            priority,
            |revision| self.capture_revision(revision),
            |transaction, snapshot| operation(transaction, &snapshot),
        )
    }

    pub fn auxiliary_write<T>(
        &self,
        priority: WorkPriority,
        resources: impl IntoIterator<Item = String>,
        item_ids: impl IntoIterator<Item = RootId>,
        operation: impl FnOnce(&rusqlite::Transaction<'_>, u64) -> Result<T>,
    ) -> Result<(T, MutationReceipt)> {
        let resources = resources.into_iter().collect::<Vec<_>>();
        let item_ids = item_ids.into_iter().collect::<Vec<_>>();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let ((output, receipt), _, _) = self.database.published_write(
            priority,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let output = operation(transaction, revision)?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(revision, resources, item_ids);
                Ok((
                    (output, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        Ok((output, receipt))
    }

    pub fn auxiliary_write_if_changed<T>(
        &self,
        priority: WorkPriority,
        resources: impl IntoIterator<Item = String>,
        item_ids: impl IntoIterator<Item = RootId>,
        operation: impl FnOnce(&rusqlite::Transaction<'_>, u64) -> Result<Option<T>>,
    ) -> Result<Option<(T, MutationReceipt)>> {
        let resources = resources.into_iter().collect::<Vec<_>>();
        let item_ids = item_ids.into_iter().collect::<Vec<_>>();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let published = self.database.published_write_if_changed(
            priority,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let Some(output) = operation(transaction, revision)? else {
                    return Ok(None);
                };
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(revision, resources, item_ids);
                Ok(Some((
                    (output, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                )))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        Ok(published.map(|((output, receipt), _, _)| (output, receipt)))
    }

    pub fn auxiliary_semantic_write_if_changed<T>(
        &self,
        priority: WorkPriority,
        resources: impl IntoIterator<Item = String>,
        item_ids: impl IntoIterator<Item = RootId>,
        operation_kind: &'static str,
        payload: serde_json::Value,
        operation: impl FnOnce(&rusqlite::Transaction<'_>, u64) -> Result<Option<T>>,
    ) -> Result<Option<(T, MutationReceipt)>> {
        self.auxiliary_write_if_changed(priority, resources, item_ids, |transaction, revision| {
            let Some(output) = operation(transaction, revision)? else {
                return Ok(None);
            };
            insert_cloud_journal(
                transaction,
                revision,
                operation_kind,
                None,
                payload,
                now_ms(),
            )?;
            Ok(Some(output))
        })
    }

    pub fn read_auxiliary_json(&self, table: &str, key: &str) -> Result<Option<String>> {
        let (table, key_column) = auxiliary_json_spec(table)?;
        self.database.read(WorkPriority::VisibleRead, |connection| {
            use rusqlite::OptionalExtension;
            connection
                .query_row(
                    &format!("SELECT value_json FROM {table} WHERE {key_column} = ?1"),
                    [key],
                    |row| row.get(0),
                )
                .optional()
                .map_err(Into::into)
        })
    }

    pub fn replace_auxiliary_json(
        &self,
        command: &'static str,
        label: &'static str,
        table: &'static str,
        key: &str,
        value: Option<String>,
    ) -> Result<MutationReceipt> {
        let (table, key_column) = auxiliary_json_spec(table)?;
        let key = key.to_owned();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let result = self.database.published_write_if_changed(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                use rusqlite::OptionalExtension;
                let before = transaction
                    .query_row(
                        &format!("SELECT value_json FROM {table} WHERE {key_column} = ?1"),
                        [&key],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?;
                if before == value {
                    return Ok(None);
                }
                if let Some(value) = value.as_ref() {
                    transaction.execute(
                        &format!(
                            "INSERT INTO {table} ({key_column}, value_json) VALUES (?1, ?2)
                             ON CONFLICT({key_column}) DO UPDATE SET value_json = excluded.value_json"
                        ),
                        rusqlite::params![key, value],
                    )?;
                } else {
                    transaction.execute(
                        &format!("DELETE FROM {table} WHERE {key_column} = ?1"),
                        [&key],
                    )?;
                }
                insert_cloud_journal(
                    transaction,
                    revision,
                    command,
                    None,
                    serde_json::json!({"key": key}),
                    now_ms(),
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["settings".into()],
                    Vec::new(),
                );
                Ok(Some((receipt.clone(), PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(HistoryEntry::for_command(
                        command,
                        label,
                        SemanticChange::AuxiliaryJson {
                            table,
                            key: key.clone(),
                            before,
                            after: value.clone(),
                            resource: "settings",
                        },
                    )),
                })))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        let Some((receipt, _, history_entry)) = result else {
            return Ok(PublicationCoordinator::receipt(
                self.database.revision()?,
                Vec::new(),
                Vec::new(),
            ));
        };
        push_history(&history, history_entry);
        Ok(receipt)
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
        let match_cache = self.match_cache.clone();
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                crate::query::page_cached(connection, &snapshot, query, page, &match_cache)
            },
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

    pub fn sidebar_counts(&self) -> Result<crate::SidebarCounts> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let counts = crate::query::counts(&snapshot);
                let recently_viewed = crate::query::matching_roots(
                    connection,
                    &snapshot,
                    &crate::query::RootQuery {
                        scope: crate::query::ItemScope::RecentlyViewed,
                        view: Default::default(),
                    },
                )?
                .len();
                let duplicates = crate::duplicate::count_visible_candidates(connection, &snapshot)?;
                let mut folders = counts
                    .folders
                    .into_iter()
                    .map(|(folder_id, count)| crate::FolderCount { folder_id, count })
                    .collect::<Vec<_>>();
                folders.sort_by_key(|entry| entry.folder_id);
                let mut smart_folders = counts
                    .smart_folders
                    .into_iter()
                    .map(|(smart_folder_id, count)| crate::SmartFolderCount {
                        smart_folder_id,
                        count,
                    })
                    .collect::<Vec<_>>();
                smart_folders.sort_by_key(|entry| entry.smart_folder_id);
                Ok(crate::SidebarCounts {
                    all: counts.all,
                    inbox: counts.inbox,
                    trash: counts.trash,
                    recently_viewed,
                    untagged: counts.untagged,
                    uncategorized: counts.uncategorized,
                    duplicates,
                    folders,
                    smart_folders,
                    revision: counts.revision,
                })
            },
        )
    }

    pub fn library_statistics(&self) -> Result<LibraryStatistics> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let values = connection.query_row(
                    "SELECT
                         (SELECT COUNT(*) FROM media_item),
                         (SELECT COUNT(*) FROM media_item media JOIN media_file file
                          ON file.file_id = media.file_id WHERE file.mime LIKE 'image/%'),
                         (SELECT COUNT(*) FROM media_item media JOIN media_file file
                          ON file.file_id = media.file_id WHERE file.mime LIKE 'video/%'),
                         (SELECT COUNT(*) FROM media_item media JOIN media_file file
                          ON file.file_id = media.file_id WHERE file.mime LIKE 'audio/%'),
                         (SELECT COUNT(*) FROM media_item media JOIN media_file file
                          ON file.file_id = media.file_id
                          WHERE file.mime NOT LIKE 'image/%'
                            AND file.mime NOT LIKE 'video/%'
                            AND file.mime NOT LIKE 'audio/%'),
                         (SELECT COUNT(*) FROM media_file),
                         (SELECT COALESCE(SUM(size_bytes), 0) FROM media_file),
                         (SELECT COUNT(*) FROM tag_definition),
                         (SELECT COUNT(*) FROM folder_definition),
                         (SELECT COUNT(*) FROM smart_folder_definition),
                         (SELECT COUNT(*) FROM subscription)",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(4)?,
                            row.get::<_, i64>(5)?,
                            row.get::<_, i64>(6)?,
                            row.get::<_, i64>(7)?,
                            row.get::<_, i64>(8)?,
                            row.get::<_, i64>(9)?,
                            row.get::<_, i64>(10)?,
                        ))
                    },
                )?;
                Ok(LibraryStatistics {
                    active_items: snapshot.lifecycle(Lifecycle::Active).len() as i64,
                    inbox_items: snapshot.lifecycle(Lifecycle::Inbox).len() as i64,
                    trash_items: snapshot.lifecycle(Lifecycle::Trash).len() as i64,
                    standalone_items: snapshot
                        .root_kinds
                        .get(&RootKind::Media)
                        .map_or(0, |roots| roots.len())
                        as i64,
                    collections: snapshot
                        .root_kinds
                        .get(&RootKind::Collection)
                        .map_or(0, |roots| roots.len()) as i64,
                    media_assets: values.0,
                    image_assets: values.1,
                    video_assets: values.2,
                    audio_assets: values.3,
                    other_assets: values.4,
                    physical_files: values.5,
                    original_bytes: values.6,
                    tags: values.7,
                    folders: values.8,
                    smart_folders: values.9,
                    subscriptions: values.10,
                    revision: snapshot.revision,
                })
            },
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

    pub fn ordered_image_selection(&self, target: &SelectionTarget) -> Result<Vec<RootId>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                crate::selection::resolve_ordered_image_roots(connection, &snapshot, target)
            },
        )
    }

    pub fn collection_note_draft(
        &self,
        target: &SelectionTarget,
    ) -> Result<crate::model::CollectionNoteDraft> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                crate::selection::collection_note_draft(connection, &snapshot, target)
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
                    "INSERT INTO recent_view (root_id, viewed_at_ms)
                     VALUES (
                         ?1,
                         MAX(
                             ?2,
                             COALESCE((SELECT MAX(viewed_at_ms) + 1 FROM recent_view), ?2)
                         )
                     )
                     ON CONFLICT(root_id) DO UPDATE SET viewed_at_ms = excluded.viewed_at_ms",
                    rusqlite::params![root_id.0, viewed_at_ms],
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["recently-viewed".into(), "navigation".into()],
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
            |connection, snapshot| list_folders(connection, &snapshot),
        )
    }

    pub fn navigation(&self) -> Result<(Vec<FolderRecord>, Vec<SmartFolderRecord>, u64)> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                Ok((
                    list_folders(connection, &snapshot)?,
                    crate::smart::list(connection, &snapshot)?,
                    snapshot.revision,
                ))
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

    pub fn tag_namespaces(&self) -> Result<Vec<crate::TagNamespaceRecord>> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            let mut statement = connection.prepare(
                "SELECT namespace.namespace_id, namespace.display_name, COUNT(tag.tag_id)
                 FROM tag_namespace namespace
                 LEFT JOIN tag_definition tag ON tag.namespace_id = namespace.namespace_id
                 GROUP BY namespace.namespace_id, namespace.display_name
                 ORDER BY namespace.display_name COLLATE NOCASE, namespace.namespace_id",
            )?;
            let values = statement
                .query_map([], |row| {
                    Ok(crate::TagNamespaceRecord {
                        namespace_id: TagNamespaceId(row.get(0)?),
                        name: row.get(1)?,
                        tag_count: row.get::<_, i64>(2)? as u64,
                    })
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(LibraryError::from)?;
            Ok(values)
        })
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
        crate::smart::validate_view(&view)?;
        let changed_at_ms = now_ms();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((smart_folder_id, receipt), _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                crate::smart::validate_capacity(transaction)?;
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
                crate::smart::refresh_subtree(transaction, &mut next, smart_folder_id)?;
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
        crate::smart::validate_view(&view)?;
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
                let query_changed = before.parent_id != parent_id || before.view != view;
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
                if query_changed {
                    crate::smart::refresh_subtree(transaction, &mut next, smart_folder_id)?;
                }
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
        self.ingest_batch_with_identity_reuse(inputs, true, false)
    }

    pub fn ingest_batch_with_auto_tags(
        &self,
        inputs: &[PreparedImport],
    ) -> Result<Vec<(RootId, MutationReceipt)>> {
        self.ingest_batch_with_identity_reuse(inputs, true, true)
    }

    /// Converter-only import path. It preserves every legacy root while still
    /// sharing physical files by content hash.
    pub fn ingest_conversion_batch(
        &self,
        inputs: &[PreparedImport],
    ) -> Result<Vec<(RootId, MutationReceipt)>> {
        self.ingest_batch_with_identity_reuse(inputs, false, false)
    }

    fn ingest_batch_with_identity_reuse(
        &self,
        inputs: &[PreparedImport],
        reuse_identity: bool,
        auto_tag: bool,
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
                let mut created_root_ids = Vec::with_capacity(inputs.len());
                let mut resources = BTreeSet::new();
                let mut bitmap_keys = HashSet::new();
                let mut folder_ids = HashSet::new();
                let mut affected = RoaringBitmap::new();
                for input in inputs {
                    let output = ingest::insert_one(
                        transaction,
                        revision,
                        next,
                        input,
                        true,
                        reuse_identity,
                    )?;
                    next = output.snapshot;
                    root_ids.push(output.root_id);
                    if output.created_root {
                        created_root_ids.push(output.root_id);
                        if let Some(attempt_id) = input
                            .source_identity
                            .as_ref()
                            .and_then(|source| source.source_attempt_id)
                        {
                            record_source_attempt_root(transaction, attempt_id, output.root_id)?;
                            resources.insert("subscriptions".to_owned());
                        }
                    }
                    resources.extend(output.resources);
                    bitmap_keys.extend(output.bitmap_keys);
                    folder_ids.extend(output.folder_ids);
                    affected |= &output.affected_roots;
                }
                ingest::persist_touched(
                    transaction,
                    revision,
                    &next,
                    bitmap_keys,
                    folder_ids,
                    affected.iter().map(RootId),
                )?;
                crate::smart::settle_affected(transaction, &mut next, &affected)?;
                if auto_tag && !created_root_ids.is_empty() {
                    ingest::enqueue_ai_tag_roots(
                        transaction,
                        created_root_ids.iter().copied(),
                        inputs
                            .iter()
                            .map(|input| input.imported_at_ms)
                            .max()
                            .unwrap_or_else(now_ms),
                    )?;
                    resources.insert("tasks".to_owned());
                }
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
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    affected.iter().map(RootId),
                );
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
        self.ingest_collection_with_identity_reuse(input, true, false)
    }

    pub fn ingest_collection_with_auto_tags(
        &self,
        input: &PreparedCollectionImport,
    ) -> Result<(RootId, MutationReceipt)> {
        self.ingest_collection_with_identity_reuse(input, true, true)
    }

    /// Converter-only collection path. Source identities are copied, but are
    /// not interpreted as retry keys while reconstructing legacy structure.
    pub fn ingest_conversion_collection(
        &self,
        input: &PreparedCollectionImport,
    ) -> Result<(RootId, MutationReceipt)> {
        self.ingest_collection_with_identity_reuse(input, false, false)
    }

    fn ingest_collection_with_identity_reuse(
        &self,
        input: &PreparedCollectionImport,
        reuse_identity: bool,
        auto_tag: bool,
    ) -> Result<(RootId, MutationReceipt)> {
        if input.members.is_empty() {
            return Err(LibraryError::InvalidInput(
                "a collection import requires at least one media member".into(),
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
                let mut root_ids = Vec::with_capacity(input.members.len() + 1);
                let mut created_root_ids = Vec::with_capacity(input.members.len());
                let mut requested_cover = input.existing_root_id;
                if let Some(existing_root_id) = input.existing_root_id {
                    if !next
                        .root_kinds
                        .get(&RootKind::Collection)
                        .is_some_and(|roots| roots.contains(existing_root_id.0))
                        && !next
                            .root_kinds
                            .get(&RootKind::Media)
                            .is_some_and(|roots| roots.contains(existing_root_id.0))
                    {
                        return Err(LibraryError::NotFound(format!(
                            "existing source root {existing_root_id}"
                        )));
                    }
                    root_ids.push(existing_root_id);
                }
                let mut resources = BTreeSet::new();
                let mut bitmap_keys = HashSet::new();
                let mut folder_ids = HashSet::new();
                let mut ingest_affected = RoaringBitmap::new();
                for (index, member) in input.members.iter().enumerate() {
                    let output = ingest::insert_one(
                        transaction,
                        revision,
                        next,
                        member,
                        false,
                        reuse_identity,
                    )?;
                    next = output.snapshot;
                    root_ids.push(output.root_id);
                    if input.existing_root_id.is_none() && index == input.cover_index {
                        requested_cover = Some(output.root_id);
                    }
                    if output.created_root {
                        created_root_ids.push(output.root_id);
                    }
                    resources.extend(output.resources);
                    bitmap_keys.extend(output.bitmap_keys);
                    folder_ids.extend(output.folder_ids);
                    ingest_affected |= &output.affected_roots;
                }
                let ordered_roots = root_ids
                    .iter()
                    .copied()
                    .fold(Vec::new(), |mut roots, root| {
                        if !roots.contains(&root) {
                            roots.push(root);
                        }
                        roots
                    });
                let existing_collections = ordered_roots
                    .iter()
                    .copied()
                    .filter(|root| {
                        next.root_kinds
                            .get(&RootKind::Collection)
                            .is_some_and(|collections| collections.contains(root.0))
                    })
                    .collect::<Vec<_>>();
                if existing_collections.len() > 1 {
                    return Err(LibraryError::InvalidState(
                        "one source collection resolved to multiple existing collections".into(),
                    ));
                }
                let winning_collection_id = existing_collections.first().copied();
                let preserve_singleton_collection = !reuse_identity && ordered_roots.len() == 1;
                let creates_collection = (ordered_roots.len() >= 2
                    || preserve_singleton_collection)
                    && winning_collection_id.is_none();
                let (published_root, mut affected) =
                    if ordered_roots.len() >= 2 || preserve_singleton_collection {
                        let output = crate::group::organize(
                            transaction,
                            revision,
                            next,
                            &GroupRequest {
                                target: SelectionTarget::Explicit {
                                    root_ids: ordered_roots.clone(),
                                },
                                cover_root_id: requested_cover.unwrap_or(ordered_roots[0]),
                                winning_collection_id,
                                // Existing collection titles are collection-owned. A later
                                // member refresh must not replace one with a member/source name.
                                name: winning_collection_id
                                    .is_none()
                                    .then(|| input.name.clone())
                                    .flatten(),
                                notes: input.members[input.cover_index].notes.clone(),
                                modified_at_ms: input.modified_at_ms,
                            },
                            true,
                        )?;
                        next = output.snapshot;
                        resources.insert("collections".to_owned());
                        (output.collection_id, output.affected)
                    } else {
                        let published_root = ordered_roots[0];
                        (
                            published_root,
                            root_ids
                                .iter()
                                .map(|root| root.0)
                                .collect::<RoaringBitmap>(),
                        )
                    };
                affected |= &ingest_affected;
                ingest::persist_touched(
                    transaction,
                    revision,
                    &next,
                    bitmap_keys,
                    folder_ids,
                    affected.iter().map(RootId),
                )?;
                crate::smart::settle_affected(transaction, &mut next, &affected)?;
                if auto_tag && !created_root_ids.is_empty() {
                    ingest::enqueue_ai_tag_roots(
                        transaction,
                        std::iter::once(published_root),
                        input.modified_at_ms,
                    )?;
                    resources.insert("tasks".to_owned());
                }
                if !created_root_ids.is_empty() || creates_collection {
                    for attempt_id in input
                        .members
                        .iter()
                        .filter_map(|member| member.source_identity.as_ref())
                        .filter_map(|source| source.source_attempt_id)
                        .collect::<BTreeSet<_>>()
                    {
                        record_source_attempt_root(transaction, attempt_id, published_root)?;
                        resources.insert("subscriptions".to_owned());
                    }
                }
                resources.extend(["tags".to_owned(), "folders".to_owned()]);
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    std::iter::once(published_root),
                );
                Ok((
                    (published_root, receipt.clone()),
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

    pub fn replace_detected_duplicate_pairs(
        &self,
        pairs: &[(FileId, FileId, u32)],
        detected_at_ms: i64,
    ) -> Result<crate::DuplicateScanResult> {
        let pairs = pairs.to_vec();
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (result, _, ()) = self.database.published_write(
            WorkPriority::Maintenance,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let affected = crate::duplicate::replace_detected(
                    transaction,
                    &snapshot,
                    &pairs,
                    detected_at_ms,
                )?;
                let candidate_count = transaction.query_row(
                    "SELECT COUNT(*) FROM duplicate_pair WHERE status = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )? as usize;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "duplicate.scan",
                    (!affected.is_empty()).then_some(&affected),
                    serde_json::json!({"candidate_count": candidate_count}),
                    detected_at_ms,
                )?;
                let mut next = (*snapshot).clone();
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["duplicates".into()],
                    affected.iter().map(RootId),
                );
                let result = crate::DuplicateScanResult {
                    candidate_count,
                    affected_item_ids: receipt.item_ids.clone(),
                    receipt: receipt.clone(),
                };
                Ok((
                    result,
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
        Ok(result)
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

    pub fn duplicate_candidates(&self, limit: usize) -> Result<Vec<crate::DuplicateCandidate>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::duplicate::list_candidates(connection, &snapshot, limit),
        )
    }

    pub fn resolve_duplicate_automatically(
        &self,
        file_id_a: FileId,
        file_id_b: FileId,
        decided_at_ms: i64,
    ) -> Result<Option<DuplicateResolutionResult>> {
        let candidate = self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                crate::duplicate::candidate_for_pair(connection, &snapshot, file_id_a, file_id_b)
            },
        )?;
        let winner_file_id = match candidate.map(|candidate| candidate.decision) {
            Some(crate::DuplicateQualityDecision::LeftBetter)
            | Some(crate::DuplicateQualityDecision::AutoTieLeft) => file_id_a,
            Some(crate::DuplicateQualityDecision::RightBetter)
            | Some(crate::DuplicateQualityDecision::AutoTieRight) => file_id_b,
            Some(crate::DuplicateQualityDecision::NeedsChoice) | None => return Ok(None),
        };
        self.resolve_duplicate(
            file_id_a,
            file_id_b,
            DuplicateResolutionChoice::KeepFile { winner_file_id },
            decided_at_ms,
        )
        .map(Some)
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
                    let mut resources = vec![
                        "duplicates".into(),
                        "media".into(),
                        "roots".into(),
                        "smart-folders".into(),
                    ];
                    if output.history.changes_names() {
                        resources.push("search".into());
                    }
                    resources
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

    /// Delete unreferenced physical files while retaining the serialized writer.
    /// Readers continue through WAL; canonical ingestion cannot race a cleanup
    /// by attaching a new media item to the file being removed.
    pub fn clean_pending_blobs(
        &self,
        limit: usize,
        mut delete: impl FnMut(&PendingBlobCleanup) -> Result<()>,
    ) -> Result<usize> {
        let protected = self.history.protected_cleanup_files();
        self.database
            .maintenance_write(WorkPriority::Maintenance, |transaction| {
                let cleanup = crate::duplicate::ready_cleanup(transaction, &protected, limit)?;
                let mut removed = 0;
                for pending in cleanup {
                    delete(&pending)?;
                    let deleted = transaction.execute(
                        "DELETE FROM media_file
                         WHERE file_id = ?1
                           AND NOT EXISTS(
                               SELECT 1 FROM media_item WHERE file_id = ?1
                           )",
                        [pending.file_id.0],
                    )?;
                    if deleted != 1 {
                        return Err(LibraryError::InvalidState(format!(
                            "blob cleanup file {} became referenced",
                            pending.file_id.0
                        )));
                    }
                    removed += 1;
                }
                Ok(removed)
            })
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

    pub fn create_tag_namespace(&self, name: &str) -> Result<MutationReceipt> {
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
                let duplicate = transaction.query_row(
                    "SELECT EXISTS(SELECT 1 FROM tag_namespace WHERE display_name = ?1)",
                    [&name],
                    |row| row.get::<_, bool>(0),
                )?;
                if duplicate {
                    return Err(LibraryError::InvalidInput(format!(
                        "tag namespace {name} already exists"
                    )));
                }
                let namespace_id = TagNamespaceId(LibraryDatabase::allocate_id(transaction)?);
                let state = TagNamespaceDefinitionState {
                    namespace_id,
                    stable_key: uuid::Uuid::new_v4().to_string(),
                    display_name: name.clone(),
                };
                transaction.execute(
                    "INSERT INTO tag_namespace(namespace_id, stable_key, display_name)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![state.namespace_id.0, state.stable_key, state.display_name,],
                )?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "tag.namespace.create",
                    None,
                    serde_json::json!({
                        "namespace_id": namespace_id.0,
                        "name": name,
                    }),
                    now_ms(),
                )?;
                let mut next = (*snapshot).clone();
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
                        history: Some(HistoryEntry::for_command(
                            "tags.group.create",
                            "Create tag namespace",
                            SemanticChange::TagNamespaceDefinition {
                                before: None,
                                after: Some(state),
                            },
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    /// Ensure portable metadata can reference tag definitions without adding
    /// synthetic assignments to a library item.
    pub fn ensure_tag_definitions(
        &self,
        names: &[String],
    ) -> Result<(Vec<crate::TagId>, MutationReceipt)> {
        let names = names
            .iter()
            .map(|name| required_name("tag", name))
            .collect::<Result<BTreeSet<_>>>()?;
        if names.is_empty() {
            return Err(LibraryError::InvalidInput(
                "at least one tag definition is required".into(),
            ));
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let ((tag_ids, receipt), _, _) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let mut next = (*snapshot).clone();
                let mut tag_ids = Vec::with_capacity(names.len());
                let mut created = Vec::new();
                for name in &names {
                    let tag_id = if let Some(tag_id) = next.tag_ids_by_name.get(name).copied() {
                        tag_id
                    } else {
                        let tag_id = ingest::ensure_tag(transaction, name)?;
                        Arc::make_mut(&mut next.tag_ids_by_name).insert(name.clone(), tag_id);
                        Arc::make_mut(&mut next.tags)
                            .entry(tag_id)
                            .or_insert_with(|| RoaringBitmap::new().into());
                        created.push(name.clone());
                        tag_id
                    };
                    tag_ids.push(tag_id);
                }
                if !created.is_empty() {
                    insert_cloud_journal(
                        transaction,
                        revision,
                        "tag.definitions.ensure",
                        None,
                        serde_json::json!({"names": created}),
                        now_ms(),
                    )?;
                }
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    if created.is_empty() {
                        Vec::new()
                    } else {
                        vec!["tags".into(), "navigation".into()]
                    },
                    Vec::new(),
                );
                Ok((
                    (tag_ids, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: None,
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        Ok((tag_ids, receipt))
    }

    pub fn rename_or_merge_tag_namespace(
        &self,
        namespace_id: TagNamespaceId,
        name: &str,
    ) -> Result<MutationReceipt> {
        let name = name.trim().to_owned();
        if name.contains(':') {
            return Err(LibraryError::InvalidInput(
                "tag namespace cannot contain a colon".into(),
            ));
        }
        if load_namespace_name_for_library(self, namespace_id)? == name {
            return self.rename_tag_namespace(namespace_id, &name);
        }
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                use rusqlite::OptionalExtension;

                let source_state = load_namespace_definition(transaction, namespace_id)?
                    .ok_or_else(|| {
                        LibraryError::NotFound(format!("tag namespace {}", namespace_id.0))
                    })?;
                if source_state.display_name.is_empty() {
                    return Err(LibraryError::InvalidInput(
                        "the unnamespaced tag group cannot be renamed".into(),
                    ));
                }
                let target_id = transaction
                    .query_row(
                        "SELECT namespace_id FROM tag_namespace
                         WHERE display_name = ?1 AND namespace_id != ?2",
                        rusqlite::params![name, namespace_id.0],
                        |row| row.get::<_, u32>(0).map(TagNamespaceId),
                    )
                    .optional()?;
                let Some(target_id) = target_id else {
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
                    return Ok((
                        receipt.clone(),
                        PublishedDelta {
                            snapshot: next,
                            receipt,
                            history: Some(HistoryEntry::for_command(
                                "tags.group.rename",
                                "Rename tag namespace",
                                SemanticChange::TagNamespaceName {
                                    namespace_id,
                                    before: source_state.display_name,
                                    after: name.clone(),
                                },
                            )),
                        },
                    ));
                };

                let source_tags = transaction
                    .prepare(
                        "SELECT tag_id, subname FROM tag_definition
                         WHERE namespace_id = ?1 ORDER BY tag_id",
                    )?
                    .query_map([namespace_id.0], |row| {
                        Ok((crate::TagId(row.get(0)?), row.get::<_, String>(1)?))
                    })?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let mut next = (*snapshot).clone();
                let mut affected = RoaringBitmap::new();
                let mut changes = Vec::new();
                for (source_tag_id, subname) in source_tags {
                    let destination = transaction
                        .query_row(
                            "SELECT tag_id FROM tag_definition
                             WHERE namespace_id = ?1 AND subname = ?2",
                            rusqlite::params![target_id.0, subname],
                            |row| row.get::<_, u32>(0).map(crate::TagId),
                        )
                        .optional()?;
                    if let Some(destination) = destination {
                        let (members, tag_changes) = remove_tag_in_transaction(
                            transaction,
                            &mut next,
                            source_tag_id,
                            Some(destination),
                            revision,
                        )?;
                        affected |= members;
                        changes.extend(tag_changes);
                    } else {
                        let before = next
                            .tag_ids_by_name
                            .iter()
                            .find_map(|(name, id)| (*id == source_tag_id).then_some(name.clone()))
                            .ok_or_else(|| {
                                LibraryError::InvalidState(format!(
                                    "tag {} has no projection name",
                                    source_tag_id.0
                                ))
                            })?;
                        let after = if name.is_empty() {
                            subname.clone()
                        } else {
                            format!("{name}:{subname}")
                        };
                        transaction.execute(
                            "UPDATE tag_definition SET namespace_id = ?2 WHERE tag_id = ?1",
                            rusqlite::params![source_tag_id.0, target_id.0],
                        )?;
                        let names = Arc::make_mut(&mut next.tag_ids_by_name);
                        names.remove(&before);
                        names.insert(after.clone(), source_tag_id);
                        changes.push(SemanticChange::TagName {
                            tag_id: source_tag_id,
                            before,
                            after,
                        });
                    }
                }
                transaction.execute(
                    "DELETE FROM tag_namespace WHERE namespace_id = ?1",
                    [namespace_id.0],
                )?;
                changes.push(SemanticChange::TagNamespaceDefinition {
                    before: Some(source_state),
                    after: None,
                });
                insert_cloud_journal(
                    transaction,
                    revision,
                    "tag.namespace.merge",
                    Some(&affected),
                    serde_json::json!({
                        "source_namespace_id": namespace_id.0,
                        "destination_namespace_id": target_id.0,
                    }),
                    now_ms(),
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
                    affected.iter().map(RootId),
                );
                Ok((
                    receipt.clone(),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "tags.group.rename",
                            "Merge tag namespaces",
                            SemanticChange::Compound(changes),
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    pub fn delete_tag_namespace(&self, namespace_id: TagNamespaceId) -> Result<MutationReceipt> {
        let general_id = self
            .database
            .read(WorkPriority::VisibleRead, |connection| {
                use rusqlite::OptionalExtension;
                connection
                    .query_row(
                        "SELECT namespace_id FROM tag_namespace WHERE display_name = ''",
                        [],
                        |row| row.get::<_, u32>(0).map(TagNamespaceId),
                    )
                    .optional()
                    .map_err(Into::into)
            })?;
        if general_id == Some(namespace_id) {
            return Err(LibraryError::InvalidInput(
                "the unnamespaced tag group cannot be deleted".into(),
            ));
        }
        self.rename_or_merge_tag_namespace(namespace_id, "")
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

    pub fn delete_unused_tags(&self) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let tag_ids = transaction
                    .prepare("SELECT tag_id FROM tag_definition ORDER BY tag_id")?
                    .query_map([], |row| row.get::<_, u32>(0).map(crate::TagId))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let unused = tag_ids
                    .into_iter()
                    .filter(|tag_id| snapshot.tags.get(tag_id).is_none_or(|roots| roots.is_empty()))
                    .collect::<Vec<_>>();
                let mut next = (*snapshot).clone();
                let mut changes = Vec::new();
                for tag_id in &unused {
                    let (_, tag_changes) = remove_tag_in_transaction(
                        transaction,
                        &mut next,
                        *tag_id,
                        None,
                        revision,
                    )?;
                    changes.extend(tag_changes);
                }
                let settings_changed =
                    if let Some(change) = prune_deleted_starred_tags(transaction, &next)? {
                        changes.push(change);
                        true
                    } else {
                        false
                    };
                insert_cloud_journal(
                    transaction,
                    revision,
                    "tag.delete_unused",
                    None,
                    serde_json::json!({"tag_ids": unused.iter().map(|id| id.0).collect::<Vec<_>>() }),
                    now_ms(),
                )?;
                next.revision = revision;
                let mut resources =
                    vec!["tags".into(), "navigation".into(), "smart-folders".into()];
                if settings_changed {
                    resources.push("settings".into());
                }
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
                    Vec::new(),
                );
                Ok((receipt.clone(), PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: (!changes.is_empty()).then(|| HistoryEntry::for_command(
                        "tags.delete_unused",
                        "Delete unused tags",
                        SemanticChange::Compound(changes),
                    )),
                }))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok(receipt)
    }

    pub fn create_folder(
        &self,
        name: &str,
        parent_id: Option<FolderId>,
    ) -> Result<(FolderId, MutationReceipt)> {
        self.create_folder_with_stable_key(name, parent_id, uuid::Uuid::new_v4().to_string())
    }

    pub fn create_folder_with_stable_key(
        &self,
        name: &str,
        parent_id: Option<FolderId>,
        stable_key: String,
    ) -> Result<(FolderId, MutationReceipt)> {
        if name.trim().is_empty() {
            return Err(LibraryError::InvalidInput("folder name is empty".into()));
        }
        if stable_key.trim().is_empty() {
            return Err(LibraryError::InvalidInput(
                "folder stable key is empty".into(),
            ));
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
                validate_folder_parent(transaction, parent_id, None)?;
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
                        stable_key,
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

    pub fn duplicate_folder(&self, source_id: FolderId) -> Result<(FolderId, MutationReceipt)> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let ((duplicate_id, receipt), _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                let source = load_folder_definition(transaction, source_id)?
                    .ok_or_else(|| LibraryError::NotFound(format!("folder {source_id}")))?;
                let duplicate_name = unique_folder_copy_name(
                    transaction,
                    source.parent_id,
                    &format!("{} copy", source.name),
                )?;
                let root_display_order = transaction.query_row(
                    "SELECT COALESCE(MAX(display_order) + 1, 0)
                     FROM folder_definition WHERE parent_id IS ?1",
                    [source.parent_id.map(|id| id.0)],
                    |row| row.get::<_, i64>(0),
                )?;
                let rows = load_folder_clone_rows(transaction, source_id)?;
                validate_folder_nesting(
                    transaction,
                    source.parent_id,
                    folder_subtree_height(transaction, source_id)?,
                )?;
                let mut replacements = HashMap::<FolderId, FolderId>::new();
                let mut changes = Vec::with_capacity(rows.len());
                let mut next = (*snapshot).clone();
                let mut duplicate_id = None;
                for row in rows {
                    let folder_id = FolderId(LibraryDatabase::allocate_id(transaction)?);
                    let parent_id = if row.folder_id == source_id {
                        source.parent_id
                    } else {
                        row.parent_id.and_then(|id| replacements.get(&id).copied())
                    };
                    let name = if row.folder_id == source_id {
                        duplicate_name.clone()
                    } else {
                        row.name.clone()
                    };
                    let display_order = if row.folder_id == source_id {
                        root_display_order
                    } else {
                        row.display_order
                    };
                    transaction.execute(
                        "INSERT INTO folder_definition
                             (folder_id, stable_key, parent_id, name, icon, color, notes,
                              auto_tag_ids, display_order)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                        rusqlite::params![
                            folder_id.0,
                            uuid::Uuid::new_v4().to_string(),
                            parent_id.map(|id| id.0),
                            name,
                            row.icon,
                            row.color,
                            row.notes,
                            row.auto_tag_ids,
                            display_order,
                        ],
                    )?;
                    let after =
                        load_folder_definition(transaction, folder_id)?.ok_or_else(|| {
                            LibraryError::InvalidState(format!(
                                "duplicated folder {} disappeared",
                                folder_id.0
                            ))
                        })?;
                    changes.push(SemanticChange::FolderDefinition {
                        folder_id,
                        before: None,
                        after: Some(Box::new(after)),
                    });
                    replacements.insert(row.folder_id, folder_id);
                    Arc::make_mut(&mut next.folder_orders).insert(folder_id, Arc::new(Vec::new()));
                    Arc::make_mut(&mut next.folders).insert(folder_id, RoaringBitmap::new().into());
                    if row.folder_id == source_id {
                        duplicate_id = Some(folder_id);
                    }
                }
                let duplicate_id = duplicate_id.ok_or_else(|| {
                    LibraryError::InvalidState("folder clone produced no root".into())
                })?;
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.duplicate",
                    None,
                    serde_json::json!({"source_id": source_id.0, "folder_id": duplicate_id.0}),
                    now_ms(),
                )?;
                next.revision = revision;
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    vec!["folders".into(), "navigation".into()],
                    Vec::new(),
                );
                Ok((
                    (duplicate_id, receipt.clone()),
                    PublishedDelta {
                        snapshot: next,
                        receipt,
                        history: Some(HistoryEntry::for_command(
                            "folders.duplicate",
                            "Duplicate folder",
                            SemanticChange::Compound(changes),
                        )),
                    },
                ))
            },
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&history, history_entry);
        Ok((duplicate_id, receipt))
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

    pub fn folder_cover(&self, folder_id: FolderId) -> Result<Option<crate::FolderCover>> {
        use rusqlite::OptionalExtension;

        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let cover_root_id = connection
                    .query_row(
                        "SELECT cover_root_id FROM folder_definition WHERE folder_id = ?1",
                        [folder_id.0],
                        |row| row.get::<_, Option<u32>>(0),
                    )
                    .map_err(|error| match error {
                        rusqlite::Error::QueryReturnedNoRows => {
                            LibraryError::NotFound(format!("folder {folder_id}"))
                        }
                        error => error.into(),
                    })?;
                let Some(root_id) = cover_root_id.map(RootId) else {
                    return Ok(None);
                };
                if !snapshot
                    .folders
                    .get(&folder_id)
                    .is_some_and(|members| members.contains(root_id.0))
                {
                    return Ok(None);
                }
                connection
                    .query_row(
                        "SELECT file.content_hash, file.mime
                         FROM library_root root
                         JOIN media_item media ON media.media_id = root.cover_media_id
                         JOIN media_file file ON file.file_id = media.file_id
                         WHERE root.root_id = ?1",
                        [root_id.0],
                        |row| {
                            Ok(crate::FolderCover {
                                root_id,
                                content_hash: row.get(0)?,
                                mime: row.get(1)?,
                            })
                        },
                    )
                    .optional()
                    .map_err(Into::into)
            },
        )
    }

    pub fn set_folder_cover(
        &self,
        folder_id: FolderId,
        root_id: RootId,
    ) -> Result<MutationReceipt> {
        let snapshot = self.projections.snapshot();
        if !snapshot
            .folders
            .get(&folder_id)
            .is_some_and(|members| members.contains(root_id.0))
        {
            return Err(LibraryError::InvalidInput(
                "folder cover root must belong to the folder".into(),
            ));
        }
        drop(snapshot);
        self.update_folder_definition(
            "folders.set_cover",
            "Set folder cover",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition SET cover_root_id = ?2 WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, root_id.0],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!("folder {folder_id}")));
                }
                Ok(())
            },
        )
    }

    pub fn set_folder_watch(
        &self,
        folder_id: FolderId,
        path: &str,
        include_subfolders: bool,
    ) -> Result<MutationReceipt> {
        let path = required_name("watch path", path)?;
        self.update_folder_definition(
            "folders.set_watch",
            "Set watched folder",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition
                     SET watch_path = ?2, watch_enabled = 1, watch_subfolders = ?3
                     WHERE folder_id = ?1",
                    rusqlite::params![folder_id.0, path, include_subfolders],
                )? == 0
                {
                    return Err(LibraryError::NotFound(format!("folder {folder_id}")));
                }
                Ok(())
            },
        )
    }

    pub fn clear_folder_watch(&self, folder_id: FolderId) -> Result<MutationReceipt> {
        self.update_folder_definition(
            "folders.clear_watch",
            "Clear watched folder",
            folder_id,
            move |transaction| {
                if transaction.execute(
                    "UPDATE folder_definition
                     SET watch_path = NULL, watch_enabled = 0, watch_subfolders = 0
                     WHERE folder_id = ?1",
                    [folder_id.0],
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

    pub fn folder_child_capacity(&self, parent_id: Option<FolderId>) -> Result<usize> {
        self.database.read(WorkPriority::VisibleRead, |connection| {
            Ok(crate::model::MAX_FOLDER_DEPTH - folder_depth(connection, parent_id)?)
        })
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

    pub fn sort_folder_tree(
        &self,
        folder_id: FolderId,
        descending: bool,
        recursive: bool,
    ) -> Result<MutationReceipt> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let history = self.history.clone();
        let (receipt, _, history_entry) = self.database.published_write(
            WorkPriority::ForegroundMutation,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| {
                require_folder(transaction, folder_id)?;
                let parent_ids = if recursive {
                    load_folder_subtree(transaction, &[folder_id])?
                        .into_iter()
                        .map(|(id, _)| id)
                        .collect::<Vec<_>>()
                } else {
                    vec![folder_id]
                };
                let mut changes = Vec::new();
                let mut update = transaction.prepare_cached(
                    "UPDATE folder_definition SET display_order = ?2 WHERE folder_id = ?1",
                )?;
                for parent_id in parent_ids {
                    let children = load_folder_children(transaction, Some(parent_id))?;
                    let mut states = children
                        .iter()
                        .map(|id| {
                            load_folder_definition(transaction, *id)?.ok_or_else(|| {
                                LibraryError::InvalidState(format!(
                                    "folder {} disappeared during sort",
                                    id.0
                                ))
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    states.sort_by(|left, right| {
                        let ordering = left
                            .name
                            .to_lowercase()
                            .cmp(&right.name.to_lowercase())
                            .then_with(|| left.folder_id.cmp(&right.folder_id));
                        if descending {
                            ordering.reverse()
                        } else {
                            ordering
                        }
                    });
                    for (display_order, before) in states.into_iter().enumerate() {
                        if before.display_order == display_order as i64 {
                            continue;
                        }
                        update
                            .execute(rusqlite::params![before.folder_id.0, display_order as i64])?;
                        let after = load_folder_definition(transaction, before.folder_id)?
                            .ok_or_else(|| {
                                LibraryError::InvalidState(format!(
                                    "folder {} disappeared during sort",
                                    before.folder_id.0
                                ))
                            })?;
                        changes.push(SemanticChange::FolderDefinition {
                            folder_id: before.folder_id,
                            before: Some(Box::new(before)),
                            after: Some(Box::new(after)),
                        });
                    }
                }
                insert_cloud_journal(
                    transaction,
                    revision,
                    "folder.sort_tree",
                    None,
                    serde_json::json!({
                        "folder_id": folder_id.0,
                        "descending": descending,
                        "recursive": recursive,
                    }),
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
                                "folders.sort_tree",
                                "Sort folders",
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

    pub fn sort_folder_items(
        &self,
        folder_id: FolderId,
        field: crate::model::ContentSortField,
    ) -> Result<MutationReceipt> {
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
                let root_ids = serde_json::to_string(
                    &roots.iter().map(|root_id| root_id.0).collect::<Vec<_>>(),
                )?;
                let order = match field {
                    crate::model::ContentSortField::Name => {
                        "lower(root.name), root.root_id"
                    }
                    crate::model::ContentSortField::ImportedAt => {
                        "root.imported_at_ms DESC, root.root_id"
                    }
                    crate::model::ContentSortField::CreatedAt => {
                        "root.captured_at_ms IS NULL, root.captured_at_ms DESC, root.root_id"
                    }
                    crate::model::ContentSortField::ModifiedAt => {
                        "root.modified_at_ms DESC, root.root_id"
                    }
                    crate::model::ContentSortField::Size => {
                        "root.total_size_bytes DESC, root.root_id"
                    }
                    crate::model::ContentSortField::Notes => {
                        "NULLIF(trim(root.notes), '') IS NULL, lower(root.notes), lower(root.name), root.root_id"
                    }
                };
                let sql = format!(
                    "SELECT root.root_id
                     FROM json_each(?1) selected
                     JOIN library_root root
                       ON root.root_id = CAST(selected.value AS INTEGER)
                     ORDER BY {order}"
                );
                let mut statement = connection.prepare(&sql)?;
                let ordered = statement
                    .query_map([root_ids], |row| row.get::<_, u32>(0))?
                    .map(|value| value.map(RootId))
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(ordered)
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
                let output = crate::group::organize(
                    transaction,
                    revision,
                    (*snapshot).clone(),
                    &request,
                    false,
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
                let (before, before_notes) = transaction
                    .query_row(
                        "SELECT cover_media_id, notes FROM library_root WHERE root_id = ?1",
                        [collection_id.0],
                        |row| Ok((MediaId(row.get(0)?), row.get::<_, Option<String>>(1)?)),
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
                    crate::smart::settle_affected(transaction, &mut next, &affected)?;
                    let after_notes = transaction.query_row(
                        "SELECT notes FROM library_root WHERE root_id = ?1",
                        [collection_id.0],
                        |row| row.get::<_, Option<String>>(0),
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
                            before_notes,
                            after_notes,
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
                        "SELECT file.file_id, file.content_hash, file.file_path,
                                CASE WHEN cloud.remote_present = 1
                                     THEN cloud.remote_extension ELSE NULL END
                         FROM media_item media
                         JOIN media_file file ON file.file_id = media.file_id
                         LEFT JOIN cloud_blob_state cloud
                           ON cloud.file_hash = file.content_hash
                         WHERE media.media_id = ?1",
                    )?;
                    for media_id in &media_ids {
                        let (file_id, content_hash, file_path, remote_extension) = statement
                            .query_row([media_id], |row| {
                                Ok((
                                    row.get::<_, u32>(0)?,
                                    row.get::<_, String>(1)?,
                                    row.get::<_, String>(2)?,
                                    row.get::<_, Option<String>>(3)?,
                                ))
                            })?;
                        files
                            .entry(file_id)
                            .or_insert((content_hash, file_path, remote_extension));
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
                    "DELETE FROM root_fts WHERE rowid IN
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
                let (device_id, retention_json) = transaction.query_row(
                    "SELECT device_id, retention_json FROM cloud_state WHERE singleton = 1",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )?;
                let retention_days = serde_json::from_str::<serde_json::Value>(&retention_json)
                    .ok()
                    .and_then(|value| value.get("deleted_blobs_days")?.as_i64())
                    .unwrap_or(7)
                    .clamp(0, 3_650);
                let deleted_at =
                    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(deleted_at_ms)
                        .ok_or_else(|| {
                            LibraryError::InvalidInput("deletion timestamp is outside range".into())
                        })?;
                let purge_after = deleted_at + chrono::Duration::days(retention_days);

                for (file_id, (content_hash, file_path, remote_extension)) in files {
                    let referenced = transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM media_item WHERE file_id = ?1)",
                        [file_id],
                        |row| row.get::<_, bool>(0),
                    )?;
                    if !referenced {
                        if let Some(remote_extension) = remote_extension {
                            transaction.execute(
                                "INSERT INTO cloud_tombstone
                                     (object_kind, object_key, mutation_id, hlc_physical_ms,
                                      hlc_logical, device_id, causal_frontier_json,
                                      deleted_at, purge_after)
                                 VALUES ('blob', ?1, ?2, ?3, 0, ?4, '{}', ?5, ?6)
                                 ON CONFLICT(object_kind, object_key) DO UPDATE SET
                                     mutation_id = excluded.mutation_id,
                                     hlc_physical_ms = excluded.hlc_physical_ms,
                                     hlc_logical = excluded.hlc_logical,
                                     device_id = excluded.device_id,
                                     causal_frontier_json = excluded.causal_frontier_json,
                                     deleted_at = excluded.deleted_at,
                                     purge_after = excluded.purge_after",
                                rusqlite::params![
                                    format!("{content_hash}.{remote_extension}"),
                                    uuid::Uuid::new_v4().to_string(),
                                    deleted_at_ms,
                                    device_id,
                                    deleted_at.to_rfc3339(),
                                    purge_after.to_rfc3339(),
                                ],
                            )?;
                        }
                        transaction.execute(
                            "INSERT INTO blob_cleanup_queue(file_id, file_path, enqueued_revision)
                             VALUES (?1, ?2, ?3)
                             ON CONFLICT(file_id) DO NOTHING",
                            rusqlite::params![file_id, file_path, revision as i64],
                        )?;
                        let file_id = crate::FileId(file_id);
                        if !protected_cleanup_files.contains(&file_id) {
                            cleanup.push(crate::PendingBlobCleanup {
                                file_id,
                                content_hash,
                                file_path,
                            });
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
                let mut next = (*snapshot).clone();
                let (source_members, mut history_changes) = remove_tag_in_transaction(
                    transaction,
                    &mut next,
                    source,
                    destination,
                    revision,
                )?;
                let settings_changed = if destination.is_none() {
                    if let Some(change) = prune_deleted_starred_tags(transaction, &next)? {
                        history_changes.push(change);
                        true
                    } else {
                        false
                    }
                } else {
                    false
                };
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
                let mut resources = vec![
                    "roots".into(),
                    "tags".into(),
                    "navigation".into(),
                    "smart-folders".into(),
                ];
                if settings_changed {
                    resources.push("settings".into());
                }
                let receipt = PublicationCoordinator::receipt(
                    revision,
                    resources,
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
        SemanticChange::AuxiliaryJson {
            table,
            key,
            before,
            after,
            resource,
        } => {
            use rusqlite::OptionalExtension;
            let (table, key_column) = auxiliary_json_spec(table)?;
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let current = transaction
                .query_row(
                    &format!("SELECT value_json FROM {table} WHERE {key_column} = ?1"),
                    [key],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if &current != expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because {table}/{key} changed"
                )));
            }
            if let Some(value) = replacement {
                transaction.execute(
                    &format!(
                        "INSERT INTO {table} ({key_column}, value_json) VALUES (?1, ?2)
                         ON CONFLICT({key_column}) DO UPDATE SET value_json = excluded.value_json"
                    ),
                    rusqlite::params![key, value],
                )?;
            } else {
                transaction.execute(
                    &format!("DELETE FROM {table} WHERE {key_column} = ?1"),
                    [key],
                )?;
            }
            resources.insert((*resource).into());
        }
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
            before_notes,
            after_notes,
        } => {
            let (expected, replacement, expected_notes, replacement_notes) = if use_after {
                (*before, *after, before_notes, after_notes)
            } else {
                (*after, *before, after_notes, before_notes)
            };
            let (current, current_notes) = transaction
                .query_row(
                    "SELECT cover_media_id, notes FROM library_root WHERE root_id = ?1",
                    [root_id.0],
                    |row| Ok((MediaId(row.get(0)?), row.get::<_, Option<String>>(1)?)),
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::NotFound(format!("collection {root_id}"))
                    }
                    error => error.into(),
                })?;
            if current != expected || &current_notes != expected_notes {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because collection {root_id} cover changed"
                )));
            }
            crate::group::set_cover(transaction, snapshot, *root_id, replacement)?;
            transaction.execute(
                "UPDATE library_root SET notes = ?2 WHERE root_id = ?1",
                rusqlite::params![root_id.0, replacement_notes],
            )?;
            if replacement_notes.is_some() {
                Arc::make_mut(&mut snapshot.notes_present).insert(root_id.0);
            } else {
                Arc::make_mut(&mut snapshot.notes_present).remove(root_id.0);
            }
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
        SemanticChange::TagNamespaceDefinition { before, after } => {
            let (expected, replacement) = if use_after {
                (before, after)
            } else {
                (after, before)
            };
            let namespace_id = expected
                .as_ref()
                .or(replacement.as_ref())
                .expect("namespace definition history has one state")
                .namespace_id;
            if load_namespace_definition(transaction, namespace_id)? != *expected {
                return Err(LibraryError::InvalidState(format!(
                    "cannot replay history because tag namespace {} changed",
                    namespace_id.0
                )));
            }
            if let Some(state) = replacement {
                transaction.execute(
                    "INSERT INTO tag_namespace(namespace_id, stable_key, display_name)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(namespace_id) DO UPDATE SET
                         stable_key = excluded.stable_key,
                         display_name = excluded.display_name",
                    rusqlite::params![state.namespace_id.0, state.stable_key, state.display_name,],
                )?;
            } else {
                transaction.execute(
                    "DELETE FROM tag_namespace WHERE namespace_id = ?1",
                    [namespace_id.0],
                )?;
            }
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
            if !queries.is_empty() {
                crate::smart::refresh_all(transaction, snapshot)?;
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
                              auto_tag_ids, cover_root_id, watch_path, watch_enabled,
                              watch_subfolders, display_order)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
                         ON CONFLICT(folder_id) DO UPDATE SET
                             stable_key = excluded.stable_key,
                             parent_id = excluded.parent_id,
                             name = excluded.name,
                             icon = excluded.icon,
                             color = excluded.color,
                             notes = excluded.notes,
                             auto_tag_ids = excluded.auto_tag_ids,
                             cover_root_id = excluded.cover_root_id,
                             watch_path = excluded.watch_path,
                             watch_enabled = excluded.watch_enabled,
                             watch_subfolders = excluded.watch_subfolders,
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
                            folder.cover_root_id.map(|id| id.0),
                            folder.watch_path,
                            folder.watch_enabled,
                            folder.watch_subfolders,
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
                    crate::smart::refresh_subtree(transaction, snapshot, *smart_folder_id)?;
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
            if state.changes_names() {
                resources.insert("search".into());
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
    if load_structural_media_notes(transaction, &expected.projection, affected)?
        != *expected.media_notes
    {
        return Err(LibraryError::InvalidState(
            "cannot replay history because retained media notes changed".into(),
        ));
    }

    for root_id in affected {
        if snapshot.collection_orders.contains_key(&RootId(root_id)) {
            ordering::delete(transaction, OrderOwnerKind::Collection, root_id)?;
        }
        crate::fts::remove_root(transaction, root_id)?;
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

    let mut restore_media_notes =
        transaction.prepare_cached("UPDATE media_item SET media_notes = ?2 WHERE media_id = ?1")?;
    for media in replacement.media_notes.iter() {
        restore_media_notes.execute(rusqlite::params![media.media_id.0, media.notes])?;
    }
    drop(restore_media_notes);

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

fn remove_tag_in_transaction(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &mut ProjectionSnapshot,
    source: crate::TagId,
    destination: Option<crate::TagId>,
    revision: u64,
) -> Result<(RoaringBitmap, Vec<SemanticChange>)> {
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
    let destination_before = destination
        .and_then(|tag_id| snapshot.tags.get(&tag_id))
        .map(|members| members.to_bitmap())
        .unwrap_or_default();
    let mut history_changes = Vec::new();

    let folder_ids = transaction
        .prepare("SELECT folder_id FROM folder_definition")?
        .query_map([], |row| row.get::<_, u32>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
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
        let mut after = destination_before.clone();
        after |= &source_members;
        if destination_before != after {
            bitmap::replace(
                transaction,
                revision,
                BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: destination.0,
                },
                &after,
            )?;
            Arc::make_mut(&mut snapshot.tags).insert(destination, after.clone().into());
            history_changes.push(SemanticChange::Bitmap {
                key: BitmapKey {
                    domain: BitmapDomain::Tag,
                    key_id: destination.0,
                },
                before: Arc::new(destination_before.clone()),
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
    Arc::make_mut(&mut snapshot.tags).remove(&source);
    Arc::make_mut(&mut snapshot.tag_ids_by_name).remove(&source_state.full_name);

    let counts = Arc::make_mut(&mut snapshot.tag_count);
    for root_id in &source_members {
        if destination.is_none() || destination_before.contains(root_id) {
            let current = counts.value(root_id).unwrap_or(0);
            counts.insert(root_id, current.saturating_sub(1));
        }
    }
    let query_changes =
        crate::smart::rewrite_tag_references(transaction, snapshot, source, destination)?;
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
    Ok((source_members, history_changes))
}

fn prune_deleted_starred_tags(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
) -> Result<Option<SemanticChange>> {
    use rusqlite::OptionalExtension;

    let before = transaction
        .query_row(
            "SELECT value_json FROM setting WHERE key = 'application'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(before) = before else {
        return Ok(None);
    };
    let mut settings = serde_json::from_str::<serde_json::Value>(&before).map_err(|error| {
        LibraryError::InvalidState(format!(
            "application settings contain invalid JSON: {error}"
        ))
    })?;
    let original = settings.clone();
    let Some(starred_tags) = settings
        .as_object_mut()
        .and_then(|object| object.get_mut("starredTags"))
        .and_then(serde_json::Value::as_array_mut)
    else {
        return Ok(None);
    };
    starred_tags.retain(|entry| {
        entry
            .as_str()
            .is_some_and(|name| snapshot.tag_ids_by_name.contains_key(name))
    });
    if settings == original {
        return Ok(None);
    }

    let after = serde_json::to_string(&settings).map_err(|error| {
        LibraryError::InvalidState(format!("application settings cannot be encoded: {error}"))
    })?;
    transaction.execute(
        "UPDATE setting SET value_json = ?1 WHERE key = 'application'",
        [&after],
    )?;
    Ok(Some(SemanticChange::AuxiliaryJson {
        table: "setting",
        key: "application".into(),
        before: Some(before),
        after: Some(after),
        resource: "settings",
    }))
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

fn load_namespace_name_for_library(
    library: &Library,
    namespace_id: TagNamespaceId,
) -> Result<String> {
    library
        .database
        .read(WorkPriority::VisibleRead, |connection| {
            load_namespace_name(connection, namespace_id)
        })
}

fn load_namespace_definition(
    connection: &rusqlite::Connection,
    namespace_id: TagNamespaceId,
) -> Result<Option<TagNamespaceDefinitionState>> {
    use rusqlite::OptionalExtension;

    connection
        .query_row(
            "SELECT stable_key, display_name FROM tag_namespace WHERE namespace_id = ?1",
            [namespace_id.0],
            |row| {
                Ok(TagNamespaceDefinitionState {
                    namespace_id,
                    stable_key: row.get(0)?,
                    display_name: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
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
                    cover_root_id, watch_path, watch_enabled, watch_subfolders, display_order
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
                    cover_root_id: row.get::<_, Option<u32>>(7)?.map(RootId),
                    watch_path: row.get(8)?,
                    watch_enabled: row.get(9)?,
                    watch_subfolders: row.get(10)?,
                    display_order: row.get(11)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}

fn list_folders(
    connection: &rusqlite::Connection,
    snapshot: &ProjectionSnapshot,
) -> Result<Vec<FolderRecord>> {
    let mut statement = connection.prepare(
        "SELECT folder_id, stable_key, parent_id, name, icon, color, notes,
                cover_root_id, watch_path, watch_enabled, watch_subfolders, display_order
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
            cover_root_id: row.get::<_, Option<u32>>(7)?.map(RootId),
            watch_path: row.get(8)?,
            watch_enabled: row.get(9)?,
            watch_subfolders: row.get(10)?,
            display_order: row.get(11)?,
            count: snapshot
                .folders
                .get(&folder_id)
                .map_or(0, |roots| (roots & snapshot.active()).len()),
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
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

fn unique_folder_copy_name(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
    base: &str,
) -> Result<String> {
    for suffix in 1..=10_000 {
        let candidate = if suffix == 1 {
            base.to_owned()
        } else {
            format!("{base} {suffix}")
        };
        let exists = connection.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM folder_definition
                 WHERE parent_id IS ?1 AND name = ?2
             )",
            rusqlite::params![parent_id.map(|id| id.0), candidate],
            |row| row.get::<_, bool>(0),
        )?;
        if !exists {
            return Ok(candidate);
        }
    }
    Err(LibraryError::InvalidState(
        "could not allocate a unique folder copy name".into(),
    ))
}

fn load_folder_clone_rows(
    connection: &rusqlite::Connection,
    root_id: FolderId,
) -> Result<Vec<FolderDefinitionState>> {
    let mut statement = connection.prepare(
        "WITH RECURSIVE subtree(folder_id, depth) AS (
             SELECT ?1, 0
             UNION ALL
             SELECT child.folder_id, subtree.depth + 1
             FROM folder_definition child
             JOIN subtree ON child.parent_id = subtree.folder_id
         )
         SELECT folder.stable_key, folder.parent_id, folder.name, folder.icon,
                folder.color, folder.notes, folder.auto_tag_ids, folder.cover_root_id,
                folder.watch_path, folder.watch_enabled, folder.watch_subfolders,
                folder.display_order, folder.folder_id
         FROM subtree
         JOIN folder_definition folder ON folder.folder_id = subtree.folder_id
         ORDER BY subtree.depth, folder.parent_id, folder.display_order, folder.folder_id",
    )?;
    let rows = statement
        .query_map([root_id.0], |row| {
            Ok(FolderDefinitionState {
                folder_id: FolderId(row.get(12)?),
                stable_key: row.get(0)?,
                parent_id: row.get::<_, Option<u32>>(1)?.map(FolderId),
                name: row.get(2)?,
                icon: row.get(3)?,
                color: row.get(4)?,
                notes: row.get(5)?,
                auto_tag_ids: row.get(6)?,
                // Clones have no memberships, so covers and watches are intentionally not copied.
                cover_root_id: None,
                watch_path: None,
                watch_enabled: false,
                watch_subfolders: false,
                display_order: row.get(11)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, rusqlite::Error>>()?;
    if rows.is_empty() {
        return Err(LibraryError::NotFound(format!("folder {root_id}")));
    }
    Ok(rows)
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
    if let Some(parent_id) = parent_id {
        require_folder(connection, parent_id)?;
    }
    if let (Some(parent_id), Some(child_id)) = (parent_id, child_id) {
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
    let subtree_height = child_id
        .map(|child_id| folder_subtree_height(connection, child_id))
        .transpose()?
        .unwrap_or(1);
    validate_folder_nesting(connection, parent_id, subtree_height)
}

fn folder_subtree_height(connection: &rusqlite::Connection, folder_id: FolderId) -> Result<usize> {
    require_folder(connection, folder_id)?;
    connection
        .query_row(
            "WITH RECURSIVE descendants(folder_id, depth) AS (
                 SELECT folder_id, 1 FROM folder_definition WHERE folder_id = ?1
                 UNION ALL
                 SELECT child.folder_id, parent.depth + 1
                 FROM folder_definition child
                 JOIN descendants parent ON child.parent_id = parent.folder_id
             )
             SELECT COALESCE(MAX(depth), 1) FROM descendants",
            [folder_id.0],
            |row| row.get::<_, u32>(0),
        )
        .map(|height| height as usize)
        .map_err(Into::into)
}

fn validate_folder_nesting(
    connection: &rusqlite::Connection,
    parent_id: Option<FolderId>,
    subtree_height: usize,
) -> Result<()> {
    let parent_depth = folder_depth(connection, parent_id)?;
    if parent_depth + subtree_height > crate::model::MAX_FOLDER_DEPTH {
        return Err(LibraryError::InvalidInput(format!(
            "folders may be nested at most {} levels deep",
            crate::model::MAX_FOLDER_DEPTH
        )));
    }
    Ok(())
}

fn folder_depth(connection: &rusqlite::Connection, folder_id: Option<FolderId>) -> Result<usize> {
    let depth = if let Some(folder_id) = folder_id {
        require_folder(connection, folder_id)?;
        connection.query_row(
            "WITH RECURSIVE ancestors(folder_id, parent_id, depth) AS (
                 SELECT folder_id, parent_id, 1
                 FROM folder_definition WHERE folder_id = ?1
                 UNION ALL
                 SELECT parent.folder_id, parent.parent_id, child.depth + 1
                 FROM folder_definition parent
                 JOIN ancestors child ON parent.folder_id = child.parent_id
             )
             SELECT COALESCE(MAX(depth), 0) FROM ancestors",
            [folder_id.0],
            |row| row.get::<_, u32>(0),
        )? as usize
    } else {
        0
    };
    Ok(depth)
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
    let parent_depth = if let Some(parent_id) = parent_id {
        transaction.query_row(
            "WITH RECURSIVE ancestors(smart_folder_id, parent_id, depth) AS (
                 SELECT smart_folder_id, parent_id, 1
                 FROM smart_folder_definition WHERE smart_folder_id = ?1
                 UNION ALL
                 SELECT parent.smart_folder_id, parent.parent_id, child.depth + 1
                 FROM smart_folder_definition parent
                 JOIN ancestors child ON parent.smart_folder_id = child.parent_id
             )
             SELECT COALESCE(MAX(depth), 0) FROM ancestors",
            [parent_id.0],
            |row| row.get::<_, u32>(0),
        )? as usize
    } else {
        0
    };
    let subtree_height = if let Some(child_id) = child_id {
        transaction.query_row(
            "WITH RECURSIVE descendants(smart_folder_id, depth) AS (
                 SELECT smart_folder_id, 1 FROM smart_folder_definition
                 WHERE smart_folder_id = ?1
                 UNION ALL
                 SELECT child.smart_folder_id, parent.depth + 1
                 FROM smart_folder_definition child
                 JOIN descendants parent ON child.parent_id = parent.smart_folder_id
             )
             SELECT COALESCE(MAX(depth), 1) FROM descendants",
            [child_id.0],
            |row| row.get::<_, u32>(0),
        )? as usize
    } else {
        1
    };
    if parent_depth + subtree_height > crate::smart::MAX_SMART_FOLDER_DEPTH {
        return Err(LibraryError::InvalidInput(format!(
            "smart folders may be nested at most {} levels deep",
            crate::smart::MAX_SMART_FOLDER_DEPTH
        )));
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
        media_notes: Arc::new(load_structural_media_notes(
            transaction,
            snapshot,
            affected,
        )?),
        projection: Arc::new(snapshot.clone()),
    })
}

fn load_structural_media_notes(
    transaction: &rusqlite::Transaction<'_>,
    snapshot: &ProjectionSnapshot,
    affected: &RoaringBitmap,
) -> Result<Vec<StructuralMediaNoteState>> {
    let mut media_ids = snapshot
        .media_owner
        .iter()
        .filter_map(|(media_id, owner)| affected.contains(owner.0).then_some(media_id))
        .collect::<Vec<_>>();
    media_ids.sort_unstable();
    let mut statement =
        transaction.prepare_cached("SELECT media_notes FROM media_item WHERE media_id = ?1")?;
    media_ids
        .into_iter()
        .map(|media_id| {
            statement
                .query_row([media_id], |row| {
                    Ok(StructuralMediaNoteState {
                        media_id: MediaId(media_id),
                        notes: row.get(0)?,
                    })
                })
                .map_err(Into::into)
        })
        .collect()
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

fn record_source_attempt_root(
    transaction: &rusqlite::Transaction<'_>,
    attempt_id: i64,
    root_id: RootId,
) -> Result<()> {
    transaction.execute(
        "INSERT INTO source_attempt_root(attempt_id, root_id, root_stable_key)
         SELECT ?1, item.local_id, item.stable_key
         FROM library_item item
         WHERE item.local_id = ?2
         ON CONFLICT DO NOTHING",
        rusqlite::params![attempt_id, root_id.0],
    )?;
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

fn auxiliary_json_spec(table: &str) -> Result<(&'static str, &'static str)> {
    match table {
        "setting" => Ok(("setting", "key")),
        "view_pref" => Ok(("view_pref", "scope")),
        _ => Err(LibraryError::InvalidInput(format!(
            "unsupported auxiliary JSON table {table}"
        ))),
    }
}
