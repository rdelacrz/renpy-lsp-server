//! Workspace index for the Ren'Py LSP server.
//!
//! Tracks open documents, a resolved symbol table, and cached diagnostics.

use std::collections::HashMap;

use tower_lsp::lsp_types::{Diagnostic, Url};

use crate::analysis::{self, Symbol};

/// Per-document state held in the index.
#[derive(Debug)]
pub struct DocumentStore {
    /// Latest full text of the document, if known.
    pub text: Option<String>,
    /// Parsed symbols from the most recent analysis run.
    pub symbols: Vec<Symbol>,
    /// Most recent diagnostics for this document.
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentStore {
    fn new() -> Self {
        Self {
            text: None,
            symbols: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

/// Aggregated symbol table built from every document in the index.
///
/// Note: currently stores a flat `Vec<Symbol>`; symbol lookups use
/// document-scoped queries through `WorkspaceIndex.documents`.  A
/// `HashMap` for by-kind+name lookup can be added back when needed.
#[derive(Debug)]
pub struct SymbolTable {
    pub symbols: Vec<Symbol>,
}

impl SymbolTable {
    fn empty() -> Self {
        Self {
            symbols: Vec::new(),
        }
    }

    /// Rebuild the table from a flat list of `Symbol`s.
    fn rebuild(&mut self, incoming: Vec<Symbol>) {
        self.symbols = incoming;
    }

    /// Return all symbols.
    pub fn all(&self) -> &[Symbol] {
        &self.symbols
    }
}

/// Central workspace index shared across the server.
#[derive(Debug)]
pub struct WorkspaceIndex {
    pub documents: HashMap<Url, DocumentStore>,
    pub symbols: SymbolTable,
}

impl WorkspaceIndex {
    pub fn new() -> Self {
        Self {
            documents: HashMap::new(),
            symbols: SymbolTable::empty(),
        }
    }

    /// Open (or replace) a document.
    pub fn open_document(&mut self, uri: Url, text: &str) {
        if let Some(doc) = self.documents.get_mut(&uri) {
            doc.text = Some(text.to_string());
        } else {
            let mut store = DocumentStore::new();
            store.text = Some(text.to_string());
            self.documents.insert(uri, store);
        }
    }

    /// Close a document.
    pub fn close_document(&mut self, uri: &Url) {
        self.documents.remove(uri);
    }

    /// Run analysis on every open document and rebuild the symbol table.
    pub fn refresh(&mut self) {
        let mut all_symbols = Vec::new();

        for (_uri, doc) in self.documents.iter_mut() {
            let text = match &doc.text {
                Some(t) => t.clone(),
                None => continue,
            };

            match analysis::analyze(&text) {
                Ok(model) => {
                    doc.symbols = model.symbols;
                    all_symbols.extend(doc.symbols.clone());
                    doc.diagnostics.clear();
                }
                Err(_err) => {
                    // Clear stale symbols on parse failure.
                    doc.symbols.clear();
                }
            }
        }

        self.symbols.rebuild(all_symbols);
    }
}
