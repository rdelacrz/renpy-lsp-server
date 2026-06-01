//! Ren'Py LSP server entry point.
//!
//! Wires `renpy_lsp::Backend` into a `tower-lsp` server that communicates
//! over stdio.

use tower_lsp::{LspService, Server};

/// Bootstrap the LSP server on stdio.
#[tokio::main]
async fn main() {
    let (stdin, stdout) = (tokio::io::stdin(), tokio::io::stdout());

    let (service, socket) = LspService::new(renpy_lsp::Backend::new);
    Server::new(stdin, stdout, socket).serve(service).await;
}
