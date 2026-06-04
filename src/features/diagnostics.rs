//! Ren'Py diagnostic checks (linting).
//!
//! Performs per-document linting: indentation consistency, filename conventions, invalid/reserved
//! variable names, obsolete API usage, undefined persistent/store variables, assignment-in-condition,
//! and stray `$` characters.

use regex::Regex;
use std::sync::OnceLock;
use tower_lsp::lsp_types::*;

use crate::parsing::text_utils::filter_string_literals;

/// Configuration for diagnostic checks.
#[derive(Debug, Clone)]
pub struct DiagnosticsConfig {
    pub filename_severity: Severity,
    pub indentation_severity: Severity,
    pub invalid_variable_names: bool,
    pub reserved_variable_names: bool,
    pub obsolete_methods: bool,
    pub undefined_persistents: bool,
    pub undefined_store_variables: bool,
}

/// Configurable severity level for a diagnostic check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Report as an LSP error.
    Error,
    /// Report as an LSP warning.
    Warning,
    /// Check is disabled (no diagnostic emitted).
    Disabled,
}

impl Default for DiagnosticsConfig {
    fn default() -> Self {
        Self {
            filename_severity: Severity::Error,
            indentation_severity: Severity::Error,
            invalid_variable_names: true,
            reserved_variable_names: true,
            obsolete_methods: true,
            undefined_persistents: true,
            undefined_store_variables: true,
        }
    }
}

impl Severity {
    /// Map to the corresponding LSP diagnostic severity.
    fn to_lsp(self) -> DiagnosticSeverity {
        match self {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
            Severity::Disabled => DiagnosticSeverity::HINT,
        }
    }
}

const KNOWN_STORE_VARS: &[&str] = &[
    "adv", "default_mouse", "main_menu", "menu", "mouse_visible",
    "name_only", "narrator", "say", "save_name", "persistent",
];

const RESERVED_PYTHON_NAMES: &[&str] = &[
    "ArithmeticError", "AssertionError", "AttributeError", "BaseException",
    "BlockingIOError", "BrokenPipeError", "BufferError", "BytesWarning",
    "ChildProcessError", "ConnectionAbortedError", "ConnectionError",
    "ConnectionRefusedError", "ConnectionResetError", "DeprecationWarning",
    "EOFError", "EnvironmentError", "Exception", "FileExistsError",
    "FileNotFoundError", "FloatingPointError", "FutureWarning", "GeneratorExit",
    "IOError", "ImportError", "ImportWarning", "IndentationError", "IndexError",
    "InterruptedError", "IsADirectoryError", "KeyError", "KeyboardInterrupt",
    "LookupError", "MemoryError", "ModuleNotFoundError", "NameError",
    "NotADirectoryError", "NotImplemented", "NotImplementedError", "OSError",
    "OverflowError", "PendingDeprecationWarning", "PermissionError",
    "ProcessLookupError", "RecursionError", "ReferenceError", "ResourceWarning",
    "RuntimeError", "RuntimeWarning", "StopAsyncIteration", "StopIteration",
    "SyntaxError", "SyntaxWarning", "SystemError", "SystemExit", "TabError",
    "TimeoutError", "TypeError", "UnboundLocalError", "UnicodeDecodeError",
    "UnicodeEncodeError", "UnicodeError", "UnicodeTranslationError",
    "UnicodeWarning", "UserWarning", "ValueError", "Warning", "ZeroDivisionError",
    "abs", "all", "any", "ascii", "bin", "bool", "breakpoint", "bytearray",
    "bytes", "callable", "chr", "classmethod", "compile", "complex", "copyright",
    "credits", "delattr", "dict", "dir", "divmod", "enumerate", "eval", "exec",
    "exit", "filter", "float", "format", "frozenset", "getattr", "globals",
    "hasattr", "hash", "help", "hex", "id", "input", "int", "isinstance",
    "issubclass", "iter", "len", "license", "list", "locals", "map", "max",
    "memoryview", "min", "next", "object", "oct", "open", "ord", "pow", "print",
    "property", "quit", "range", "repr", "reversed", "round", "set", "setattr",
    "slice", "sorted", "staticmethod", "str", "sum", "super", "tuple", "type",
    "vars", "zip",
];

fn re_invalid_var() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(default|define)\s+([^a-zA-Z\s_][a-zA-Z0-9_]*)\s+=").unwrap())
}

fn re_define_var() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(default|define)\s+([a-zA-Z_][a-zA-Z0-9_]*)\s+=").unwrap())
}

fn re_underscore_var() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(default|define)\s+(_[a-zA-Z0-9]*)\s+=").unwrap())
}

fn re_obsolete() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(im\.\w+|ui\.\w+|LiveCrop|LiveComposite|Tooltip)\b").unwrap())
}

fn re_persistent_use() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bpersistent\.(\w+)").unwrap())
}

fn re_persistent_def() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(default|define)\s+persistent\.(\w+)\s+=").unwrap())
}

