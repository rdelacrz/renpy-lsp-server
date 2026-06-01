//! Line-by-line top-level symbol extraction for Ren'Py source.
//!
//! Parses `label`, `screen`, `transform`, `style`, `image`, `define`, and
//! `init python` statements that appear at zero indentation. Everything else
//! (indented lines, comments, blank lines) is skipped.
//!
//! Symbols are stored with their line number instead of raw byte offsets
//! to keep LSP compatibility simple. Callers that need full
//! `Position`/`Range` values can reconstruct them from the line number.

use std::fmt;

/// Lightweight error produced during analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisError {
    message: String,
}

impl AnalysisError {
    /// Create an error from any string-like value.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Return the error message.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AnalysisError {}

/// Class of top-level Ren'Py statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SymbolKind {
    Label,
    Screen,
    Function,
    Define,
    Image,
    Transform,
    Style,
}

/// A named top-level symbol with its kind and 0-based line number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub kind: SymbolKind,
    pub name: String,
    pub line: usize,
}

/// Result of a single-file analysis pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisModel {
    pub symbols: Vec<Symbol>,
}

/// Parse *source* and extract every top-level symbol.
///
/// Only lines with zero indentation are examined; indented lines, comments,
/// and blank lines are ignored.
pub fn analyze(source: &str) -> Result<AnalysisModel, AnalysisError> {
    let mut symbols = Vec::new();

    for (lineno, line) in source.lines().enumerate() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.is_empty() || trimmed.starts_with('#') || indent != 0 {
            continue;
        }

        if let Some(name) = parse_keyword(trimmed, "label")? {
            symbols.push(Symbol {
                kind: SymbolKind::Label,
                name,
                line: lineno,
            });
        } else if let Some(name) = parse_screen_or_transform_or_style(trimmed, "screen")? {
            symbols.push(Symbol {
                kind: SymbolKind::Screen,
                name,
                line: lineno,
            });
        } else if let Some(name) = parse_screen_or_transform_or_style(trimmed, "transform")? {
            symbols.push(Symbol {
                kind: SymbolKind::Transform,
                name,
                line: lineno,
            });
        } else if let Some(name) = parse_screen_or_transform_or_style(trimmed, "style")? {
            symbols.push(Symbol {
                kind: SymbolKind::Style,
                name,
                line: lineno,
            });
        } else if let Some(name) = parse_define_line(trimmed, "image")? {
            symbols.push(Symbol {
                kind: SymbolKind::Image,
                name,
                line: lineno,
            });
        } else if let Some(name) = parse_define_line(trimmed, "define")? {
            symbols.push(Symbol {
                kind: SymbolKind::Define,
                name,
                line: lineno,
            });
        } else if let Some(name) = parse_init_python_line(trimmed)? {
            symbols.push(Symbol {
                kind: SymbolKind::Function,
                name,
                line: lineno,
            });
        }
    }

    Ok(AnalysisModel { symbols })
}

// --- private parsers ---------------------------------------------------

fn parse_keyword(line: &str, keyword: &str) -> Result<Option<String>, AnalysisError> {
    if !line.starts_with(keyword) {
        return Ok(None);
    }
    let rest = line[keyword.len()..].trim_start();
    if rest.is_empty() {
        return Ok(None);
    }
    if !rest.ends_with(':') {
        return Err(AnalysisError::new(format!(
            "{} statement must end with ':'",
            keyword
        )));
    }
    let name = rest[..rest.len() - 1].trim();
    if name.is_empty() {
        return Err(AnalysisError::new(format!(
            "{} statement is missing a name",
            keyword
        )));
    }
    validate_ident(name)?;
    Ok(Some(name.to_string()))
}

fn parse_screen_or_transform_or_style(
    line: &str,
    keyword: &str,
) -> Result<Option<String>, AnalysisError> {
    if !line.starts_with(keyword) {
        return Ok(None);
    }
    let rest = line[keyword.len()..].trim_start();
    if rest.is_empty() {
        return Ok(None);
    }
    if !rest.ends_with(':') {
        return Err(AnalysisError::new(format!(
            "{} statement must end with ':'",
            keyword
        )));
    }
    // Strip parenthesized parameter list if present, e.g.
    //   screen my_screen(arg1, arg2=0):
    // Also strip `style is parent` inherit clause for styles.
    let without_parens = rest[..rest.len() - 1].trim();
    let without_inherit = without_parens
        .split_whitespace()
        .take_while(|w| *w != "is")
        .collect::<Vec<_>>()
        .join(" ");
    let name_part = without_inherit
        .split('(')
        .next()
        .unwrap_or(&without_inherit);
    let name = name_part.trim();
    if name.is_empty() {
        return Err(AnalysisError::new(format!(
            "{} statement is missing a name",
            keyword
        )));
    }
    validate_ident(name)?;
    Ok(Some(name.to_string()))
}

