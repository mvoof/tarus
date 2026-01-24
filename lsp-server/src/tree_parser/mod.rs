//! Unified Tree-sitter based parser for Rust and frontend languages
//!
//! This module provides a single entry point for parsing all supported file types
//! using Tree-sitter queries defined in external .scm files.

use crate::indexer::{FileIndex, Finding};
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::{Position, Range};
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// Query files embedded at compile time
const RUST_QUERY: &str = include_str!("../queries/rust.scm");

// Common ECMAScript patterns (shared by TypeScript and JavaScript)
const COMMON_ECMA_QUERY: &str = include_str!("../queries/common-ecma.scm");

// TypeScript-specific patterns (generics, await with types)
const TS_SPECIFIC_QUERY: &str = include_str!("../queries/typescript-specific.scm");

// JavaScript uses only common patterns (no generics)
const JS_QUERY: &str = COMMON_ECMA_QUERY;

// Lazy-initialized combined TypeScript query (common + TypeScript-specific)
static TS_COMBINED_QUERY: OnceLock<String> = OnceLock::new();

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

/// Get the query string for a given language
fn get_query_source(lang: LangType) -> &'static str {
    match lang {
        LangType::Rust => RUST_QUERY,
        LangType::TypeScript | LangType::Vue | LangType::Svelte | LangType::Angular => {
            // Combine common ECMAScript patterns with TypeScript-specific patterns
            TS_COMBINED_QUERY.get_or_init(|| {
                format!("{}\n\n{}", COMMON_ECMA_QUERY, TS_SPECIFIC_QUERY)
            })
        }
        LangType::JavaScript => JS_QUERY,
    }
}

/// Extract ALL script blocks from SFC (Single File Component: Vue, Svelte, etc.)
/// Returns tuples of (script_content, line_offset) for each <script> block found
fn extract_script_blocks(content: &str) -> Vec<(String, usize)> {
    let mut blocks = Vec::new();
    let mut search_pos = 0;

    while let Some(tag_start) = content[search_pos..].find("<script") {
        let absolute_tag_start = search_pos + tag_start;

        // Find end of opening tag (>)
        let Some(tag_close_offset) = content[absolute_tag_start..].find('>') else {
            break;
        };
        let tag_close = absolute_tag_start + tag_close_offset + 1;

        // Find closing </script>
        let Some(end_tag_offset) = content[tag_close..].find("</script>") else {
            break;
        };
        let script_end = tag_close + end_tag_offset;

        // Extract script content
        let script_content = &content[tag_close..script_end];

        // Calculate line offset
        let line_offset = content[..tag_close].lines().count().saturating_sub(1);

        blocks.push((script_content.to_string(), line_offset));

        // Move search position past this script block
        search_pos = script_end + "</script>".len();
    }

    blocks
}

/// Convert tree-sitter Point to LSP Position
fn point_to_position(point: tree_sitter::Point) -> Position {
    Position {
        line: point.row as u32,
        character: point.column as u32,
    }
}

/// Adjust position by line offset (for Vue/Svelte script extraction)
fn adjust_position(pos: Position, line_offset: usize) -> Position {
    Position {
        line: pos.line + line_offset as u32,
        character: pos.character,
    }
}

/// Adjust range by line offset
fn adjust_range(range: Range, line_offset: usize) -> Range {
    Range {
        start: adjust_position(range.start, line_offset),
        end: adjust_position(range.end, line_offset),
    }
}

/// Get method patterns for Rust backend
fn get_rust_event_patterns() -> HashMap<&'static str, (EntityType, Behavior)> {
    let mut patterns = HashMap::new();
    // Emit methods
    patterns.insert("emit", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_to", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_str", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_str_to", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_filter", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_str_filter", (EntityType::Event, Behavior::Emit));
    // Listen methods
    patterns.insert("listen", (EntityType::Event, Behavior::Listen));
    patterns.insert("listen_any", (EntityType::Event, Behavior::Listen));
    patterns.insert("once", (EntityType::Event, Behavior::Listen));
    patterns.insert("once_any", (EntityType::Event, Behavior::Listen));
    patterns
}

