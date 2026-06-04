//! LSP `textDocument/definition` (go-to-definition) implementation.
//!
//! Resolves the identifier at the cursor position in the workspace symbol index and
//! returns the definition location(s).

use dashmap::DashMap;
use tower_lsp::lsp_types::*;

use crate::document::DocumentStore;
use crate::parsing::text_utils;

/// Look up definition for the word at position in the workspace index.
pub fn compute(
    uri: &Url,
    position: Position,
    documents: &DocumentStore,
    index: &DashMap<String, Vec<(Url, Range, SymbolKind)>>,
) -> Option<GotoDefinitionResponse> {
    let doc = documents.get(uri)?;
    let line_text = doc.rope.get_line(position.line as usize)?.to_string();
    let word = text_utils::keyword_prefix(&line_text, position.character as usize)?;

    let entries = index.get(&word)?;
    let locations: Vec<Location> = entries
        .iter()
        .map(|(u, range, _)| Location { uri: u.clone(), range: *range })
        .collect();

    if locations.is_empty() {
        None
    } else if locations.len() == 1 {
        Some(GotoDefinitionResponse::Scalar(locations.into_iter().next().unwrap()))
    } else {
        Some(GotoDefinitionResponse::Array(locations))
    }
}
