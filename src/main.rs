//! Ren'Py language server binary entry point.
//!
//! Initializes tracing, creates the [`tower_lsp`] service backed by [`backend::Backend`],
//! and serves it over stdin/stdout using the Language Server Protocol.

mod backend;
mod data;
mod document;
mod error;
mod features;
mod index;
mod parsing;

use std::sync::atomic::Ordering;
use std::task::{Context, Poll};
use tower::Service;
use tower_lsp::jsonrpc::Request;
use tower_lsp::{LspService, Server};

/// tower-lsp 0.20 otherwise waits for another frame/EOF after `exit`.
/// Terminate immediately with the status required by the LSP specification,
/// without waiting for Tokio's uncancellable blocking stdin worker.
struct ExitService(LspService<backend::Backend>);

impl Service<Request> for ExitService {
    type Response = <LspService<backend::Backend> as Service<Request>>::Response;
    type Error = <LspService<backend::Backend> as Service<Request>>::Error;
    type Future = <LspService<backend::Backend> as Service<Request>>::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        if request.method() == "exit" && request.id().is_none() {
            let clean = self.0.inner().shutdown_requested.load(Ordering::SeqCst);
            std::process::exit(if clean { 0 } else { 1 });
        }
        self.0.call(request)
    }
}

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

    Server::new(stdin, stdout, socket)
        .serve(ExitService(service))
        .await;
    // EOF also needs to bypass Tokio's blocking stdin worker during teardown.
    std::process::exit(0);
}
