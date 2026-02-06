//! Frontend (TypeScript/JavaScript) language parser using tree-sitter

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use super::extractors::{extract_ts_interface_fields, extract_ts_params};
use super::query_helpers::CaptureIndices;
use super::utils::{adjust_range, get_query_source, point_to_position, LangType, NodeTextExt};

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
pub fn parse_frontend(
    content: &str,
    lang: LangType,
    line_offset: usize,
) -> ParseResult<Vec<Finding>> {
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

    // Get capture indices using helper
    let indices = CaptureIndices::from_query(
        &query,
        &[
            "func_name",
            "arg_value",
            "func_name_second",
            "arg_value_second",
            "imported_name",
            "local_alias",
            "type_args",
            "invoke_args",
            "interface_def",
            "interface_name",
        ],
    );

    let all_patterns = get_all_frontend_patterns();

    // First pass: collect aliases and interface definitions
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Collect aliases
        if let Some(imp_cap) = indices.find_capture(m.captures, "imported_name") {
            if let Some(local_cap) = indices.find_capture(m.captures, "local_alias") {
                let imported_name = imp_cap.node.text_or_default(content);
                let local_name = local_cap.node.text_or_default(content);
                aliases.insert(local_name, imported_name);
            }
        }

        // Collect interfaces
        if let Some(iface_cap) = indices.find_capture(m.captures, "interface_def") {
            if let Some(name_cap) = indices.find_capture(m.captures, "interface_name") {
                let name = name_cap.node.text_or_default(content);
                let fields = extract_ts_interface_fields(iface_cap.node, content);

                findings.push(Finding {
                    key: name,
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

    // Second pass: collect function calls
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Try first argument pattern (func_name + arg_value)
        if let Some(func_cap) = indices.find_capture(m.captures, "func_name") {
            if let Some(arg_cap) = indices.find_capture(m.captures, "arg_value") {
                let func_name = func_cap.node.text_or_default(content);
                let arg_value = arg_cap.node.text_or_default(content);

                // Resolve alias to original name
                let original_name = aliases
                    .get(&func_name)
                    .map_or(func_name.as_str(), std::string::String::as_str);

                // Find matching pattern (first argument)
                if let Some(pattern) = all_patterns
                    .iter()
                    .find(|p| p.name == original_name && p.arg_position == ArgPosition::First)
                {
                    let range = Range {
                        start: point_to_position(arg_cap.node.start_position()),
                        end: point_to_position(arg_cap.node.end_position()),
                    };

                    let parameters = indices
                        .find_capture(m.captures, "invoke_args")
                        .map(|cap| extract_ts_params(cap.node, content));

                    let return_type = indices
                        .find_capture(m.captures, "type_args")
                        .map(|cap| cap.node.text_or_default(content));

                    findings.push(Finding {
                        key: arg_value,
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
        if let Some(func_cap) = indices.find_capture(m.captures, "func_name_second") {
            if let Some(arg_cap) = indices.find_capture(m.captures, "arg_value_second") {
                let func_name = func_cap.node.text_or_default(content);
                let arg_value = arg_cap.node.text_or_default(content);

                // Resolve alias to original name
                let original_name = aliases
                    .get(&func_name)
                    .map_or(func_name.as_str(), std::string::String::as_str);

                // Find matching pattern (second argument)
                if let Some(pattern) = all_patterns
                    .iter()
                    .find(|p| p.name == original_name && p.arg_position == ArgPosition::Second)
                {
                    let range = Range {
                        start: point_to_position(arg_cap.node.start_position()),
                        end: point_to_position(arg_cap.node.end_position()),
                    };

                    let parameters = indices
                        .find_capture(m.captures, "invoke_args")
                        .map(|cap| extract_ts_params(cap.node, content));

                    let return_type = indices
                        .find_capture(m.captures, "type_args")
                        .map(|cap| cap.node.text_or_default(content));

                    findings.push(Finding {
                        key: arg_value,
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
pub fn is_angular_file(content: &str) -> bool {
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
