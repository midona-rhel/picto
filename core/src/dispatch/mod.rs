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
mod files_review;
pub mod folders;
pub mod grid;
pub mod ptr;
pub mod selection;
pub mod smart_folders;
pub mod subscriptions;
pub mod system;
pub mod tags;

pub use common::{
    de, de_opt, de_opt_strict, get_field, ok_null, snake_to_camel, to_json, value_type_name,
};

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

    let state = crate::state::get_state()?;

    // ─── Domain handler routing ──────────────────────────────
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
