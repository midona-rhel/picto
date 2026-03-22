//! Runtime contract types — the structured communication protocol between
//! backend and frontend.
//!
//! All types here derive `ts_rs::TS` for automatic TypeScript generation.
//! They define state changes (what changed), snapshots (full state hydration),
//! and tasks (background operation progress).

pub mod change_builder;
pub mod snapshot;
pub mod state_change;
pub mod task;
