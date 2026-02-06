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
use super::query_helpers::CaptureIndices;
use super::utils::{get_query_source, point_to_position, LangType, NodeTextExt};

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

    // Get capture indices using helper
    let indices = CaptureIndices::from_query(
        &query,
        &[
            "command_name",
            "command_params",
            "command_return_type",
            "struct_def",
            "struct_name",
            "struct_attr",
            "enum_def",
            "enum_name",
            "enum_attr",
            "method_name",
            "event_name",
        ],
    );

    let rust_event_patterns = get_rust_event_patterns();

    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Process struct definitions
        if let Some(struct_cap) = indices.find_capture(m.captures, "struct_def") {
            if let Some(name_cap) = indices.find_capture(m.captures, "struct_name") {
                let name = name_cap.node.text_or_default(content);
                let fields = extract_rust_struct_fields(struct_cap.node, content);

                let attributes: Vec<String> = indices
                    .find_captures(m.captures, "struct_attr")
                    .iter()
                    .map(|cap| cap.node.text_or_default(content))
                    .collect();

                findings.push(Finding {
                    key: name,
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

        // Process enum definitions
        if let Some(enum_cap) = indices.find_capture(m.captures, "enum_def") {
            if let Some(name_cap) = indices.find_capture(m.captures, "enum_name") {
                let name = name_cap.node.text_or_default(content);
                let variants = extract_rust_enum_variants(enum_cap.node, content);

                let attributes: Vec<String> = indices
                    .find_captures(m.captures, "enum_attr")
                    .iter()
                    .map(|cap| cap.node.text_or_default(content))
                    .collect();

                findings.push(Finding {
                    key: name,
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

        // Process command definitions
        for capture in indices.find_captures(m.captures, "command_name") {
            let node = capture.node;
            let name = node.text_or_default(content);

            // Extract parameters and return type
            let parameters = indices
                .find_capture(m.captures, "command_params")
                .map(|cap| extract_rust_params(cap.node, content));

            let return_type = indices
                .find_capture(m.captures, "command_return_type")
                .map(|cap| cap.node.text_or_default(content));

            findings.push(Finding {
                key: name,
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

        // Process event method calls
        if let Some(method_cap) = indices.find_capture(m.captures, "method_name") {
            if let Some(event_cap) = indices.find_capture(m.captures, "event_name") {
                let method_name = method_cap.node.text_or_default(content);
                let event_name = event_cap.node.text_or_default(content);

                if let Some((entity, behavior)) = rust_event_patterns.get(method_name.as_str()) {
                    findings.push(Finding {
                        key: event_name,
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
