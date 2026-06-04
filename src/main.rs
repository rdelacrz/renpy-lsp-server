//! Ren'Py language server binary entry point.
//!
//! Initializes tracing, creates the [`tower_lsp`] service backed by [`backend::Backend`],
//! and serves it over stdin/stdout using the Language Server Protocol.

mod backend;
mod document;
mod error;
mod data;
mod features;
mod index;
mod parsing;

use tower_lsp::{LspService, Server};

/// Entry point: sets up tracing to stderr, then runs the LSP server on stdin/stdout.
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(backend::Backend::new);

    Server::new(stdin, stdout, socket).serve(service).await;
}
