//! In-memory document storage for open files.
//!
//! Each open document is tracked as a [`DocumentState`] (rope text + cached parse artifacts).
//! [`DocumentStore`] wraps a concurrent map for thread-safe access from async handlers.

use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::lsp_types::*;

/// Cached state for a single open document.
pub struct DocumentState {
    /// Full text content stored as a rope for efficient line access.
    pub rope: Rope,
    /// LSP document version, incremented on each edit.
    pub version: i32,
    /// Parsed document symbols (label, screen, define, etc.).
    pub symbols: Vec<DocumentSymbol>,
    /// Most recently computed diagnostics for this document.
    pub diagnostics: Vec<Diagnostic>,
}

/// Thread-safe store mapping document URIs to their cached state.
pub struct DocumentStore(DashMap<Url, DocumentState>);

impl DocumentStore {
    /// Create an empty document store.
    pub fn new() -> Self {
        Self(DashMap::new())
    }

    /// Insert or replace the state for the given URI.
    pub fn upsert(&self, uri: Url, state: DocumentState) {
        self.0.insert(uri, state);
    }

    /// Get a shared reference to the document state for `uri`, if open.
    pub fn get(&self, uri: &Url) -> Option<dashmap::mapref::one::Ref<'_, Url, DocumentState>> {
        self.0.get(uri)
    }

    /// Remove the document from the store (called on `didClose`).
    pub fn remove(&self, uri: &Url) {
        self.0.remove(uri);
    }

    /// Read the full text content of a document as a `String`.
    pub fn read_text(&self, uri: &Url) -> Option<String> {
        self.0.get(uri).map(|doc| doc.rope.to_string())
    }

    /// Iterate over all open documents.
    pub fn iter(&self) -> dashmap::iter::Iter<'_, Url, DocumentState> {
        self.0.iter()
    }
}
