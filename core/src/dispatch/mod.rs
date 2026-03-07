//! Command dispatcher — routes command names to typed domain handlers.
//!
//! The napi-rs addon calls `dispatch("command_name", "{...args}")` and
//! gets back a JSON string result.
//!
//! All commands are handled through typed dispatch (`typed::typed_dispatch`),
//! except two pre-state commands (`close_library`, `get_runtime_snapshot`)
//! which are handled inline before state acquisition.

pub mod common;
pub mod typed;

pub use common::{ok_null, to_json};

/// Dispatch a command by name with JSON arguments. Returns JSON result.
pub async fn dispatch(command: &str, args_json: &str) -> Result<String, String> {
    let args: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("Invalid JSON args: {}", e))?;

    // ─── Library lifecycle (no state needed) ──────────────────
    if command == "close_library" {
        crate::state::close_library().await?;
        crate::events::emit_empty(crate::events::event_names::LIBRARY_CLOSED);
        return ok_null();
    }

    // ─── Runtime commands (stateless — don't need AppState) ──
    if command == "get_runtime_snapshot" {
        let snapshot = crate::runtime_state::get_runtime_snapshot();
        return to_json(&snapshot);
    }

    let state = crate::state::get_state()?;

    // ─── Typed command dispatch (PBI-234) ────────────────────
    if let Some(result) = typed::typed_dispatch(&state, command, &args).await {
        return result;
    }

    Err(format!("Unknown command: {}", command))
}
