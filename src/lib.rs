//! Minimal Ren'Py language server built on `tower-lsp`.
//!
//! Tracks open documents via a workspace index, supports full-sync text-document
//! notifications, and provides language features: document symbols, go-to-definition,
//! hover, completion, and diagnostics.

use std::sync::Mutex;

use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

pub mod analysis;
pub mod index;

use index::WorkspaceIndex;

/// Ren'Py keywords offered as completions when no project symbol matches.
const RENPY_KEYWORDS: &[&str] = &[
    "label",
    "screen",
    "image",
    "define",
    "transform",
    "style",
    "init",
    "python",
    "call",
    "jump",
    "scene",
    "show",
    "hide",
    "menu",
    "return",
    "with",
    "play",
    "stop",
];

/// Ren'Py builtins available after `renpy.`.
const RENPY_BUILTINS: &[&str] = &[
    "audio",
    "config",
    "exports",
    "game",
    "get_translation",
    "image",
    "jump",
    "persistent",
    "preferences",
    "python",
    "quit",
    "random",
    "restart_interaction",
    "rollback",
    "store",
    "ui",
];

/// Ren'Py configuration variables available after `config.`.
const CONFIG_COMPLETIONS: &[&str] = &[
    "developer",
    "debug",
    "default_text_cps",
    "window_title",
    "name",
    "version",
    "has_sound",
    "has_music",
    "has_voice",
    "layers",
    "overlay_during_with",
    "rollback_enabled",
];

/// Ren'Py GUI variables available after `gui.`.
const GUI_COMPLETIONS: &[&str] = &[
    "text_size",
    "name_text_size",
    "button_text_size",
    "slider_text_size",
    "slot_button_xsize",
    "slot_button_ysize",
    "settings_spacing",
    "navigation_spacing",
    "dialogue_xpos",
    "dialogue_ypos",
    "dialogue_width",
];

/// Transition names suggested after `with `.
const TRANSITION_COMPLETIONS: &[&str] = &[
    "dissolve",
    "fade",
    "move",
    "moveinleft",
    "moveinright",
    "moveintop",
    "moveinbottom",
    "moveoutleft",
    "moveoutright",
    "moveouttop",
    "moveoutbottom",
    "pixellate",
    "vpunch",
    "hpunch",
    "wipeleft",
    "wiperight",
    "wipedown",
    "wipeup",
];

/// Ren'Py text tags suggested inside dialogue strings.
const TEXT_TAG_COMPLETIONS: &[&str] = &[
    "b", "i", "u", "s", "color", "size", "font", "alpha", "p", "w", "nw", "rt", "rb", "a", "image",
];

/// Core LSP backend that holds the client handle and a workspace index.
#[derive(Debug)]
pub struct Backend {
    client: Client,
    index: Mutex<WorkspaceIndex>,
}

