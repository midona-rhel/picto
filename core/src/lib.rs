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
pub mod app;
pub mod duplicates;
pub mod duplicates_v2;
pub mod folders;
pub mod folders_v2;
pub mod gallery_dl_source_v2;
pub mod import;
pub mod ingest;
pub mod ingest_queue;
pub mod ingest_queue_v2;
pub mod ingest_v2;
pub mod media_analysis;
pub mod media_capabilities;
pub mod scope;
pub mod selection;
pub mod settings;
pub mod smart_folders;
pub mod smart_v2;
pub mod subscription_catalog_v2;
pub mod subscription_runtime_v2;
pub mod subscriptions;
pub mod subscriptions_v2;
pub mod tags;
pub mod tags_v2;

// ── Cross-cutting infrastructure ────────────────────
pub mod background_runtime_v2;
pub mod background_work;
pub mod blob_store;
pub mod constants;
pub mod credential_store;
pub mod dispatch;
pub mod events;
pub mod media_processing;
pub mod media_processing_v2;
pub mod operations_v2;
pub mod oplog;
pub mod perf;
pub mod poison;
pub mod projection_v2;
pub mod query_v2;
pub mod rate_limiter;
pub mod runtime_contract;
pub mod runtime_state;
pub mod runtime_v2;
pub mod scheduler;
pub mod state;
pub mod store;
pub mod types;
pub mod workers;
pub mod workers_v2;
