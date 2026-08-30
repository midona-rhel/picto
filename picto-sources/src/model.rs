use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SourcePartition(pub String);

impl SourcePartition {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Clone, Default)]
pub struct RequestCredentials {
    pub headers: BTreeMap<String, String>,
    pub cookies: BTreeMap<String, String>,
    pub username: Option<String>,
    pub api_key: Option<String>,
    pub oauth_token: Option<String>,
    pub oauth_token_secret: Option<String>,
    /// Exact hosts or parent domains allowed to receive this authentication.
    /// Empty credentials may leave this empty; non-empty credentials may not.
    pub allowed_domains: BTreeSet<String>,
}

impl std::fmt::Debug for RequestCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RequestCredentials")
            .field("header_count", &self.headers.len())
            .field("cookie_count", &self.cookies.len())
            .field("allowed_domain_count", &self.allowed_domains.len())
            .finish()
    }
}

impl RequestCredentials {
    pub fn is_empty(&self) -> bool {
        self.headers.is_empty()
            && self.cookies.is_empty()
            && self.username.is_none()
            && self.api_key.is_none()
            && self.oauth_token.is_none()
            && self.oauth_token_secret.is_none()
    }

    pub fn permits(&self, host: &str) -> bool {
        self.is_empty()
            || self.allowed_domains.iter().any(|domain| {
                host == domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryRequest {
    pub query: String,
    pub partition: SourcePartition,
    pub cursor: Option<String>,
    pub page_size: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalTag {
    pub namespace: String,
    pub value: String,
}

impl CanonicalTag {
    pub fn new(namespace: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaDescriptor {
    pub stable_id: String,
    pub position: u32,
    pub url: String,
    pub canonical_url: Option<String>,
    pub file_name: Option<String>,
    pub mime_hint: Option<String>,
    pub expected_size: Option<u64>,
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaDelivery {
    Direct,
    Dash,
    Hls,
}

impl MediaDescriptor {
    /// Providers declare segmented delivery with the standard manifest MIME
    /// type. The URL suffix is a fallback for APIs that omit a content type.
    pub fn delivery(&self) -> MediaDelivery {
        match self.mime_hint.as_deref().map(str::to_ascii_lowercase) {
            Some(mime) if mime == "application/dash+xml" => MediaDelivery::Dash,
            Some(mime)
                if matches!(
                    mime.as_str(),
                    "application/vnd.apple.mpegurl"
                        | "application/x-mpegurl"
                        | "audio/mpegurl"
                        | "audio/x-mpegurl"
                ) =>
            {
                MediaDelivery::Hls
            }
            _ => delivery_from_url(&self.url),
        }
    }
}

fn delivery_from_url(value: &str) -> MediaDelivery {
    let path = url::Url::parse(value)
        .ok()
        .map(|url| url.path().to_ascii_lowercase())
        .unwrap_or_else(|| {
            value
                .split(['?', '#'])
                .next()
                .unwrap_or(value)
                .to_ascii_lowercase()
        });
    if path.ends_with(".mpd") {
        MediaDelivery::Dash
    } else if path.ends_with(".m3u8") {
        MediaDelivery::Hls
    } else {
        MediaDelivery::Direct
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePost {
    pub site_id: String,
    pub partition: SourcePartition,
    pub stable_id: String,
    pub canonical_url: Option<String>,
    pub creator: Option<String>,
    pub name: Option<String>,
    pub notes: Option<String>,
    pub created_at: Option<String>,
    pub tags: Vec<CanonicalTag>,
    pub media: Vec<MediaDescriptor>,
    /// Exact provider cursor to persist only after this post settles.
    pub resume_cursor_after: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryBatch {
    pub posts: Vec<SourcePost>,
    pub exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedMedia {
    pub descriptor: MediaDescriptor,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkipReason {
    NoUsableMedia,
    ExactDuplicate,
    AlreadyImported,
    UnsupportedMedia,
    SourceUnavailable,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SourcePostOutcome {
    Added { root_ids: Vec<u32> },
    Skipped { reason: SkipReason },
    Failed { reason: String, retryable: bool },
}

impl SourcePostOutcome {
    pub fn consumes_added_budget(&self) -> bool {
        matches!(self, Self::Added { .. })
    }
}
