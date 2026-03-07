//! Command dispatcher — routes command names to domain handler modules.
//!
//! Routes command names to domain handler modules.
//! The napi-rs addon calls `dispatch("command_name", "{...args}")` and
//! gets back a JSON string result.
//!
//! ## Module structure
//!
//! Domain-specific handlers live in sub-modules. Each module exposes an async
//! `handle()` that returns `Option<Result<String, String>>` — `Some` if the
//! command belongs to that domain, `None` to fall through. New commands should
//! be added to the appropriate domain module, not here.

pub mod common;
pub mod duplicates;
pub mod files;
mod files_lifecycle;
mod files_media;
mod files_metadata;
pub mod typed;
pub mod folders;
pub mod grid;
pub mod ptr;
pub mod selection;
pub mod smart_folders;
pub mod subscriptions;
pub mod system;
pub mod tags;

pub use common::{de, de_opt, get_field, ok_null, to_json};

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

    // ─── Legacy domain handler routing ───────────────────────
    if let Some(result) = files::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = grid::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = tags::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = folders::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = selection::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = ptr::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = duplicates::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = smart_folders::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = subscriptions::handle(&state, command, &args).await {
        return result;
    }
    if let Some(result) = system::handle(&state, command, &args).await {
        return result;
    }

    Err(format!("Unknown command: {}", command))
}
