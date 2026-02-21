//! Frontend (TypeScript/JavaScript) language parser using tree-sitter

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseResult};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::ls_types::Range;
use tree_sitter::Language;

use super::extractors::{extract_ts_interface_fields, extract_ts_params, FindingBuilder};
use super::patterns::{get_all_frontend_patterns, ArgPosition, FunctionPatternWithPos};
use super::query_helpers::CaptureIndices;
use super::utils::{
    adjust_range, get_query_source, point_to_position, LangType, NodeTextExt, ParseContext,
};

// Local pattern definitions removed - using patterns::{ArgPosition, FunctionPatternWithPos, get_all_frontend_patterns}

/// Process interface definition match
#[must_use]
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

/// Specta/Typegen/ts-rs call pattern descriptors for table-driven matching
const SPECTA_PATTERNS: &[(&str, &str, Option<&str>)] = &[
    // (object/verify_key, method_key, optional commands_key for namespaced patterns)
    ("specta_call_object", "specta_call_method", None),
    ("specta_await_object", "specta_await_method", None),
    (
        "specta_ns_object",
        "specta_ns_method",
        Some("specta_ns_commands"),
    ),
    (
        "specta_ns_await_object",
        "specta_ns_await_method",
        Some("specta_ns_await_commands"),
    ),
];

/// Process Specta method call match (commands.methodName or Specta.commands.methodName)
#[must_use]
pub fn process_specta_call_match(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
    line_offset: usize,
) -> Option<Finding> {
    for &(obj_key, method_key, cmd_key) in SPECTA_PATTERNS {
        let Some(obj_cap) = indices.find_capture(m.captures, obj_key) else {
            continue;
        };
        let Some(method_cap) = indices.find_capture(m.captures, method_key) else {
            continue;
        };

        // For namespaced patterns, verify the intermediate "commands" identifier
        let verify_name = if let Some(ck) = cmd_key {
            indices
                .find_capture(m.captures, ck)
                .map(|c| c.node.text_or_default(content))
        } else {
            Some(obj_cap.node.text_or_default(content))
        };

        if verify_name.as_deref() != Some("commands") {
            continue;
        }

        let method_name = method_cap.node.text_or_default(content);
        return Some(
            FindingBuilder::new(
                crate::syntax::camel_to_snake(&method_name),
                EntityType::Command,
                Behavior::Call,
                adjust_range(
                    Range {
                        start: point_to_position(method_cap.node.start_position()),
                        end: point_to_position(method_cap.node.end_position()),
                    },
                    line_offset,
                ),
            )
            .build(),
        );
    }

    None
}

/// Capture name pairs for first/second argument position patterns
const ARG_POSITION_CAPTURES: &[(&str, &str, ArgPosition)] = &[
    ("func_name", "arg_value", ArgPosition::First),
    ("func_name_second", "arg_value_second", ArgPosition::Second),
];

/// Process function call match (handles both first and second argument patterns)
#[must_use]
pub fn process_function_call_match<S: std::hash::BuildHasher>(
    m: &tree_sitter::QueryMatch,
    indices: &CaptureIndices,
    content: &str,
    line_offset: usize,
    aliases: &HashMap<String, String, S>,
    patterns: &[FunctionPatternWithPos],
) -> Option<Finding> {
    for &(func_key, arg_key, ref pos) in ARG_POSITION_CAPTURES {
        let Some(func_cap) = indices.find_capture(m.captures, func_key) else {
            continue;
        };
        let Some(arg_cap) = indices.find_capture(m.captures, arg_key) else {
            continue;
        };

        let func_name = func_cap.node.text_or_default(content);
        let arg_value = arg_cap.node.text_or_default(content);
        let original_name = aliases.get(&func_name).unwrap_or(&func_name);

        let Some(pattern) = patterns
            .iter()
            .find(|p| p.name == original_name && p.arg_position == *pos)
        else {
            continue;
        };

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

    None
}

/// Parse TypeScript/JavaScript source code
///
/// # Errors
///
/// Returns `ParseError` if:
/// *   The language could not be set for the parser.
/// *   The content could not be parsed.
/// *   The tree-sitter query could not be created.
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

    let ctx = ParseContext::new(&ts_lang, get_query_source(lang), content, path)?;
    let mut cursor = ParseContext::cursor();
    let root = ctx.root_node();

    // Build alias map from imports
    let mut aliases: HashMap<String, String> = HashMap::new();

    // Get capture indices using helper
    let indices = CaptureIndices::from_query(
        &ctx.query,
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
            "specta_call_object",
            "specta_call_method",
            "specta_await_object",
            "specta_await_method",
            "specta_ns_object",
            "specta_ns_commands",
            "specta_ns_method",
            "specta_ns_await_object",
            "specta_ns_await_commands",
            "specta_ns_await_method",
        ],
    );

    let all_patterns = get_all_frontend_patterns();

    // First pass: collect aliases and interface definitions
    let mut matches = cursor.matches(&ctx.query, root, content.as_bytes());

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
        if let Some(finding) = process_interface_match(m, &indices, content, line_offset) {
            findings.push(finding);
        }
    }

    // Second pass: collect function calls and Specta calls
    let mut cursor = ParseContext::cursor();
    let mut matches = cursor.matches(&ctx.query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Try Specta patterns first
        if let Some(finding) = process_specta_call_match(m, &indices, content, line_offset) {
            findings.push(finding);
            continue;
        }

        // Try regular function call patterns
        if let Some(finding) =
            process_function_call_match(m, &indices, content, line_offset, &aliases, &all_patterns)
        {
            findings.push(finding);
        }
    }

    // Deduplicate findings that share the same (key, entity, behavior, range).
    // This happens when `await commands.method()` matches both the direct and await
    // tree-sitter patterns, producing identical findings.
    let mut seen = std::collections::HashSet::new();
    findings.retain(|f| {
        seen.insert((
            f.key.clone(),
            f.entity,
            f.behavior,
            f.range.start.line,
            f.range.start.character,
            f.range.end.line,
            f.range.end.character,
        ))
    });

    Ok(findings)
}

/// Check if TypeScript file contains Angular decorators
#[must_use]
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
