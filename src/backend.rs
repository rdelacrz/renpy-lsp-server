//! Stdio LSP backend. Open buffers override disk-backed workspace documents.
use dashmap::DashMap;
use regex::Regex;
use ropey::Rope;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::data::loader::GameData;
use crate::document::{DocumentState, DocumentStore};
use crate::features;
use crate::features::diagnostics::DiagnosticsConfig;
use crate::index;
use crate::parsing::{parser, text_utils};

pub struct Backend {
    pub client: Client,
    pub documents: DocumentStore,
    pub index: index::SymbolIndex,
    pub diagnostics_config: DiagnosticsConfig,
    pub game_data: GameData,
    open_files: DashMap<Url, ()>,
    watch_files: AtomicBool,
    pub shutdown_requested: AtomicBool,
}

impl Backend {
    pub fn new(client: Client) -> Self {
        let game_data = crate::data::loader::load_game_data().unwrap_or_else(|e| {
            tracing::error!("Failed to load game data: {e}");
            GameData::default()
        });
        Self {
            client,
            documents: DocumentStore::new(),
            index: DashMap::new(),
            diagnostics_config: DiagnosticsConfig::default(),
            game_data,
            open_files: DashMap::new(),
            watch_files: AtomicBool::new(false),
            shutdown_requested: AtomicBool::new(false),
        }
    }

    /// Index a saved script without publishing diagnostics for unopened files.
    fn load_disk_document(&self, uri: &Url) {
        let text = uri.to_file_path().ok().and_then(|path| {
            let metadata = std::fs::symlink_metadata(&path).ok()?;
            if !metadata.is_file() || metadata.len() > 8 * 1024 * 1024 {
                return None;
            }
            std::fs::read_to_string(path).ok()
        });
        if let Some(text) = text {
            let (symbols, references) = parser::parse(uri, &text);
            index::update_index(&self.index, uri, &symbols, &references);
            self.documents.upsert(
                uri.clone(),
                DocumentState {
                    rope: Rope::from_str(&text),
                    version: -1,
                    symbols: parser::to_document_symbols(&symbols),
                    diagnostics: vec![],
                },
            );
        } else {
            self.documents.remove(uri);
            index::update_index(&self.index, uri, &[], &[]);
        }
    }

    /// Bound traversal, skip generated directories, and never follow symlinks.
    fn scan_workspace(&self, root: &Path) {
        let mut pending = vec![root.to_path_buf()];
        let mut visited = 0;
        while let Some(dir) = pending.pop() {
            let Ok(entries) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in entries.flatten() {
                visited += 1;
                if visited > 100_000 {
                    tracing::warn!("Workspace scan stopped at 100000 entries");
                    return;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                if kind.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if !name.starts_with('.')
                        && !matches!(
                            name.as_ref(),
                            "node_modules" | "target" | "cache" | "saves" | "renpy"
                        )
                    {
                        pending.push(path);
                    }
                } else if kind.is_file() && is_script(&path) {
                    if let Ok(uri) = Url::from_file_path(path) {
                        self.load_disk_document(&uri);
                    }
                }
            }
        }
    }

    async fn on_document_change(&self, uri: Url, text: String, version: i32) {
        let (symbols, references) = parser::parse(&uri, &text);
        let diagnostics = features::diagnostics::compute(&uri, &text, &self.diagnostics_config);
        index::update_index(&self.index, &uri, &symbols, &references);
        self.documents.upsert(
            uri.clone(),
            DocumentState {
                rope: Rope::from_str(&text),
                version,
                symbols: parser::to_document_symbols(&symbols),
                diagnostics: diagnostics.clone(),
            },
        );
        self.client
            .publish_diagnostics(uri, diagnostics, Some(version))
            .await;
    }

