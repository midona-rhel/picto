//! Picto core library — pure business logic.
//!
//! This crate contains all database, import, tag, folder, subscription,
//! and media processing logic. It is consumed by the napi-rs addon (Electron).

// ── Domain modules ──────────────────────────────────
pub mod duplicates;
pub mod folders;
pub mod grid;
pub mod import;
pub mod lifecycle;
pub mod metadata;
pub mod ptr;
pub mod selection;
pub mod settings;
pub mod sidebar;
pub mod smart_folders;
pub mod subscriptions;
pub mod tags;

// ── Cross-cutting infrastructure ────────────────────
pub mod blob_store;
pub mod constants;
pub mod credential_store;
pub mod dispatch;
pub mod events;
pub mod media_processing;
pub mod perf;
pub mod poison;
pub mod rate_limiter;
pub mod runtime_contract;
pub mod runtime_state;
pub mod sqlite;
pub mod state;
pub mod types;
