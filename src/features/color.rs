//! LSP `textDocument/documentColor` and `textDocument/colorPresentation` implementation.
//!
//! Detects color literals in Ren'Py source: hex strings (`"#rrggbb"`), `Color((r, g, b))` tuples,
//! `rgb = (r, g, b)` float triples, and `color = (r, g, b)` integer tuples.

use regex::Regex;
use std::sync::OnceLock;
use tower_lsp::lsp_types::*;

use crate::document::DocumentStore;

/// Regex for hex color strings like `"#rrggbb"` or `'#rgba'`.
fn re_hex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"["']#([0-9a-fA-F]{3,8})["']"#).unwrap())
}

/// Regex for `Color((r, g, b))` or `Color((r, g, b, a))` with integer 0–255 components.
fn re_color_tuple() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"Color\(\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*(\d+))?\s*\)\s*\)").unwrap())
}

/// Regex for `rgb = (r, g, b)` with float 0.0–1.0 components.
fn re_rgb() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"rgb\s*=\s*\(\s*([0-9.]+)\s*,\s*([0-9.]+)\s*,\s*([0-9.]+)\s*\)").unwrap())
}

/// Regex for `color = (r, g, b)` or `color = (r, g, b, a)` with integer 0–255 components.
fn re_color_eq() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"color\s*=\s*\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*(\d+))?\s*\)").unwrap())
}

/// Compute document colors for the given document.
pub fn compute(uri: &Url, documents: &DocumentStore) -> Vec<ColorInformation> {
    let text = match documents.read_text(uri) {
        Some(t) => t,
        None => return Vec::new(),
    };

    let mut colors = Vec::new();

    for (line_idx, line) in text.lines().enumerate() {
        let line_num = line_idx as u32;

        for cap in re_hex().captures_iter(line) {
            let full = cap.get(0).unwrap();
            let hex = cap.get(1).unwrap().as_str();
            if let Some(color) = parse_hex(hex) {
                colors.push(ColorInformation {
                    range: line_range(line_num, full.start() as u32, full.end() as u32),
                    color,
                });
            }
        }

        for cap in re_color_tuple().captures_iter(line) {
            let full = cap.get(0).unwrap();
            let r = cap[1].parse::<f32>().unwrap_or(0.0) / 255.0;
            let g = cap[2].parse::<f32>().unwrap_or(0.0) / 255.0;
            let b = cap[3].parse::<f32>().unwrap_or(0.0) / 255.0;
            let a = cap.get(4).map_or(1.0, |m| m.as_str().parse::<f32>().unwrap_or(255.0) / 255.0);
            colors.push(ColorInformation {
                range: line_range(line_num, full.start() as u32, full.end() as u32),
                color: Color { red: r, green: g, blue: b, alpha: a },
            });
        }

        for cap in re_rgb().captures_iter(line) {
            let full = cap.get(0).unwrap();
            let r = cap[1].parse::<f32>().unwrap_or(0.0);
            let g = cap[2].parse::<f32>().unwrap_or(0.0);
            let b = cap[3].parse::<f32>().unwrap_or(0.0);
            colors.push(ColorInformation {
                range: line_range(line_num, full.start() as u32, full.end() as u32),
                color: Color { red: r, green: g, blue: b, alpha: 1.0 },
            });
        }

        for cap in re_color_eq().captures_iter(line) {
            let full = cap.get(0).unwrap();
            let r = cap[1].parse::<f32>().unwrap_or(0.0) / 255.0;
            let g = cap[2].parse::<f32>().unwrap_or(0.0) / 255.0;
            let b = cap[3].parse::<f32>().unwrap_or(0.0) / 255.0;
            let a = cap.get(4).map_or(1.0, |m| m.as_str().parse::<f32>().unwrap_or(255.0) / 255.0);
            colors.push(ColorInformation {
                range: line_range(line_num, full.start() as u32, full.end() as u32),
                color: Color { red: r, green: g, blue: b, alpha: a },
            });
        }
    }

    colors
}

/// Construct a single-line LSP range from start/end character offsets.
fn line_range(line: u32, start: u32, end: u32) -> Range {
    Range {
        start: Position { line, character: start },
        end: Position { line, character: end },
    }
}