    fn compute_references(
        &self,
        uri: &Url,
        position: Position,
        include_declaration: bool,
    ) -> Option<Vec<Location>> {
        let doc = self.documents.get(uri)?;
        let line = doc.rope.get_line(position.line as usize)?.to_string();
        let word = text_utils::identifier_at_position(&line, position.character as usize)?;
        drop(doc);
        let re = word_boundary_regex(&word)?;
        let mut locations = vec![];
        for entry in self.documents.iter() {
            for (row, line) in entry.value().rope.to_string().lines().enumerate() {
                for range in identifier_ranges(line, row as u32, &re) {
                    let declaration = self.index.get(&word).is_some_and(|definitions| {
                        definitions
                            .iter()
                            .any(|(u, r, _)| u == entry.key() && r == &range)
                    });
                    if include_declaration || !declaration {
                        locations.push(Location {
                            uri: entry.key().clone(),
                            range,
                        });
                    }
                }
            }
        }
        Some(locations)
    }

    fn compute_document_highlight(
        &self,
        uri: &Url,
        position: Position,
    ) -> Option<Vec<DocumentHighlight>> {
        let doc = self.documents.get(uri)?;
        let line = doc.rope.get_line(position.line as usize)?.to_string();
        let word = text_utils::identifier_at_position(&line, position.character as usize)?;
        let re = word_boundary_regex(&word)?;
        Some(
            doc.rope
                .to_string()
                .lines()
                .enumerate()
                .flat_map(|(row, line)| {
                    identifier_ranges(line, row as u32, &re)
                        .into_iter()
                        .map(|range| DocumentHighlight {
                            range,
                            kind: Some(DocumentHighlightKind::TEXT),
                        })
                })
                .collect(),
        )
    }

    fn compute_folding_ranges(&self, uri: &Url) -> Option<Vec<FoldingRange>> {
        let doc = self.documents.get(uri)?;
        let text = doc.rope.to_string();
        let lines: Vec<&str> = text.lines().collect();
        let mut ranges = vec![];
        for (i, line) in lines.iter().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            let mut end = i;
            for (j, next) in lines.iter().enumerate().skip(i + 1) {
                if next.trim().is_empty() {
                    continue;
                }
                if next.len() - next.trim_start().len() <= indent {
                    break;
                }
                end = j;
            }
            if end > i {
                ranges.push(FoldingRange {
                    start_line: i as u32,
                    end_line: end as u32,
                    start_character: None,
                    end_character: None,
                    kind: Some(FoldingRangeKind::Region),
                    collapsed_text: None,
                });
            }
        }
        Some(ranges)
    }
}

fn is_script(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()),
        Some("rpy" | "rpym")
    )
}

fn word_boundary_regex(word: &str) -> Option<Regex> {
    // Rust regex deliberately does not implement lookaround.
    Regex::new(&format!(r"\b{}\b", regex::escape(word))).ok()
}

