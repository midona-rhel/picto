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
use crate::query_request::{OpenCancellation, OpenStage};
use crate::selection::{SelectionSummary, SelectionTarget};
use crate::smart::SmartFolderRecord;
use crate::{LibraryDatabase, LibraryError, Result};

// Library methods grouped by the aggregate they write. Shared helpers
// (publication, projections, history plumbing) stay in this file.
mod tags;
mod folders;
mod smart_folders;
mod imports;
mod duplicates;
mod structure;
mod undo;
use tags::*;
use folders::*;
use smart_folders::*;
use structure::*;
use undo::*;

struct PublishedDelta {
    snapshot: ProjectionSnapshot,
    receipt: MutationReceipt,
    history: Option<HistoryEntry>,
}

/// The one owner of resident derived state. Every publishing transaction
/// settles here before commit: the dirty-root journal names every root whose
/// canonical browsing facts changed, and the receipt names the auxiliary
/// tables that changed. A failed transaction publishes nothing, so the
/// resident model never drifts from what readers can see.
impl crate::database::PublicationDelta for PublishedDelta {
    fn settle(&mut self, transaction: &rusqlite::Transaction<'_>) -> Result<()> {
        let snapshot = &mut self.snapshot;
        // Most publications change no drawing fact; do not copy the drawing
        // index for them.
        let dirty = if crate::browsing::DrawingIndex::journal_is_dirty(transaction)? {
            Arc::make_mut(&mut snapshot.drawing).settle(transaction)?
        } else {
            RoaringBitmap::new()
        };
        if !dirty.is_empty() {
            Arc::make_mut(&mut snapshot.name_index).refresh(transaction, &dirty)?;
        }
        // Receipts carry the renderer's resource names.
        let changed = |resource: &str| self.receipt.resources.iter().any(|value| value == resource);
        if changed("recently_viewed")
            || !snapshot.recent_views.members.is_disjoint(&dirty)
        {
            snapshot.recent_views = Arc::new(crate::projection::load_recent_views(transaction)?.into());
        }
        if changed("folders") {
            snapshot.folder_parents =
                Arc::new(crate::projection::load_folder_parents(transaction)?);
        }
        Ok(())
    }
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
    order_cache: Arc<crate::query::OrderCache>,
    smart_counts: Arc<crate::smart::SmartCountCache>,
    tag_list: Arc<tags::TagListCache>,
    window_requests: crate::query_request::WindowRequests,
}