impl Backend {
    /// Create a new backend wired to the given LSP client.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            index: Mutex::new(WorkspaceIndex::new()),
        }
    }

    /// Lock the index, run a closure, and return its result.
    ///
    /// Uses `unwrap_or_else(|e| e.into_inner())` to survive a poisoned
    /// Mutex instead of panicking the whole server.
    fn with_index<R>(&self, f: impl FnOnce(&mut WorkspaceIndex) -> R) -> R {
        f(&mut self
            .index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()))
    }

    /// Find the symbol at a given line in a list of symbols.
    ///
    /// O(n) linear scan — fine for v0.1; can be optimised with a
    /// `HashMap<usize, Symbol>` if symbol counts grow large.
    fn symbol_at_line(symbols: &[analysis::Symbol], line: usize) -> Option<&analysis::Symbol> {
        symbols.iter().find(|s| s.line == line)
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    /// Advertise full text-document sync, document symbols, go-to-definition,
    /// hover, and completion.
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                document_symbol_provider: Some(OneOf::Left(true)),
                definition_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".to_string(),
                        " ".to_string(),
                        "\"".to_string(),
                    ]),
                    ..Default::default()
                }),
                ..ServerCapabilities::default()
            },
            server_info: Some(ServerInfo {
                name: "renpy-lsp-server".to_string(),
                version: Some(version().to_string()),
            }),
            ..InitializeResult::default()
        })
    }

    /// Log a ready message once initialization completes.
    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "renpy-lsp-server initialized")
            .await;
    }

    /// No-op graceful shutdown.
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    /// Insert opened document text, re-index, and publish diagnostics.
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        // Clone URI before locking so the lock isn't held across a clone.
        let uri = params.text_document.uri;
        let diags = self.with_index(|index| {
            index.open_document(uri.clone(), &params.text_document.text);
            index.refresh();
            index
                .documents
                .get(&uri)
                .map(|d| d.diagnostics.clone())
                .unwrap_or_default()
        });
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    /// Replace document text with the last full content change, re-index,
    /// and publish diagnostics.
    ///
    /// Only the *last* change is processed because we advertise
    /// `TextDocumentSyncKind::FULL` — every change carries the complete
    /// document text.  If the sync mode ever switches to INCREMENTAL a
    /// proper incremental-diff implementation will be needed.
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Clone URI before locking so the lock isn't held across a clone.
        let uri = params.text_document.uri;
        let text = params
            .content_changes
            .into_iter()
            .last()
            .map(|c| c.text)
            .unwrap_or_default();
        let diags = self.with_index(|index| {
            index.open_document(uri.clone(), &text);
            index.refresh();
            index
                .documents
                .get(&uri)
                .map(|d| d.diagnostics.clone())
                .unwrap_or_default()
        });
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    /// Remove closed document from tracking, re-index, and clear diagnostics.
    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        // Clone URI before locking.
        let uri = params.text_document.uri;
        self.with_index(|index| {
            index.close_document(&uri);
            index.refresh();
        });
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    /// Re-index and publish diagnostics on save.
    ///
    /// NOTE: we do not use the optional `params.text`; we re-analyse the
    /// stored text from `did_open` / `did_change`.  Passing `params.text`
    /// would let us avoid a clone, but `did_save` may not always include
    /// it depending on the client's `TextDocumentSaveOptions.includeText`.
    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // Clone URI before locking.
        let uri = params.text_document.uri;
        let diags = self.with_index(|index| {
            index.refresh();
            index
                .documents
                .get(&uri)
                .map(|d| d.diagnostics.clone())
                .unwrap_or_default()
        });
        self.client.publish_diagnostics(uri, diags, None).await;
    }

    /// Provide document symbols.
    ///
    /// We return flat `SymbolInformation` instead of hierarchical
    /// `DocumentSymbol` (tower-lsp v0.20 `document_symbol` response type
    /// is deprecated in newer LSP specs in favour of
    /// `text_document_document_symbol`).  The `#[allow(deprecated)]` is
    /// intentional; see tower-lsp docs for migration guidance.
    #[allow(deprecated)]
    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        // Clone URI before locking.
        let uri = params.text_document.uri;
        let result = self.with_index(|index| {
            let store = index.documents.get(&uri);
            let symbols = store.map(|s| &s.symbols[..]).unwrap_or(&[]);
            symbols
                .iter()
                .map(|sym| SymbolInformation {
                    name: sym.name.clone(),
                    kind: symbol_kind_to_lsp(sym.kind),
                    location: Location {
                        uri: uri.clone(),
                        range: Range {
                            start: Position::new(sym.line as u32, 0),
                            end: Position::new(sym.line as u32, 0),
                        },
                    },
                    container_name: None,
                    tags: None,
                    deprecated: None,
                })
                .collect::<Vec<_>>()
        });
        Ok(Some(DocumentSymbolResponse::Flat(result)))
    }

    /// Provide go-to-definition: jump to the line where a symbol is defined.
    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        // Clone URI before locking.
        let uri = params.text_document_position_params.text_document.uri;
        let line = params.text_document_position_params.position.line as usize;
        let result = self.with_index(|index| {
            let store = index.documents.get(&uri)?;
            let sym = Self::symbol_at_line(&store.symbols, line)?;
            Some(GotoDefinitionResponse::Scalar(Location {
                uri: uri.clone(),
                range: Range {
                    start: Position::new(sym.line as u32, 0),
                    end: Position::new(sym.line as u32, 0),
                },
            }))
        });
        Ok(result)
    }

    /// Provide hover content for a symbol at the cursor position.
    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        // Clone URI before locking.
        let uri = params.text_document_position_params.text_document.uri;
        let line = params.text_document_position_params.position.line as usize;
        let result = self.with_index(|index| {
            let store = index.documents.get(&uri)?;
            let sym = Self::symbol_at_line(&store.symbols, line)?;
            Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!(
                        "**{}**  \n`{}` defined on line {}",
                        kind_label(sym.kind),
                        sym.name,
                        sym.line + 1
                    ),
                }),
                range: None,
            })
        });
        Ok(result)
    }

    /// Provide completion items: project symbols + Ren'Py keywords.
    ///
    /// Collects cheap references inside the lock, then builds the full
    /// `Vec<CompletionItem>` outside so String clones don't hold the
    /// Mutex.
    async fn completion(&self, _params: CompletionParams) -> Result<Option<CompletionResponse>> {
        // Collect (name, kind, line) tuples under the lock.
        let symbol_data: Vec<(String, analysis::SymbolKind, usize)> = self.with_index(|index| {
            index
                .symbols
                .all()
                .iter()
                .map(|s| (s.name.clone(), s.kind, s.line))
                .collect()
        });

        // Build completion items outside the lock.
        let mut items: Vec<CompletionItem> =
            Vec::with_capacity(symbol_data.len() + RENPY_KEYWORDS.len());

        for (name, kind, line) in symbol_data {
            items.push(CompletionItem {
                label: name,
                kind: Some(completion_kind_for(kind)),
                detail: Some(format!("{} defined on line {}", kind_label(kind), line + 1)),
                ..Default::default()
            });
        }

        for kw in RENPY_KEYWORDS {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some("Ren'Py keyword".to_string()),
                ..Default::default()
            });
        }

        Ok(Some(CompletionResponse::Array(items)))
    }
}

