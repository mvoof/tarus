//! Rust language parser using tree-sitter

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::ls_types::Range;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use super::extractors::{
    extract_rust_enum_variants, extract_rust_params, extract_rust_struct_fields, FindingBuilder,
};
use super::patterns::get_rust_event_patterns;
use super::query_helpers::CaptureIndices;
use super::utils::{get_query_source, point_to_position, LangType, NodeTextExt};

// Local pattern definition removed - using patterns::get_rust_event_patterns

/// Process struct definition match
#[must_use]
pub fn process_struct_match(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
) -> Option<Finding> {
    let struct_cap = indices.find_capture(m.captures, "struct_def")?;
    let name_cap = indices.find_capture(m.captures, "struct_name")?;

    let name = name_cap.node.text_or_default(content);
    let fields = extract_rust_struct_fields(struct_cap.node, content);

    let attributes: Vec<String> = indices
        .find_captures(m.captures, "struct_attr")
        .iter()
        .map(|cap| cap.node.text_or_default(content))
        .collect();

    Some(
        FindingBuilder::new(
            name,
            EntityType::Struct,
            Behavior::Definition,
            Range {
                start: point_to_position(name_cap.node.start_position()),
                end: point_to_position(name_cap.node.end_position()),
            },
        )
        .with_fields(fields)
        .with_attributes(attributes)
        .build(),
    )
}

/// Process enum definition match
#[must_use]
pub fn process_enum_match(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
) -> Option<Finding> {
    let enum_cap = indices.find_capture(m.captures, "enum_def")?;
    let name_cap = indices.find_capture(m.captures, "enum_name")?;

    let name = name_cap.node.text_or_default(content);
    let variants = extract_rust_enum_variants(enum_cap.node, content);

    let attributes: Vec<String> = indices
        .find_captures(m.captures, "enum_attr")
        .iter()
        .map(|cap| cap.node.text_or_default(content))
        .collect();

    Some(
        FindingBuilder::new(
            name,
            EntityType::Enum,
            Behavior::Definition,
            Range {
                start: point_to_position(name_cap.node.start_position()),
                end: point_to_position(name_cap.node.end_position()),
            },
        )
        .with_fields(variants)
        .with_attributes(attributes)
        .build(),
    )
}

/// Process command definition matches
#[must_use]
pub fn process_command_matches(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
) -> Vec<Finding> {
    let mut findings = Vec::new();

    for capture in indices.find_captures(m.captures, "command_name") {
        let node = capture.node;
        let name = node.text_or_default(content);

        let parameters = indices
            .find_capture(m.captures, "command_params")
            .map(|cap| extract_rust_params(cap.node, content));

        let return_type = indices
            .find_capture(m.captures, "command_return_type")
            .map(|cap| cap.node.text_or_default(content));

        findings.push(
            FindingBuilder::new(
                name,
                EntityType::Command,
                Behavior::Definition,
                Range {
                    start: point_to_position(node.start_position()),
                    end: point_to_position(node.end_position()),
                },
            )
            .with_parameters_opt(parameters)
            .with_return_type_opt(return_type)
            .build(),
        );
    }

    findings
}

/// Process event method call match
#[must_use]
pub fn process_event_match<S: std::hash::BuildHasher>(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
    patterns: &HashMap<&str, (EntityType, Behavior), S>,
) -> Option<Finding> {
    let method_cap = indices.find_capture(m.captures, "method_name")?;
    let event_cap = indices.find_capture(m.captures, "event_name")?;

    let method_name = method_cap.node.text_or_default(content);
    let event_name = event_cap.node.text_or_default(content);

    let (entity, behavior) = patterns.get(method_name.as_str())?;

    let return_type = indices
        .find_capture(m.captures, "event_args")
        .and_then(|cap| infer_payload_type(cap.node, content));

    Some(
        FindingBuilder::new(
            event_name,
            *entity,
            *behavior,
            Range {
                start: point_to_position(event_cap.node.start_position()),
                end: point_to_position(event_cap.node.end_position()),
            },
        )
        .with_return_type_opt(return_type)
        .build(),
    )
}

/// Infer the Rust type from a payload expression AST node
///
/// Analyzes the tree-sitter node kind to determine the Rust type of an event payload.
fn infer_payload_type(node: tree_sitter::Node, content: &str) -> Option<String> {
    match node.kind() {
        // CalculationStatus::Success -> "CalculationStatus"
        "scoped_identifier" => {
            let path = node.child_by_field_name("path")?;
            Some(path.text_or_default(content))
        }
        // MyStruct { field: value } -> "MyStruct"
        // CalculationStatus::Partial { warning: ... } -> "CalculationStatus"
        "struct_expression" => {
            let name = node.child_by_field_name("name")?;
            // For struct-like enum variants, the name is a scoped_identifier
            // (e.g., CalculationStatus::Partial) — extract just the enum type
            if name.kind() == "scoped_identifier" {
                let path = name.child_by_field_name("path")?;
                Some(path.text_or_default(content))
            } else {
                Some(name.text_or_default(content))
            }
        }
        // Type::new() or Type::from(...) -> "Type"
        "call_expression" => {
            let func = node.child_by_field_name("function")?;
            if func.kind() == "scoped_identifier" {
                let path = func.child_by_field_name("path")?;
                Some(path.text_or_default(content))
            } else {
                None
            }
        }
        // Literal types
        "string_literal" => Some("String".to_string()),
        "integer_literal" => Some("i32".to_string()),
        "float_literal" => Some("f64".to_string()),
        "true" | "false" => Some("bool".to_string()),
        // &payload -> recurse on inner expression
        "reference_expression" => {
            let inner = node.child_by_field_name("value")?;
            infer_payload_type(inner, content)
        }
        // Variable identifier -> resolve via let-binding or function parameter lookup
        "identifier" => resolve_variable_type(node, content),
        _ => None,
    }
}

