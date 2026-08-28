use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("incompatible library: {0}")]
    Incompatible(String),
    #[error("invalid library state: {0}")]
    InvalidState(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("operation requires non-undoable confirmation")]
    UndoLimitExceeded,
}

pub type Result<T> = std::result::Result<T, LibraryError>;
