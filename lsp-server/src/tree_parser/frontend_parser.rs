//! Frontend (TypeScript/JavaScript) language parser using tree-sitter

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use super::extractors::{extract_ts_interface_fields, extract_ts_params, FindingBuilder};
use super::patterns::{get_all_frontend_patterns, ArgPosition, FunctionPatternWithPos};
use super::query_helpers::CaptureIndices;
use super::utils::{adjust_range, get_query_source, point_to_position, LangType, NodeTextExt};

// Local pattern definitions removed - using patterns::{ArgPosition, FunctionPatternWithPos, get_all_frontend_patterns}

/// Process interface definition match
pub fn process_interface_match(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
    line_offset: usize,
) -> Option<Finding> {
    let iface_cap = indices.find_capture(m.captures, "interface_def")?;
    let name_cap = indices.find_capture(m.captures, "interface_name")?;

    let name = name_cap.node.text_or_default(content);
    let fields = extract_ts_interface_fields(iface_cap.node, content);

    Some(
        FindingBuilder::new(
            name,
            EntityType::Interface,
            Behavior::Definition,
            adjust_range(
                Range {
                    start: point_to_position(name_cap.node.start_position()),
                    end: point_to_position(name_cap.node.end_position()),
                },
                line_offset,
            ),
        )
        .with_fields(fields)
        .build(),
    )
}

/// Process function call match (handles both first and second argument patterns)
pub fn process_function_call_match(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
    line_offset: usize,
    aliases: &HashMap<String, String>,
    patterns: &[FunctionPatternWithPos],
) -> Option<Finding> {
    // Try first argument pattern
    if let (Some(func_cap), Some(arg_cap)) = (
        indices.find_capture(m.captures, "func_name"),
        indices.find_capture(m.captures, "arg_value"),
    ) {
        let func_name = func_cap.node.text_or_default(content);
        let arg_value = arg_cap.node.text_or_default(content);

        // Check if this is an aliased import
        let original_name = aliases.get(&func_name).unwrap_or(&func_name);

        if let Some(pattern) = patterns
            .iter()
            .find(|p| p.name == original_name && p.arg_position == ArgPosition::First)
        {
            let parameters = indices
                .find_capture(m.captures, "invoke_args")
                .map(|cap| extract_ts_params(cap.node, content));

            let return_type = indices
                .find_capture(m.captures, "type_args")
                .map(|cap| cap.node.text_or_default(content));

            return Some(
                FindingBuilder::new(
                    arg_value,
                    pattern.entity,
                    pattern.behavior,
                    adjust_range(
                        Range {
                            start: point_to_position(arg_cap.node.start_position()),
                            end: point_to_position(arg_cap.node.end_position()),
                        },
                        line_offset,
                    ),
                )
                .with_parameters_opt(parameters)
                .with_return_type_opt(return_type)
                .build(),
            );
        }
    }

    // Try second argument pattern
    if let (Some(func_cap), Some(arg_cap)) = (
        indices.find_capture(m.captures, "func_name_second"),
        indices.find_capture(m.captures, "arg_value_second"),
    ) {
        let func_name = func_cap.node.text_or_default(content);
        let arg_value = arg_cap.node.text_or_default(content);

        let original_name = aliases.get(&func_name).unwrap_or(&func_name);

        if let Some(pattern) = patterns
            .iter()
            .find(|p| p.name == original_name && p.arg_position == ArgPosition::Second)
        {
            let parameters = indices
                .find_capture(m.captures, "invoke_args")
                .map(|cap| extract_ts_params(cap.node, content));

            let return_type = indices
                .find_capture(m.captures, "type_args")
                .map(|cap| cap.node.text_or_default(content));

            return Some(
                FindingBuilder::new(
                    arg_value,
                    pattern.entity,
                    pattern.behavior,
                    adjust_range(
                        Range {
                            start: point_to_position(arg_cap.node.start_position()),
                            end: point_to_position(arg_cap.node.end_position()),
                        },
                        line_offset,
                    ),
                )
                .with_parameters_opt(parameters)
                .with_return_type_opt(return_type)
                .build(),
            );
        }
    }

    None
}

/// Parse TypeScript/JavaScript source code
#[allow(clippy::too_many_lines)]
pub fn parse_frontend(
    path: &std::path::Path,
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
    parser.set_language(&ts_lang).map_err(|e| {
        ParseError::LanguageError(
            format!("Failed to set {lang:?} language: {e}"),
            Some(path.to_string_lossy().to_string()),
        )
    })?;

    let tree = parser.parse(content, None).ok_or_else(|| {
        ParseError::SyntaxError(
            format!("Failed to parse {lang:?} file"),
            Some(path.to_string_lossy().to_string()),
        )
    })?;

    let query_src = get_query_source(lang);
    let query = Query::new(&ts_lang, query_src).map_err(|e| {
        ParseError::QueryError(
            format!("Failed to create {lang:?} query: {e}"),
            Some(path.to_string_lossy().to_string()),
        )
    })?;

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
        if let Some(finding) = process_interface_match(&m, &indices, content, line_offset) {
            findings.push(finding);
        }
    }

    // Second pass: collect function calls
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        if let Some(finding) =
            process_function_call_match(&m, &indices, content, line_offset, &aliases, &all_patterns)
        {
            findings.push(finding);
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
