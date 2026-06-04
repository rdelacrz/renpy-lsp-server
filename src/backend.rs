//! LSP backend implementation for the Ren'Py language server.
//!
//! [`Backend`] holds shared state (open documents, workspace symbol index, game data)
//! and implements the [`LanguageServer`] trait to handle all incoming LSP requests.

use dashmap::DashMap;
use regex::Regex;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::data::loader::GameData;
use crate::document::{DocumentState, DocumentStore};
use crate::features;
use crate::features::diagnostics::DiagnosticsConfig;
use crate::index;
use crate::parsing::{parser, text_utils};

/// Core server state shared across all LSP request handlers.
pub struct Backend {
    /// The LSP client handle, used to push diagnostics and log messages.
    pub client: Client,
    /// In-memory store of open document contents and cached parse results.
    pub documents: DocumentStore,
    /// Workspace-wide symbol index: symbol name → list of (uri, range, kind).
    pub index: DashMap<String, Vec<(Url, Range, SymbolKind)>>,
    /// Configuration controlling which diagnostics are enabled.
    pub diagnostics_config: DiagnosticsConfig,
    /// Statically-loaded Ren'Py API data (functions, config vars, auto-complete hints).
    pub game_data: GameData,
}

impl Backend {
    /// Creates a new backend, loading embedded game data at startup.
    pub fn new(client: Client) -> Self {
        let game_data = crate::data::loader::load_game_data().unwrap_or_else(|e| {
            tracing::error!("Failed to load game data: {}", e);
            GameData::default()
        });
        Self {
            client,
            documents: DocumentStore::new(),
            index: DashMap::new(),
            diagnostics_config: DiagnosticsConfig::default(),
            game_data,
        }
    }

