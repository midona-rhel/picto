#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceErrorKind {
    Authentication,
    AccessDenied,
    InvalidQuery,
    RateLimited,
    Network,
    InvalidResponse,
    Download,
    Cancelled,
}

#[derive(Debug, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct SourceError {
    pub kind: SourceErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl SourceError {
    pub fn new(kind: SourceErrorKind, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable,
        }
    }
}
