//! Unified Tree-sitter based parser for Rust and frontend languages
//!
//! This module provides a single entry point for parsing all supported file types
//! using Tree-sitter queries defined in external .scm files.

use crate::indexer::{FileIndex, Finding, Parameter};
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::{Position, Range};
use tree_sitter::{Language, Node, Parser, Query, QueryCursor};

/// Query files embedded at compile time
const RUST_QUERY: &str = include_str!("queries/rust.scm");
const TS_QUERY: &str = include_str!("queries/typescript.scm");
const JS_QUERY: &str = include_str!("queries/javascript.scm");

/// Extract Rust function parameters
fn extract_rust_params(node: Node, content: &str) -> Vec<Parameter> {
    let mut params = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "parameter" {
            let name_node = child.child_by_field_name("pattern");
            let type_node = child.child_by_field_name("type");

            if let (Some(n), Some(t)) = (name_node, type_node) {
                params.push(Parameter {
                    name: n
                        .utf8_text(content.as_bytes())
                        .unwrap_or_default()
                        .to_string(),
                    type_name: t
                        .utf8_text(content.as_bytes())
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }
    }
    params
}

/// Extract Rust struct fields
fn extract_rust_struct_fields(node: Node, content: &str) -> Vec<Parameter> {
    let mut fields = Vec::new();
    let mut cursor = node.walk();

    // Navigate to field_declaration_list
    for child in node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            let mut field_cursor = child.walk();

            for field in child.children(&mut field_cursor) {
                if field.kind() == "field_declaration" {
                    let name_node = field.child_by_field_name("name");
                    let type_node = field.child_by_field_name("type");

                    if let (Some(n), Some(t)) = (name_node, type_node) {
                        fields.push(Parameter {
                            name: n
                                .utf8_text(content.as_bytes())
                                .unwrap_or_default()
                                .to_string(),
                            type_name: t
                                .utf8_text(content.as_bytes())
                                .unwrap_or_default()
                                .to_string(),
                        });
                    }
                }
            }
        }
    }

    fields
}

/// Extract Rust enum variants
fn extract_rust_enum_variants(node: Node, content: &str) -> Vec<Parameter> {
    let mut variants = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "enum_variant_list" {
            let mut variant_cursor = child.walk();

            for variant in child.children(&mut variant_cursor) {
                if variant.kind() == "enum_variant" {
                    let name_node = variant.child_by_field_name("name");

                    if let Some(n) = name_node {
                        variants.push(Parameter {
                            name: n
                                .utf8_text(content.as_bytes())
                                .unwrap_or_default()
                                .to_string(),
                            type_name: "enum_variant".to_string(),
                        });
                    }
                }
            }
        }
    }

    variants
}

