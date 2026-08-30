//! Native subscription extraction and download primitives.
//!
//! This crate knows source protocols but never library persistence, UI state,
//! credentials storage, or canonical ingest. Callers settle one returned post
//! before requesting the next one.

mod adapter;
mod cursor;
mod download;
mod engine;
mod error;
mod http;
mod json;
mod media;
mod model;
pub mod providers;
mod query;
mod segmented;
mod tags;
mod text;

pub use adapter::{
    AdapterFuture, MediaFuture, NativeSourceAdapter, PostFuture, ProviderDescriptor,
    ProviderRegistry,
};
pub use cursor::{BeforeIdCursor, OpaqueCursor, PageCursor};
pub use download::{
    DownloadProgress, DownloadStream, MediaDownloadFailure, PostDownload, PostDownloader,
};
pub use engine::{NextPost, PartitionedSourceSession, SourceSession};
pub use error::{SourceError, SourceErrorKind};
pub use http::{DomainPolicy, HttpPolicy, HttpRuntime};
pub use json::{JsonPageSource, JsonSourceAdapter};
pub use media::MediaDescriptorBuilder;
pub use model::{
    CanonicalTag, DiscoveryBatch, DiscoveryRequest, DownloadedMedia, MediaDelivery,
    MediaDescriptor, MediaFallback, OAuthTokenUpdater, RequestCredentials, SkipReason,
    SourcePartition, SourcePost, SourcePostOutcome,
};
pub use query::SearchQueryPolicy;
pub use segmented::{FfmpegMuxer, MediaMuxer, MuxFuture, MuxInput, MuxInputKind};
pub use tags::{CanonicalTagSet, NamespaceMap, RatingMap};
pub use text::normalize_source_text;
