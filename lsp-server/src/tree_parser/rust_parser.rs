//! Rust language parser using tree-sitter

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use super::extractors::{
    extract_rust_enum_variants, extract_rust_params, extract_rust_struct_fields,
};
use super::utils::{get_query_source, point_to_position, LangType};

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
pub fn parse_rust(content: &str) -> ParseResult<Vec<Finding>> {
    let mut findings = Vec::new();

    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();

    parser
        .set_language(&ts_lang)
        .map_err(|e| ParseError::LanguageError(format!("Failed to set Rust language: {e}")))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError("Failed to parse Rust file".to_string()))?;

    let query_src = get_query_source(LangType::Rust);
    let query = Query::new(&ts_lang, query_src)
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
