//! Picto core library — pure business logic.
//!
//! This crate contains all database, import, tag, folder, subscription,
//! and media processing logic. It is consumed by the napi-rs addon (Electron).

pub mod blob_store;
pub mod constants;
pub mod credential_store;
pub mod dispatch;
pub mod duplicate_controller;
pub mod duplicates;
pub mod events;
pub mod media_processing;
pub mod flow_controller;
pub mod folder_controller;
pub mod gallery_dl_runner;
pub mod grid_controller;
pub mod import;
pub mod import_controller;
pub mod lifecycle_controller;
pub mod metadata_controller;
pub mod perf;
pub mod poison;
pub mod ptr_client;
pub mod ptr_controller;
pub mod ptr_sync;
pub mod ptr_types;
pub mod rate_limiter;
pub mod runtime_contract;
pub mod runtime_state;
pub mod selection_controller;
pub mod selection_helpers;
pub mod settings;
pub mod sidebar_controller;
pub mod smart_folder_controller;
pub mod sqlite;
pub mod sqlite_ptr;
pub mod state;
pub mod subscription_controller;
pub mod subscription_sync;
pub mod tag_controller;
pub mod tags;
pub mod types;
pub mod view_prefs_controller;