fn identifier_ranges(line: &str, row: u32, re: &Regex) -> Vec<Range> {
    re.find_iter(line)
        .filter_map(|m| {
            let start = text_utils::byte_to_utf16(line, m.start());
            if !text_utils::position_is_clean(line, start) {
                return None;
            }
            Some(Range {
                start: Position::new(row, start as u32),
                end: Position::new(row, text_utils::byte_to_utf16(line, m.end()) as u32),
            })
        })
        .collect()
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        self.watch_files.store(
            params
                .capabilities
                .workspace
                .as_ref()
                .and_then(|w| w.did_change_watched_files.as_ref())
                .and_then(|w| w.dynamic_registration)
                .unwrap_or(false),
            Ordering::Relaxed,
        );
        let roots: Vec<Url> = if let Some(folders) = params.workspace_folders {
            folders.into_iter().map(|f| f.uri).collect()
        } else {
            #[allow(deprecated)]
            params.root_uri.into_iter().collect()
        };
        for root in roots {
            if let Ok(path) = root.to_file_path() {
                self.scan_workspace(&path);
            }
        }
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                position_encoding: Some(PositionEncodingKind::UTF16),
                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        ..Default::default()
                    },
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![
                        ".".into(),
                        " ".into(),
                        "@".into(),
                        "-".into(),
                        "(".into(),
                    ]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into(), "=".into()]),
                    retrigger_characters: None,
                    work_done_progress_options: Default::default(),
                }),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                document_symbol_provider: Some(OneOf::Left(true)),
                color_provider: Some(ColorProviderCapability::Simple(true)),
                folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),
                document_highlight_provider: Some(OneOf::Left(true)),
                // Semantic tokens are a stub; do not advertise them over working Tree-sitter colors.
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "renpy-language-server".into(),
                version: Some(env!("CARGO_PKG_VERSION").into()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        if self.watch_files.load(Ordering::Relaxed) {
            let registration = Registration {
                id: "renpy-script-watch".into(),
                method: "workspace/didChangeWatchedFiles".into(),
                register_options: Some(serde_json::json!({
                    "watchers": [{"globPattern": "**/*.rpy"}, {"globPattern": "**/*.rpym"}]
                })),
            };
            if let Err(error) = self.client.register_capability(vec![registration]).await {
                tracing::warn!("File watching unavailable: {error}");
            }
        }
        self.client
            .log_message(MessageType::INFO, "Ren'Py language server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        self.shutdown_requested.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let td = params.text_document;
        self.open_files.insert(td.uri.clone(), ());
        self.on_document_change(td.uri, td.text, td.version).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if let Some(change) = params.content_changes.into_iter().last() {
            self.on_document_change(
                params.text_document.uri,
                change.text,
                params.text_document.version,
            )
            .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        self.open_files.remove(&uri);
        self.load_disk_document(&uri);
        self.client.publish_diagnostics(uri, vec![], None).await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = self.documents.get(&uri).map(|doc| doc.version).unwrap_or(0);
        if let Some(text) = params.text.or_else(|| self.documents.read_text(&uri)) {
            self.on_document_change(uri, text, version).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        for change in params.changes {
            if !self.open_files.contains_key(&change.uri)
                && change.uri.to_file_path().is_ok_and(|path| is_script(&path))
            {
                self.load_disk_document(&change.uri);
            }
        }
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let response = features::completion::compute(&uri, pos, &self.documents, &self.game_data);
        let mut items = match response {
            Some(CompletionResponse::Array(items)) => items,
            Some(CompletionResponse::List(list)) => list.items,
            None => vec![],
        };
        if let Some(doc) = self.documents.get(&uri) {
            if let Some(line) = doc.rope.get_line(pos.line as usize) {
                let line = line.to_string();
                if text_utils::position_is_clean(&line, pos.character as usize) {
                    let prefix = text_utils::keyword_prefix(&line, pos.character as usize)
                        .unwrap_or_default();
                    for entry in self.index.iter() {
                        if entry.key().starts_with(&prefix) {
                            items.push(CompletionItem {
                                label: entry.key().clone(),
                                kind: Some(CompletionItemKind::REFERENCE),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
        items.sort_by(|a, b| a.label.cmp(&b.label));
        items.dedup_by(|a, b| a.label == b.label);
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let p = params.text_document_position_params;
        Ok(features::hover::compute(
            &p.text_document.uri,
            p.position,
            &self.documents,
            &self.game_data,
        ))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let p = params.text_document_position_params;
        Ok(features::signature::compute(
            &p.text_document.uri,
            p.position,
            &self.documents,
            &self.game_data,
        ))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let p = params.text_document_position_params;
        Ok(features::definition::compute(
            &p.text_document.uri,
            p.position,
            &self.documents,
            &self.index,
        ))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let p = params.text_document_position;
        Ok(self.compute_references(
            &p.text_document.uri,
            p.position,
            params.context.include_declaration,
        ))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        Ok(features::symbols::compute(
            &params.text_document.uri,
            &self.documents,
        ))
    }

    async fn document_color(&self, params: DocumentColorParams) -> Result<Vec<ColorInformation>> {
        Ok(features::color::compute(
            &params.text_document.uri,
            &self.documents,
        ))
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
        let p = params.text_document_position_params;
        Ok(self.compute_document_highlight(&p.text_document.uri, p.position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_regex_works_and_excludes_strings_and_comments() {
        let re = word_boundary_regex("score").unwrap();
        let line = "$ text = \"😀 score\"; score += 1 # score";
        let ranges = identifier_ranges(line, 0, &re);
        assert_eq!(ranges.len(), 1);
        let byte = line.find("score +=").unwrap();
        assert_eq!(
            ranges[0].start.character as usize,
            text_utils::byte_to_utf16(line, byte)
        );
        assert!(identifier_ranges("score_extra", 0, &re).is_empty());
    }
}
