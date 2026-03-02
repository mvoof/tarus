//! Unified Tree-sitter based parser for Rust and frontend languages
//!
//! This module provides a single entry point for parsing all supported file types
//! using Tree-sitter queries defined in external .scm files.

use crate::indexer::{FileIndex, Finding};
use crate::rust_type_extractor;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::{Position, Range};
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// Query files embedded at compile time
const RUST_QUERY: &str = include_str!("queries/rust.scm");
const TS_QUERY: &str = include_str!("queries/typescript.scm");
const JS_QUERY: &str = include_str!("queries/javascript.scm");

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

/// Get the query string for a given language
fn get_query_source(lang: LangType) -> &'static str {
    match lang {
        LangType::Rust => RUST_QUERY,
        LangType::TypeScript | LangType::Vue | LangType::Svelte | LangType::Angular => TS_QUERY,
        LangType::JavaScript => JS_QUERY,
    }
}

/// Extract ALL script blocks from SFC (Single File Component: Vue, Svelte, etc.)
/// Returns tuples of (`script_content`, `line_offset`) for each <script> block found
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
#[allow(clippy::cast_possible_truncation)]
fn point_to_position(point: tree_sitter::Point) -> Position {
    Position {
        line: point.row as u32,
        character: point.column as u32,
    }
}

/// Adjust position by line offset (for Vue/Svelte script extraction)
#[allow(clippy::cast_possible_truncation)]
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
        .map_err(|e| ParseError::LanguageError(format!("Failed to set Rust language: {e}")))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError("Failed to parse Rust file".to_string()))?;

    let query = Query::new(&ts_lang, RUST_QUERY)
        .map_err(|e| ParseError::QueryError(format!("Failed to create Rust query: {e}")))?;

    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

    // Get capture indices
    let fn_name_idx = query.capture_index_for_name("fn_name");
    let fn_item_idx = query.capture_index_for_name("fn_item");
    let method_name_idx = query.capture_index_for_name("method_name");
    let event_name_idx = query.capture_index_for_name("event_name");

    let rust_event_patterns = get_rust_event_patterns();

    let mut matches = cursor.matches(&query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        // Process function_item — check if it's a #[tauri::command] via sibling walk
        if let (Some(name_idx), Some(item_idx)) = (fn_name_idx, fn_item_idx) {
            let name_cap = m.captures.iter().find(|c| c.index == name_idx);
            let item_cap = m.captures.iter().find(|c| c.index == item_idx);

            if let (Some(name_cap), Some(item_cap)) = (name_cap, item_cap) {
                if rust_type_extractor::has_tauri_command_attr(item_cap.node, content) {
                    let name = name_cap
                        .node
                        .utf8_text(content.as_bytes())
                        .unwrap_or_default();
                    findings.push(Finding {
                        key: name.to_string(),
                        entity: EntityType::Command,
                        behavior: Behavior::Definition,
                        range: Range {
                            start: point_to_position(name_cap.node.start_position()),
                            end: point_to_position(name_cap.node.end_position()),
                        },
                        call_arg_count: None,
                        call_param_keys: None,
                        return_type: None,
                        call_name_end: None,
                        type_arg_range: None,
                    });
                }
                continue;
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
                        call_arg_count: None,
                        call_param_keys: None,
                        return_type: None,
                        call_name_end: None,
                        type_arg_range: None,
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
        // First argument patterns - Commands
        FunctionPatternWithPos {
            name: "invoke",
            entity: EntityType::Command,
            behavior: Behavior::Call,
            arg_position: ArgPosition::First,
        },
        // First argument patterns - Events (emit)
        FunctionPatternWithPos {
            name: "emit",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::First,
        },
        // First argument patterns - Events (listen/subscribe)
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
#[allow(clippy::too_many_lines)]
fn parse_frontend(content: &str, lang: LangType, line_offset: usize) -> ParseResult<Vec<Finding>> {
    let mut findings = Vec::new();

    let ts_lang: Language = match lang {
        LangType::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        _ => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
    };

    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .map_err(|e| ParseError::LanguageError(format!("Failed to set {lang:?} language: {e}")))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError(format!("Failed to parse {lang:?} file")))?;

    let query_src = get_query_source(lang);
    let query = Query::new(&ts_lang, query_src)
        .map_err(|e| ParseError::QueryError(format!("Failed to create {lang:?} query: {e}")))?;

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
    // Get capture indices for generic call nodes (to extract type_arguments)
    let call_generic_idx = query.capture_index_for_name("call_generic");
    let call_await_generic_idx = query.capture_index_for_name("call_await_generic");
    // Get capture indices for Specta calls
    let specta_method_name_idx = query.capture_index_for_name("specta_method_name");
    let specta_call_idx = query.capture_index_for_name("specta_call");

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
                    .map_or(func_name, std::string::String::as_str);

                // Find matching pattern (first argument)
                if let Some(pattern) = all_patterns
                    .iter()
                    .find(|p| p.name == original_name && p.arg_position == ArgPosition::First)
                {
                    let range = Range {
                        start: point_to_position(arg_cap.node.start_position()),
                        end: point_to_position(arg_cap.node.end_position()),
                    };

                    // Extract call_name_end: end of the function identifier (e.g. end of "invoke")
                    let call_name_end = Some(adjust_position(
                        point_to_position(func_cap.node.end_position()),
                        line_offset,
                    ));

                    // Extract type argument from generic calls: invoke<T>("cmd") → "T"
                    let type_arg_info = extract_type_argument_info(m, call_generic_idx, call_await_generic_idx, content);
                    let return_type = type_arg_info.as_ref().map(|i| i.type_text.clone());
                    let type_arg_range = type_arg_info.map(|i| adjust_range(i.type_arg_range, line_offset));

                    findings.push(Finding {
                        key: arg_value.to_string(),
                        entity: pattern.entity,
                        behavior: pattern.behavior,
                        range: adjust_range(range, line_offset),
                        call_arg_count: None,
                        call_param_keys: None,
                        return_type,
                        call_name_end,
                        type_arg_range,
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
                    .map_or(func_name, std::string::String::as_str);

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
                        call_arg_count: None,
                        call_param_keys: None,
                        return_type: None,
                        call_name_end: None,
                        type_arg_range: None,
                    });
                }
            }
        }

        // Try SpectaCall pattern (commands.methodName(...))
        if let Some(specta_idx) = specta_method_name_idx {
            if let Some(specta_cap) = m.captures.iter().find(|c| c.index == specta_idx) {
                let camel_name = specta_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();
                let snake_name = crate::utils::camel_to_snake(camel_name);

                let method_range = Range {
                    start: point_to_position(specta_cap.node.start_position()),
                    end: point_to_position(specta_cap.node.end_position()),
                };

                // Count arguments by walking the call_expression node's arguments
                let arg_count = count_specta_call_args(m, specta_call_idx, content);

                findings.push(Finding {
                    key: snake_name,
                    entity: EntityType::Command,
                    behavior: Behavior::SpectaCall,
                    range: adjust_range(method_range, line_offset),
                    call_arg_count: Some(arg_count),
                    call_param_keys: None,
                    return_type: None,
                    call_name_end: None,
                    type_arg_range: None,
                });
            }
        }
    }

    Ok(findings)
}