fn re_store_use() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bstore\.(\w+)").unwrap())
}

fn re_store_def() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(default|define)\s+([\w.]+)\s+=").unwrap())
}

fn re_assignment_cond() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(if|while)\s+(\w+)\s*([=])\s*\w+").unwrap())
}

/// Compute diagnostics for a Ren'Py document.
pub fn compute(uri: &Url, text: &str, cfg: &DiagnosticsConfig) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let lines: Vec<&str> = text.lines().collect();

    check_filename(uri, text, cfg, &mut diagnostics);

    // Pre-collect defined persistents and store vars for cross-reference
    let defined_persistents: Vec<&str> = lines.iter().filter_map(|l| {
        re_persistent_def().captures(l).and_then(|c| c.get(2)).map(|m| m.as_str())
    }).collect();

    let defined_stores: Vec<&str> = lines.iter().filter_map(|l| {
        re_store_def().captures(l).and_then(|c| c.get(2)).map(|m| m.as_str())
    }).collect();

    let mut first_indent: Option<usize> = None;

    for (line_idx, &line) in lines.iter().enumerate() {
        let line_num = line_idx as u32;
        let filtered = filter_string_literals(line);

        if cfg.indentation_severity != Severity::Disabled {
            if let Some(pos) = line.find('\t') {
                let end = line[pos..].chars().take_while(|&c| c == '\t').count();
                diagnostics.push(make_diag(
                    line_num, pos as u32, line_num, (pos + end) as u32,
                    cfg.indentation_severity.to_lsp(),
                    "Tab characters are not allowed; use spaces.".to_string(),
                ));
            }

            let stripped = line.trim_start();
            if !stripped.is_empty() && !line.starts_with('\t') {
                let indent = line.len() - stripped.len();
                if indent > 0 {
                    match first_indent {
                        None => { first_indent = Some(indent); }
                        Some(fi) if indent % fi != 0 => {
                            diagnostics.push(make_diag(
                                line_num, 0, line_num, indent as u32,
                                cfg.indentation_severity.to_lsp(),
                                format!("Inconsistent indentation: {indent} spaces (expected multiple of {fi})."),
                            ));
                        }
                        _ => {}
                    }
                }
            }
        }

        if cfg.invalid_variable_names {
            if let Some(caps) = re_invalid_var().captures(&filtered) {
                if let Some(m) = caps.get(2) {
                    diagnostics.push(make_diag(
                        line_num, m.start() as u32, line_num, m.end() as u32,
                        DiagnosticSeverity::ERROR,
                        format!("Invalid variable name '{}': must begin with a letter or underscore.", m.as_str()),
                    ));
                }
            }
        }

        if cfg.reserved_variable_names {
            if let Some(caps) = re_define_var().captures(&filtered) {
                if let Some(m) = caps.get(2) {
                    let name = m.as_str();
                    if RESERVED_PYTHON_NAMES.contains(&name) {
                        diagnostics.push(make_diag(
                            line_num, m.start() as u32, line_num, m.end() as u32,
                            DiagnosticSeverity::WARNING,
                            format!("'{name}' is a reserved Python name."),
                        ));
                    }
                }
            }
            if let Some(caps) = re_underscore_var().captures(&filtered) {
                if let Some(m) = caps.get(2) {
                    diagnostics.push(make_diag(
                        line_num, m.start() as u32, line_num, m.end() as u32,
                        DiagnosticSeverity::WARNING,
                        format!("'{}' uses a reserved Ren'Py prefix '_'.", m.as_str()),
                    ));
                }
            }
        }

        if cfg.obsolete_methods {
            for m in re_obsolete().find_iter(&filtered) {
                diagnostics.push(make_diag(
                    line_num, m.start() as u32, line_num, m.end() as u32,
                    DiagnosticSeverity::WARNING,
                    format!("'{}' is obsolete.", m.as_str()),
                ));
            }
        }

        if cfg.undefined_persistents {
            for caps in re_persistent_use().captures_iter(&filtered) {
                if let Some(m) = caps.get(1) {
                    let name = m.as_str();
                    if !defined_persistents.contains(&name) {
                        diagnostics.push(make_diag(
                            line_num, m.start() as u32, line_num, m.end() as u32,
                            DiagnosticSeverity::WARNING,
                            format!("Persistent variable 'persistent.{name}' may not be defined."),
                        ));
                    }
                }
            }
        }

        if cfg.undefined_store_variables {
            for caps in re_store_use().captures_iter(&filtered) {
                if let Some(m) = caps.get(1) {
                    let name = m.as_str();
                    if !KNOWN_STORE_VARS.contains(&name)
                        && !name.starts_with('_')
                        && !defined_stores.contains(&name)
                    {
                        diagnostics.push(make_diag(
                            line_num, m.start() as u32, line_num, m.end() as u32,
                            DiagnosticSeverity::WARNING,
                            format!("Store variable 'store.{name}' may not be defined."),
                        ));
                    }
                }
            }
        }

        // Always-on: assignment in conditional
        if let Some(caps) = re_assignment_cond().captures(&filtered) {
            let full = caps.get(0).unwrap().as_str();
            if !full.contains("==") {
                if let Some(m) = caps.get(3) {
                    diagnostics.push(make_diag(
                        line_num, m.start() as u32, line_num, m.end() as u32,
                        DiagnosticSeverity::WARNING,
                        "Use '==' for comparison, not '=' (assignment).".to_string(),
                    ));
                }
            }
        }

        // Always-on: stray dollar
        let trimmed = filtered.trim_start();
        if !trimmed.is_empty() && !trimmed.starts_with('$') {
            let leading = filtered.len() - trimmed.len();
            if let Some(pos) = trimmed.find('$') {
                let col = (leading + pos) as u32;
                diagnostics.push(make_diag(
                    line_num, col, line_num, col + 1,
                    DiagnosticSeverity::WARNING,
                    "Stray '$' character.".to_string(),
                ));
            }
        }
    }

    diagnostics
}

