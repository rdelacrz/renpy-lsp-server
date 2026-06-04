//! LSP feature implementations (completion, hover, diagnostics, etc.).
//!
//! Each sub-module exposes a `compute` function (or equivalent) called by the backend's
//! [`LanguageServer`](tower_lsp::LanguageServer) trait implementation.

pub mod color;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod hover;
pub mod semantic_tokens;
pub mod signature;
pub mod symbols;
