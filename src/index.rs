//! Workspace index for the Ren'Py LSP server.
//!
//! Tracks open documents, a resolved symbol table, and cached diagnostics.

use std::collections::HashMap;

use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, Position, Range, Url};

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

fn extract_error_line(message: &str) -> Option<u32> {
    let lowered = message.to_ascii_lowercase();
    for key in ["line ", "line:", "line="] {
        if let Some(idx) = lowered.find(key) {
            let start = idx + key.len();
            let digits: String = message[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(line) = digits.parse::<u32>() {
                return line.checked_sub(1).or(Some(0));
            }
        }
    }
    None
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
                Err(err) => {
                    // Clear stale symbols on parse failure and surface the
                    // analysis problem as an LSP diagnostic.
                    doc.symbols.clear();
                    let line = extract_error_line(err.message()).unwrap_or(0);
                    doc.diagnostics = vec![Diagnostic {
                        range: Range {
                            start: Position::new(line, 0),
                            end: Position::new(line, 1),
                        },
                        severity: Some(DiagnosticSeverity::ERROR),
                        code: None,
                        code_description: None,
                        source: Some("renpy-lsp".to_string()),
                        message: err.to_string(),
                        related_information: None,
                        tags: None,
                        data: None,
                    }];
                }
            }
        }

        self.symbols.rebuild(all_symbols);
    }
}
