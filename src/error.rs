//! Error types for the Ren'Py language server.

use tower_lsp::lsp_types::Url;

/// Server-internal error variants.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// Filesystem I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON serialization/deserialization failure.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Requested document is not currently open.
    #[error("Document not found: {0}")]
    DocumentNotFound(Url),
    /// Generic parse failure.
    #[error("Parse error: {0}")]
    Parse(String),
}

/// Convenience alias used throughout the server crate.
pub type Result<T> = std::result::Result<T, Error>;
