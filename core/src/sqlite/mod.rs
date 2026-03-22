//! Core SQLite database module.
//!
//! All queries go through `SqliteDatabase` methods.
//! rusqlite is synchronous — all DB calls wrapped in `spawn_blocking`.

pub mod bitmaps;
pub mod compilers;
pub mod files;
pub mod hash_index;
pub mod projections;
pub mod publish;
pub mod read_model;
pub mod schema;

mod diagnostics;
mod open;
mod resolve;
#[cfg(test)]
mod tests;

use bitmaps::BitmapStore;
use hash_index::HashIndex;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub use read_model::{DerivedArtifact, PublishedArtifacts, ReadModelBatchResult, ReadModelEvent};
pub use resolve::EntityExpansionMode;

/// Main library database handle.
pub struct SqliteDatabase {
    conn: Arc<Mutex<Connection>>,
    /// Pool of read-only connections for concurrent SELECT queries.
    read_pool: Vec<Arc<Mutex<Connection>>>,
    /// Round-robin counter for read pool.
    read_pool_idx: AtomicUsize,
    pub bitmaps: Arc<BitmapStore>,
    pub hash_index: Arc<HashIndex>,
    pub manifest: Arc<publish::Manifest>,
    pub read_model_tx: mpsc::UnboundedSender<ReadModelEvent>,
    read_model_rx: Arc<Mutex<Option<mpsc::UnboundedReceiver<ReadModelEvent>>>>,
    db_path: PathBuf,
    /// When true, read model events are buffered and bitmap inserts skipped.
    pub(crate) events_held: AtomicBool,
    held_events: std::sync::Mutex<Vec<ReadModelEvent>>,
}
