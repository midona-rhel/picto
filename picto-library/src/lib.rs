//! The media library: its database, its queries and its writes.
//!
//! This crate owns the media roots, their memberships in tags and folders,
//! the queries, the mutations, the publication of committed changes, and the
//! session's undo history. It does not depend on the application shell.

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
    merged_notes, translate_view, Library, MergeTaskInput, SharedFileMerge, MERGE_BACKUP,
    MERGE_LIBRARY,
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
