//! Event emission abstraction.
//!
//! Uses a global callback that can be wired to any host runtime transport
//! (Electron IPC, napi-rs addon, etc.).
//!
//! The callback is stored behind an `Arc` so that `emit_event` can clone a
//! reference, drop the lock, and invoke the callback without holding the mutex.
//! This prevents deadlocks if a callback triggers a nested emit or if a slow
//! handler blocks new registrations.

use std::sync::{Arc, Mutex, OnceLock};

type EventCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;
static EVENT_CB: OnceLock<Mutex<EventCallback>> = OnceLock::new();

/// Register the global event callback. Called once at initialization by the
/// host runtime (e.g. napi-rs addon).
pub fn set_event_callback(cb: impl Fn(&str, &str) + Send + Sync + 'static) {
    let slot = EVENT_CB.get_or_init(|| Mutex::new(Arc::new(|_, _| {})));
    *crate::poison::mutex_or_recover(slot, "events::set_callback") = Arc::new(cb);
}

/// Emit an event to the frontend.
///
/// `name` is the event name (e.g. "gallery-refresh").
/// `payload_json` is a JSON string payload (e.g. `"{}"` or `"null"`).
///
/// The callback is cloned out from under the lock before invocation so that
/// slow callbacks do not block other emitters or registration/unregistration.
pub fn emit_event(name: &str, payload_json: &str) {
    let cb = EVENT_CB
        .get()
        .and_then(|slot| slot.lock().ok().map(|guard| Arc::clone(&guard)));
    if let Some(cb) = cb {
        cb(name, payload_json);
    }
}

/// Convenience: emit an event with a serializable payload.
pub fn emit<T: serde::Serialize>(name: &str, payload: &T) {
    if let Ok(json) = serde_json::to_string(payload) {
        emit_event(name, &json);
    }
}

/// Convenience: emit an event with no payload.
pub fn emit_empty(name: &str) {
    emit_event(name, "null");
}

// SEQ counter lives in runtime_state — single source of truth.

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Files,
    Folders,
    SmartFolders,
    Tags,
    Sidebar,
    Selection,
    ViewPrefs,
    Ptr,
    Subscriptions,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct Invalidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidebar_tree: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selection_summary: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_hashes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub view_prefs: Option<bool>,
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct SidebarCounts {
    pub all_images: i64,
    pub inbox: i64,
    pub trash: i64,
}

#[derive(Debug, Clone, Default)]
pub struct MutationImpact {
    pub domains: Vec<Domain>,
    pub file_hashes: Option<Vec<String>>,
    pub folder_ids: Option<Vec<i64>>,
    pub smart_folder_ids: Option<Vec<i64>>,
    pub invalidate: Invalidate,
    pub compiler_batch_done: Option<bool>,
    pub sidebar_counts: Option<SidebarCounts>,
}

impl MutationImpact {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn domains(mut self, d: &[Domain]) -> Self {
        self.domains = d.to_vec();
        self
    }
    pub fn file_hashes(mut self, h: Vec<String>) -> Self {
        self.file_hashes = Some(h);
        self
    }
    pub fn folder_ids(mut self, ids: Vec<i64>) -> Self {
        self.folder_ids = Some(ids);
        self
    }
    pub fn smart_folder_ids(mut self, ids: Vec<i64>) -> Self {
        self.smart_folder_ids = Some(ids);
        self
    }
    pub fn sidebar_tree(mut self) -> Self {
        self.invalidate.sidebar_tree = Some(true);
        self
    }
    pub fn grid_scopes(mut self, s: Vec<String>) -> Self {
        self.invalidate.grid_scopes = Some(s);
        self
    }
    pub fn grid_all(mut self) -> Self {
        self.invalidate.grid_scopes = Some(vec!["system:all".into()]);
        self
    }
    pub fn selection_summary(mut self) -> Self {
        self.invalidate.selection_summary = Some(true);
        self
    }
    pub fn metadata_hashes(mut self, h: Vec<String>) -> Self {
        self.invalidate.metadata_hashes = Some(h);
        self
    }
    pub fn view_prefs(mut self) -> Self {
        self.invalidate.view_prefs = Some(true);
        self
    }
    pub fn sidebar_counts(mut self, c: SidebarCounts) -> Self {
        self.sidebar_counts = Some(c);
        self
    }
    pub fn sidebar_counts_from(mut self, db: &crate::sqlite::SqliteDatabase) -> Self {
        self.sidebar_counts = Some(sidebar_counts_from_bitmaps(db));
        self
    }

