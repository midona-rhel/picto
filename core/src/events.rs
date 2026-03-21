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
use crate::runtime_contract::mutation_builder::MutationImpact;

/// Emit a `runtime/mutation_committed` event with a `MutationReceipt`.
///
/// Builds `MutationFacts` from the impact and emits the receipt.
/// The frontend derives stale resources from `facts` directly.
pub fn emit_mutation(origin: &str, impact: MutationImpact) {
    use crate::runtime_contract::mutation::{MutationFacts, MutationReceipt};

    let seq = crate::runtime_state::next_seq();
    let ts = chrono::Utc::now().to_rfc3339();

    let facts = MutationFacts {
        domains: impact.domains,
        file_hashes: impact.file_hashes,
        folder_ids: impact.folder_ids,
        smart_folder_ids: impact.smart_folder_ids,
        compiler_batch_done: impact.compiler_batch_done,
        status_changed: impact.status_changed,
        tags_changed: impact.tags_changed,
        tag_structure_changed: impact.tag_structure_changed,
        folder_membership_changed: impact.folder_membership_changed,
        view_prefs_changed: impact.view_prefs_changed,
        extra_grid_scopes: impact.extra_grid_scopes,
    };

    let receipt = MutationReceipt {
        seq,
        ts,
        origin_command: origin.to_string(),
        facts,
        sidebar_counts: impact.sidebar_counts,
    };

    emit(event_names::RUNTIME_MUTATION_COMMITTED, &receipt);
}

pub mod event_names {
    // --- Runtime contract (authoritative) ---
    pub const RUNTIME_MUTATION_COMMITTED: &str = "runtime/mutation_committed";
    pub const RUNTIME_TASK_UPSERTED: &str = "runtime/task_upserted";
    pub const RUNTIME_TASK_REMOVED: &str = "runtime/task_removed";

    // --- Non-task events ---
    pub const LIBRARY_CLOSED: &str = "library-closed";
    pub const ZOOM_FACTOR_CHANGED: &str = "zoom-factor-changed";
    pub const FILE_IMPORTED: &str = "file-imported";
    pub const MANUAL_IMPORT_PROGRESS: &str = "manual-import-progress";
    pub const MEDIA_EXPORT_PROGRESS: &str = "media-export-progress";
    pub const OPEN_DETAIL_WINDOW: &str = "open-detail-window";
    pub const DUPLICATE_AUTO_MERGE_FINISHED: &str = "duplicate-auto-merge-finished";
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct ManualImportProgressEvent {
    pub done: usize,
    pub total: usize,
    pub current_file: String,
    pub imported: usize,
    pub skipped: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaExportProgressEvent {
    pub done: usize,
    pub total: usize,
    pub current_file: String,
    pub exported: usize,
    pub skipped: usize,
    pub errors: usize,
}