/// Parse a hex string (3, 4, 6, or 8 hex digits) into an LSP `Color` with 0.0–1.0 components.
fn parse_hex(hex: &str) -> Option<Color> {
    let (r, g, b, a) = match hex.len() {
        3 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            (r, g, b, 255u8)
        }
        4 => {
            let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
            let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
            let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
            let a = u8::from_str_radix(&hex[3..4].repeat(2), 16).ok()?;
            (r, g, b, a)
        }
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            (r, g, b, 255u8)
        }
        8 => {
            let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
            let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
            let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
            let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color {
        red: r as f32 / 255.0,
        green: g as f32 / 255.0,
        blue: b as f32 / 255.0,
        alpha: a as f32 / 255.0,
    })
}

/// Compute color presentations for a given color.
pub fn presentations(color: Color, range: Range) -> Vec<ColorPresentation> {
    let r = (color.red * 255.0).round() as u8;
    let g = (color.green * 255.0).round() as u8;
    let b = (color.blue * 255.0).round() as u8;
    let a = (color.alpha * 255.0).round() as u8;

    let hex6 = format!("\"#{:02x}{:02x}{:02x}\"", r, g, b);
    let tuple = format!("Color(({}, {}, {}, {}))", r, g, b, a);

    let mut presentations = vec![
        make_presentation(hex6, range),
        make_presentation(tuple, range),
    ];

    // Insert hex8 with alpha before the tuple when alpha != 255
    if a != 255 {
        let hex8 = format!("\"#{:02x}{:02x}{:02x}{:02x}\"", r, g, b, a);
        presentations.insert(1, make_presentation(hex8, range));
    }

    presentations
}

/// Build a [`ColorPresentation`] with a text edit that replaces `range` with `label`.
fn make_presentation(label: String, range: Range) -> ColorPresentation {
    ColorPresentation {
        text_edit: Some(TextEdit { range, new_text: label.clone() }),
        label,
        additional_text_edits: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_6() {
        let c = parse_hex("ff0000").unwrap();
        assert!((c.red - 1.0).abs() < 0.01);
        assert!(c.green.abs() < 0.01);
        assert!(c.blue.abs() < 0.01);
    }

    #[test]
    fn test_parse_hex_3() {
        let c = parse_hex("f00").unwrap();
        assert!((c.red - 1.0).abs() < 0.01);
        assert!(c.green.abs() < 0.01);
    }

    #[test]
    fn test_parse_hex_8_with_alpha() {
        let c = parse_hex("ff000080").unwrap();
        assert!((c.red - 1.0).abs() < 0.01);
        assert!((c.alpha - 0.502).abs() < 0.01);
    }

    #[test]
    fn test_presentations_roundtrip() {
        let color = Color { red: 1.0, green: 0.0, blue: 0.0, alpha: 1.0 };
        let range = Range::default();
        let ps = presentations(color, range);
        assert!(ps[0].label.contains("ff0000"));
    }

    #[test]
    fn test_compute_finds_hex_color() {
        let store = crate::document::DocumentStore::new();
        let uri = Url::parse("file:///test.rpy").unwrap();
        store.upsert(uri.clone(), crate::document::DocumentState {
            rope: ropey::Rope::from_str("style text color \"#ff0000\"\n"),
            version: 1,
            symbols: vec![],
            diagnostics: vec![],
        });
        let colors = compute(&uri, &store);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_compute_finds_color_tuple() {
        let store = crate::document::DocumentStore::new();
        let uri = Url::parse("file:///test.rpy").unwrap();
        store.upsert(uri.clone(), crate::document::DocumentState {
            rope: ropey::Rope::from_str("x = Color((255, 128, 0))\n"),
            version: 1,
            symbols: vec![],
            diagnostics: vec![],
        });
        let colors = compute(&uri, &store);
        assert_eq!(colors.len(), 1);
        assert!((colors[0].color.red - 1.0).abs() < 0.01);
        assert!((colors[0].color.green - 0.502).abs() < 0.01);
    }
}