/// Parse Rust source code
fn parse_rust(content: &str) -> ParseResult<Vec<Finding>> {
    let mut findings = Vec::new();

    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .map_err(|e| ParseError::LanguageError(format!("Failed to set Rust language: {}", e)))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError("Failed to parse Rust file".to_string()))?;

    let query = Query::new(&ts_lang, RUST_QUERY)
        .map_err(|e| ParseError::QueryError(format!("Failed to create Rust query: {}", e)))?;

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    // Get capture indices
    let command_name_idx = query.capture_index_for_name("command_name");
    let method_name_idx = query.capture_index_for_name("method_name");
    let event_name_idx = query.capture_index_for_name("event_name");

    let rust_event_patterns = get_rust_event_patterns();

    let mut matches = cursor.matches(&query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        // Process command definitions
        if let Some(cmd_idx) = command_name_idx {
            for capture in m.captures.iter().filter(|c| c.index == cmd_idx) {
                let node = capture.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or_default();

                findings.push(Finding {
                    key: name.to_string(),
                    entity: EntityType::Command,
                    behavior: Behavior::Definition,
                    range: Range {
                        start: point_to_position(node.start_position()),
                        end: point_to_position(node.end_position()),
                    },
                });
            }
        }

        // Process event method calls
        if let (Some(method_idx), Some(event_idx)) = (method_name_idx, event_name_idx) {
            let method_capture = m.captures.iter().find(|c| c.index == method_idx);
            let event_capture = m.captures.iter().find(|c| c.index == event_idx);

            if let (Some(method_cap), Some(event_cap)) = (method_capture, event_capture) {
                let method_name = method_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();
                let event_name = event_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();

                if let Some((entity, behavior)) = rust_event_patterns.get(method_name) {
                    findings.push(Finding {
                        key: event_name.to_string(),
                        entity: *entity,
                        behavior: *behavior,
                        range: Range {
                            start: point_to_position(event_cap.node.start_position()),
                            end: point_to_position(event_cap.node.end_position()),
                        },
                    });
                }
            }
        }
    }

    Ok(findings)
}

/// Function patterns with their argument position
struct FunctionPatternWithPos {
    name: &'static str,
    entity: EntityType,
    behavior: Behavior,
    arg_position: ArgPosition,
}

#[derive(Clone, Copy, PartialEq)]
enum ArgPosition {
    First,
    Second,
}

/// Get all frontend patterns including those with second argument
fn get_all_frontend_patterns() -> Vec<FunctionPatternWithPos> {
    vec![
        // First argument patterns
        FunctionPatternWithPos {
            name: "invoke",
            entity: EntityType::Command,
            behavior: Behavior::Call,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "emit",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "listen",
            entity: EntityType::Event,
            behavior: Behavior::Listen,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "once",
            entity: EntityType::Event,
            behavior: Behavior::Listen,
            arg_position: ArgPosition::First,
        },
        // Second argument patterns
        FunctionPatternWithPos {
            name: "emitTo",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::Second,
        },
    ]
}