fn symbol_kind_to_lsp(kind: analysis::SymbolKind) -> SymbolKind {
    match kind {
        analysis::SymbolKind::Label => SymbolKind::KEY,
        analysis::SymbolKind::Screen => SymbolKind::INTERFACE,
        analysis::SymbolKind::Function => SymbolKind::FUNCTION,
        analysis::SymbolKind::Define => SymbolKind::VARIABLE,
        analysis::SymbolKind::Image => SymbolKind::FILE,
        analysis::SymbolKind::Transform => SymbolKind::OPERATOR,
        analysis::SymbolKind::Style => SymbolKind::PROPERTY,
    }
}

fn kind_label(kind: analysis::SymbolKind) -> &'static str {
    match kind {
        analysis::SymbolKind::Label => "Label",
        analysis::SymbolKind::Screen => "Screen",
        analysis::SymbolKind::Function => "init python",
        analysis::SymbolKind::Define => "Define",
        analysis::SymbolKind::Image => "Image",
        analysis::SymbolKind::Transform => "Transform",
        analysis::SymbolKind::Style => "Style",
    }
}

fn completion_kind_for(kind: analysis::SymbolKind) -> CompletionItemKind {
    match kind {
        analysis::SymbolKind::Label => CompletionItemKind::KEYWORD,
        analysis::SymbolKind::Screen => CompletionItemKind::INTERFACE,
        analysis::SymbolKind::Function => CompletionItemKind::FUNCTION,
        analysis::SymbolKind::Define => CompletionItemKind::VARIABLE,
        analysis::SymbolKind::Image => CompletionItemKind::FILE,
        analysis::SymbolKind::Transform => CompletionItemKind::OPERATOR,
        analysis::SymbolKind::Style => CompletionItemKind::PROPERTY,
    }
}