    /// Reparse a document after open/change, update index, cache state, and push diagnostics.
    async fn on_document_change(&self, uri: Url, text: String, version: i32) {
        let rope = Rope::from_str(&text);
        let (symbols, references) = parser::parse(&uri, &text);
        let doc_symbols = parser::to_document_symbols(&symbols);
        let diagnostics = features::diagnostics::compute(&uri, &text, &self.diagnostics_config);

        index::update_index(&self.index, &uri, &symbols, &references);

        self.documents.upsert(uri.clone(), DocumentState {
            rope,
            version,
            symbols: doc_symbols,
            diagnostics: diagnostics.clone(),
        });

        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    /// Find all occurrences of the word at `position` across open documents (regex word-boundary match).
    fn compute_references(&self, uri: &Url, position: Position) -> Option<Vec<Location>> {
        let doc = self.documents.get(uri)?;
        let line_text = doc.rope.get_line(position.line as usize)?.to_string();
        let word = text_utils::keyword_prefix(&line_text, position.character as usize)?;
        drop(doc);

        let re = word_boundary_regex(&word)?;
        let mut locations = Vec::new();

        for entry in self.documents.iter() {
            let doc_uri = entry.key().clone();
            let text = entry.value().rope.to_string();
            for (line_idx, line) in text.lines().enumerate() {
                let filtered = text_utils::filter_string_literals(line);
                for m in re.find_iter(&filtered) {
                    locations.push(Location {
                        uri: doc_uri.clone(),
                        range: Range {
                            start: Position { line: line_idx as u32, character: text_utils::byte_to_utf16(&filtered, m.start()) as u32 },
                            end: Position { line: line_idx as u32, character: text_utils::byte_to_utf16(&filtered, m.end()) as u32 },
                        },
                    });
                }
            }
        }

        if locations.is_empty() { None } else { Some(locations) }
    }

    /// Compute indent-based folding ranges for a document.
    fn compute_folding_ranges(&self, uri: &Url) -> Option<Vec<FoldingRange>> {
        let doc = self.documents.get(uri)?;
        let text = doc.rope.to_string();
        let lines: Vec<&str> = text.lines().collect();
        let mut ranges = Vec::new();

        for i in 0..lines.len() {
            let line = lines[i];
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            // Find extent of the block (lines with greater indent)
            let mut end = i;
            for j in (i + 1)..lines.len() {
                if lines[j].trim().is_empty() {
                    continue;
                }
                if lines[j].len() - lines[j].trim_start().len() > indent {
                    end = j;
                } else {
                    break;
                }
            }
            if end > i {
                ranges.push(FoldingRange {
                    start_line: i as u32,
                    start_character: None,
                    end_line: end as u32,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                });
            }
        }

        if ranges.is_empty() { None } else { Some(ranges) }
    }

    /// Highlight all occurrences of the word at `position` within the same document.
    fn compute_document_highlight(&self, uri: &Url, position: Position) -> Option<Vec<DocumentHighlight>> {
        let doc = self.documents.get(uri)?;
        let line_text = doc.rope.get_line(position.line as usize)?.to_string();
        let word = text_utils::keyword_prefix(&line_text, position.character as usize)?;

        let text = doc.rope.to_string();
        let re = word_boundary_regex(&word)?;

        let mut highlights = Vec::new();
        for (line_idx, line) in text.lines().enumerate() {
            for m in re.find_iter(line) {
                highlights.push(DocumentHighlight {
                    range: Range {
                        start: Position { line: line_idx as u32, character: text_utils::byte_to_utf16(line, m.start()) as u32 },
                        end: Position { line: line_idx as u32, character: text_utils::byte_to_utf16(line, m.end()) as u32 },
                    },
                    kind: Some(DocumentHighlightKind::TEXT),
                });
            }
        }

        if highlights.is_empty() { None } else { Some(highlights) }
    }
}

/// Build a word-boundary regex for exact identifier matching.
fn word_boundary_regex(word: &str) -> Option<Regex> {
    Regex::new(&format!(r"(?<![a-zA-Z_]){}(?![a-zA-Z_0-9])", regex::escape(word))).ok()
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _params: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".into(), " ".into(), "@".into(), "-".into(), "(".into(),
                    ]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec![
                        "(".into(), ",".into(), "=".into(),
                    ]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                color_provider: Some(ColorProviderCapability::Simple(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                semantic_tokens_provider: Some(
                    SemanticTokensServerCapabilities::SemanticTokensOptions(
                        SemanticTokensOptions {
                            legend: features::semantic_tokens::legend(),
                            full: Some(SemanticTokensFullOptions::Bool(true)),
                            range: None,
                            work_done_progress_options: Default::default(),
                        },
                    ),
                ),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "renpy-language-server".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _params: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Ren'Py language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let td = params.text_document;
        self.on_document_change(td.uri, td.text, td.version).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.on_document_change(params.text_document.uri, change.text, params.text_document.version).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.documents.remove(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        Ok(features::completion::compute(&uri, pos, &self.documents, &self.game_data))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        Ok(features::hover::compute(&uri, pos, &self.documents, &self.game_data))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        Ok(features::signature::compute(&uri, pos, &self.documents, &self.game_data))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        Ok(features::definition::compute(&uri, pos, &self.documents, &self.index))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        Ok(self.compute_references(&uri, pos))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(features::symbols::compute(&params.text_document.uri, &self.documents))
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        Ok(features::color::compute(&params.text_document.uri, &self.documents))
    }

    async fn color_presentation(
        &self,
        params: ColorPresentationParams,
    ) -> Result<Vec<ColorPresentation>> {
        Ok(features::color::presentations(params.color, params.range))
    }

    async fn folding_range(&self, params: FoldingRangeParams) -> Result<Option<Vec<FoldingRange>>> {
        Ok(self.compute_folding_ranges(&params.text_document.uri))
    }

    async fn document_highlight(
        &self,
        params: DocumentHighlightParams,
    ) -> Result<Option<Vec<DocumentHighlight>>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        Ok(self.compute_document_highlight(&uri, pos))
    }

    async fn semantic_tokens_full(
        &self,
        params: SemanticTokensParams,
    ) -> Result<Option<SemanticTokensResult>> {
        Ok(features::semantic_tokens::compute(&params.text_document.uri, &self.documents))
    }
}
