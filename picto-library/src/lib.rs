//! Greenfield media-library kernel.
//!
//! This crate owns media roots, canonical memberships, queries, mutations,
//! publication, and session history. It deliberately has no dependency on the
//! application shell or the backend it replaces.

pub mod bitmap;
mod browsing;
pub mod checkpoint;
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
pub use mutation::{Library, SharedFileMerge};
pub use projection::{ProjectionSnapshot, ProjectionStore};
pub use publication::{LibraryChanged, MutationReceipt};
pub use query_request::{LoadStage, OpenCancellation, OpenStage};
pub use smart::SmartFolderRecord;
