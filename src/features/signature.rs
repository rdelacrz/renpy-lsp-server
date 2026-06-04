//! LSP `textDocument/signatureHelp` implementation.
//!
//! Identifies the function call surrounding the cursor by finding the nearest open parenthesis,
//! resolves the function name in game data, and returns its parameter signature with the
//! active parameter index based on comma count.

use tower_lsp::lsp_types::*;

use crate::data::loader::GameData;
use crate::document::DocumentStore;
use crate::parsing::text_utils;

/// Compute signature help for the given position.
pub fn compute(
    uri: &Url,
    position: Position,
    documents: &DocumentStore,
    game_data: &GameData,
) -> Option<SignatureHelp> {
    let doc = documents.get(uri)?;
    let line_text = doc.rope.get_line(position.line as usize)?.to_string();
    let col = position.character as usize;

    // Convert UTF-16 column to a byte offset for safe slicing
    let byte_col = text_utils::utf16_to_byte(&line_text, col);
    let prefix = &line_text[..byte_col];
    let paren_pos = prefix.rfind('(')?;
    let before_paren = &prefix[..paren_pos];
    let func_name = text_utils::keyword_prefix(before_paren, usize::MAX)?;

    // Look up in game_data
    let entry = game_data.renpy.get(&func_name)
        .or_else(|| game_data.config.get(&func_name))
        .or_else(|| game_data.internal.get(&func_name));

    let e = entry?;
    let args = e.args.as_deref()?;

    // Count commas to determine active parameter
    let after_paren = &prefix[paren_pos + 1..];
    let active_param = after_paren.chars().filter(|&c| c == ',').count() as u32;

    let params: Vec<ParameterInformation> = args
        .trim_start_matches('(')
        .trim_end_matches(')')
        .split(',')
        .map(|p| ParameterInformation {
            label: ParameterLabel::Simple(p.trim().to_string()),
            documentation: None,
        })
        .collect();

    Some(SignatureHelp {
        signatures: vec![SignatureInformation {
            label: format!("{}{}", func_name, args),
            documentation: e.docstring.as_ref().map(|d| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: d.trim().to_string(),
                })
            }),
            parameters: Some(params),
            active_parameter: Some(active_param),
        }],
        active_signature: Some(0),
        active_parameter: Some(active_param),
    })
}
