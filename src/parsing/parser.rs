//! Regex-based Ren'Py document parser.
//!
//! Extracts symbols (labels, screens, defines, images, transforms, etc.) and references
//! (jump, call, show, use) from Ren'Py source text. Symbols are organized into a nested
//! tree based on indentation level.

use crate::parsing::text_utils;
use regex::Regex;
use std::sync::OnceLock;
use tower_lsp::lsp_types::*;

/// A parsed symbol extracted from a Ren'Py document.
#[derive(Debug, Clone)]
pub struct ParsedSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub range: Range,
    pub selection_range: Range,
    pub children: Vec<ParsedSymbol>,
}

/// A reference usage found in a Ren'Py document (jump, call, show, use, etc.).
#[derive(Debug, Clone)]
pub struct ParsedReference {
    pub name: String,
    pub kind: ReferenceKind,
    pub range: Range,
}

/// The kind of reference target being referenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReferenceKind {
    /// A label reference (jump/call target).
    Label,
    /// An image name reference (show/scene/hide).
    Image,
    /// A screen name reference (call screen/show screen/use).
    Screen,
    /// A variable reference.
    Variable,
}

/// Intermediate representation of a regex-matched symbol before tree construction.
struct SymbolMatch {
    name: String,
    kind: SymbolKind,
    indent: usize,
    line: u32,
    name_start: u32,
    name_end: u32,
}

/// Parse a Ren'Py document, returning symbols and references.
pub fn parse(_uri: &Url, text: &str) -> (Vec<ParsedSymbol>, Vec<ParsedReference>) {
    let lines: Vec<&str> = text.lines().collect();
    let matches = extract_symbol_matches(&lines);
    let references = extract_references(&lines);
    let symbols = build_symbol_tree(&lines, matches);
    (symbols, references)
}

/// Compiled regexes for matching Ren'Py symbol declarations.
struct SymbolRegexes {
    label: Regex,
    screen: Regex,
    transform: Regex,
    image: Regex,
    define: Regex,
    default: Regex,
    style: Regex,
    def: Regex,
    class: Regex,
}

/// Lazily-initialized symbol declaration regexes (singleton).
fn symbol_regexes() -> &'static SymbolRegexes {
    static REGEXES: OnceLock<SymbolRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| SymbolRegexes {
        label: Regex::new(r"^(\s*)label\s+([\w.]+)").unwrap(),
        screen: Regex::new(r"^(\s*)screen\s+(\w+)").unwrap(),
        transform: Regex::new(r"^(\s*)transform\s+(?:\d+\s+)?([\w.]+)").unwrap(),
        image: Regex::new(r"^(\s*)image\s+([\w\s]+?)\s*[=:]").unwrap(),
        define: Regex::new(r"^(\s*)define\s+(?:\d+\s+)?([\w.]+)").unwrap(),
        default: Regex::new(r"^(\s*)default\s+(?:\d+\s+)?([\w.]+)").unwrap(),
        style: Regex::new(r"^(\s*)style\s+(\w+)").unwrap(),
        def: Regex::new(r"^(\s*)def\s+(\w+)").unwrap(),
        class: Regex::new(r"^(\s*)class\s+(\w+)").unwrap(),
    })
}

/// Scan all lines and collect symbol matches (label, screen, define, etc.).
fn extract_symbol_matches(lines: &[&str]) -> Vec<SymbolMatch> {
    let re = symbol_regexes();
    let checks: &[(&Regex, SymbolKind)] = &[
        (&re.label, SymbolKind::FUNCTION),
        (&re.screen, SymbolKind::CLASS),
        (&re.transform, SymbolKind::FUNCTION),
        (&re.image, SymbolKind::VARIABLE),
        (&re.define, SymbolKind::CONSTANT),
        (&re.default, SymbolKind::VARIABLE),
        (&re.style, SymbolKind::CLASS),
        (&re.def, SymbolKind::FUNCTION),
        (&re.class, SymbolKind::CLASS),
    ];

    let mut results = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        for (regex, kind) in checks {
            if let Some(caps) = regex.captures(line) {
                let indent = caps.get(1).map_or(0, |m| m.as_str().len());
                let name_match = caps.get(2).unwrap();
                results.push(SymbolMatch {
                    name: name_match.as_str().trim().to_string(),
                    kind: *kind,
                    indent,
                    line: i as u32,
                    name_start: text_utils::byte_to_utf16(line, name_match.start()) as u32,
                    name_end: text_utils::byte_to_utf16(line, name_match.end()) as u32,
                });
                break;
            }
        }
    }
    results
}

/// Compiled regexes for matching reference usages (jump, call, show, etc.).
struct RefRegexes {
    jump: Regex,
    call_screen: Regex,
    call: Regex,
    show_screen: Regex,
    show: Regex,
    scene: Regex,
    hide: Regex,
    use_: Regex,
}