/// Extract TypeScript interface fields
fn extract_ts_interface_fields(node: Node, content: &str) -> Vec<Parameter> {
    let mut fields = Vec::new();
    let mut cursor = node.walk();

    // Navigate to interface_body
    for child in node.children(&mut cursor) {
        if child.kind() == "interface_body" {
            let mut field_cursor = child.walk();

            for field in child.children(&mut field_cursor) {
                if field.kind() == "property_signature" {
                    let name_node = field.child_by_field_name("name");
                    let type_ann_node = field.child_by_field_name("type");

                    if let (Some(n), Some(ta)) = (name_node, type_ann_node) {
                        // type_annotation has a child which is the actual type
                        let mut ta_cursor = ta.walk();

                        let type_node = ta
                            .children(&mut ta_cursor)
                            .find(|c| c.kind() != ":" && c.kind() != "comment");

                        if let Some(tn) = type_node {
                            fields.push(Parameter {
                                name: n
                                    .utf8_text(content.as_bytes())
                                    .unwrap_or_default()
                                    .to_string(),
                                type_name: tn
                                    .utf8_text(content.as_bytes())
                                    .unwrap_or_default()
                                    .to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    fields
}

/// Extract TypeScript parameters from an object literal (invoke arguments)
fn extract_ts_params(node: Node, content: &str) -> Vec<Parameter> {
    let mut params = Vec::new();

    if node.kind() == "object" {
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            // Handle { key: value } syntax
            if child.kind() == "pair" {
                let key_node = child.child_by_field_name("key");
                let value_node = child.child_by_field_name("value");

                if let Some(k) = key_node {
                    let name = k
                        .utf8_text(content.as_bytes())
                        .unwrap_or_default()
                        .to_string();
                    let mut type_name = "any".to_string();

                    if let Some(v) = value_node {
                        // Very basic type inference from literal values
                        type_name = match v.kind() {
                            "string" => "string",
                            "number" => "number",
                            "true" | "false" => "boolean",
                            "array" => "any[]",
                            "object" => "object",
                            _ => "any",
                        }
                        .to_string();
                    }

                    params.push(Parameter { name, type_name });
                }
            }
            // Handle { name } shorthand syntax (shorthand_property_identifier)
            else if child.kind() == "shorthand_property_identifier"
                || child.kind() == "shorthand_property_identifier_pattern"
            {
                let name = child
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default()
                    .to_string();

                // For shorthand, we can't infer type from literal - it's a variable reference
                params.push(Parameter {
                    name,
                    type_name: "any".to_string(),
                });
            }
        }
    }
    params
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

#[allow(clippy::too_many_lines)]
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
    let command_name_idx = query.capture_index_for_name("command_name");
    let command_params_idx = query.capture_index_for_name("command_params");
    let command_return_type_idx = query.capture_index_for_name("command_return_type");
    let struct_def_idx = query.capture_index_for_name("struct_def");
    let struct_name_idx = query.capture_index_for_name("struct_name");
    let struct_attr_idx = query.capture_index_for_name("struct_attr");
    let enum_def_idx = query.capture_index_for_name("enum_def");
    let enum_name_idx = query.capture_index_for_name("enum_name");
    let enum_attr_idx = query.capture_index_for_name("enum_attr");
    let method_name_idx = query.capture_index_for_name("method_name");
    let event_name_idx = query.capture_index_for_name("event_name");

    let rust_event_patterns = get_rust_event_patterns();

    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Process struct definitions
        if let Some(struct_idx) = struct_def_idx {
            if let Some(struct_cap) = m.captures.iter().find(|c| c.index == struct_idx) {
                // Find name within this match
                if let Some(name_idx) = struct_name_idx {
                    if let Some(name_cap) = m.captures.iter().find(|c| c.index == name_idx) {
                        let name = name_cap
                            .node
                            .utf8_text(content.as_bytes())
                            .unwrap_or_default();
                        let fields = extract_rust_struct_fields(struct_cap.node, content);

                        let mut attributes = Vec::new();

                        if let Some(attr_idx) = struct_attr_idx {
                            for attr_cap in m.captures.iter().filter(|c| c.index == attr_idx) {
                                attributes.push(
                                    attr_cap
                                        .node
                                        .utf8_text(content.as_bytes())
                                        .unwrap_or_default()
                                        .to_string(),
                                );
                            }
                        }

                        findings.push(Finding {
                            key: name.to_string(),
                            entity: EntityType::Struct,
                            behavior: Behavior::Definition,
                            range: Range {
                                start: point_to_position(name_cap.node.start_position()),
                                end: point_to_position(name_cap.node.end_position()),
                            },
                            parameters: None,
                            return_type: None,
                            fields: Some(fields),
                            attributes: if attributes.is_empty() {
                                None
                            } else {
                                Some(attributes)
                            },
                        });
                    }
                }
            }
        }

        // Process enum definitions
        if let Some(enum_idx) = enum_def_idx {
            if let Some(enum_cap) = m.captures.iter().find(|c| c.index == enum_idx) {
                if let Some(name_idx) = enum_name_idx {
                    if let Some(name_cap) = m.captures.iter().find(|c| c.index == name_idx) {
                        let name = name_cap
                            .node
                            .utf8_text(content.as_bytes())
                            .unwrap_or_default();
                        let variants = extract_rust_enum_variants(enum_cap.node, content);

                        let mut attributes = Vec::new();

                        if let Some(attr_idx) = enum_attr_idx {
                            for attr_cap in m.captures.iter().filter(|c| c.index == attr_idx) {
                                attributes.push(
                                    attr_cap
                                        .node
                                        .utf8_text(content.as_bytes())
                                        .unwrap_or_default()
                                        .to_string(),
                                );
                            }
                        }

                        findings.push(Finding {
                            key: name.to_string(),
                            entity: EntityType::Enum,
                            behavior: Behavior::Definition,
                            range: Range {
                                start: point_to_position(name_cap.node.start_position()),
                                end: point_to_position(name_cap.node.end_position()),
                            },
                            parameters: None,
                            return_type: None,
                            fields: Some(variants),
                            attributes: if attributes.is_empty() {
                                None
                            } else {
                                Some(attributes)
                            },
                        });
                    }
                }
            }
        }

        // Process command definitions
        if let Some(cmd_idx) = command_name_idx {
            for capture in m.captures.iter().filter(|c| c.index == cmd_idx) {
                let node = capture.node;
                let name = node.utf8_text(content.as_bytes()).unwrap_or_default();

                // Extract parameters and return type
                let mut parameters = None;
                let mut return_type = None;

                if let Some(params_idx) = command_params_idx {
                    if let Some(params_cap) = m.captures.iter().find(|c| c.index == params_idx) {
                        parameters = Some(extract_rust_params(params_cap.node, content));
                    }
                }

                if let Some(ret_idx) = command_return_type_idx {
                    if let Some(ret_cap) = m.captures.iter().find(|c| c.index == ret_idx) {
                        return_type = Some(
                            ret_cap
                                .node
                                .utf8_text(content.as_bytes())
                                .unwrap_or_default()
                                .to_string(),
                        );
                    }
                }

                findings.push(Finding {
                    key: name.to_string(),
                    entity: EntityType::Command,
                    behavior: Behavior::Definition,
                    range: Range {
                        start: point_to_position(node.start_position()),
                        end: point_to_position(node.end_position()),
                    },
                    parameters,
                    return_type,
                    fields: None,
                    attributes: None,
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
                        parameters: None,
                        return_type: None,
                        fields: None,
                        attributes: None,
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
    // New captures
    let type_args_idx = query.capture_index_for_name("type_args");
    let invoke_args_idx = query.capture_index_for_name("invoke_args");
    let interface_def_idx = query.capture_index_for_name("interface_def");
    let interface_name_idx = query.capture_index_for_name("interface_name");

    let all_patterns = get_all_frontend_patterns();

    // First pass: collect aliases and interface definitions
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Collect aliases
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

        // Collect interfaces
        if let Some(iface_idx) = interface_def_idx {
            if let Some(iface_cap) = m.captures.iter().find(|c| c.index == iface_idx) {
                if let Some(name_idx) = interface_name_idx {
                    if let Some(name_cap) = m.captures.iter().find(|c| c.index == name_idx) {
                        let name = name_cap
                            .node
                            .utf8_text(content.as_bytes())
                            .unwrap_or_default();

                        let fields = extract_ts_interface_fields(iface_cap.node, content);

                        findings.push(Finding {
                            key: name.to_string(),
                            entity: EntityType::Interface,
                            behavior: Behavior::Definition,
                            range: adjust_range(
                                Range {
                                    start: point_to_position(name_cap.node.start_position()),
                                    end: point_to_position(name_cap.node.end_position()),
                                },
                                line_offset,
                            ),
                            parameters: None,
                            return_type: None,
                            fields: Some(fields),
                            attributes: None,
                        });
                    }
                }
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

                    let mut parameters = None;
                    let mut return_type = None;

                    if let Some(args_idx) = invoke_args_idx {
                        if let Some(args_cap) = m.captures.iter().find(|c| c.index == args_idx) {
                            parameters = Some(extract_ts_params(args_cap.node, content));
                        }
                    }

                    if let Some(t_idx) = type_args_idx {
                        if let Some(t_cap) = m.captures.iter().find(|c| c.index == t_idx) {
                            return_type = Some(
                                t_cap
                                    .node
                                    .utf8_text(content.as_bytes())
                                    .unwrap_or_default()
                                    .to_string(),
                            );
                        }
                    }

                    findings.push(Finding {
                        key: arg_value.to_string(),
                        entity: pattern.entity,
                        behavior: pattern.behavior,
                        range: adjust_range(range, line_offset),
                        parameters,
                        return_type,
                        fields: None,
                        attributes: None,
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

                    let mut parameters = None;
                    let mut return_type = None;

                    if let Some(args_idx) = invoke_args_idx {
                        if let Some(args_cap) = m.captures.iter().find(|c| c.index == args_idx) {
                            parameters = Some(extract_ts_params(args_cap.node, content));
                        }
                    }

                    if let Some(t_idx) = type_args_idx {
                        if let Some(t_cap) = m.captures.iter().find(|c| c.index == t_idx) {
                            return_type = Some(
                                t_cap
                                    .node
                                    .utf8_text(content.as_bytes())
                                    .unwrap_or_default()
                                    .to_string(),
                            );
                        }
                    }

                    findings.push(Finding {
                        key: arg_value.to_string(),
                        entity: pattern.entity,
                        behavior: pattern.behavior,
                        range: adjust_range(range, line_offset),
                        parameters,
                        return_type,
                        fields: None,
                        attributes: None,
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
