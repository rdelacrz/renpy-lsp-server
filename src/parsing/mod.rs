//! Ren'Py document parsing and text utilities.
//!
//! Provides regex-based extraction of symbols and references from `.rpy` source files,
//! plus UTF-16 ↔ byte/char offset conversion utilities needed for LSP position handling.

pub mod parser;
pub mod text_utils;