/// Result of extracting type argument info from a generic call expression.
struct TypeArgInfo {
    /// The type text (e.g. "User" from `invoke<User>`)
    type_text: String,
    /// The range of the full `<User>` including angle brackets
    type_arg_range: Range,
}

/// Extract the type argument text and range from a generic call expression.
///
/// For `invoke<User>("cmd")`, the `call_expression` node has a `type_arguments` child
/// containing `<User>`. We strip the angle brackets to return `"User"` and also
/// return the range of `<User>` for code action replacement.
fn extract_type_argument_info(
    m: &tree_sitter::QueryMatch<'_, '_>,
    call_generic_idx: Option<u32>,
    call_await_generic_idx: Option<u32>,
    content: &str,
) -> Option<TypeArgInfo> {
    // Find the call_expression node from the generic pattern captures
    let call_node = call_generic_idx
        .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
        .or_else(|| {
            call_await_generic_idx
                .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
        })?;

    // Walk children to find type_arguments
    let node = call_node.node;
    let mut tree_cursor = node.walk();
    for child in node.children(&mut tree_cursor) {
        if child.kind() == "type_arguments" {
            let text = child.utf8_text(content.as_bytes()).unwrap_or_default();
            let type_arg_range = Range {
                start: point_to_position(child.start_position()),
                end: point_to_position(child.end_position()),
            };
            // Strip angle brackets: "<User>" → "User"
            let trimmed = text.strip_prefix('<').unwrap_or(text);
            let trimmed = trimmed.strip_suffix('>').unwrap_or(trimmed);
            let trimmed = trimmed.trim();
            if !trimmed.is_empty() {
                return Some(TypeArgInfo {
                    type_text: trimmed.to_string(),
                    type_arg_range,
                });
            }
        }
    }

    None
}

/// Count the positional arguments in a `SpectaCall` expression.
fn count_specta_call_args(
    m: &tree_sitter::QueryMatch<'_, '_>,
    specta_call_idx: Option<u32>,
    content: &str,
) -> u32 {
    let Some(call_idx) = specta_call_idx else {
        return 0;
    };
    let Some(call_cap) = m.captures.iter().find(|c| c.index == call_idx) else {
        return 0;
    };

    // Find the arguments node among children of the call_expression
    let call_node = call_cap.node;
    let mut tree_cursor = call_node.walk();
    let children: Vec<_> = call_node.children(&mut tree_cursor).collect();

    for child in &children {
        if child.kind() == "arguments" {
            // Count non-punctuation children of arguments
            let mut arg_cursor = child.walk();
            let count = child
                .children(&mut arg_cursor)
                .filter(|n| n.kind() != "," && n.kind() != "(" && n.kind() != ")")
                .count();
            #[allow(clippy::cast_possible_truncation)]
            return count as u32;
        }
    }

    // Fallback: look for arguments via text
    let _ = content;
    0
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

    ANGULAR_DECORATORS
        .iter()
        .any(|decorator| content.contains(decorator))
}

/// Main parsing function - entry point for all file types
///
/// # Errors
///
/// Returns error if tree-sitter fails to parse the file or query execution fails
///
/// # Panics
///
/// Panics if language detection succeeds but lang is None (should never happen due to match guards)
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
        Some(LangType::TypeScript | LangType::JavaScript | LangType::Angular) => {
            parse_frontend(content, lang.unwrap(), 0)?
        }
        Some(LangType::Vue | LangType::Svelte) => {
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
