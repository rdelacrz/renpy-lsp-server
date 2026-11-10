//! Text and encoding utilities for LSP position handling.
//!
//! LSP uses UTF-16 code-unit offsets for column positions, while Rust strings are UTF-8.
//! This module provides conversion functions between these encodings, plus helpers for
//! filtering string literals and extracting keyword prefixes from source lines.

/// Convert a UTF-16 column offset (as used by LSP `Position.character`) to a char index.
/// Clamped: if `utf16_col` is past the end of the line, returns the total char count.
/// If `utf16_col` lands inside a surrogate pair, clamps to the start of that char.
pub fn utf16_to_char_idx(line: &str, utf16_col: usize) -> usize {
    let mut utf16_acc = 0;
    for (i, ch) in line.chars().enumerate() {
        if utf16_acc >= utf16_col {
            return i;
        }
        let w = ch.len_utf16();
        // If utf16_col falls inside this char (mid-surrogate), clamp to this char's start
        if utf16_acc + w > utf16_col {
            return i;
        }
        utf16_acc += w;
    }
    line.chars().count()
}

/// Convert a UTF-16 column offset to a byte offset into `line`.
/// Clamped: if `utf16_col` is past end, returns `line.len()`.
/// If `utf16_col` lands inside a surrogate pair, clamps to byte start of that char.
pub fn utf16_to_byte(line: &str, utf16_col: usize) -> usize {
    let mut utf16_acc = 0;
    for (byte_idx, ch) in line.char_indices() {
        if utf16_acc >= utf16_col {
            return byte_idx;
        }
        let w = ch.len_utf16();
        if utf16_acc + w > utf16_col {
            return byte_idx;
        }
        utf16_acc += w;
    }
    line.len()
}

/// Number of UTF-16 code units in `line[..byte_col]` (byte offset → LSP character offset).
/// `byte_col` is clamped to `line.len()`; if it lands mid-char it counts up to the preceding boundary.
pub fn byte_to_utf16(line: &str, byte_col: usize) -> usize {
    let mut utf16_acc = 0;
    for (byte_idx, ch) in line.char_indices() {
        if byte_idx + ch.len_utf8() > byte_col {
            return utf16_acc;
        }
        utf16_acc += ch.len_utf16();
    }
    utf16_acc
}

/// Replace contents of string literals with spaces, preserving quotes and length.
/// Handles both single and double quoted strings.
pub fn filter_string_literals(line: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '"' || ch == '\'' {
            // Check for triple-quote
            let triple = i + 2 < chars.len() && chars[i + 1] == ch && chars[i + 2] == ch;
            if triple {
                result.push(ch);
                result.push(ch);
                result.push(ch);
                i += 3;
                // consume until closing triple
                while i < chars.len() {
                    if i + 2 < chars.len()
                        && chars[i] == ch
                        && chars[i + 1] == ch
                        && chars[i + 2] == ch
                    {
                        result.push(ch);
                        result.push(ch);
                        result.push(ch);
                        i += 3;
                        break;
                    } else {
                        result.push(' ');
                        i += 1;
                    }
                }
            } else {
                result.push(ch);
                i += 1;
                // consume until closing quote (same line)
                while i < chars.len() && chars[i] != ch {
                    if chars[i] == '\\' {
                        result.push(' ');
                        i += 1;
                        if i < chars.len() {
                            result.push(' ');
                            i += 1;
                        }
                    } else {
                        result.push(' ');
                        i += 1;
                    }
                }
                if i < chars.len() {
                    result.push(ch); // closing quote
                    i += 1;
                }
            }
        } else {
            result.push(ch);
            i += 1;
        }
    }
    result
}

