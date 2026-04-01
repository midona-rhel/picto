//! Picto core library — pure business logic.
//!
//! This crate contains all database, import, tag, folder, subscription,
//! and media processing logic. It is consumed by the napi-rs addon (Electron).

// ── Database boundary ──────────────────────────────
pub mod db;

// ── Application engine ─────────────────────────────
pub mod engine;

// ── Domain modules ──────────────────────────────────
pub mod ai_tagger;
pub mod duplicates;
pub mod folders;
pub mod import;
pub mod ingest;
pub mod ingest_queue;
pub mod media_analysis;
pub mod media_capabilities;
pub mod media_delivery;
pub mod media_derivatives;
pub mod metadata;
pub mod scope;
pub mod selection;
pub mod settings;
pub mod sidebar;
pub mod smart_folders;
pub mod subscriptions;
pub mod tags;

// ── Cross-cutting infrastructure ────────────────────
pub mod background_work;
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
pub mod scheduler;
pub mod sqlite;
pub mod state;
pub mod types;
pub mod workers;
