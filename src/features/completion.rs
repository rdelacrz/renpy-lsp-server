//! LSP `textDocument/completion` implementation.
//!
//! Provides context-sensitive completions: `renpy.*` / `config.*` API members from game data,
//! auto-complete suffixes keyed by the first word on the line, Ren'Py keywords, and screen
//! displayable names.

use tower_lsp::lsp_types::*;

use crate::data::keywords;
use crate::data::loader::GameData;
use crate::document::DocumentStore;
use crate::parsing::text_utils;

/// Compute completion items for the given position.
pub fn compute(
    uri: &Url,
    position: Position,
    documents: &DocumentStore,
    game_data: &GameData,
) -> Option<CompletionResponse> {
    let doc = documents.get(uri)?;
    let line_text = doc.rope.get_line(position.line as usize)?.to_string();

    // Don't complete inside comments or strings
    if !text_utils::position_is_clean(&line_text, position.character as usize) {
        return None;
    }

    let prefix =
        text_utils::keyword_prefix(&line_text, position.character as usize).unwrap_or_default();

    let mut items: Vec<CompletionItem> = Vec::new();

    // Return only the suffix after the last dot; inserting `music.play` after
    // `renpy.music.` would otherwise duplicate the namespace in Zed.
    let namespace_end = prefix.rfind('.').map_or(0, |index| index + 1);
    let namespace = &prefix[..namespace_end];
    let filter = &prefix[namespace_end..];
    if prefix.starts_with("renpy.") || prefix.starts_with("config.") {
        let entries = if prefix.starts_with("renpy.") {
            &game_data.renpy
        } else {
            &game_data.config
        };
        for (key, entry) in entries {
            if let Some(short) = key.strip_prefix(namespace) {
                if short.starts_with(filter) {
                    items.push(make_item(
                        short,
                        entry.kind.as_deref(),
                        entry.docstring.as_deref(),
                    ));
                }
            }
        }
        return Some(CompletionResponse::Array(items));
    }

    // Auto-complete keywords (context from line)
    let trimmed = line_text.trim_start();
    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    if let Some(opts) = game_data.auto_complete.get(first_word) {
        for opt in opts.split('|') {
            if !opt.starts_with('{') {
                items.push(CompletionItem {
                    label: opt.to_string(),
                    kind: Some(CompletionItemKind::VALUE),
                    ..Default::default()
                });
            }
        }
    }

    // Ren'Py keywords
    for kw in keywords::renpy_keywords() {
        if prefix.is_empty() || kw.starts_with(&prefix) {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }
    }

    // Screen displayables
    for d in keywords::screen_displayables() {
        if prefix.is_empty() || d.starts_with(&prefix) {
            items.push(CompletionItem {
                label: d.to_string(),
                kind: Some(CompletionItemKind::CLASS),
                ..Default::default()
            });
        }
    }

    if items.is_empty() {
        None
    } else {
        Some(CompletionResponse::Array(items))
    }
}

/// Build a [`CompletionItem`] from a label, optional kind string, and optional documentation.
fn make_item(label: &str, kind: Option<&str>, doc: Option<&str>) -> CompletionItem {
    let item_kind = match kind {
        Some("function") | Some("method") => Some(CompletionItemKind::FUNCTION),
        Some("class") => Some(CompletionItemKind::CLASS),
        Some("var") => Some(CompletionItemKind::VARIABLE),
        _ => Some(CompletionItemKind::PROPERTY),
    };
    CompletionItem {
        label: label.to_string(),
        kind: item_kind,
        documentation: doc.map(|d| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: d.to_string(),
            })
        }),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentState, DocumentStore};

    fn make_store(text: &str) -> (Url, DocumentStore, GameData) {
        let store = DocumentStore::new();
        let uri = Url::parse("file:///test.rpy").unwrap();
        store.upsert(
            uri.clone(),
            DocumentState {
                rope: ropey::Rope::from_str(text),
                version: 1,
                symbols: vec![],
                diagnostics: vec![],
            },
        );
        let gd = crate::data::loader::load_game_data().unwrap();
        (uri, store, gd)
    }

    #[test]
    fn test_keywords_returned() {
        let (uri, store, gd) = make_store("lab\n");
        let result = compute(
            &uri,
            Position {
                line: 0,
                character: 3,
            },
            &store,
            &gd,
        );
        let items = match result.unwrap() {
            CompletionResponse::Array(v) => v,
            _ => panic!("expected array"),
        };
        assert!(items.iter().any(|i| i.label == "label"));
    }

    #[test]
    fn test_no_completion_in_comment() {
        let (uri, store, gd) = make_store("# label\n");
        let result = compute(
            &uri,
            Position {
                line: 0,
                character: 5,
            },
            &store,
            &gd,
        );
        assert!(result.is_none());
    }
}