/// Returns false if position `col` (UTF-16 offset) is inside a `#` comment or string literal.
pub fn position_is_clean(line: &str, col: usize) -> bool {
    let col = utf16_to_char_idx(line, col);
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() && i < col {
        let ch = chars[i];
        if ch == '#' {
            return false; // rest of line is comment
        }
        if ch == '"' || ch == '\'' {
            let quote = ch;
            i += 1;
            // skip string body
            while i < chars.len() && chars[i] != quote {
                if chars[i] == '\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            if i < chars.len() {
                i += 1; // closing quote
            }
            // If col was inside this string, it's not clean
            if col <= i {
                return false;
            }
        } else {
            i += 1;
        }
    }
    // Check if col itself is at a # (if col == i and chars[col] == '#')
    if col < chars.len() && chars[col] == '#' {
        return false;
    }
    true
}

/// Walk backward from `col` (UTF-16 offset) across `.` to build a dotted keyword prefix.
/// Returns None if position is not at an identifier.
pub fn keyword_prefix(line: &str, col: usize) -> Option<String> {
    let col = utf16_to_char_idx(line, col);
    let chars: Vec<char> = line.chars().collect();
    if col == 0 || col > chars.len() {
        return None;
    }
    // Walk back from col over the current word
    let mut start = col;
    while start > 0
        && (chars[start - 1].is_alphanumeric()
            || chars[start - 1] == '_'
            || chars[start - 1] == '.')
    {
        start -= 1;
    }
    // If we didn't move, col is not at a word char
    if start == col {
        return None;
    }
    // Continue walking back across dots and preceding identifiers
    while start > 0 && chars[start - 1] == '.' {
        start -= 1; // skip dot
        while start > 0 && (chars[start - 1].is_alphanumeric() || chars[start - 1] == '_') {
            start -= 1;
        }
    }
    let prefix: String = chars[start..col].iter().collect();
    if prefix.is_empty() {
        None
    } else {
        Some(prefix)
    }
}