    /// File lifecycle (import, status change, delete). Invalidates sidebar, grid, selection, counts.
    pub fn file_lifecycle(db: &crate::sqlite::SqliteDatabase) -> Self {
        Self::new()
            .domains(&[Domain::Files, Domain::Sidebar, Domain::SmartFolders])
            .sidebar_tree()
            .grid_all()
            .selection_summary()
            .sidebar_counts_from(db)
    }

    /// File metadata change (rating, name, notes, urls). Only invalidates detail view.
    pub fn file_metadata(hash: String) -> Self {
        Self::new()
            .domains(&[Domain::Files])
            .metadata_hashes(vec![hash.clone()])
            .file_hashes(vec![hash])
    }

    /// Tag change on specific files. Invalidates file metadata display.
    pub fn file_tags(hash: String) -> Self {
        Self::new()
            .domains(&[Domain::Tags, Domain::Files])
            .metadata_hashes(vec![hash.clone()])
            .file_hashes(vec![hash])
    }

    /// Batch tag/selection change. Invalidates grid and selection summary.
    pub fn batch_tags() -> Self {
        Self::new()
            .domains(&[Domain::Tags, Domain::Files])
            .grid_all()
            .selection_summary()
    }

    /// Sidebar structure change (folder/smart folder/subscription CRUD).
    pub fn sidebar(domain: Domain) -> Self {
        Self::new()
            .domains(&[domain, Domain::Sidebar])
            .sidebar_tree()
    }
}

/// Emit a `runtime/mutation_committed` event with a `MutationReceipt`.
///
/// This is the single mutation event the frontend subscribes to.
/// It replaces the legacy `state-changed` / `sidebar-invalidated` /
/// `grid-snapshot-invalidated` events.
pub fn emit_mutation(origin: &str, impact: MutationImpact) {
    use crate::runtime_contract::mutation::{
        DerivedInvalidation, MutationFacts, MutationReceipt,
        SidebarCounts as ContractSidebarCounts,
    };

    let seq = crate::runtime_state::next_seq();
    let ts = chrono::Utc::now().to_rfc3339();

    let receipt = MutationReceipt {
        seq,
        ts,
        origin_command: origin.to_string(),
        facts: MutationFacts {
            domains: impact.domains,
            file_hashes: impact.file_hashes,
            folder_ids: impact.folder_ids,
            smart_folder_ids: impact.smart_folder_ids,
            compiler_batch_done: impact.compiler_batch_done,
        },
        invalidate: DerivedInvalidation {
            sidebar_tree: impact.invalidate.sidebar_tree,
            grid_scopes: impact.invalidate.grid_scopes,
            selection_summary: impact.invalidate.selection_summary,
            metadata_hashes: impact.invalidate.metadata_hashes,
            view_prefs: impact.invalidate.view_prefs,
        },
        sidebar_counts: impact.sidebar_counts.map(|c| ContractSidebarCounts {
            all_images: c.all_images,
            inbox: c.inbox,
            trash: c.trash,
        }),
    };

    emit(event_names::RUNTIME_MUTATION_COMMITTED, &receipt);
}

/// Compute system sidebar counts from bitmaps (O(1)).
/// Call AFTER inline bitmap updates so values reflect the mutation.
///
/// `all_images` = active only (Status 1). Inbox and trash are separate.
pub fn sidebar_counts_from_bitmaps(db: &crate::sqlite::SqliteDatabase) -> SidebarCounts {
    use crate::sqlite::bitmaps::BitmapKey;
    SidebarCounts {
        all_images: db.bitmaps.len(&BitmapKey::Status(1)) as i64,
        inbox: db.bitmaps.len(&BitmapKey::Status(0)) as i64,
        trash: db.bitmaps.len(&BitmapKey::Status(2)) as i64,
    }
}