/// Lazily-initialized reference regexes (singleton).
fn ref_regexes() -> &'static RefRegexes {
    static REGEXES: OnceLock<RefRegexes> = OnceLock::new();
    REGEXES.get_or_init(|| RefRegexes {
        jump: Regex::new(r"^\s*jump\s+([\w.]+)").unwrap(),
        call_screen: Regex::new(r"^\s*call\s+screen\s+(\w+)").unwrap(),
        call: Regex::new(r"^\s*call\s+([\w.]+)").unwrap(),
        show_screen: Regex::new(r"^\s*show\s+screen\s+(\w+)").unwrap(),
        show: Regex::new(r"^\s*show\s+([\w]+(?:\s+[\w]+)*)").unwrap(),
        scene: Regex::new(r"^\s*scene\s+([\w]+(?:\s+[\w]+)*)").unwrap(),
        hide: Regex::new(r"^\s*hide\s+([\w]+(?:\s+[\w]+)*)").unwrap(),
        use_: Regex::new(r"^\s*use\s+(\w+)").unwrap(),
    })
}

/// Scan all lines and extract reference usages (jump, call, show, scene, hide, use).
fn extract_references(lines: &[&str]) -> Vec<ParsedReference> {
    let re = ref_regexes();
    let mut refs = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        let line_num = i as u32;

        // Handle "call screen X" before generic "call X"
        if trimmed.starts_with("call screen ") || trimmed.starts_with("call screen\t") {
            if let Some(caps) = re.call_screen.captures(line) {
                push_ref(&mut refs, &caps, line_num, ReferenceKind::Screen, false);
            }
            continue;
        }

        // Handle "show screen X" before generic "show X"
        if trimmed.starts_with("show screen ") || trimmed.starts_with("show screen\t") {
            if let Some(caps) = re.show_screen.captures(line) {
                push_ref(&mut refs, &caps, line_num, ReferenceKind::Screen, false);
            }
            continue;
        }

        let checks: &[(&Regex, ReferenceKind, bool)] = &[
            (&re.jump, ReferenceKind::Label, false),
            (&re.call, ReferenceKind::Label, false),
            (&re.show, ReferenceKind::Image, true),
            (&re.scene, ReferenceKind::Image, true),
            (&re.hide, ReferenceKind::Image, true),
            (&re.use_, ReferenceKind::Screen, false),
        ];

        for (regex, kind, is_image) in checks {
            if let Some(caps) = regex.captures(line) {
                push_ref(&mut refs, &caps, line_num, kind.clone(), *is_image);
                break;
            }
        }
    }
    refs
}

/// Helper to construct a `ParsedReference` from a regex capture and push it onto `refs`.
fn push_ref(
    refs: &mut Vec<ParsedReference>,
    caps: &regex::Captures,
    line: u32,
    kind: ReferenceKind,
    is_image: bool,
) {
    let m = caps.get(1).unwrap();
    let name = m.as_str().trim().to_string();
    let name = if is_image {
        filter_image_modifiers(&name)
    } else {
        name
    };
    if !name.is_empty() {
        refs.push(ParsedReference {
            name,
            kind,
            range: Range {
                start: Position {
                    line,
                    character: m.start() as u32,
                },
                end: Position {
                    line,
                    character: m.end() as u32,
                },
            },
        });
    }
}

