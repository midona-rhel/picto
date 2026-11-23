//! The media library: its SQLite database, the projections that are built
//! from it, the queries, the writes and the undo history of a session.
//!
//! The crate does not depend on the application core or on the app. Both
//! reach a library through [`Library`].

pub mod bitmap;
mod browsing;
pub mod checkpoint;
pub mod content;
pub mod database;
pub mod duplicate;
pub mod error;
pub mod fts;
mod group;
pub mod history;
pub mod ingest;
pub mod ingest_queue;
pub mod model;
pub mod mutation;
pub mod name_index;
mod name_quality;
pub mod ordering;
pub mod predicate;
pub mod projection;
pub mod publication;
pub mod query;
mod query_dependencies;
pub mod query_request;
pub mod schema;
pub mod selection;
pub mod smart;
pub mod subscription_counts;
pub mod text_index;
pub mod work_queue;

pub use database::LibraryDatabase;
pub use error::{LibraryError, Result};
pub use model::*;
pub use mutation::{
    AppliedMerge, EntryClock, EntryKind, EntryTime, FileRecord, FileView, Library,
    MergeTaskInput, SharedEntry, SharedFileMerge, WantedFile, MERGE_BACKUP, MERGE_LIBRARY, merged_notes, remove_item_tombstones,
    reset_published_entries, translate_view,
};

/// Returns the name that is kept when two items become one: `incoming` when
/// it is a more descriptive kind of name than `existing`, and `existing`
/// otherwise.
pub fn preferred_name<'a>(existing: &'a str, incoming: &'a str) -> &'a str {
    name_quality::preferred(existing, incoming)
}
pub use projection::{ProjectionSnapshot, ProjectionStore};
pub use publication::{LibraryChanged, MutationReceipt};
pub use query_request::{LoadStage, OpenCancellation, OpenStage};
pub use smart::SmartFolderRecord;
