//! LSP `textDocument/documentSymbol` implementation.
//!
//! Returns the cached document symbols (parsed on open/change) for outline display.

use tower_lsp::lsp_types::*;

use crate::document::DocumentStore;

/// Return the cached document symbols for the given document.
pub fn compute(uri: &Url, documents: &DocumentStore) -> Option<DocumentSymbolResponse> {
    let doc = documents.get(uri)?;
    if doc.symbols.is_empty() {
        None
    } else {
        Some(DocumentSymbolResponse::Nested(doc.symbols.clone()))
    }
}
