# Ren'Py LSP Server

A **Rust** implementation of a [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) (LSP) server for [Ren'Py](https://www.renpy.org/), built with [`tower-lsp`](https://crates.io/crates/tower-lsp).

This server powers intelligent editing features for Ren'Py scripts (`.rpy` files) in editors that support LSP — including [Zed](https://zed.dev/), VS Code, Neovim, and others.

## Features

- **Full text document synchronization** (`didOpen`, `didChange`, `didClose`, `didSave`)
- **Document symbols** — outline view of labels, screens, images, defines, etc.
- **Go to Definition** — jump to label/screen/define/image declarations (cross-file support)
- **Hover** — shows symbol kind, name, and line number
- **Completion** — context-aware suggestions:
  - Ren'Py keywords (`label`, `screen`, `image`, `define`, `transform`, `style`, `jump`, `call`, `show`, `scene`, `menu`, etc.)
  - Project symbols filtered by context (e.g. only labels after `jump`/`call`, only images after `show`/`scene`)
- **Diagnostics** — parse errors, missing colons, invalid identifiers, duplicate labels
- **References** — find all usages of a label, screen, or symbol across the workspace
- **Workspace Symbol Search** — fuzzy search across all symbols in the project

## Architecture

The server is split into clean layers:

- **`src/main.rs`** — Tokio-based stdio server entrypoint
- **`src/lib.rs`** — Core `Backend` implementing the `LanguageServer` trait
- **`src/analysis.rs`** — Pure parser that extracts top-level symbols from Ren'Py source (line-based, zero-indentation only)
- **`src/index.rs`** — Workspace index maintaining open documents, symbol table, and cached diagnostics

Symbols are stored with line numbers (not byte offsets) for simplicity and LSP compatibility.

## Installation & Usage

### From Source

```bash
cd renpy-lsp-server
cargo build --release
```

The binary will be at `target/release/renpy-lsp-server`.

### With Zed

1. Build the release binary
2. Configure Zed to use it (see the companion `zed-language-renpy` extension)
3. Open a Ren'Py project — the language server should auto-start on `.rpy` files

### With Other Editors

Configure your editor to run the binary with stdio transport. Most LSP clients support this via a language server configuration.

## Development

### Running Tests

```bash
# Run all tests
cargo test

# Run with verbose output
cargo test -- --nocapture

# Check formatting and linting
cargo fmt --all -- --check
cargo check
```

### Building

```bash
# Debug build
cargo build

# Optimized release build
cargo build --release
```

### Project Structure

```
renpy-lsp-server/
├── Cargo.toml
├── src/
│   ├── main.rs          # Binary entrypoint
│   ├── lib.rs           # LSP backend + handlers
│   ├── analysis.rs      # Ren'Py parser & symbol extraction
│   └── index.rs         # Workspace index & symbol table
└── README.md
```

## Testing

The project includes comprehensive tests:

- **Parser tests** (`analysis.rs`): symbol extraction, error cases, CRLF handling, identifier validation
- **Integration tests** (`lib.rs`): full LSP handler behavior, index updates, completion, hover, definition
- **21 tests total**, all passing

## Contributing

1. Run `cargo test` and `cargo fmt` before submitting changes
2. Follow the existing code style and architecture
3. Add tests for new features
4. Update `CHECKLIST.md` and `REVIEW_NOTES.md` in `.hermes-orchestration/`

## License

This project is part of the Zed Ren'Py language support ecosystem. See the root repository for licensing information.

---

**Made with ❤️ for the Ren'Py community**
