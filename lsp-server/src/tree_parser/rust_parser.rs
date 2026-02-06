//! Rust language parser using tree-sitter

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
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
        .build(),
    )
}

#[allow(clippy::too_many_lines)]
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
