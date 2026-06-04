//! LSP `textDocument/semanticTokens/full` implementation.
//!
//! Currently a stub — returns `None` because tree-sitter highlighting handles tokenization.
//! The legend is advertised so the capability is registered for future use.

use tower_lsp::lsp_types::*;

use crate::document::DocumentStore;

/// The semantic token legend used by this server.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::CLASS,
            SemanticTokenType::PARAMETER,
            SemanticTokenType::VARIABLE,
            SemanticTokenType::KEYWORD,
        ],
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFAULT_LIBRARY,
        ],
    }
}

/// Compute semantic tokens for the given document.
/// v1: returns None (tree-sitter handles highlighting; full semantic tokens deferred).
pub fn compute(
    _uri: &Url,
    _documents: &DocumentStore,
) -> Option<SemanticTokensResult> {
    None
}
