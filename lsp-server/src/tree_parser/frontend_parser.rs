//! TypeScript/JavaScript/Vue/Svelte/Angular parsing for Tauri invoke/emit/listen calls

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use crate::utils::{find_capture, point_to_position};
use std::collections::HashMap;
use std::sync::LazyLock;
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

static ALL_FRONTEND_PATTERNS: LazyLock<Vec<FunctionPatternWithPos>> = LazyLock::new(|| {
    vec![
        FunctionPatternWithPos {
            name: "invoke",
            entity: EntityType::Command,
            behavior: Behavior::Call,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "emit",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::First,
        },
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
        FunctionPatternWithPos {
            name: "emitTo",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::Second,
        },
    ]
});

/// Capture indices extracted from the query, grouped for readability
struct FrontendCaptures {
    func_name: Option<u32>,
    arg_value: Option<u32>,
    func_name_second: Option<u32>,
    arg_value_second: Option<u32>,
    imported_name: Option<u32>,
    local_alias: Option<u32>,
    import_source: Option<u32>,
    call_generic: Option<u32>,
    call_await_generic: Option<u32>,
    specta_method_name: Option<u32>,
    specta_call: Option<u32>,
    specta_event_name: Option<u32>,
    specta_event_method: Option<u32>,
}

impl FrontendCaptures {
    fn from_query(query: &Query) -> Self {
        Self {
            func_name: query.capture_index_for_name("func_name"),
            arg_value: query.capture_index_for_name("arg_value"),
            func_name_second: query.capture_index_for_name("func_name_second"),
            arg_value_second: query.capture_index_for_name("arg_value_second"),
            imported_name: query.capture_index_for_name("imported_name"),
            local_alias: query.capture_index_for_name("local_alias"),
            import_source: query.capture_index_for_name("import_source"),
            call_generic: query.capture_index_for_name("call_generic"),
            call_await_generic: query.capture_index_for_name("call_await_generic"),
            specta_method_name: query.capture_index_for_name("specta_method_name"),
            specta_call: query.capture_index_for_name("specta_call"),
            specta_event_name: query.capture_index_for_name("specta_event_name"),
            specta_event_method: query.capture_index_for_name("specta_event_method"),
        }
    }
}

/// Parse TypeScript/JavaScript source code
pub(super) fn parse_frontend(
    content: &str,
    lang: LangType,
    line_offset: usize,
) -> ParseResult<Vec<Finding>> {
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

    let caps = FrontendCaptures::from_query(&query);
    let root = tree.root_node();
    let bytes = content.as_bytes();

    // First pass: collect import aliases
    let aliases = collect_aliases(&query, root, bytes, &caps);

    // Second pass: collect function calls
    let mut findings = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, root, bytes);

    while let Some(m) = matches.next() {
        if let Some(f) = process_first_arg_pattern(m, &caps, bytes, &aliases, content, line_offset)
        {
            findings.push(f);
        }
        if let Some(f) = process_second_arg_pattern(m, &caps, bytes, &aliases, line_offset) {
            findings.push(f);
        }
        if let Some(f) = process_specta_call(m, &caps, bytes, content, line_offset) {
            findings.push(f);
        }
        if let Some(f) = process_specta_event(m, &caps, bytes, line_offset) {
            findings.push(f);
        }
    }

    Ok(findings)
}

fn collect_aliases(
    query: &Query,
    root: tree_sitter::Node<'_>,
    bytes: &[u8],
    caps: &FrontendCaptures,
) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, bytes);

    while let Some(m) = matches.next() {
        let src_cap = find_capture(m, caps.import_source);

        if let Some(src_node) = src_cap {
            let source = src_node.node.utf8_text(bytes).unwrap_or_default();

            if source.starts_with("@tauri-apps/") {
                let imp = find_capture(m, caps.imported_name);
                let loc = find_capture(m, caps.local_alias);

                if let (Some(imp_cap), Some(loc_cap)) = (imp, loc) {
                    let imported = imp_cap.node.utf8_text(bytes).unwrap_or_default();
                    let local = loc_cap.node.utf8_text(bytes).unwrap_or_default();

                    aliases.insert(local.to_string(), imported.to_string());
                } else if let Some(imp_cap) = imp {
                    let imported = imp_cap.node.utf8_text(bytes).unwrap_or_default();

                    aliases.insert(imported.to_string(), imported.to_string());
                }
            }
        }
    }

    aliases
}