/// Return the crate version string from `CARGO_PKG_VERSION`.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;
    use analysis::{Symbol, SymbolKind};
    use tower_lsp::LspService;
    use tower_lsp::lsp_types::Url;

    // ---------------------------------------------------------------------------
    // Backend construction
    // ---------------------------------------------------------------------------

    /// Create a `&'static Backend` via a leaked `LspService`.
    ///
    /// This leaks a small amount of memory on purpose — it simplifies the
    /// test signatures without requiring complicated lifetime management.
    /// Acceptable for unit tests that run in a single process.
    fn backend() -> &'static Backend {
        let (svc, _socket) = LspService::new(Backend::new);
        let leaked: &'static LspService<Backend> = Box::leak(Box::new(svc));
        leaked.inner()
    }

    // ---------------------------------------------------------------------------
    // Unit tests for pure functions
    // ---------------------------------------------------------------------------

    #[test]
    fn test_version() {
        // Cargo.toml says version = "0.1.0"
        assert_eq!(version(), "0.1.0");
    }

    #[test]
    fn test_renpy_keywords() {
        assert!(!RENPY_KEYWORDS.is_empty());
        assert!(RENPY_KEYWORDS.contains(&"label"));
        assert!(RENPY_KEYWORDS.contains(&"screen"));
        assert!(RENPY_KEYWORDS.contains(&"image"));
        assert!(RENPY_KEYWORDS.contains(&"define"));
        assert!(RENPY_KEYWORDS.contains(&"transform"));
        assert!(RENPY_KEYWORDS.contains(&"style"));
        assert!(RENPY_KEYWORDS.contains(&"menu"));
        assert!(RENPY_KEYWORDS.contains(&"return"));
        assert_eq!(RENPY_KEYWORDS.len(), 18);
    }

    #[test]
    fn test_symbol_kind_to_lsp() {
        use tower_lsp::lsp_types::SymbolKind as LspKind;

        assert_eq!(symbol_kind_to_lsp(SymbolKind::Label), LspKind::KEY);
        assert_eq!(symbol_kind_to_lsp(SymbolKind::Screen), LspKind::INTERFACE);
        assert_eq!(symbol_kind_to_lsp(SymbolKind::Function), LspKind::FUNCTION);
        assert_eq!(symbol_kind_to_lsp(SymbolKind::Define), LspKind::VARIABLE);
        assert_eq!(symbol_kind_to_lsp(SymbolKind::Image), LspKind::FILE);
        assert_eq!(symbol_kind_to_lsp(SymbolKind::Transform), LspKind::OPERATOR);
        assert_eq!(symbol_kind_to_lsp(SymbolKind::Style), LspKind::PROPERTY);
    }

    #[test]
    fn test_completion_kind_for() {
        use tower_lsp::lsp_types::CompletionItemKind;

        assert_eq!(
            completion_kind_for(SymbolKind::Label),
            CompletionItemKind::KEYWORD
        );
        assert_eq!(
            completion_kind_for(SymbolKind::Screen),
            CompletionItemKind::INTERFACE
        );
        assert_eq!(
            completion_kind_for(SymbolKind::Function),
            CompletionItemKind::FUNCTION
        );
        assert_eq!(
            completion_kind_for(SymbolKind::Define),
            CompletionItemKind::VARIABLE
        );
        assert_eq!(
            completion_kind_for(SymbolKind::Image),
            CompletionItemKind::FILE
        );
        assert_eq!(
            completion_kind_for(SymbolKind::Transform),
            CompletionItemKind::OPERATOR
        );
        assert_eq!(
            completion_kind_for(SymbolKind::Style),
            CompletionItemKind::PROPERTY
        );
    }

    #[test]
    fn test_kind_label() {
        assert_eq!(kind_label(SymbolKind::Label), "Label");
        assert_eq!(kind_label(SymbolKind::Screen), "Screen");
        assert_eq!(kind_label(SymbolKind::Function), "init python");
        assert_eq!(kind_label(SymbolKind::Define), "Define");
        assert_eq!(kind_label(SymbolKind::Image), "Image");
        assert_eq!(kind_label(SymbolKind::Transform), "Transform");
        assert_eq!(kind_label(SymbolKind::Style), "Style");
    }

    #[test]
    fn test_symbol_at_line_found() {
        let syms = vec![
            Symbol {
                kind: SymbolKind::Label,
                name: "start".into(),
                line: 0,
            },
            Symbol {
                kind: SymbolKind::Screen,
                name: "main".into(),
                line: 3,
            },
        ];
        let found = Backend::symbol_at_line(&syms, 3);
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "main");
        assert_eq!(found.unwrap().kind, SymbolKind::Screen);
    }

    #[test]
    fn test_symbol_at_line_not_found() {
        let syms = vec![Symbol {
            kind: SymbolKind::Label,
            name: "start".into(),
            line: 0,
        }];
        assert!(Backend::symbol_at_line(&syms, 99).is_none());
    }

    #[test]
    fn test_symbol_at_line_empty() {
        let syms: Vec<Symbol> = vec![];
        assert!(Backend::symbol_at_line(&syms, 0).is_none());
    }

    // ---------------------------------------------------------------------------
    // Backend integration tests (require a constructed Backend)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_backend_new_creates_backend() {
        let b = backend();
        let idx = b
            .index
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(idx.documents.is_empty());
        assert!(idx.symbols.all().is_empty());
    }

    #[test]
    fn test_with_index_opens_and_tracks_document() {
        let b = backend();
        let uri: Url = "file:///test/my_game.rpy".parse().unwrap();

        b.with_index(|index| {
            index.open_document(uri.clone(), "label start:\nscreen main:\n");
            index.refresh();
        });

        let doc = b
            .with_index(|index| index.documents.get(&uri).map(|d| d.symbols.clone()))
            .unwrap();
        assert_eq!(doc.len(), 2);
        assert_eq!(doc[0].name, "start");
        assert_eq!(doc[0].kind, SymbolKind::Label);
        assert_eq!(doc[1].name, "main");
        assert_eq!(doc[1].kind, SymbolKind::Screen);
    }

    #[test]
    fn test_with_index_close_removes_document() {
        let b = backend();
        let uri: Url = "file:///test/my_game.rpy".parse().unwrap();

        b.with_index(|index| {
            index.open_document(uri.clone(), "label start:\n");
            index.refresh();
        });

        b.with_index(|index| {
            index.close_document(&uri);
            index.refresh();
        });

        let has = b.with_index(|index| index.documents.contains_key(&uri));
        assert!(!has);
    }

    #[test]
    fn test_symbol_at_line_used_by_backend() {
        let b = backend();
        let uri: Url = "file:///test/my_game.rpy".parse().unwrap();

        b.with_index(|index| {
            index.open_document(uri.clone(), "label start:\n    \"hello\"\nscreen main:\n");
            index.refresh();
        });

        let sym = b.with_index(|index| {
            let store = index.documents.get(&uri).unwrap();
            Backend::symbol_at_line(&store.symbols, 2) // line 2 = "screen main:"
                .map(|s| (s.name.clone(), s.kind))
        });

        assert!(sym.is_some());
        let (name, kind) = sym.unwrap();
        assert_eq!(name, "main");
        assert_eq!(kind, SymbolKind::Screen);
    }

    #[test]
    fn test_completion_empty_document() {
        let b = backend();
        let uri: Url = "file:///test/empty.rpy".parse().unwrap();
        let params = CompletionParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position: Position::new(0, 0),
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: None,
        };
        let result = {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(b.completion(params))
        }
        .unwrap();
        let items = match result.unwrap() {
            CompletionResponse::Array(v) => v,
            _ => panic!("Expected array response"),
        };
        assert_eq!(items.len(), RENPY_KEYWORDS.len());
        assert!(
            items
                .iter()
                .all(|i| i.kind == Some(CompletionItemKind::KEYWORD))
        );
    }
}