impl Library {
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_database(LibraryDatabase::create(path)?)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with(path, &OpenCancellation::default(), &mut |_| {})
    }

    /// Open under a cancellation handle, reporting each stage in order. The
    /// handle is checked at every stage boundary and inside the migration and
    /// resident scans; a cancelled open returns `OpenCancelled` having
    /// committed nothing. `progress` runs on the opening thread while
    /// database connections are held, so it must not block.
    pub fn open_with(
        path: impl AsRef<Path>,
        cancellation: &OpenCancellation,
        progress: &mut dyn FnMut(OpenStage),
    ) -> Result<Self> {
        let token = crate::query_request::Cancellation::for_open(cancellation);
        token.check()?;
        // Entered on this thread for the synchronous open only; dropped
        // before the caller regains control.
        let _execution = token.enter();
        let database = LibraryDatabase::open_with(path, cancellation, progress)?;
        let library = Self::from_database_with(database, progress)?;
        progress(OpenStage::Ready);
        Ok(library)
    }

    /// Merge the legacy explicit `general` namespace into the canonical
    /// unnamespaced group. This is safe to run on every open and publishes the
    /// merge so cloud peers converge on the repaired tag identities as well.
    pub fn canonicalize_general_tag_namespace(&self) -> Result<Option<MutationReceipt>> {
        let namespace_ids = self
            .tag_namespaces()?
            .into_iter()
            .filter(|namespace| namespace.name.eq_ignore_ascii_case("general"))
            .map(|namespace| namespace.namespace_id)
            .collect::<Vec<_>>();
        let mut receipt = None;
        for namespace_id in namespace_ids {
            receipt = Some(self.rename_or_merge_tag_namespace(namespace_id, "")?);
        }
        if receipt.is_some() {
            // Opening a library must not expose automatic data repair as a
            // user action that can be undone back into the invalid state.
            self.history.clear();
        }
        Ok(receipt)
    }

    fn from_database(database: LibraryDatabase) -> Result<Self> {
        Self::from_database_with(database, &mut |_| {})
    }

    fn from_database_with(
        database: LibraryDatabase,
        progress: &mut dyn FnMut(OpenStage),
    ) -> Result<Self> {
        let database = Arc::new(database);
        // Checked before, not inside, this bounded housekeeping write so it
        // stays atomic.
        crate::query_request::check()?;
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
            // The resident model is rebuilt from canonical rows below, so
            // journal rows left behind by an earlier session carry nothing.
            transaction.execute("DELETE FROM browsing_dirty_root", [])?;
            Ok(())
        })?;
        let projections = Arc::new(ProjectionStore::load_with(&database, progress)?);
        Ok(Self {
            database,
            projections,
            publication: Arc::new(PublicationCoordinator::default()),
            history: Arc::new(SessionHistory::default()),
            match_cache: Arc::new(crate::query::MatchCache::default()),
            smart_counts: Arc::new(crate::smart::SmartCountCache::default()),
            tag_list: Arc::new(tags::TagListCache::default()),
            order_cache: Arc::new(crate::query::OrderCache::default()),
            window_requests: crate::query_request::WindowRequests::default(),
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
        let (output, receipt) = self.publish(priority, |transaction, revision, snapshot| {
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
        })?;
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
        self.update_auxiliary_json(command, label, table, key, true, move |_| Ok(value))
    }

    /// Read-modify-write of one auxiliary JSON value inside a single writer
    /// transaction, so concurrent patches cannot overwrite each other.
    /// `undoable` decides whether the change enters session history; view
    /// preferences (sort, zoom, layout) are not edits and stay out of it.
    pub fn update_auxiliary_json(
        &self,
        command: &'static str,
        label: &'static str,
        table: &'static str,
        key: &str,
        undoable: bool,
        update: impl FnOnce(Option<&str>) -> Result<Option<String>>,
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
                let value = update(before.as_deref())?;
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
                    history: undoable.then(|| HistoryEntry::for_command(
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

    /// A version of everything snapshot-only queries can observe. Unlike the
    /// database revision, queue and subscription bookkeeping does not move it,
    /// so cached query responses survive background work.
    pub fn query_epoch(&self) -> u64 {
        self.projections.snapshot().query_versions.epoch()
    }

    pub fn query(&self, query: &RootQuery, page: &PageRequest) -> Result<RootPage> {
        self.scoped(&query.scope, |snapshot, media| {
            crate::query::page_cached(
                snapshot,
                query,
                page,
                media,
                &self.match_cache,
                &self.order_cache,
            )
        })
    }

    pub fn mime_facets(&self, query: &RootQuery) -> Result<crate::query::MimeFacets> {
        self.scoped(&query.scope, |snapshot, media| {
            crate::query::mime_facets_cached(snapshot, query, media, Some(&self.match_cache))
        })
    }

    /// Run a query on one captured snapshot. Duplicate inspection is the one
    /// scope that still reads canonical tables, inside a consistent read that
    /// captures the matching snapshot; every other query never touches SQLite.
    fn scoped<T>(
        &self,
        scope: &crate::query::ItemScope,
        run: impl FnOnce(&ProjectionSnapshot, &crate::query::MediaMatches) -> Result<T>,
    ) -> Result<T> {
        if crate::query::MediaMatches::required(scope) {
            return self.database.read_consistent(
                WorkPriority::VisibleRead,
                |revision| self.capture_revision(revision),
                |connection, snapshot| {
                    let media = crate::query::MediaMatches::resolve(connection, &snapshot, scope)?;
                    run(&snapshot, &media)
                },
            );
        }
        run(
            &self.projections.snapshot(),
            &crate::query::MediaMatches::default(),
        )
    }

    pub fn query_statistics(&self) -> crate::query::QueryStatistics {
        self.match_cache.statistics()
    }

    /// Retain the open database/projections but discard all browsing result
    /// caches. Only benchmark binaries expose this operation.
    #[cfg(feature = "benchmark-internals")]
    pub fn reset_browsing_caches_for_benchmark(&mut self) {
        self.match_cache = Arc::new(crate::query::MatchCache::default());
        self.order_cache = Arc::new(crate::query::OrderCache::default());
    }

    pub fn query_window(
        &self,
        query: &RootQuery,
        window: &crate::query::WindowRequest,
    ) -> Result<crate::query::RootWindow> {
        self.query_window_requested(query, window, None)
    }

    pub fn supersede_window(&self, request: &crate::query_request::QueryRequest) -> Result<()> {
        self.window_requests.begin(request)?;
        Ok(())
    }

    pub fn query_window_requested(
        &self,
        query: &RootQuery,
        window: &crate::query::WindowRequest,
        request: Option<&crate::query_request::QueryRequest>,
    ) -> Result<crate::query::RootWindow> {
        self.prepare_window(query, window, request, Ok)
    }

    pub fn query_window_json(
        &self,
        query: &RootQuery,
        window: &crate::query::WindowRequest,
        request: Option<&crate::query_request::QueryRequest>,
    ) -> Result<String> {
        self.prepare_window(query, window, request, |result| {
            crate::query_request::serialize(&result)
        })
    }

    fn prepare_window<T>(
        &self,
        query: &RootQuery,
        window: &crate::query::WindowRequest,
        request: Option<&crate::query_request::QueryRequest>,
        finish: impl FnOnce(crate::query::RootWindow) -> Result<T>,
    ) -> Result<T> {
        let cancellation = request
            .map(|request| self.window_requests.begin(request))
            .transpose()?
            .unwrap_or_else(crate::query_request::Cancellation::deadline);
        cancellation.check()?;
        let _execution = cancellation.enter();
        let result = self.scoped(&query.scope, |snapshot, media| {
            cancellation.check()?;
            let result = crate::query::window_cached(
                snapshot,
                query,
                window,
                media,
                &self.match_cache,
                &self.order_cache,
            );
            cancellation.check()?;
            result
        })?;
        finish(result)
    }

    pub fn details(&self, root_id: RootId) -> Result<crate::RootDetails> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| crate::query::details(connection, &snapshot, root_id),
        )
    }

    pub fn counts(&self) -> Result<LibraryCounts> {
        Ok(crate::query::counts(&self.projections.snapshot()))
    }

    pub fn sidebar_counts(&self) -> Result<crate::SidebarCounts> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let counts = crate::query::scope_counts(&snapshot, &self.smart_counts);
                let recently_viewed = crate::query::matching_roots(
                    &snapshot,
                    &crate::query::RootQuery {
                        scope: crate::query::ItemScope::RecentlyViewed,
                        view: Default::default(),
                    },
                    &crate::query::MediaMatches::default(),
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
                    // One pass over media and one over files, instead of six scans.
                    "WITH assets AS (
                         SELECT COUNT(*) AS total,
                                COALESCE(SUM(file.mime LIKE 'image/%'), 0) AS images,
                                COALESCE(SUM(file.mime LIKE 'video/%'), 0) AS videos,
                                COALESCE(SUM(file.mime LIKE 'audio/%'), 0) AS audio
                         FROM media_item media JOIN media_file file ON file.file_id = media.file_id
                     ), files AS (
                         SELECT COUNT(*) AS total, COALESCE(SUM(size_bytes), 0) AS bytes
                         FROM media_file
                     )
                     SELECT
                         assets.total, assets.images, assets.videos, assets.audio,
                         assets.total - assets.images - assets.videos - assets.audio,
                         files.total,
                         files.bytes,
                         (SELECT COUNT(*) FROM tag_definition),
                         (SELECT COUNT(*) FROM folder_definition),
                         (SELECT COUNT(*) FROM smart_folder_definition),
                         (SELECT COUNT(*) FROM subscription)
                     FROM assets, files",
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

    /// Query membership for only the supplied roots, without building a result order.
    pub fn root_membership(
        &self,
        query: &crate::query::RootQuery,
        roots: &[RootId],
    ) -> Result<Vec<bool>> {
        if roots.is_empty() {
            return Ok(Vec::new());
        }
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let candidates = roots.iter().map(|id| id.0).collect();
                let media = crate::query::MediaMatches::resolve(connection, &snapshot, &query.scope)?;
                let matches = crate::query::matching_roots_within(
                    &snapshot, query, &media, Some(&candidates),
                )?;
                Ok(roots.iter().map(|id| matches.contains(id.0)).collect())
            },
        )
    }

    /// Current positions of `roots` in `query`'s order (`None` when absent).
    pub fn root_positions(
        &self,
        query: &crate::query::RootQuery,
        roots: &[RootId],
    ) -> Result<Vec<Option<u64>>> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                crate::selection::root_positions(
                    connection,
                    &snapshot,
                    query,
                    roots,
                    (&self.match_cache, &self.order_cache),
                )
            },
        )
    }

    pub fn selection_summary(&self, target: &SelectionTarget) -> Result<SelectionSummary> {
        self.database.read_consistent(
            WorkPriority::VisibleRead,
            |revision| self.capture_revision(revision),
            |connection, snapshot| {
                let selection = crate::selection::resolve_with(connection, &snapshot, target, Some((&self.match_cache, &self.order_cache)))?;
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

    pub fn record_recent_view(
        &self,
        root_id: RootId,
        viewed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
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
        })?;
        Ok(receipt)
    }

    pub fn clear_recent_views(&self) -> Result<MutationReceipt> {
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
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
        })?;
        Ok(receipt)
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
                Arc::make_mut(&mut next.text_index).refresh(transaction, &settled)?;
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &settled,
                    &[crate::predicate::DependencyChange::RootText],
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
        // The checkpoint is a startup optimization; re-encoding an unchanged
        // projection every few minutes costs seconds of CPU for nothing.
        let stored = self.database.read(WorkPriority::Maintenance, |connection| {
            use rusqlite::OptionalExtension;
            connection
                .query_row(
                    "SELECT database_revision FROM projection_checkpoint WHERE singleton = 1",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .optional()
                .map_err(Into::into)
        })?;
        if stored == Some(revision as i64) {
            return Ok(0);
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
        self.update_media_facts_and_content(media_id, update, None, changed_at_ms)
    }

    pub fn repair_media_content(
        &self,
        media_id: MediaId,
        content: &crate::model::RepairedMediaContent,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        self.update_media_facts_and_content(
            media_id,
            &MediaFactsUpdate::default(),
            Some(content),
            changed_at_ms,
        )
    }

    fn update_media_facts_and_content(
        &self,
        media_id: MediaId,
        update: &MediaFactsUpdate,
        content: Option<&crate::model::RepairedMediaContent>,
        changed_at_ms: i64,
    ) -> Result<MutationReceipt> {
        if update == &MediaFactsUpdate::default() && content.is_none() {
            return Err(LibraryError::InvalidInput(
                "media facts update contains no changes".into(),
            ));
        }
        let update = update.clone();
        let receipt = self.publish(WorkPriority::Maintenance, |transaction, revision, snapshot| {
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
                            serde_json::from_str::<Vec<crate::model::LabColor>>(&row.get::<_, String>(7)?).unwrap_or_default(),
                        ))
                    },
                )
                .map_err(|error| match error {
                    rusqlite::Error::QueryReturnedNoRows => {
                        LibraryError::NotFound(format!("media {media_id}"))
                    }
                    error => error.into(),
                })?;
            let before = (
                mime.clone(),
                width,
                height,
                duration_ms,
                frame_count,
                palette.clone(),
            );
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
            if let Some(content) = content {
                if content.content_hash.len() != 64 || !content.content_hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                    return Err(LibraryError::InvalidInput("Invalid repaired content hash".into()));
                }
                let actual_hash: String = transaction.query_row(
                    "SELECT content_hash FROM media_file WHERE file_id = ?1", [file_id], |row| row.get(0),
                )?;
                if actual_hash != content.expected_hash {
                    return Err(LibraryError::InvalidState("Media changed during repair".into()));
                }
                // Remote bytes belong to the old hash. Never mark repaired bytes as uploaded.
                transaction.execute("DELETE FROM cloud_blob_state WHERE file_hash = ?1", [&actual_hash])?;
                transaction.execute(
                    "UPDATE media_file SET content_hash = ?2, file_path = ?3, size_bytes = ?4 WHERE file_id = ?1",
                    rusqlite::params![file_id, content.content_hash, content.file_path,
                        i64::try_from(content.size_bytes).map_err(|_| LibraryError::InvalidInput("File size exceeds SQLite range".into()))?],
                )?;
                transaction.execute(
                    "UPDATE work_item SET file_hash = ?2 WHERE file_id = ?1",
                    rusqlite::params![file_id, content.content_hash],
                )?;
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
            // Derivative work fills in one fact at a time. A perceptual
            // hash is read only by the duplicate scan, and a palette only
            // by the colour index; neither needs the MIME, image or size
            // projections rebuilt.
            let shape_changed = content.is_some()
                || (&before.0, before.1, before.2, before.3, before.4)
                    != (&mime, width, height, duration_ms, frame_count);
            let palette_changed = before.5 != palette;
            let mut next = (*snapshot).clone();
            let affected = {
                let media_ids = transaction
                    .prepare_cached("SELECT media_id FROM media_item WHERE file_id = ?1")?
                    .query_map([file_id], |row| row.get::<_, u32>(0))?
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                let mut roots = RoaringBitmap::new();
                for media_id in media_ids {
                    if shape_changed {
                        let image_media = Arc::make_mut(&mut next.image_media);
                        if mime.starts_with("image/") {
                            image_media.insert(media_id);
                        } else {
                            image_media.remove(media_id);
                        }
                    }
                    if let Some(owner) = snapshot.media_owner.get(media_id) {
                        roots.insert(owner.0);
                    }
                }
                roots
            };
            for root_id in affected.iter().filter(|_| shape_changed || palette_changed) {
                if !shape_changed {
                    crate::group::refresh_cover_projection(
                        transaction,
                        &mut next,
                        RootId(root_id),
                    )?;
                    continue;
                }
                if content.is_some() {
                    let members = next.collection_orders.get(&RootId(root_id))
                        .map(|members| members.iter().map(|member| member.0).collect::<Vec<_>>())
                        .unwrap_or_else(|| vec![root_id]);
                    let total: i64 = transaction.query_row(
                        "SELECT SUM(file.size_bytes) FROM media_item media JOIN media_file file ON file.file_id = media.file_id WHERE media.media_id IN (SELECT value FROM json_each(?1))",
                        [serde_json::to_string(&members)?], |row| row.get(0),
                    )?;
                    transaction.execute("UPDATE library_root SET total_size_bytes = ?2 WHERE root_id = ?1", rusqlite::params![root_id, total])?;
                    Arc::make_mut(&mut next.total_bytes).insert(root_id, total as u64);
                }
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
            if shape_changed || palette_changed {
                crate::smart::settle_affected_for(
                    transaction,
                    &mut next,
                    &affected,
                    if content.is_some() { &[crate::predicate::DependencyChange::All] }
                    else { &[crate::predicate::DependencyChange::CoverFacts] },
                )?;
            }
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
            let resources: Vec<String> = if shape_changed || palette_changed {
                vec!["media".into(), "roots".into(), "smart-folders".into()]
            } else {
                // Nothing a view shows changed; the duplicate scan that
                // reads the hash publishes its own result.
                Vec::new()
            };
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
        })?;
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
            "items.set_rating",
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
        let detail = match (rating, notes.is_some(), source_urls.is_some()) {
            (Some(rating), false, false) => Some(format!("rating:{}", rating.bitmap_key())),
            (None, true, false) => Some("notes".to_owned()),
            (None, false, true) => Some("source_urls".to_owned()),
            _ => None,
        };
        let target = target.clone();
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let selection = crate::selection::resolve_with(transaction, &snapshot, &target, Some((&self.match_cache, &self.order_cache)))?;
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
                    bitmap::replace_changed(transaction, revision, key, &after, &(&before ^ &after))?;
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
            let mut smart_changes = Vec::new();
            if rating.is_some() {
                smart_changes.push(crate::predicate::DependencyChange::Rating);
            }
            if notes.is_some() || source_urls.is_some() {
                smart_changes.push(crate::predicate::DependencyChange::RootText);
            }
            crate::smart::settle_affected_for(transaction, &mut next, &selection, &smart_changes)?;
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
            )
            .about(selection.len(), detail.as_deref());
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(history),
                },
            ))
        })?;
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
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
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
            Arc::make_mut(&mut next.name_index).refresh(transaction, &changed)?;
            crate::smart::settle_affected_for(
                transaction,
                &mut next,
                &changed,
                &[crate::predicate::DependencyChange::RootText],
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
            )
            .about(changed.len(), None);
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history: Some(history),
                },
            ))
        })?;
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
            "items.set_notes",
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
            "items.set_source_urls",
            "Change source URLs",
            TextMutation::SourceUrls(source_urls),
            modified_at_ms,
        )
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
        let receipt = self.publish(WorkPriority::ForegroundMutation, |transaction, revision, snapshot| {
            let selection = crate::selection::resolve_with(transaction, &snapshot, &target, Some((&self.match_cache, &self.order_cache)))?;
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
                &[crate::predicate::DependencyChange::RootText],
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
        })?;
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
        let receipt = self.publish(priority, |transaction, revision, snapshot| {
            let selection = crate::selection::resolve_with(transaction, &snapshot, &target, Some((&self.match_cache, &self.order_cache)))?;
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
                    bitmap::replace_changed(transaction, revision, key, &after, &(&before ^ &after))?;
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
                &[smart_change],
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
            let detail = match domain {
                BitmapDomain::Lifecycle => Lifecycle::ALL
                    .into_iter()
                    .find(|value| value.bitmap_key() == destination)
                    .map(|value| format!("{value:?}").to_lowercase()),
                BitmapDomain::Rating => Rating::ALL
                    .iter()
                    .position(|value| value.bitmap_key() == destination)
                    .map(|stars| stars.to_string()),
                BitmapDomain::Tag => None,
            };
            let history = (!changes.is_empty()).then(|| {
                HistoryEntry::for_command(command, label, SemanticChange::Compound(changes))
                    .about(selection.len(), detail.as_deref())
            });
            Ok((
                receipt.clone(),
                PublishedDelta {
                    snapshot: next,
                    receipt,
                    history,
                },
            ))
        })?;
        Ok(receipt)
    }

    /// Run one canonical write, publish its projection delta and, when the
    /// write is undoable, record its history entry.
    fn publish<T>(
        &self,
        priority: WorkPriority,
        operation: impl FnOnce(
            &rusqlite::Transaction<'_>,
            u64,
            Arc<ProjectionSnapshot>,
        ) -> Result<(T, PublishedDelta)>,
    ) -> Result<T> {
        let projections = self.projections.clone();
        let publication = self.publication.clone();
        let (output, _, history_entry) = self.database.published_write(
            priority,
            |revision| self.capture_revision(revision),
            |transaction, _, revision, snapshot| operation(transaction, revision, snapshot),
            move |_, delta| publish_delta(&projections, &publication, delta),
        )?;
        push_history(&self.history, history_entry);
        Ok(output)
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
    mut delta: PublishedDelta,
) -> Option<HistoryEntry> {
    // Only known auxiliary-only publications can preserve SQL-derived results.
    // Unknown resources and item changes conservatively invalidate them.
    if !delta.receipt.item_ids.is_empty()
        || delta.receipt.resources.iter().any(|resource| {
            !matches!(
                resource.as_str(),
                "settings" | "tasks" | "subscriptions" | "duplicates"
            )
        })
    {
        delta
            .snapshot
            .query_versions
            .invalidate_sql(delta.snapshot.revision);
    }
    // Announce smart folders only when a result actually changed, so edits
    // that cannot move any folder do not make every listener reload.
    if delta.receipt.resources.iter().any(|resource| resource == "smart_folders")
        && Arc::ptr_eq(
            &projections.snapshot().smart_results,
            &delta.snapshot.smart_results,
        )
        && Arc::ptr_eq(
            &projections.snapshot().smart_queries,
            &delta.snapshot.smart_queries,
        )
    {
        delta.receipt.resources.retain(|resource| resource != "smart_folders");
    }
    projections.publish(delta.snapshot);
    publication.register(&delta.receipt);
    delta.history
}

fn push_history(history: &SessionHistory, entry: Option<HistoryEntry>) {
    if let Some(entry) = entry {
        history.push(entry);
    }
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