pub mod event_names {
    pub const SUBSCRIPTION_STARTED: &str = "subscription-started";
    pub const SUBSCRIPTION_PROGRESS: &str = "subscription-progress";
    pub const SUBSCRIPTION_FINISHED: &str = "subscription-finished";

    pub const FLOW_STARTED: &str = "flow-started";
    pub const FLOW_PROGRESS: &str = "flow-progress";
    pub const FLOW_FINISHED: &str = "flow-finished";

    pub const PTR_SYNC_STARTED: &str = "ptr-sync-started";
    pub const PTR_SYNC_PROGRESS: &str = "ptr-sync-progress";
    pub const PTR_SYNC_FINISHED: &str = "ptr-sync-finished";
    pub const PTR_SYNC_PHASE_CHANGED: &str = "ptr-sync-phase-changed";

    pub const PTR_BOOTSTRAP_STARTED: &str = "ptr-bootstrap-started";
    pub const PTR_BOOTSTRAP_PROGRESS: &str = "ptr-bootstrap-progress";
    pub const PTR_BOOTSTRAP_FINISHED: &str = "ptr-bootstrap-finished";
    pub const PTR_BOOTSTRAP_FAILED: &str = "ptr-bootstrap-failed";

    pub const LIBRARY_CLOSED: &str = "library-closed";
    pub const ZOOM_FACTOR_CHANGED: &str = "zoom-factor-changed";

    pub const FILE_IMPORTED: &str = "file-imported";
    pub const OPEN_DETAIL_WINDOW: &str = "open-detail-window";

    pub const DUPLICATE_AUTO_MERGE_FINISHED: &str = "duplicate-auto-merge-finished";

    // --- Runtime contract (new)
    pub const RUNTIME_MUTATION_COMMITTED: &str = "runtime/mutation_committed";
    pub const RUNTIME_TASK_UPSERTED: &str = "runtime/task_upserted";
    pub const RUNTIME_TASK_REMOVED: &str = "runtime/task_removed";
}

// --- Subscription lifecycle

#[derive(Debug, Clone, serde::Serialize)]
pub struct SubscriptionStartedEvent {
    pub subscription_id: String,
    pub subscription_name: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_name: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SubscriptionFinishedEvent {
    pub subscription_id: String,
    pub subscription_name: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_name: Option<String>,
    pub status: String,
    pub files_downloaded: usize,
    pub files_skipped: usize,
    pub errors_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_kind: Option<String>,
    pub metadata_validated: usize,
    pub metadata_invalid: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_metadata_error: Option<String>,
}

// --- Flow lifecycle

#[derive(Debug, Clone, serde::Serialize)]
pub struct FlowStartedEvent {
    pub flow_id: String,
    pub subscription_count: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FlowProgressEvent {
    pub flow_id: String,
    pub total: u32,
    pub done: usize,
    pub remaining: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct FlowFinishedEvent {
    pub flow_id: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// --- PTR sync lifecycle

#[derive(Debug, Clone, serde::Serialize)]
pub struct PtrSyncFinishedEvent {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updates_processed: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags_added: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_rebuild: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_rebuild: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed_hashes_truncated: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PtrSyncPhaseChangedEvent {
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_update_index: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

// --- PTR bootstrap lifecycle

#[derive(Debug, Clone, serde::Serialize)]
pub struct PtrBootstrapStartedEvent {
    pub snapshot_dir: String,
    pub service_id: i64,
    pub mode: String,
}

/// Superset struct for ptr-bootstrap-progress. Different call sites populate
/// different subsets of fields; all optional fields are skipped when None.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct PtrBootstrapProgressEvent {
    pub phase: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_done: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_total: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_done_stage: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_total_stage: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows_per_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_seconds: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PtrBootstrapFinishedEvent {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_index: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta_sync_started: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub counts: Option<serde_json::Value>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PtrBootstrapFailedEvent {
    pub success: bool,
    pub error: String,
}

// --- System / misc

#[derive(Debug, Clone, serde::Serialize)]
pub struct ZoomFactorChangedEvent {
    pub factor: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenDetailWindowEvent {
    pub hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DuplicateAutoMergeFinishedEvent {
    pub winner_hash: String,
    pub loser_hash: String,
    pub distance: u32,
    pub tags_merged: usize,
}