fn parse_define_line(line: &str, keyword: &str) -> Result<Option<String>, AnalysisError> {
    if !line.starts_with(keyword) {
        return Ok(None);
    }
    let rest = line[keyword.len()..].trim_start();
    if rest.is_empty() {
        return Ok(None);
    }
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts
        .next()
        .ok_or_else(|| AnalysisError::new("missing define name"))?;
    validate_ident(name)?;
    Ok(Some(name.to_string()))
}

/// Parse `init [offset] python [hide] [:]` and return a readable name
/// such as `"init 5 python"` or `"init python"`.
fn parse_init_python_line(line: &str) -> Result<Option<String>, AnalysisError> {
    if !line.starts_with("init") {
        return Ok(None);
    }
    let after_init = line["init".len()..].trim_start();

    // Skip optional integer priority
    let after_offset = after_init.trim_start_matches(|c: char| c.is_ascii_digit());
    let after_offset = after_offset.trim_start();

    if !after_offset.starts_with("python") {
        return Ok(None);
    }

    let rest = &after_offset["python".len()..];
    let colon_rest = rest
        .strip_prefix(|c: char| c == ' ' || c == '\t')
        .unwrap_or(rest);
    let _colon_rest = colon_rest
        .strip_prefix("hide: ")
        .or_else(|| colon_rest.strip_prefix("hide :"))
        .or_else(|| colon_rest.strip_prefix("hide:"))
        .or_else(|| colon_rest.strip_prefix(':'))
        .map(|r| r.trim())
        .filter(|r| r.is_empty() || r.starts_with('#'))
        .ok_or_else(|| {
            AnalysisError::new(
                "init python statement must end with ':', 'hide:', or 'hide :' \
                 followed by optional whitespace/comment",
            )
        })?;

    // Build a readable name from the init offset if present.
    let offset_num = after_offset
        .trim_start_matches(|c: char| c.is_ascii_digit() || c == ' ' || c == '\t')
        .trim();
    if offset_num.is_empty() {
        Ok(Some("init python".to_string()))
    } else {
        Ok(Some(format!("init {} python", offset_num)))
    }
}

/// Validate that `ident` looks like a reasonable Ren'Py identifier.
///
/// Currently only accepts ASCII alphabetic / alphanumeric + underscore
/// identifiers.  Real Ren'Py allows Unicode identifier characters; this
/// restriction can be lifted by switching from `is_ascii_alphabetic` to
/// `is_alphabetic` etc. when needed.
fn validate_ident(ident: &str) -> Result<(), AnalysisError> {
    let mut chars = ident.chars();
    let first = chars
        .next()
        .ok_or_else(|| AnalysisError::new("empty identifier"))?;
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(AnalysisError::new(format!("invalid identifier: {ident}")));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(AnalysisError::new(format!("invalid identifier: {ident}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_all_kinds() {
        let source = r#"label start:
    scene bg room
screen main(a, b=2):
transform shake(x, y):
style my_style is default:
image eileen = "eileen.png"
define e = Character("Eileen")
init python:
    pass
init 5 python hide:
    pass
"#;
        let model = analyze(source).unwrap();
        let kinds: Vec<SymbolKind> = model.symbols.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SymbolKind::Label));
        assert!(kinds.contains(&SymbolKind::Screen));
        assert!(kinds.contains(&SymbolKind::Transform));
        assert!(kinds.contains(&SymbolKind::Style));
        assert!(kinds.contains(&SymbolKind::Image));
        assert!(kinds.contains(&SymbolKind::Define));
        assert!(kinds.contains(&SymbolKind::Function));
        // init python appears twice
        let funcs: Vec<_> = model
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .collect();
        assert_eq!(funcs.len(), 2);
    }

    #[test]
    fn handles_crlf() {
        let source = "label start:\r\n    \"hello\"\r\nscreen main:\r\n";
        let model = analyze(source).unwrap();
        assert_eq!(model.symbols.len(), 2);
        assert_eq!(model.symbols[0].line, 0);
        assert_eq!(model.symbols[1].line, 2);
    }

    #[test]
    fn rejects_invalid_label_name() {
        let err = analyze("label 123bad:\n").unwrap_err();
        assert!(err.to_string().contains("invalid identifier"));
    }

    #[test]
    fn rejects_missing_colon() {
        let err = analyze("screen main\n").unwrap_err();
        assert!(err.to_string().contains("must end with ':'"));
    }

    #[test]
    fn ignores_empty_and_comments() {
        let source = "\n  # a comment\n   \nlabel start:\n";
        let model = analyze(source).unwrap();
        assert_eq!(model.symbols.len(), 1);
        assert_eq!(model.symbols[0].kind, SymbolKind::Label);
    }
}