/// Resolve the type of a variable by searching for its let-binding or function parameter
///
/// Walks up the tree to find the enclosing block, then searches for a `let` declaration
/// that matches the variable name. If found, extracts the type from the annotation or
/// infers it from the initializer value.
fn resolve_variable_type(node: tree_sitter::Node, content: &str) -> Option<String> {
    let var_name = node.text_or_default(content);

    // Walk up to find the enclosing block or function body
    let mut current = node.parent()?;
    loop {
        if current.kind() == "block" || current.kind() == "function_item" {
            break;
        }
        current = current.parent()?;
    }

    // If we found a block, search its children for let declarations
    if current.kind() == "block" {
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.kind() == "let_declaration" {
                // Check if this let binds our variable name
                let pattern = child.child_by_field_name("pattern");
                if let Some(pat) = pattern {
                    if pat.text_or_default(content) == var_name {
                        // Check for explicit type annotation: let x: Type = ...
                        if let Some(type_node) = child.child_by_field_name("type") {
                            return Some(type_node.text_or_default(content));
                        }
                        // Otherwise, infer from initializer value
                        if let Some(value_node) = child.child_by_field_name("value") {
                            return infer_from_initializer(value_node, content);
                        }
                    }
                }
            }
        }
    }

    // Check function parameters (current node or its parent)
    let func_node = if current.kind() == "function_item" {
        Some(current)
    } else {
        current.parent().filter(|p| p.kind() == "function_item")
    };
    if let Some(func) = func_node {
        if let Some(result) = find_param_type(func, &var_name, content) {
            return Some(result);
        }
    }

    None
}

/// Find a parameter type by name in a `function_item` node
fn find_param_type(
    func_node: tree_sitter::Node,
    var_name: &str,
    content: &str,
) -> Option<String> {
    let params = func_node.child_by_field_name("parameters")?;
    let mut cursor = params.walk();
    for child in params.children(&mut cursor) {
        if child.kind() == "parameter" {
            let name_node = child.child_by_field_name("pattern");
            let type_node = child.child_by_field_name("type");
            if let (Some(n), Some(t)) = (name_node, type_node) {
                if n.text_or_default(content) == var_name {
                    return Some(t.text_or_default(content));
                }
            }
        }
    }
    None
}

/// Infer the type from an initializer expression
///
/// Handles common patterns like `if` expressions and `match` expressions
/// by analyzing their inner blocks.
fn infer_from_initializer(node: tree_sitter::Node, content: &str) -> Option<String> {
    match node.kind() {
        "if_expression" => {
            // Analyze the consequence block's last expression
            let consequence = node.child_by_field_name("consequence")?;
            last_expression_type(consequence, content)
        }
        "match_expression" => {
            // Analyze the first match arm's body
            let body = node.child_by_field_name("body")?;
            let mut cursor = body.walk();
            for child in body.children(&mut cursor) {
                if child.kind() == "match_arm" {
                    let value = child.child_by_field_name("value")?;
                    return infer_payload_type(value, content);
                }
            }
            None
        }
        _ => infer_payload_type(node, content),
    }
}

/// Get the type from the last expression in a block
fn last_expression_type(block: tree_sitter::Node, content: &str) -> Option<String> {
    let mut cursor = block.walk();
    let mut last = None;
    for child in block.children(&mut cursor) {
        let kind = child.kind();
        if kind != "{" && kind != "}" {
            last = Some(child);
        }
    }
    let last_node = last?;
    // If it's an expression_statement, get the inner expression
    if last_node.kind() == "expression_statement" {
        let inner = last_node.child(0)?;
        infer_payload_type(inner, content)
    } else {
        infer_payload_type(last_node, content)
    }
}

/// Parse Rust source code
///
/// # Errors
///
/// Returns `ParseError` if:
/// *   The Rust language could not be set for the parser.
/// *   The content could not be parsed.
/// *   The tree-sitter query could not be created.
pub fn parse_rust(path: &std::path::Path, content: &str) -> ParseResult<Vec<Finding>> {
    let mut findings = Vec::new();

    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();

    parser.set_language(&ts_lang).map_err(|e| {
        ParseError::Language(
            format!("Failed to set Rust language: {e}"),
            Some(path.to_string_lossy().to_string()),
        )
    })?;

    let tree = parser.parse(content, None).ok_or_else(|| {
        ParseError::Syntax(
            "Failed to parse Rust file".to_string(),
            Some(path.to_string_lossy().to_string()),
        )
    })?;

    let query_src = get_query_source(LangType::Rust);
    let query = Query::new(&ts_lang, query_src).map_err(|e| {
        ParseError::Query(
            format!("Failed to create Rust query: {e}"),
            Some(path.to_string_lossy().to_string()),
        )
    })?;

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
            "event_args",
        ],
    );

    let rust_event_patterns = get_rust_event_patterns();

    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        if let Some(finding) = process_struct_match(m, &indices, content) {
            findings.push(finding);
        }

        if let Some(finding) = process_enum_match(m, &indices, content) {
            findings.push(finding);
        }

        findings.extend(process_command_matches(m, &indices, content));

        if let Some(finding) = process_event_match(m, &indices, content, &rust_event_patterns) {
            findings.push(finding);
        }
    }

    Ok(findings)
}
