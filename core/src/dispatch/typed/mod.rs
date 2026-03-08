//! Domain command modules.
//!
//! Each module exports Input types (with ts-rs derives for frontend code generation)
//! and handler functions called from the flat dispatch match in `dispatch/mod.rs`.

pub mod duplicates;
pub mod folders;
pub mod grid;
pub mod media_io;
pub mod media_lifecycle;
pub mod media_metadata;
pub mod selection;
pub mod smart_folders;
pub mod subscriptions;
pub mod system;
pub mod tags;