/// Extract the complete dotted identifier under the cursor, not just a completion prefix.
pub fn identifier_at_position(line: &str, col: usize) -> Option<String> {
    if !position_is_clean(line, col) {
        return None;
    }
    let chars: Vec<char> = line.chars().collect();
    let col = utf16_to_char_idx(line, col);
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '.';
    let mut start = col;
    let mut end = col;
    while start > 0 && is_ident(chars[start - 1]) {
        start -= 1;
    }
    while end < chars.len() && is_ident(chars[end]) {
        end += 1;
    }
    let word: String = chars[start..end].iter().collect();
    (!word.is_empty() && word != ".").then_some(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_after_dot() {
        assert_eq!(keyword_prefix("$ renpy.", 8).as_deref(), Some("renpy."));
        assert_eq!(
            keyword_prefix("$ renpy.music.", 14).as_deref(),
            Some("renpy.music.")
        );
    }

    #[test]
    fn identifier_at_start_middle_end_and_unicode() {
        for col in [0, 3, 8, 11] {
            assert_eq!(
                identifier_at_position("renpy.pause(1)", col).as_deref(),
                Some("renpy.pause")
            );
        }
        assert_eq!(
            identifier_at_position("$ 名前 = 1", 3).as_deref(),
            Some("名前")
        );
        assert!(identifier_at_position("# renpy.pause", 5).is_none());
        assert!(identifier_at_position("\"renpy.pause\"", 5).is_none());
    }

    #[test]
    fn test_filter_string_literals_basic() {
        let line = r#"say "hello world" and 'foo'"#;
        let filtered = filter_string_literals(line);
        assert_eq!(filtered, r#"say "           " and '   '"#);
    }

    #[test]
    fn test_filter_string_literals_escaped() {
        let line = r#"x = "he said \"hi\"" + y"#;
        let filtered = filter_string_literals(line);
        // Inside string, all content replaced with spaces
        assert!(filtered.starts_with("x = \""));
        assert!(filtered.ends_with("+ y"));
        assert_eq!(filtered.len(), line.len());
    }

    #[test]
    fn test_filter_string_literals_no_strings() {
        let line = "x = 42 + y";
        assert_eq!(filter_string_literals(line), line);
    }

    #[test]
    fn test_position_is_clean_normal() {
        let line = "define x = 5";
        assert!(position_is_clean(line, 7));
    }

    #[test]
    fn test_position_is_clean_in_comment() {
        let line = "x = 5 # comment";
        assert!(!position_is_clean(line, 8));
    }

    #[test]
    fn test_position_is_clean_in_string() {
        let line = r#"x = "hello" + y"#;
        assert!(!position_is_clean(line, 6)); // inside "hello"
        assert!(position_is_clean(line, 14)); // at y
    }

    #[test]
    fn test_keyword_prefix_dotted() {
        let line = "renpy.music.play";
        assert_eq!(
            keyword_prefix(line, 16),
            Some("renpy.music.play".to_string())
        );
    }

    #[test]
    fn test_keyword_prefix_single() {
        let line = "    label start:";
        assert_eq!(keyword_prefix(line, 9), Some("label".to_string()));
    }

    #[test]
    fn test_keyword_prefix_none() {
        let line = "    + 5";
        assert_eq!(keyword_prefix(line, 4), None);
    }

    #[test]
    fn test_keyword_prefix_at_start() {
        let line = "renpy.music";
        assert_eq!(keyword_prefix(line, 0), None);
    }

    // --- UTF-16 converter tests ---

    #[test]
    fn test_utf16_to_char_idx_ascii() {
        let line = "hello";
        assert_eq!(utf16_to_char_idx(line, 0), 0);
        assert_eq!(utf16_to_char_idx(line, 3), 3);
        assert_eq!(utf16_to_char_idx(line, 5), 5);
    }

    #[test]
    fn test_utf16_to_char_idx_multibyte_bmp() {
        // "あ" is U+3042: 1 UTF-16 unit, 3 bytes, 1 char
        let line = "あbc";
        assert_eq!(utf16_to_char_idx(line, 0), 0); // before あ
        assert_eq!(utf16_to_char_idx(line, 1), 1); // after あ, before b
        assert_eq!(utf16_to_char_idx(line, 2), 2); // after b, before c
        assert_eq!(utf16_to_char_idx(line, 3), 3); // after c
    }

    #[test]
    fn test_utf16_to_char_idx_astral() {
        // "😀" is U+1F600: 2 UTF-16 units, 4 bytes, 1 char
        let line = "😀bc";
        assert_eq!(utf16_to_char_idx(line, 0), 0); // before 😀
        assert_eq!(utf16_to_char_idx(line, 1), 0); // mid surrogate pair → clamp to start
        assert_eq!(utf16_to_char_idx(line, 2), 1); // after 😀, before b
        assert_eq!(utf16_to_char_idx(line, 3), 2); // after b
        assert_eq!(utf16_to_char_idx(line, 4), 3); // after c
    }

    #[test]
    fn test_utf16_to_char_idx_past_end() {
        let line = "ab";
        assert_eq!(utf16_to_char_idx(line, 100), 2);
    }

    #[test]
    fn test_utf16_to_byte_ascii() {
        let line = "hello";
        assert_eq!(utf16_to_byte(line, 0), 0);
        assert_eq!(utf16_to_byte(line, 3), 3);
        assert_eq!(utf16_to_byte(line, 5), 5);
    }

    #[test]
    fn test_utf16_to_byte_multibyte_bmp() {
        // "あ" = 3 bytes, 1 UTF-16 unit
        let line = "あbc";
        assert_eq!(utf16_to_byte(line, 0), 0);
        assert_eq!(utf16_to_byte(line, 1), 3); // after あ
        assert_eq!(utf16_to_byte(line, 2), 4); // after b
        assert_eq!(utf16_to_byte(line, 3), 5); // after c (end)
    }

    #[test]
    fn test_utf16_to_byte_astral() {
        // "😀" = 4 bytes, 2 UTF-16 units
        let line = "😀bc";
        assert_eq!(utf16_to_byte(line, 0), 0);
        assert_eq!(utf16_to_byte(line, 1), 0); // mid surrogate → clamp to start
        assert_eq!(utf16_to_byte(line, 2), 4); // after 😀
        assert_eq!(utf16_to_byte(line, 3), 5); // after b
    }

    #[test]
    fn test_utf16_to_byte_past_end() {
        let line = "ab";
        assert_eq!(utf16_to_byte(line, 100), 2);
    }

    // --- Non-ASCII tests for keyword_prefix / position_is_clean ---

    #[test]
    fn test_keyword_prefix_after_japanese() {
        // "こんにちは " is 5 Japanese chars (each 1 UTF-16 unit) + space = 6 UTF-16 units
        // Then "label" at UTF-16 positions 6..11
        let line = "こんにちは label";
        // UTF-16 col 11 = after "label" (6 + 5)
        assert_eq!(keyword_prefix(line, 11), Some("label".to_string()));
    }

    #[test]
    fn test_position_is_clean_after_japanese() {
        let line = "こんにちは x = 5";
        // "こんにちは " = 6 UTF-16 units, 'x' at UTF-16 col 6
        assert!(position_is_clean(line, 6));
    }

    #[test]
    fn test_position_is_clean_in_comment_after_multibyte() {
        let line = "あ # comment";
        // "あ " = 2 UTF-16 units, '#' at UTF-16 col 2
        assert!(!position_is_clean(line, 3)); // inside comment
    }

    /// Regression: slicing at a raw UTF-16 col after a multi-byte char must not panic.
    #[test]
    fn test_utf16_to_byte_no_panic_mid_char() {
        let line = "café"; // 'é' is 2 bytes in UTF-8, 1 UTF-16 unit
                           // byte offset of col 4 should be safe (after 'é')
        let b = utf16_to_byte(line, 4);
        let _prefix = &line[..b]; // must not panic
        assert_eq!(_prefix, "café");
    }

    /// Regression for W-4: slicing after multi-byte chars with raw col would panic.
    #[test]
    fn test_signature_slice_regression_no_panic() {
        // Simulating the old W-4 bug: "あfunc(" = 7 bytes, but col=5 in UTF-16
        // Old code: &line_text[..5] would panic (mid-char)
        // New code: utf16_to_byte clamps correctly
        let line = "あfunc(x, y)";
        let col: usize = 6; // UTF-16: あ=1, f=1, u=1, n=1, c=1, (=1 → 6
        let b = utf16_to_byte(line, col);
        let prefix = &line[..b]; // must not panic
        assert!(prefix.contains('('));
    }

    /// keyword_prefix with usize::MAX col (as used in signature.rs) must not panic.
    #[test]
    fn test_keyword_prefix_col_max_no_panic() {
        let line = "renpy.play";
        // usize::MAX col → utf16_to_char_idx clamps to line length, returns full prefix
        assert_eq!(
            keyword_prefix(line, usize::MAX),
            Some("renpy.play".to_string())
        );
    }

    /// keyword_prefix after an emoji (astral/surrogate pair) resolves correctly.
    #[test]
    fn test_keyword_prefix_after_emoji() {
        // "😀 " = 2 UTF-16 units + 1 space = 3 UTF-16 units, then "func" at 3..7
        let line = "😀 func";
        assert_eq!(keyword_prefix(line, 7), Some("func".to_string()));
    }

    // --- byte_to_utf16 tests ---

    #[test]
    fn test_byte_to_utf16_ascii_identity() {
        let line = "hello world";
        assert_eq!(byte_to_utf16(line, 0), 0);
        assert_eq!(byte_to_utf16(line, 5), 5);
        assert_eq!(byte_to_utf16(line, 11), 11);
    }

    #[test]
    fn test_byte_to_utf16_bmp() {
        // "あ" is U+3042: 3 UTF-8 bytes, 1 UTF-16 unit
        let line = "あbc";
        assert_eq!(byte_to_utf16(line, 0), 0); // before あ
        assert_eq!(byte_to_utf16(line, 3), 1); // after あ, before b
        assert_eq!(byte_to_utf16(line, 4), 2); // after b
        assert_eq!(byte_to_utf16(line, 5), 3); // after c
    }

    #[test]
    fn test_byte_to_utf16_astral() {
        // "😀" is U+1F600: 4 UTF-8 bytes, 2 UTF-16 units
        let line = "😀bc";
        assert_eq!(byte_to_utf16(line, 0), 0); // before 😀
        assert_eq!(byte_to_utf16(line, 4), 2); // after 😀
        assert_eq!(byte_to_utf16(line, 5), 3); // after b
        assert_eq!(byte_to_utf16(line, 6), 4); // after c
    }

    #[test]
    fn test_byte_to_utf16_zero() {
        let line = "anything";
        assert_eq!(byte_to_utf16(line, 0), 0);
    }

    #[test]
    fn test_byte_to_utf16_clamp_past_end() {
        let line = "ab";
        assert_eq!(byte_to_utf16(line, 100), 2);
    }

    #[test]
    fn test_byte_to_utf16_mid_char_clamps() {
        // "あ" is 3 bytes; byte 1 and 2 are mid-char, should count preceding boundary
        let line = "あx";
        assert_eq!(byte_to_utf16(line, 1), 0); // mid-char → counts nothing before boundary
        assert_eq!(byte_to_utf16(line, 2), 0); // still mid-char
        assert_eq!(byte_to_utf16(line, 3), 1); // after あ
    }

    #[test]
    fn test_byte_to_utf16_roundtrip() {
        // Round-trip: byte_to_utf16(s, utf16_to_byte(s, n)) == n at char boundaries
        let line = "あ😀xé";
        // char boundaries in UTF-16: あ=1, 😀=2, x=1, é=1 → total 5
        for n in [0usize, 1, 3, 4, 5] {
            let byte = utf16_to_byte(line, n);
            assert_eq!(
                byte_to_utf16(line, byte),
                n,
                "roundtrip failed for utf16_col={}",
                n
            );
        }
    }

    /// Integration: simulates compute_references/highlight path — regex match on a line
    /// with non-ASCII before the identifier, converting byte offsets to UTF-16 columns.
    #[test]
    fn test_byte_to_utf16_reference_highlight_scenario() {
        // Line: "    こんにちは myvar = 1"
        // "    " = 4 bytes, "こんにちは" = 15 bytes (5×3), " " = 1, "myvar" starts at byte 20
        let line = "    こんにちは myvar = 1";
        let filtered = filter_string_literals(line);

        // Use \b word boundary (supported by regex crate) to simulate identifier matching
        let re = regex::Regex::new(r"\bmyvar\b").unwrap();
        let m = re.find(&filtered).expect("should find myvar");

        // Byte offsets from regex
        let start_byte = m.start(); // 20
        let end_byte = m.end(); // 25

        // Convert to UTF-16 columns (as done in compute_references / compute_document_highlight)
        let start_utf16 = byte_to_utf16(&filtered, start_byte);
        let end_utf16 = byte_to_utf16(&filtered, end_byte);

        // "    " = 4 UTF-16 units, "こんにちは" = 5 UTF-16 units, " " = 1, total before myvar = 10
        assert_eq!(
            start_utf16, 10,
            "start should be UTF-16 col 10, not byte col {}",
            start_byte
        );
        assert_eq!(
            end_utf16, 15,
            "end should be UTF-16 col 15, not byte col {}",
            end_byte
        );

        // Verify these are NOT the byte offsets (which would be wrong)
        assert_ne!(
            start_utf16 as usize, start_byte,
            "must differ from raw byte offset"
        );

        // Round-trip: converting back to byte should recover the original byte positions
        assert_eq!(utf16_to_byte(line, start_utf16 as usize), start_byte);
        assert_eq!(utf16_to_byte(line, end_utf16 as usize), end_byte);
    }
}
