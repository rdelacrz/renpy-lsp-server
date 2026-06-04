# renpy-language-server

A [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) server for the
[Ren'Py](https://www.renpy.org/) visual-novel scripting language, written in Rust on
[`tower-lsp`](https://github.com/ebkalderon/tower-lsp).

This is the **language-intelligence half** of the Zed Ren'Py extension. It is a standalone native
binary that speaks LSP over **stdio**. The Zed extension (a WASM crate in the repo root) locates and
spawns this binary; the editor then drives it with `textDocument/*` requests. The server is editor
agnostic — any LSP client can use it.

```
Zed editor ──spawns──▶ renpy-language-server  (this crate, native binary)
          ◀──LSP/stdio──▶  completion · hover · definition · diagnostics · …
```

## Capabilities

Advertised in `initialize` (see `src/backend.rs`):

| LSP request | Module | Notes |
|-------------|--------|-------|
| `textDocument/completion` | `features/completion.rs` | keywords, screen displayables, `renpy.`/`config.`/`gui.` API, in-file symbols. Triggers: `.` ` ` `@` `-` `(` |
| `textDocument/hover` | `features/hover.rs` | docstring + signature from the embedded Ren'Py API DB |
| `textDocument/signatureHelp` | `features/signature.rs` | param hints; triggers `(` `,` `=` |
| `textDocument/definition` | `features/definition.rs` | resolves via the workspace symbol index |
| `textDocument/references` | `backend.rs` (inline) | word-boundary search across open docs, string-literals filtered out |
| `textDocument/documentSymbol` | `features/symbols.rs` | labels, screens, transforms, defines, defaults, styles, images, classes |
| `textDocument/documentColor` (+ `colorPresentation`) | `features/color.rs` | 9 formats: hex `#rgb`/`#rgba`/`#rrggbb`/`#rrggbbaa`, `Color((…))`, `rgb=()`, `color=()` |
| `textDocument/foldingRange` | `backend.rs` (inline) | indentation-based |
| `textDocument/documentHighlight` | `backend.rs` (inline) | same-identifier occurrences |
| `textDocument/publishDiagnostics` | `features/diagnostics.rs` | pushed on change/save; ~11 checks (indentation/tabs, invalid/reserved variable names, obsolete methods, undefined persistents/store vars, `=` in `if`/`while`, stray `$`, invalid filename) |

Text sync is **FULL** (whole document on each change). Diagnostics are **push** based.

## Architecture / source layout

```
server/
├── Cargo.toml             # own [workspace]; deps: tower-lsp, tokio, dashmap, ropey, regex, serde, thiserror, tracing
├── assets/
│   ├── renpy.json         # Ren'Py API/keyword DB (from the VSCode extension, MIT) — embedded via include_str!
│   └── renpyauto.json     # context completion options (pipe-delimited, {placeholder} expansion)
└── src/
    ├── main.rs            # entry: stdio transport wiring + tracing→stderr (stdout is the LSP channel)
    ├── backend.rs         # `Backend` + `impl LanguageServer`: capabilities, lifecycle, FULL sync,
    │                      #   request dispatch, and the inline references/folding/highlight handlers
    ├── document.rs        # DocumentState (rope, version, cached symbols+diagnostics) + DocumentStore (DashMap)
    ├── index.rs           # workspace symbol index (name → [(uri, range, kind)]) populated on change
    ├── error.rs           # crate-wide `Error` enum (thiserror) + `Result<T>` alias
    ├── parsing/
    │   ├── parser.rs      # line/indentation regex parser → symbols + references (mirrors the VSCode tokenizer)
    │   └── text_utils.rs  # filter_string_literals, position_is_clean, keyword_prefix
    ├── data/
    │   ├── loader.rs      # load_game_data(): parses the embedded JSON assets into GameData
    │   └── keywords.rs    # static Ren'Py keyword / screen-displayable / ATL / style property tables
    └── features/          # one module per LSP feature (see capability table above)
```

### Parsing model (v1)
A line-by-line, indentation-aware **regex** parser (`parsing/parser.rs`) — not a full grammar. It
extracts definitions (`label`/`screen`/`transform`/`image`/`define`/`default`/`style` + python
`def`/`class` + `Character(...)`) and references (`jump`/`call`/`show`/`scene`/`hide`/`use`). Syntax
*highlighting* is handled separately by the tree-sitter grammar (see `../tree-sitter-renpy`), not by
this server.

## Build, test, run

```bash
cd server
cargo build --release        # produces target/release/renpy-language-server
cargo test                   # 43 unit tests (parser, color, loader, index, diagnostics, text_utils)
```

The server reads/writes LSP on stdio; **all logging goes to stderr** (`tracing`). To use it with the
Zed dev extension, build it and either put it on `PATH` or set
`lsp.renpy-language-server.binary.path` in Zed settings (the extension calls `worktree.which(...)`
first, then that setting).

## Deferred (future work)
- `textDocument/semanticTokens` body (legend is declared; compute returns `None` — tree-sitter does highlighting for now).
- Workspace-wide file scanning on `initialized` (the index currently covers open documents only), which also widens `references`.
- Context-aware ATL/style/screen-property completion (tables exist in `data/keywords.rs`, not yet wired to cursor context).