/// Parse TypeScript/JavaScript source code
fn parse_frontend(content: &str, lang: LangType, line_offset: usize) -> ParseResult<Vec<Finding>> {
    let mut findings = Vec::new();

    let ts_lang: Language = match lang {
        LangType::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };

    let mut parser = Parser::new();
    parser.set_language(&ts_lang).map_err(|e| {
        ParseError::LanguageError(format!("Failed to set {:?} language: {}", lang, e))
    })?;

    let tree = parser.parse(content, None).ok_or_else(|| {
        ParseError::SyntaxError(format!("Failed to parse {:?} file", lang))
    })?;

    let query_src = get_query_source(lang);
    let query = Query::new(&ts_lang, query_src).map_err(|e| {
        ParseError::QueryError(format!("Failed to create {:?} query: {}", lang, e))
    })?;

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    // Build alias map from imports
    let mut aliases: HashMap<String, String> = HashMap::new();

    // Get capture indices for first argument patterns
    let func_name_idx = query.capture_index_for_name("func_name");
    let arg_value_idx = query.capture_index_for_name("arg_value");
    // Get capture indices for second argument patterns
    let func_name_second_idx = query.capture_index_for_name("func_name_second");
    let arg_value_second_idx = query.capture_index_for_name("arg_value_second");
    // Get capture indices for imports
    let imported_name_idx = query.capture_index_for_name("imported_name");
    let local_alias_idx = query.capture_index_for_name("local_alias");

    let all_patterns = get_all_frontend_patterns();

    // First pass: collect aliases
    let mut matches = cursor.matches(&query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        if let (Some(imp_idx), Some(alias_idx)) = (imported_name_idx, local_alias_idx) {
            let imported = m.captures.iter().find(|c| c.index == imp_idx);
            let local = m.captures.iter().find(|c| c.index == alias_idx);

            if let (Some(imp_cap), Some(local_cap)) = (imported, local) {
                let imported_name = imp_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();
                let local_name = local_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();

                aliases.insert(local_name.to_string(), imported_name.to_string());
            }
        }
    }

    // Second pass: collect function calls
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        // Try first argument pattern (func_name + arg_value)
        if let (Some(func_idx), Some(arg_idx)) = (func_name_idx, arg_value_idx) {
            let func_capture = m.captures.iter().find(|c| c.index == func_idx);
            let arg_capture = m.captures.iter().find(|c| c.index == arg_idx);

            if let (Some(func_cap), Some(arg_cap)) = (func_capture, arg_capture) {
                let func_name = func_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();
                let arg_value = arg_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();

                // Resolve alias to original name
                let original_name = aliases
                    .get(func_name)
                    .map(|s| s.as_str())
                    .unwrap_or(func_name);

                // Find matching pattern (first argument)
                if let Some(pattern) = all_patterns
                    .iter()
                    .find(|p| p.name == original_name && p.arg_position == ArgPosition::First)
                {
                    let range = Range {
                        start: point_to_position(arg_cap.node.start_position()),
                        end: point_to_position(arg_cap.node.end_position()),
                    };

                    findings.push(Finding {
                        key: arg_value.to_string(),
                        entity: pattern.entity,
                        behavior: pattern.behavior,
                        range: adjust_range(range, line_offset),
                    });
                }
            }
        }

        // Try second argument pattern (func_name_second + arg_value_second)
        if let (Some(func_idx), Some(arg_idx)) = (func_name_second_idx, arg_value_second_idx) {
            let func_capture = m.captures.iter().find(|c| c.index == func_idx);
            let arg_capture = m.captures.iter().find(|c| c.index == arg_idx);

            if let (Some(func_cap), Some(arg_cap)) = (func_capture, arg_capture) {
                let func_name = func_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();
                let arg_value = arg_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();

                // Resolve alias to original name
                let original_name = aliases
                    .get(func_name)
                    .map(|s| s.as_str())
                    .unwrap_or(func_name);

                // Find matching pattern (second argument)
                if let Some(pattern) = all_patterns
                    .iter()
                    .find(|p| p.name == original_name && p.arg_position == ArgPosition::Second)
                {
                    let range = Range {
                        start: point_to_position(arg_cap.node.start_position()),
                        end: point_to_position(arg_cap.node.end_position()),
                    };

                    findings.push(Finding {
                        key: arg_value.to_string(),
                        entity: pattern.entity,
                        behavior: pattern.behavior,
                        range: adjust_range(range, line_offset),
                    });
                }
            }
        }
    }

    Ok(findings)
}

/// Check if TypeScript file contains Angular decorators
fn is_angular_file(content: &str) -> bool {
    // Angular decorators that indicate this is an Angular file
    const ANGULAR_DECORATORS: &[&str] = &[
        "@Component(",
        "@Injectable(",
        "@NgModule(",
        "@Directive(",
        "@Pipe(",
    ];

    ANGULAR_DECORATORS.iter().any(|decorator| content.contains(decorator))
}

/// Main parsing function - entry point for all file types
pub fn parse(path: &Path, content: &str) -> ParseResult<FileIndex> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    // Check for Angular: content-based detection for .ts files
    let is_angular = ext == "ts" && is_angular_file(content);

    let lang = if is_angular {
        Some(LangType::Angular)
    } else {
        LangType::from_extension(ext)
    };

    let findings = match lang {
        Some(LangType::Rust) => parse_rust(content)?,
        Some(LangType::TypeScript) | Some(LangType::JavaScript) | Some(LangType::Angular) => {
            parse_frontend(content, lang.unwrap(), 0)?
        }
        Some(LangType::Vue) | Some(LangType::Svelte) => {
            let blocks = extract_script_blocks(content);
            let mut all_findings = Vec::new();

            for (script_content, line_offset) in blocks {
                let findings = parse_frontend(&script_content, LangType::TypeScript, line_offset)?;
                all_findings.extend(findings);
            }

            all_findings
        }
        None => Vec::new(),
    };

    Ok(FileIndex {
        path: path.to_path_buf(),
        findings,
    })
}

#[cfg(test)]
mod tests;