fn process_first_arg_pattern(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &[u8],
    aliases: &HashMap<String, String>,
    content: &str,
    line_offset: usize,
) -> Option<Finding> {
    let func_cap = find_capture(m, caps.func_name)?;
    let arg_cap = find_capture(m, caps.arg_value)?;

    let func_name = func_cap.node.utf8_text(bytes).unwrap_or_default();
    let arg_value = arg_cap.node.utf8_text(bytes).unwrap_or_default();
    let original_name = aliases.get(func_name)?;

    let pattern = ALL_FRONTEND_PATTERNS
        .iter()
        .find(|p| p.name == original_name && p.arg_position == ArgPosition::First)?;

    let range = Range {
        start: point_to_position(arg_cap.node.start_position()),
        end: point_to_position(arg_cap.node.end_position()),
    };
    let call_name_end = Some(adjust_position(
        point_to_position(func_cap.node.end_position()),
        line_offset,
    ));
    let type_arg_info =
        extract_type_argument_info(m, caps.call_generic, caps.call_await_generic, content);
    let return_type = type_arg_info.as_ref().map(|i| i.type_text.clone());
    let type_arg_range = type_arg_info.map(|i| adjust_range(i.type_arg_range, line_offset));

    Some(Finding {
        return_type,
        call_name_end,
        type_arg_range,
        ..Finding::new(
            arg_value.to_string(),
            pattern.entity,
            pattern.behavior,
            adjust_range(range, line_offset),
        )
    })
}

fn process_second_arg_pattern(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &[u8],
    aliases: &HashMap<String, String>,
    line_offset: usize,
) -> Option<Finding> {
    let func_cap = find_capture(m, caps.func_name_second)?;
    let arg_cap = find_capture(m, caps.arg_value_second)?;

    let func_name = func_cap.node.utf8_text(bytes).unwrap_or_default();
    let arg_value = arg_cap.node.utf8_text(bytes).unwrap_or_default();
    let original_name = aliases.get(func_name)?;

    let pattern = ALL_FRONTEND_PATTERNS
        .iter()
        .find(|p| p.name == original_name && p.arg_position == ArgPosition::Second)?;

    let range = Range {
        start: point_to_position(arg_cap.node.start_position()),
        end: point_to_position(arg_cap.node.end_position()),
    };

    Some(Finding::new(
        arg_value.to_string(),
        pattern.entity,
        pattern.behavior,
        adjust_range(range, line_offset),
    ))
}

fn process_specta_call(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &[u8],
    content: &str,
    line_offset: usize,
) -> Option<Finding> {
    let specta_cap = find_capture(m, caps.specta_method_name)?;

    let camel_name = specta_cap.node.utf8_text(bytes).unwrap_or_default();
    let snake_name = crate::utils::camel_to_snake(camel_name);
    let method_range = Range {
        start: point_to_position(specta_cap.node.start_position()),
        end: point_to_position(specta_cap.node.end_position()),
    };
    let arg_count = count_specta_call_args(m, caps.specta_call, content);

    Some(Finding {
        call_arg_count: Some(arg_count),
        ..Finding::new(
            snake_name,
            EntityType::Command,
            Behavior::SpectaCall,
            adjust_range(method_range, line_offset),
        )
    })
}

fn process_specta_event(
    m: &tree_sitter::QueryMatch<'_, '_>,
    caps: &FrontendCaptures,
    bytes: &[u8],
    line_offset: usize,
) -> Option<Finding> {
    let name_cap = find_capture(m, caps.specta_event_name)?;
    let method_cap = find_capture(m, caps.specta_event_method)?;

    let camel_name = name_cap.node.utf8_text(bytes).unwrap_or_default();
    let method_name = method_cap.node.utf8_text(bytes).unwrap_or_default();

    let behavior = match method_name {
        "emit" => Behavior::Emit,
        "listen" | "once" => Behavior::Listen,
        _ => return None,
    };

    let kebab_name = crate::utils::camel_to_kebab(camel_name);
    let name_range = Range {
        start: point_to_position(name_cap.node.start_position()),
        end: point_to_position(name_cap.node.end_position()),
    };

    Some(Finding {
        codegen_origin: Some(crate::indexer::GeneratorKind::Specta),
        ..Finding::new(
            kebab_name,
            EntityType::Event,
            behavior,
            adjust_range(name_range, line_offset),
        )
    })
}
