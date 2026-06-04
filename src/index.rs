//! Workspace-wide symbol index.
//!
//! Maintains a map from symbol names to their definition locations across all open documents.
//! Updated incrementally as documents are opened, changed, or closed.

use dashmap::DashMap;
use tower_lsp::lsp_types::*;

use crate::parsing::parser::{ParsedSymbol, ParsedReference};

/// Workspace-wide symbol index: name → list of (uri, range, kind).
pub type SymbolIndex = DashMap<String, Vec<(Url, Range, SymbolKind)>>;

/// Update the workspace index for a given URI: remove old entries, add new ones.
pub fn update_index(
    index: &SymbolIndex,
    uri: &Url,
    symbols: &[ParsedSymbol],
    _references: &[ParsedReference],
) {
    // Remove all entries for this URI
    index.retain(|_, entries| {
        entries.retain(|(u, _, _)| u != uri);
        !entries.is_empty()
    });

    // Add new symbol entries
    add_symbols_to_index(index, uri, symbols);
}

/// Recursively insert parsed symbols into the index.
fn add_symbols_to_index(index: &SymbolIndex, uri: &Url, symbols: &[ParsedSymbol]) {
    for sym in symbols {
        index
            .entry(sym.name.clone())
            .or_default()
            .push((uri.clone(), sym.selection_range, sym.kind));
        // Recurse into children
        if !sym.children.is_empty() {
            add_symbols_to_index(index, uri, &sym.children);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_index_adds_symbols() {
        let index = SymbolIndex::new();
        let uri = Url::parse("file:///test.rpy").unwrap();
        let symbols = vec![ParsedSymbol {
            name: "start".into(),
            kind: SymbolKind::FUNCTION,
            range: Range {
                start: Position { line: 0, character: 0 },
                end: Position { line: 5, character: 0 },
            },
            selection_range: Range {
                start: Position { line: 0, character: 6 },
                end: Position { line: 0, character: 11 },
            },
            children: vec![],
        }];
        update_index(&index, &uri, &symbols, &[]);
        assert!(index.contains_key("start"));
    }

    #[test]
    fn test_update_index_removes_old() {
        let index = SymbolIndex::new();
        let uri = Url::parse("file:///test.rpy").unwrap();
        let sym = ParsedSymbol {
            name: "old_label".into(),
            kind: SymbolKind::FUNCTION,
            range: Range::default(),
            selection_range: Range::default(),
            children: vec![],
        };
        update_index(&index, &uri, &[sym], &[]);
        assert!(index.contains_key("old_label"));

        // Update with new symbols (old_label gone)
        let new_sym = ParsedSymbol {
            name: "new_label".into(),
            kind: SymbolKind::FUNCTION,
            range: Range::default(),
            selection_range: Range::default(),
            children: vec![],
        };
        update_index(&index, &uri, &[new_sym], &[]);
        assert!(!index.contains_key("old_label"));
        assert!(index.contains_key("new_label"));
    }
}
