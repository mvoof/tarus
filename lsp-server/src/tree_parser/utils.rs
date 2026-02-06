//! Common utilities for tree-sitter parsing

use tower_lsp_server::lsp_types::{Position, Range};
use tree_sitter::Node;

/// Extension trait for convenient text extraction from tree-sitter nodes
pub trait NodeTextExt {
    /// Extract text from node, returning empty string if extraction fails
    fn text_or_default(&self, content: &str) -> String;
}

impl NodeTextExt for Node<'_> {
    fn text_or_default(&self, content: &str) -> String {
        self.utf8_text(content.as_bytes())
            .unwrap_or_default()
            .to_string()
    }
}

/// Supported language types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LangType {
    Rust,
    TypeScript,
    JavaScript,
    Vue,
    Svelte,
    Angular,
}

impl LangType {
    /// Get language type from file extension
    #[must_use]
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "ts" | "tsx" => Some(Self::TypeScript),
            "js" | "jsx" => Some(Self::JavaScript),
            "vue" => Some(Self::Vue),
            "svelte" => Some(Self::Svelte),
            "component.ts" => Some(Self::Angular),
            _ => None,
        }
    }
}

/// Query files embedded at compile time
const RUST_QUERY: &str = include_str!("../queries/rust.scm");
const TS_QUERY: &str = include_str!("../queries/typescript.scm");
const JS_QUERY: &str = include_str!("../queries/javascript.scm");

/// Get the query string for a given language
pub fn get_query_source(lang: LangType) -> &'static str {
    match lang {
        LangType::Rust => RUST_QUERY,
        LangType::TypeScript | LangType::Vue | LangType::Svelte | LangType::Angular => TS_QUERY,
        LangType::JavaScript => JS_QUERY,
    }
}

/// Convert tree-sitter Point to LSP Position
#[allow(clippy::cast_possible_truncation)]
pub fn point_to_position(point: tree_sitter::Point) -> Position {
    Position {
        line: point.row as u32,
        character: point.column as u32,
    }
}

/// Adjust position by line offset (for Vue/Svelte script extraction)
#[allow(clippy::cast_possible_truncation)]
pub fn adjust_position(pos: Position, line_offset: usize) -> Position {
    Position {
        line: pos.line + line_offset as u32,
        character: pos.character,
    }
}

/// Adjust range by line offset
pub fn adjust_range(range: Range, line_offset: usize) -> Range {
    Range {
        start: adjust_position(range.start, line_offset),
        end: adjust_position(range.end, line_offset),
    }
}
