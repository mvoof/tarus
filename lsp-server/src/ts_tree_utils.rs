//! Shared tree-sitter helpers for parsing TypeScript content.

use tree_sitter::{Language, Parser, Tree};

/// Parse a string as TypeScript and return the tree.
///
/// Returns `None` if parsing fails.
#[must_use]
pub fn parse_ts(content: &str) -> Option<Tree> {
    let ts_lang: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let mut parser = Parser::new();
    parser.set_language(&ts_lang).ok()?;
    parser.parse(content, None)
}

/// Parse a string as Rust and return the tree.
///
/// Returns `None` if parsing fails.
#[must_use]
pub fn parse_rust(content: &str) -> Option<Tree> {
    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&ts_lang).ok()?;
    parser.parse(content, None)
}
