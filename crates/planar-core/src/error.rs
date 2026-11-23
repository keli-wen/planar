use std::io;

/// Unified error type for `planar-core` operations.
#[derive(Debug, thiserror::Error)]
pub enum PlanarError {
    /// Filesystem or I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    /// SQLite storage error.
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// JSON parsing or serialization error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Caller supplied invalid parameters or identifiers.
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Requested entity does not exist.
    #[error("Not found: {0}")]
    NotFound(String),

    /// Background blocking task failed to join.
    #[error("Task execution error: {0}")]
    TaskJoin(String),

    /// External trajectory processor (CLI or HTTP API) failed.
    #[error("Processor error: {0}")]
    External(String),
}

/// Convenience `Result` alias for `planar-core`.
pub type Result<T> = std::result::Result<T, PlanarError>;
