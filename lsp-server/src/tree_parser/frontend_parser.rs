//! TypeScript/JavaScript/Vue/Svelte/Angular parsing for Tauri invoke/emit/listen calls

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use crate::utils::{find_capture, point_to_position};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
use tree_sitter::{Language, Parser, Query, QueryCursor};

use super::extractors::{count_specta_call_args, extract_type_argument_info};
use super::lang_config::{get_query_source, LangType};
use super::sfc_parser::{adjust_position, adjust_range};

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
pub(super) fn parse_frontend(
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
    // Get capture indices for Specta events
    let specta_event_name_idx = query.capture_index_for_name("specta_event_name");
    let specta_event_method_idx = query.capture_index_for_name("specta_event_method");

    let all_patterns = get_all_frontend_patterns();

    // First pass: collect aliases
    let mut matches = cursor.matches(&query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        if let (Some(imp_idx), Some(alias_idx)) = (imported_name_idx, local_alias_idx) {
            let imported = find_capture(m, Some(imp_idx));
            let local = find_capture(m, Some(alias_idx));

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
            let func_capture = find_capture(m, Some(func_idx));
            let arg_capture = find_capture(m, Some(arg_idx));

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
                    let type_arg_info = extract_type_argument_info(
                        m,
                        call_generic_idx,
                        call_await_generic_idx,
                        content,
                    );
                    let return_type = type_arg_info.as_ref().map(|i| i.type_text.clone());
                    let type_arg_range =
                        type_arg_info.map(|i| adjust_range(i.type_arg_range, line_offset));

                    findings.push(Finding {
                        return_type,
                        call_name_end,
                        type_arg_range,
                        ..Finding::new(
                            arg_value.to_string(),
                            pattern.entity,
                            pattern.behavior,
                            adjust_range(range, line_offset),
                        )
                    });
                }
            }
        }

        // Try second argument pattern (func_name_second + arg_value_second)
        if let (Some(func_idx), Some(arg_idx)) = (func_name_second_idx, arg_value_second_idx) {
            let func_capture = find_capture(m, Some(func_idx));
            let arg_capture = find_capture(m, Some(arg_idx));

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

                    findings.push(Finding::new(
                        arg_value.to_string(),
                        pattern.entity,
                        pattern.behavior,
                        adjust_range(range, line_offset),
                    ));
                }
            }
        }

        // Try SpectaCall pattern (commands.methodName(...))
        if let Some(specta_idx) = specta_method_name_idx {
            if let Some(specta_cap) = find_capture(m, Some(specta_idx)) {
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
                    call_arg_count: Some(arg_count),
                    ..Finding::new(
                        snake_name,
                        EntityType::Command,
                        Behavior::SpectaCall,
                        adjust_range(method_range, line_offset),
                    )
                });
            }
        }

        // Try Specta event pattern (events.eventName.listen/emit/once(...))
        if let (Some(event_name_idx), Some(event_method_idx)) =
            (specta_event_name_idx, specta_event_method_idx)
        {
            let event_name_cap = find_capture(m, Some(event_name_idx));
            let event_method_cap = find_capture(m, Some(event_method_idx));

            if let (Some(name_cap), Some(method_cap)) = (event_name_cap, event_method_cap) {
                let camel_name = name_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();
                let method_name = method_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();

                // Map method name to behavior using the same patterns as frontend events
                let behavior = match method_name {
                    "emit" => Some(Behavior::Emit),
                    "listen" | "once" => Some(Behavior::Listen),
                    _ => None,
                };

                if let Some(behavior) = behavior {
                    let kebab_name = crate::utils::camel_to_kebab(camel_name);
                    let name_range = Range {
                        start: point_to_position(name_cap.node.start_position()),
                        end: point_to_position(name_cap.node.end_position()),
                    };

                    findings.push(Finding {
                        codegen_origin: Some(crate::indexer::GeneratorKind::Specta),
                        ..Finding::new(
                            kebab_name,
                            EntityType::Event,
                            behavior,
                            adjust_range(name_range, line_offset),
                        )
                    });
                }
            }
        }
    }

    Ok(findings)
}
