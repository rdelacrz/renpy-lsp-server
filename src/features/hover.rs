//! LSP `textDocument/hover` implementation.
//!
//! Looks up the identifier at the cursor in the game data maps (renpy, config, internal)
//! and returns a Markdown hover with the signature and docstring.

use tower_lsp::lsp_types::*;

use crate::data::loader::GameData;
use crate::document::DocumentStore;
use crate::parsing::text_utils;

/// Compute hover information for the given position.
pub fn compute(
    uri: &Url,
    position: Position,
    documents: &DocumentStore,
    game_data: &GameData,
) -> Option<Hover> {
    let doc = documents.get(uri)?;
    let line_text = doc.rope.get_line(position.line as usize)?.to_string();

    if !text_utils::position_is_clean(&line_text, position.character as usize) {
        return None;
    }

    let word = text_utils::identifier_at_position(&line_text, position.character as usize)?;

    // Look up in game data (renpy, config, internal)
    let entry = game_data
        .renpy
        .get(&word)
        .or_else(|| game_data.config.get(&word))
        .or_else(|| game_data.internal.get(&word));

    if let Some(e) = entry {
        let mut content = format!("**{}**", word);
        if let Some(ref args) = e.args {
            content.push_str(&format!(" `{}`", args));
        }
        if let Some(ref doc) = e.docstring {
            content.push_str("\n\n");
            content.push_str(doc.trim());
        }
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: content,
            }),
            range: None,
        });
    }

    None
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
    fn test_hover_known_symbol() {
        let (uri, store, gd) = make_store("renpy.pause\n");
        // cursor at end of "renpy.pause" (col 11) so keyword_prefix resolves the full dotted name
        let result = compute(
            &uri,
            Position {
                line: 0,
                character: 11,
            },
            &store,
            &gd,
        );
        assert!(result.is_some());
    }

    #[test]
    fn test_hover_in_comment_returns_none() {
        let (uri, store, gd) = make_store("# renpy.pause\n");
        let result = compute(
            &uri,
            Position {
                line: 0,
                character: 8,
            },
            &store,
            &gd,
        );
        assert!(result.is_none());
    }
}
