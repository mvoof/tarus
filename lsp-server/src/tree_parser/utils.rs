//! Common utilities for tree-sitter parsing

use crate::syntax::{ParseError, ParseResult};
use std::path::Path;
use tower_lsp_server::ls_types::{Position, Range};
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, Tree};

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
pub fn point_to_position(point: tree_sitter::Point) -> Position {
    Position {
        line: u32::try_from(point.row).unwrap_or(u32::MAX),
        character: u32::try_from(point.column).unwrap_or(u32::MAX),
    }
}

/// Adjust position by line offset (for Vue/Svelte script extraction)
pub fn adjust_position(pos: Position, line_offset: usize) -> Position {
    Position {
        line: pos.line + u32::try_from(line_offset).unwrap_or(u32::MAX),
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

/// Parse context encapsulating tree-sitter parser setup
///
/// This struct eliminates boilerplate by providing a single initialization
/// point for Parser -> Language -> Tree -> Query setup.
pub struct ParseContext {
    pub tree: Tree,
    pub query: Query,
    pub language: Language,
}

impl ParseContext {
    /// Initialize parser, parse content, and compile query
    ///
    /// # Errors
    ///
    /// Returns `ParseError` if:
    /// - Language cannot be set on parser
    /// - Content cannot be parsed
    /// - Query cannot be compiled
    pub fn new(
        lang: &Language,
        query_source: &str,
        content: &str,
        path: &Path,
    ) -> ParseResult<Self> {
        let mut parser = Parser::new();
        parser.set_language(lang).map_err(|e| {
            ParseError::Language(e.to_string(), Some(path.to_string_lossy().to_string()))
        })?;

        let tree = parser.parse(content, None).ok_or_else(|| {
            ParseError::Syntax(
                "Failed to parse".to_string(),
                Some(path.to_string_lossy().to_string()),
            )
        })?;

        let query = Query::new(lang, query_source).map_err(|e| {
            ParseError::Query(e.to_string(), Some(path.to_string_lossy().to_string()))
        })?;

        Ok(Self {
            tree,
            query,
            language: lang.clone(),
        })
    }

    /// Create a new query cursor for iterating over matches
    #[must_use]
    pub fn cursor(&self) -> QueryCursor {
        QueryCursor::new()
    }

    /// Get the root node of the parsed tree
    #[must_use]
    pub fn root_node(&self) -> Node {
        self.tree.root_node()
    }
}