/// Strip ATL/display modifiers (at, as, with, behind, etc.) from an image name.
fn filter_image_modifiers(name: &str) -> String {
    const MODIFIERS: &[&str] = &["at", "as", "with", "behind", "onlayer", "zorder"];
    name.split_whitespace()
        .take_while(|p| !MODIFIERS.contains(p))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Find the end line of a block starting at `start_line` with the given indent.
fn find_block_end(lines: &[&str], start_line: u32, indent: usize) -> u32 {
    let total_lines = lines.len() as u32;
    let mut block_end = start_line;
    for l in (start_line + 1)..total_lines {
        let line_text = lines[l as usize];
        if line_text.trim().is_empty() {
            continue;
        }
        let line_indent = line_text.len() - line_text.trim_start().len();
        if line_indent <= indent {
            break;
        }
        block_end = l;
    }
    block_end
}

/// Build a flat list of `ParsedSymbol`s from matches, compute block ranges, then nest by indent.
fn build_symbol_tree(lines: &[&str], matches: Vec<SymbolMatch>) -> Vec<ParsedSymbol> {
    let mut symbols: Vec<ParsedSymbol> = Vec::new();

    for (idx, m) in matches.iter().enumerate() {
        // Determine block end: find last child symbol, then walk forward to find actual block end
        let last_child_line = matches[idx + 1..]
            .iter()
            .take_while(|next| next.indent > m.indent)
            .last()
            .map_or(m.line, |next| next.line);

        let end_line = find_block_end(lines, last_child_line.max(m.line), m.indent);
        let end_char = lines
            .get(end_line as usize)
            .map_or(0, |l| l.encode_utf16().count() as u32);

        symbols.push(ParsedSymbol {
            name: m.name.clone(),
            kind: m.kind,
            range: Range {
                start: Position {
                    line: m.line,
                    character: 0,
                },
                end: Position {
                    line: end_line,
                    character: end_char,
                },
            },
            selection_range: Range {
                start: Position {
                    line: m.line,
                    character: m.name_start,
                },
                end: Position {
                    line: m.line,
                    character: m.name_end,
                },
            },
            children: Vec::new(),
        });
    }

    nest_symbols(&matches, &mut symbols);

    // Return only top-level symbols
    let top_indent = matches.iter().map(|m| m.indent).min().unwrap_or(0);
    matches
        .iter()
        .enumerate()
        .filter(|(_, m)| m.indent == top_indent)
        .map(|(i, _)| symbols[i].clone())
        .collect()
}

/// Assign children to parent symbols based on indentation depth (bottom-up construction).
fn nest_symbols(matches: &[SymbolMatch], symbols: &mut [ParsedSymbol]) {
    let n = matches.len();
    let mut children_map: Vec<Vec<usize>> = vec![Vec::new(); n];

    for i in 1..n {
        for j in (0..i).rev() {
            if matches[j].indent < matches[i].indent {
                children_map[j].push(i);
                break;
            }
        }
    }

    // Build bottom-up to ensure children are fully populated
    for i in (0..n).rev() {
        let children: Vec<ParsedSymbol> = children_map[i]
            .iter()
            .map(|&ci| symbols[ci].clone())
            .collect();
        symbols[i].children = children;
    }
}

/// Convert parsed symbols into LSP DocumentSymbol objects.
pub fn to_document_symbols(symbols: &[ParsedSymbol]) -> Vec<DocumentSymbol> {
    symbols
        .iter()
        .map(|s| {
            #[allow(deprecated)]
            DocumentSymbol {
                name: s.name.clone(),
                detail: None,
                kind: s.kind,
                tags: None,
                deprecated: None,
                range: s.range,
                selection_range: s.selection_range,
                children: if s.children.is_empty() {
                    None
                } else {
                    Some(to_document_symbols(&s.children))
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_url() -> Url {
        Url::parse("file:///test.rpy").unwrap()
    }

    #[test]
    fn test_parse_label() {
        let text = "label start:\n    \"Hello\"\n    return\n";
        let (symbols, _) = parse(&test_url(), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "start");
        assert_eq!(symbols[0].kind, SymbolKind::FUNCTION);
    }

    #[test]
    fn test_parse_screen() {
        let text = "screen main_menu():\n    vbox:\n        textbutton \"Start\"\n";
        let (symbols, _) = parse(&test_url(), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "main_menu");
        assert_eq!(symbols[0].kind, SymbolKind::CLASS);
    }

    #[test]
    fn test_parse_define_default() {
        let text = "define e = Character(\"Eileen\")\ndefault score = 0\n";
        let (symbols, _) = parse(&test_url(), text);
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].name, "e");
        assert_eq!(symbols[0].kind, SymbolKind::CONSTANT);
        assert_eq!(symbols[1].name, "score");
        assert_eq!(symbols[1].kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn test_parse_references_jump_call() {
        let text = "label start:\n    jump chapter1\n    call helper\n";
        let (_, refs) = parse(&test_url(), text);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "chapter1");
        assert_eq!(refs[0].kind, ReferenceKind::Label);
        assert_eq!(refs[1].name, "helper");
        assert_eq!(refs[1].kind, ReferenceKind::Label);
    }

    #[test]
    fn test_parse_references_show() {
        let text = "    show eileen happy at left\n";
        let (_, refs) = parse(&test_url(), text);
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].name, "eileen happy");
        assert_eq!(refs[0].kind, ReferenceKind::Image);
    }

    #[test]
    fn test_parse_references_use_screen() {
        let text = "    use navigation\n    call screen preferences\n";
        let (_, refs) = parse(&test_url(), text);
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].name, "navigation");
        assert_eq!(refs[0].kind, ReferenceKind::Screen);
        assert_eq!(refs[1].name, "preferences");
        assert_eq!(refs[1].kind, ReferenceKind::Screen);
    }

    #[test]
    fn test_parse_nested_symbols() {
        let text = "init python:\n    def helper():\n        pass\n    class Foo:\n        pass\n";
        let (symbols, _) = parse(&test_url(), text);
        // def and class are inside init python block but since init doesn't match our regex,
        // they appear at indent 4
        assert!(symbols.len() >= 2);
    }

    #[test]
    fn test_parse_image() {
        let text = "image eileen happy = \"eileen_happy.png\"\n";
        let (symbols, _) = parse(&test_url(), text);
        assert_eq!(symbols.len(), 1);
        assert_eq!(symbols[0].name, "eileen happy");
        assert_eq!(symbols[0].kind, SymbolKind::VARIABLE);
    }

    #[test]
    fn test_to_document_symbols() {
        let symbols = vec![ParsedSymbol {
            name: "test".into(),
            kind: SymbolKind::FUNCTION,
            range: Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 2,
                    character: 10,
                },
            },
            selection_range: Range {
                start: Position {
                    line: 0,
                    character: 6,
                },
                end: Position {
                    line: 0,
                    character: 10,
                },
            },
            children: vec![],
        }];
        let doc_symbols = to_document_symbols(&symbols);
        assert_eq!(doc_symbols.len(), 1);
        assert_eq!(doc_symbols[0].name, "test");
    }
}
