//! Event emission abstraction.
//!
//! Uses a global callback that can be wired to any host runtime transport
//! (Electron IPC, napi-rs addon, etc.).
//!
//! The callback is stored behind an `Arc` so that `emit_event` can clone a
//! reference, drop the lock, and invoke the callback without holding the mutex.
//! This prevents deadlocks if a callback triggers a nested emit or if a slow
//! handler blocks new registrations.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

type EventCallback = Arc<dyn Fn(&str, &str) + Send + Sync>;
static EVENT_CB: OnceLock<Mutex<EventCallback>> = OnceLock::new();

/// Controls whether log events are forwarded to the frontend.
/// Enabled once the frontend signals it wants logs (avoids spam during startup).
static LOG_FORWARDING_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn enable_log_forwarding() {
    LOG_FORWARDING_ENABLED.store(true, Ordering::SeqCst);
}

pub fn disable_log_forwarding() {
    LOG_FORWARDING_ENABLED.store(false, Ordering::SeqCst);
}

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
use crate::runtime_contract::change_builder::ChangeImpact;

/// Emit a `runtime/state_changed` event with a `StateChangedEvent`.
///
/// Builds `StateChanges` from the impact and emits the event.
/// The frontend derives stale resources from `changes` directly.
pub fn emit_state_changed(origin: &str, impact: ChangeImpact) {
    use crate::runtime_contract::state_change::{StateChangedEvent, StateChanges};

    let seq = crate::runtime_state::next_seq();
    let ts = chrono::Utc::now().to_rfc3339();

    let changes = StateChanges {
        domains: impact.domains,
        entity_hashes: impact.entity_hashes,
        member_hashes: impact.member_hashes,
        folder_ids: impact.folder_ids,
        smart_folder_ids: impact.smart_folder_ids,
        compiler_batch_done: impact.compiler_batch_done,
        status_changed: impact.status_changed,
        tags_changed: impact.tags_changed,
        tag_changes: impact.tag_changes,
        tag_structure_changed: impact.tag_structure_changed,
        folder_membership_changed: impact.folder_membership_changed,
        view_prefs_changed: impact.view_prefs_changed,
        media_metadata_changed: impact.media_metadata_changed,
        media_fields_changed: impact.media_fields_changed,
        media_derivatives_changed: impact.media_derivatives_changed,
        derivative_fields_changed: impact.derivative_fields_changed,
        extra_grid_scopes: impact.extra_grid_scopes,
        group_ids: impact.group_ids,
        subscription_ids: impact.subscription_ids,
        query_ids: impact.query_ids,
        credential_categories: impact.credential_categories,
        folder_parent_changes: impact.folder_parent_changes,
        folder_order_changes: impact.folder_order_changes,
        smart_folder_parent_changes: impact.smart_folder_parent_changes,
        smart_folder_order_changes: impact.smart_folder_order_changes,
        sidebar_node_patches: impact.sidebar_node_patches,
        smart_folder_counts: impact.smart_folder_counts,
        grid_reorder: impact.grid_reorder,
    };

    let event = StateChangedEvent {
        seq,
        ts,
        origin: origin.to_string(),
        changes,
        sidebar_counts: impact.sidebar_counts,
    };

    emit(event_names::RUNTIME_STATE_CHANGED, &event);
}

pub mod event_names {
    // --- Runtime contract (authoritative) ---
    pub const RUNTIME_STATE_CHANGED: &str = "runtime/state_changed";
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
    pub const LOG: &str = "log";
}

// ── Tracing layer that forwards log events to the frontend ──────────────

use tracing_subscriber::Layer;

/// A tracing Layer that emits log records as `"log"` events to the frontend.
pub struct EventEmitLayer;

impl<S: tracing::Subscriber> Layer<S> for EventEmitLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if !LOG_FORWARDING_ENABLED.load(Ordering::Relaxed) {
            return;
        }

        let meta = event.metadata();
        let level = meta.level().as_str();
        let target = meta.target();

        // Extract the message field from the event
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let payload = serde_json::json!({
            "level": level,
            "target": target,
            "message": visitor.message,
            "timestamp": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        });

        if let Ok(json) = serde_json::to_string(&payload) {
            emit_event(event_names::LOG, &json);
        }
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // Strip surrounding quotes from Debug formatting
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message
                .push_str(&format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            if !self.message.is_empty() {
                self.message.push(' ');
            }
            self.message
                .push_str(&format!("{}={}", field.name(), value));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        if !self.message.is_empty() {
            self.message.push(' ');
        }
        self.message
            .push_str(&format!("{}={}", field.name(), value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if !self.message.is_empty() {
            self.message.push(' ');
        }
        self.message
            .push_str(&format!("{}={}", field.name(), value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        if !self.message.is_empty() {
            self.message.push(' ');
        }
        self.message
            .push_str(&format!("{}={}", field.name(), value));
    }
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
