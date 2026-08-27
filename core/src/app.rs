//! Replacement application boundary.
//!
//! Commands call one application operation. Operations own their SQLite
//! transaction, projection settlement, revision, and invalidation receipt.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use ts_rs::TS;

use crate::blob_store::BlobStore;
use crate::projection_v2::{
    FolderOrderProjectionChange, GroupOrderProjectionChange, ItemProjectionChange,
    MembershipProjectionChange, ProjectionStore, RootProjectionChange, StructureProjectionDelta,
    TagGraphProjectionDelta, TagIdentityProjectionChange,
};
use crate::store::history::{
    HistoryDescriptor, HistoryDirection, HistoryEntrySummary, HistoryProjectionRequest,
    HistoryState, SemanticHistoryPayload, SemanticHistoryRecord,
};
use crate::store::Store;

pub const LIBRARY_CHANGED_EVENT: &str = "library/changed";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct ItemId(#[ts(type = "number")] pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct MediaId(#[ts(type = "number")] pub i64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(transparent)]
pub struct FileHash(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Media,
    Collection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Inbox,
    Active,
    Trash,
}

impl Lifecycle {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Active => "active",
            Self::Trash => "trash",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ItemScope {
    All,
    Inbox,
    Trash,
    RecentlyViewed,
    Untagged,
    Uncategorized,
    Folder {
        #[ts(type = "number")]
        folder_id: i64,
    },
    SmartFolder {
        #[ts(type = "number")]
        smart_folder_id: i64,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum FilterMatchMode {
    #[default]
    Any,
    All,
    Exact,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemFilters {
    #[serde(default)]
    pub include_tags: Vec<String>,
    #[serde(default)]
    pub exclude_tags: Vec<String>,
    #[serde(default)]
    pub tag_match_mode: FilterMatchMode,
    #[serde(default)]
    #[ts(type = "Array<number>")]
    pub include_folder_ids: Vec<i64>,
    #[serde(default)]
    #[ts(type = "Array<number>")]
    pub exclude_folder_ids: Vec<i64>,
    #[serde(default)]
    pub folder_match_mode: FilterMatchMode,
    #[serde(default)]
    #[ts(type = "Array<number>")]
    pub ratings: Vec<i64>,
    #[serde(default)]
    pub include_mime_types: Vec<String>,
    #[serde(default)]
    pub exclude_mime_types: Vec<String>,
    pub text: Option<String>,
    pub color_hex: Option<String>,
    pub imported_after: Option<String>,
    pub imported_before: Option<String>,
    pub modified_after: Option<String>,
    pub modified_before: Option<String>,
    pub min_duration_ms: Option<i64>,
    pub max_duration_ms: Option<i64>,
    pub min_size_bytes: Option<i64>,
    pub max_size_bytes: Option<i64>,
    pub min_width: Option<i64>,
    pub max_width: Option<i64>,
    pub min_height: Option<i64>,
    pub max_height: Option<i64>,
    pub notes_present: Option<bool>,
    pub notes_contains: Option<String>,
    pub source_url_present: Option<bool>,
    pub source_url_contains: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum ItemSortField {
    ImportedAt,
    CapturedAt,
    Name,
    Rating,
    Size,
    Random,
    FolderOrder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    Ascending,
    Descending,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemSort {
    pub field: ItemSortField,
    pub direction: SortDirection,
    pub random_seed: Option<String>,
}

impl Default for ItemSort {
    fn default() -> Self {
        Self {
            field: ItemSortField::ImportedAt,
            direction: SortDirection::Descending,
            random_seed: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct ItemQuery {
    pub scope: ItemScope,
    #[serde(default)]
    pub filters: ItemFilters,
    #[serde(default)]
    pub sort: ItemSort,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ItemTarget {
    Explicit {
        item_ids: Vec<ItemId>,
    },
    Query {
        query: ItemQuery,
        #[serde(default)]
        excluded_item_ids: Vec<ItemId>,
    },
    Range {
        query: ItemQuery,
        anchor_item_id: ItemId,
        focus_item_id: ItemId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct MutationReceipt {
    #[ts(type = "number")]
    pub revision: u64,
    pub resources: Vec<String>,
    pub item_ids: Vec<ItemId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryOperationResult {
    pub entry: HistoryEntrySummary,
    pub state: HistoryState,
    pub receipt: MutationReceipt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export_to = "../../src/shared/types/generated/application/")]
pub struct LibraryChanged {
    #[ts(type = "number")]
    pub revision: u64,
    pub resources: Vec<String>,
    pub item_ids: Vec<ItemId>,
}

impl From<&MutationReceipt> for LibraryChanged {
    fn from(receipt: &MutationReceipt) -> Self {
        Self {
            revision: receipt.revision,
            resources: receipt.resources.clone(),
            item_ids: receipt.item_ids.clone(),
        }
    }
}

pub struct Application {
    store: Arc<Store>,
    blobs: Arc<BlobStore>,
    projections: Arc<ProjectionStore>,
    publication: crate::publication::PublicationCoordinator,
    ai_sessions: crate::ai_tagger::inference::SharedTaggerSessions,
    ai_prediction_cache: crate::ai_tagger::inference::SharedPredictionCache,
    ai_model_downloads: tokio::sync::Mutex<HashMap<String, AiModelDownload>>,
    ai_model_lifecycle: tokio::sync::Mutex<()>,
    ai_worker_status: std::sync::Mutex<AiWorkerStatus>,
    ingest_execution: std::sync::Mutex<()>,
}

#[derive(Debug, Clone)]
pub(crate) struct AiWorkerStatus {
    pub active: bool,
    pub detail: String,
}

impl Default for AiWorkerStatus {
    fn default() -> Self {
        Self {
            active: false,
            detail: "Idle".into(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct AiModelDownload {
    pub cancel: CancellationToken,
    pub downloaded_bytes: Arc<std::sync::atomic::AtomicU64>,
    pub total_bytes: u64,
}

impl Application {
    pub fn new(store: Arc<Store>) -> Self {
        Self::try_new(store).expect("replacement projection initialization failed")
    }

    pub fn try_new(store: Arc<Store>) -> Result<Self, String> {
        let blobs = Arc::new(
            BlobStore::open(store.library_root())
                .map_err(|error| format!("Failed to open blob store: {error}"))?,
        );
        let projections = Arc::new(store.read_result(ProjectionStore::initialize)?);
        Ok(Self {
            store,
            blobs,
            projections,
            publication: crate::publication::PublicationCoordinator::new(),
            ai_sessions: crate::ai_tagger::inference::new_shared_sessions(),
            ai_prediction_cache: crate::ai_tagger::inference::new_prediction_cache(),
            ai_model_downloads: tokio::sync::Mutex::new(HashMap::new()),
            ai_model_lifecycle: tokio::sync::Mutex::new(()),
            ai_worker_status: std::sync::Mutex::new(AiWorkerStatus::default()),
            ingest_execution: std::sync::Mutex::new(()),
        })
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn projections(&self) -> &ProjectionStore {
        &self.projections
    }

    pub fn blobs(&self) -> &BlobStore {
        &self.blobs
    }

    pub(crate) fn lock_ingest_execution(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.ingest_execution
            .lock()
            .map_err(|_| "Ingest execution lock was poisoned".to_string())
    }

    pub(crate) fn ai_sessions(&self) -> &crate::ai_tagger::inference::SharedTaggerSessions {
        &self.ai_sessions
    }

    pub(crate) fn ai_prediction_cache(
        &self,
    ) -> &crate::ai_tagger::inference::SharedPredictionCache {
        &self.ai_prediction_cache
    }

    pub(crate) fn ai_model_downloads(
        &self,
    ) -> &tokio::sync::Mutex<HashMap<String, AiModelDownload>> {
        &self.ai_model_downloads
    }

    pub(crate) fn ai_model_lifecycle(&self) -> &tokio::sync::Mutex<()> {
        &self.ai_model_lifecycle
    }

    pub(crate) fn set_ai_worker_status(&self, active: bool, detail: impl Into<String>) {
        if let Ok(mut status) = self.ai_worker_status.lock() {
            status.active = active;
            status.detail = detail.into();
        }
    }

    pub(crate) fn ai_worker_status(&self) -> AiWorkerStatus {
        self.ai_worker_status
            .lock()
            .map(|status| status.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn cancel_ai_model_downloads(&self) {
        for download in self.ai_model_downloads.lock().await.values() {
            download.cancel.cancel();
        }
    }

    #[track_caller]
    pub fn transaction<T, D>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, D)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64), String> {
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.transaction_settled(
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    #[track_caller]
    pub(crate) fn transaction_captured<T, D, C>(
        &self,
        capture: impl FnOnce(&ProjectionStore) -> C,
        operation: impl FnOnce(&rusqlite::Transaction<'_>, C) -> rusqlite::Result<(T, D)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64), String> {
        let capture_projection = Arc::clone(&self.projections);
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.transaction_settled_captured(
            move || capture(&capture_projection),
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    #[track_caller]
    pub fn undoable_transaction<T, D>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, D)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>), String> {
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.undoable_transaction_settled(
            descriptor,
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    #[track_caller]
    pub fn undoable_transaction_if_changed<T, D>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.undoable_transaction_if_changed_settled(
            descriptor,
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    /// Broad operations use compact semantic inverses rather than SQLite
    /// Session changesets. Their normal forward projection delta is prepared
    /// by the operation and settled once after commit.
    #[track_caller]
    pub fn semantic_undoable_transaction_if_changed<T, D>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(
            &rusqlite::Transaction<'_>,
        ) -> rusqlite::Result<(T, D, Option<SemanticHistoryRecord>, bool)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.semantic_undoable_transaction_if_changed_settled(
            descriptor,
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    #[track_caller]
    pub(crate) fn semantic_undoable_transaction_if_changed_captured<T, D, C>(
        &self,
        descriptor: HistoryDescriptor,
        capture: impl FnOnce(&ProjectionStore) -> C,
        operation: impl FnOnce(
            &rusqlite::Transaction<'_>,
            C,
        ) -> rusqlite::Result<(T, D, Option<SemanticHistoryRecord>, bool)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>, bool), String> {
        let capture_projection = Arc::clone(&self.projections);
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store
            .semantic_undoable_transaction_if_changed_settled_captured(
                descriptor,
                move || capture(&capture_projection),
                operation,
                move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
                move |prepared| publish_projection.publish_prepared(prepared),
            )
    }

    #[track_caller]
    pub fn semantic_undoable_transaction<T, D>(
        &self,
        descriptor: HistoryDescriptor,
        operation: impl FnOnce(
            &rusqlite::Transaction<'_>,
        ) -> rusqlite::Result<(T, D, SemanticHistoryRecord)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, Option<HistoryEntrySummary>), String> {
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.semantic_undoable_transaction_settled(
            descriptor,
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    pub fn history_state(&self) -> Result<HistoryState, String> {
        self.store.history_state()
    }

    pub fn undo(&self) -> Result<HistoryOperationResult, String> {
        self.apply_history(HistoryDirection::Undo)
    }

    pub fn redo(&self) -> Result<HistoryOperationResult, String> {
        self.apply_history(HistoryDirection::Redo)
    }

    fn apply_history(&self, direction: HistoryDirection) -> Result<HistoryOperationResult, String> {
        let history_projection = Arc::clone(&self.projections);
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        let mutation = self.store.apply_history_prepared(
            direction,
            move |transaction, payload| {
                let Some((roots, active_after)) = payload.lifecycle_target() else {
                    return Ok(());
                };
                history_projection
                    .selection_snapshot()
                    .lifecycle_summary_delta_for_active(&roots, &active_after)?
                    .stage(transaction)
                    .map_err(|error| error.to_string())
            },
            move |request| {
                prepare_projection.prepare(|candidate| match request {
                    HistoryProjectionRequest::Changeset => Ok(()),
                    HistoryProjectionRequest::Semantic {
                        payload,
                        group_summaries,
                    } => {
                        apply_semantic_projection(candidate, payload)?;
                        if group_summaries.is_empty() {
                            Ok(())
                        } else {
                            // Re-created roots regain their numeric summary
                            // entries; the replay purged them on the way out.
                            candidate.apply_root_summary_changes(
                                &group_summaries,
                                &roaring::RoaringBitmap::new(),
                            )
                        }
                    }
                })
            },
            move |prepared| publish_projection.publish_prepared(prepared),
        )?;
        Ok(HistoryOperationResult {
            entry: mutation.entry,
            state: mutation.state,
            receipt: MutationReceipt {
                revision: mutation.revision,
                resources: mutation.resources,
                item_ids: mutation.item_ids.into_iter().map(ItemId).collect(),
            },
        })
    }

    #[track_caller]
    pub fn transaction_if_changed<T, D>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, bool), String> {
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.transaction_if_changed_settled(
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    /// Ingest is user-visible and exact, but direct manipulation must be able
    /// to overtake a queued ingest publication.
    #[track_caller]
    pub(crate) fn transaction_if_changed_maintenance<T, D>(
        &self,
        operation: impl FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<(T, D, bool)>,
        settle: impl FnOnce(&ProjectionStore, D) -> Result<(), String>,
    ) -> Result<(T, u64, bool), String> {
        let prepare_projection = Arc::clone(&self.projections);
        let publish_projection = Arc::clone(&self.projections);
        self.store.transaction_if_changed_settled_maintenance(
            operation,
            move |delta| prepare_projection.prepare(|candidate| settle(candidate, delta)),
            move |prepared| publish_projection.publish_prepared(prepared),
        )
    }

    pub fn publish(&self, receipt: &MutationReceipt) {
        self.publication.submit(receipt);
    }
}

fn apply_semantic_projection(
    projections: &ProjectionStore,
    payload: &SemanticHistoryPayload,
) -> Result<(), String> {
    match payload {
        SemanticHistoryPayload::Lifecycle(delta) => {
            projections.apply_lifecycle_bitmap(&delta.inbox, Lifecycle::Inbox)?;
            projections.apply_lifecycle_bitmap(&delta.active, Lifecycle::Active)?;
            projections.apply_lifecycle_bitmap(&delta.trash, Lifecycle::Trash)
        }
        SemanticHistoryPayload::Tags(changes) => {
            for change in changes {
                projections.apply_root_tag_bitmap(change.relation_id, &change.remove, false)?;
                projections.apply_root_tag_bitmap(change.relation_id, &change.add, true)?;
            }
            Ok(())
        }
        SemanticHistoryPayload::Folders(changes) => {
            for change in changes {
                projections.apply_folder_bitmap(change.relation_id, &change.remove, false)?;
                projections.apply_folder_bitmap(change.relation_id, &change.add, true)?;
            }
            Ok(())
        }
        SemanticHistoryPayload::FolderOrders(changes) => {
            projections.apply_structure_delta(StructureProjectionDelta {
                folder_orders: changes
                    .iter()
                    .map(|change| FolderOrderProjectionChange {
                        folder_id: change.folder_id,
                        item_ids: change.item_ids.clone(),
                        cleared: false,
                    })
                    .collect(),
                ..StructureProjectionDelta::default()
            })
        }
        SemanticHistoryPayload::Ratings(delta) => {
            projections.apply_rating_bitmap(&delta.unrated, None)?;
            for (rating, roots) in &delta.rated {
                let rating = u8::try_from(*rating)
                    .map_err(|_| format!("rating {rating} is outside the projection range"))?;
                projections.apply_rating_bitmap(roots, Some(rating))?;
            }
            Ok(())
        }
        SemanticHistoryPayload::TagGraph(delta) => {
            projections.apply_tag_graph_delta(TagGraphProjectionDelta {
                identities: delta
                    .removed_tag_ids
                    .iter()
                    .map(|tag_id| TagIdentityProjectionChange {
                        source_tag_id: *tag_id,
                        target_tag_id: None,
                        remove_tag: true,
                    })
                    .collect(),
            })?;
            for change in &delta.projection_tags {
                projections.apply_root_tag_bitmap(change.relation_id, &change.remove, false)?;
                projections.apply_root_tag_bitmap(change.relation_id, &change.add, true)?;
            }
            Ok(())
        }
        SemanticHistoryPayload::Group(delta) => {
            let mut structure = StructureProjectionDelta::default();
            for item_id in &delta.remove_item_ids {
                structure.items.push(ItemProjectionChange {
                    item_id: i64::from(item_id),
                    kind: ItemKind::Media,
                    present: false,
                });
            }
            for item_id in &delta.remove_root_ids {
                structure.roots.push(RootProjectionChange {
                    item_id: i64::from(item_id),
                    lifecycle: None,
                });
            }
            for root in &delta.roots {
                structure.items.push(ItemProjectionChange {
                    item_id: root.item_id,
                    kind: root.kind,
                    present: true,
                });
                structure.roots.push(RootProjectionChange {
                    item_id: root.item_id,
                    lifecycle: Some(root.lifecycle),
                });
            }
            for member in &delta.members {
                structure.memberships.push(MembershipProjectionChange {
                    collection_id: member.collection_id,
                    media_id: member.media_item_id,
                    present: member.present,
                });
            }
            let mut ordered = std::collections::BTreeMap::<i64, Vec<(i64, i64)>>::new();
            for member in delta.members.iter().filter(|member| member.present) {
                ordered
                    .entry(member.collection_id)
                    .or_default()
                    .push((member.position_rank, member.media_item_id));
            }
            structure.group_orders.extend(ordered.into_iter().map(
                |(collection_id, mut members)| {
                    members.sort_unstable();
                    GroupOrderProjectionChange {
                        collection_id,
                        media_ids: members
                            .into_iter()
                            .map(|(_, media_item_id)| media_item_id)
                            .collect(),
                    }
                },
            ));
            projections.apply_structure_delta(structure)?;
            apply_semantic_projection(
                projections,
                &SemanticHistoryPayload::Folders(delta.folder_changes.clone()),
            )?;
            apply_semantic_projection(
                projections,
                &SemanticHistoryPayload::Tags(delta.tag_changes.clone()),
            )?;
            apply_semantic_projection(
                projections,
                &SemanticHistoryPayload::Ratings(delta.rating_changes.clone()),
            )?;
            Ok(())
        }
        SemanticHistoryPayload::Composite(payloads) => {
            for payload in payloads {
                apply_semantic_projection(projections, payload)?;
            }
            Ok(())
        }
    }
}

pub mod resources {
    pub const LIBRARY: &str = "library";
    pub const SIDEBAR: &str = "sidebar";
    pub const RECENTLY_VIEWED: &str = "recently_viewed";
    pub const FOLDERS: &str = "folders";
    pub const SMART_FOLDERS: &str = "smart_folders";
    pub const TAGS: &str = "tags";
    pub const DUPLICATES: &str = "duplicates";
    pub const SUBSCRIPTIONS: &str = "subscriptions";
    pub const SETTINGS: &str = "settings";
    pub const TASKS: &str = "tasks";
    pub const CLOUD: &str = "cloud";

    pub fn item(item_id: i64) -> String {
        format!("item:{item_id}")
    }
}
