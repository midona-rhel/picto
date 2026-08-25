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

pub mod event_names {
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
            "message": visitor.render(),
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
    fields: Vec<String>,
}

impl MessageVisitor {
    fn render(self) -> String {
        if self.fields.is_empty() {
            self.message
        } else if self.message.is_empty() {
            self.fields.join(" ")
        } else {
            format!("{} {}", self.message, self.fields.join(" "))
        }
    }
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
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields.push(format!("{}={}", field.name(), value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields.push(format!("{}={}", field.name(), value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields.push(format!("{}={}", field.name(), value));
    }
}