/// Check filename conventions (must start with alphanumeric, must not start with "00").
fn check_filename(uri: &Url, text: &str, cfg: &DiagnosticsConfig, diags: &mut Vec<Diagnostic>) {
    if cfg.filename_severity == Severity::Disabled {
        return;
    }
    if let Some(first_line) = text.lines().next() {
        if first_line.contains("#@Ren'Py-Filename: Ignore") {
            return;
        }
    }
    let path = uri.path();
    if path.contains("renpy/common") {
        return;
    }
    if let Some(filename) = path.rsplit('/').next() {
        let stem = filename.split('.').next().unwrap_or(filename);
        if !stem.is_empty() {
            let first_char = stem.chars().next().unwrap();
            if !first_char.is_ascii_alphanumeric() {
                diags.push(make_diag(
                    0, 0, 0, stem.len() as u32,
                    cfg.filename_severity.to_lsp(),
                    format!("Filename must begin with a letter or digit: '{filename}'"),
                ));
            } else if stem.starts_with("00") {
                diags.push(make_diag(
                    0, 0, 0, stem.len() as u32,
                    cfg.filename_severity.to_lsp(),
                    format!("Filename must not begin with '00': '{filename}'"),
                ));
            }
        }
    }
}

/// Construct a [`Diagnostic`] with the given range, severity, and message.
fn make_diag(
    start_line: u32, start_col: u32,
    end_line: u32, end_col: u32,
    severity: DiagnosticSeverity,
    message: String,
) -> Diagnostic {
    Diagnostic {
        range: Range {
            start: Position { line: start_line, character: start_col },
            end: Position { line: end_line, character: end_col },
        },
        severity: Some(severity),
        source: Some("renpy".to_string()),
        message,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_uri() -> Url {
        Url::parse("file:///game/script.rpy").unwrap()
    }

    #[test]
    fn test_tab_characters() {
        let text = "\tdefine x = 5\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("Tab")));
    }

    #[test]
    fn test_inconsistent_indentation() {
        let text = "label start:\n    say hello\n      bad indent\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("Inconsistent")));
    }

    #[test]
    fn test_invalid_variable_name() {
        let text = "define 1bad = 5\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("Invalid variable name")));
    }

    #[test]
    fn test_reserved_python_name() {
        let text = "define list = []\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("reserved Python")));
    }

    #[test]
    fn test_reserved_underscore_name() {
        let text = "define _myvar = 5\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("reserved Ren'Py prefix")));
    }

    #[test]
    fn test_obsolete_methods() {
        let text = "$ ui.text(\"hello\")\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("obsolete")));
    }

    #[test]
    fn test_undefined_persistent() {
        let text = "if persistent.unknown_flag:\n    pass\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("persistent.unknown_flag")));
    }

    #[test]
    fn test_defined_persistent_no_warning() {
        let text = "default persistent.flag = False\nif persistent.flag:\n    pass\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(!diags.iter().any(|d| d.message.contains("may not be defined")));
    }

    #[test]
    fn test_undefined_store_var() {
        let text = "if store.unknown_var:\n    pass\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("store.unknown_var")));
    }

    #[test]
    fn test_assignment_in_conditional() {
        let text = "if x = 5\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("'=='")));
    }

    #[test]
    fn test_stray_dollar() {
        let text = "label start $ bad\n";
        let diags = compute(&test_uri(), text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("Stray '$'")));
    }

    #[test]
    fn test_invalid_filename() {
        let uri = Url::parse("file:///game/_bad.rpy").unwrap();
        let text = "label start:\n    pass\n";
        let diags = compute(&uri, text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("Filename must begin")));
    }

    #[test]
    fn test_filename_00_prefix() {
        let uri = Url::parse("file:///game/00init.rpy").unwrap();
        let text = "label start:\n    pass\n";
        let diags = compute(&uri, text, &DiagnosticsConfig::default());
        assert!(diags.iter().any(|d| d.message.contains("00")));
    }
}
