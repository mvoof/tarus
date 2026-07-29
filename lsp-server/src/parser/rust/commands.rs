//! Rust source code parsing for Tauri commands and events

use crate::indexer::{DynamicUsages, Finding};
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use crate::utils::{find_capture, point_to_position};
use std::collections::HashMap;
use std::sync::LazyLock;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
use tree_sitter::{Language, Query, QueryCursor};

use crate::parser::lang_config::RUST_QUERY;

static RUST_EVENT_PATTERNS: LazyLock<HashMap<&'static str, (EntityType, Behavior)>> =
    LazyLock::new(|| {
        let mut m = HashMap::new();
        m.insert("emit", (EntityType::Event, Behavior::Emit));
        m.insert("emit_to", (EntityType::Event, Behavior::Emit));
        m.insert("emit_str", (EntityType::Event, Behavior::Emit));
        m.insert("emit_str_to", (EntityType::Event, Behavior::Emit));
        m.insert("emit_filter", (EntityType::Event, Behavior::Emit));
        m.insert("emit_str_filter", (EntityType::Event, Behavior::Emit));
        m.insert("listen", (EntityType::Event, Behavior::Listen));
        m.insert("listen_any", (EntityType::Event, Behavior::Listen));
        m.insert("once", (EntityType::Event, Behavior::Listen));
        m.insert("once_any", (EntityType::Event, Behavior::Listen));
        m
    });

/// Result of parsing one Rust file.
pub struct RustParse {
    pub findings: Vec<Finding>,
    pub dynamic_usages: DynamicUsages,
}

/// Extract findings from a pre-parsed Rust tree root node.
///
/// # Errors
///
/// Returns error if tree-sitter query execution fails.
#[allow(clippy::implicit_hasher)]
pub fn extract_rust_findings(
    root: tree_sitter::Node<'_>,
    content: &str,
    ts_lang: &Language,
    global_constants: &HashMap<String, String>,
) -> ParseResult<RustParse> {
    let query = Query::new(ts_lang, RUST_QUERY)
        .map_err(|e| ParseError::QueryError(format!("Failed to create Rust query: {e}")))?;

    let mut cursor = QueryCursor::new();
    let bytes = content.as_bytes();

    let fn_name_idx = query.capture_index_for_name("fn_name");
    let fn_item_idx = query.capture_index_for_name("fn_item");
    let method_name_idx = query.capture_index_for_name("method_name");
    let event_name_idx = query.capture_index_for_name("event_name");
    let struct_name_idx = query.capture_index_for_name("struct_name");
    let struct_item_idx = query.capture_index_for_name("struct_item");
    let specta_emit_struct_idx = query.capture_index_for_name("specta_emit_struct");

    let mut local_constants = crate::utils::extract_rust_constants(root, content);
    // Merge global constants as fallback (local takes priority)
    for (k, v) in global_constants {
        local_constants
            .entry(k.clone())
            .or_insert_with(|| v.clone());
    }

    let mut findings = Vec::new();
    let mut dynamic_usages = DynamicUsages::default();
    let mut matches = cursor.matches(&query, root, bytes);

    while let Some(m) = matches.next() {
        if let Some(f) = process_specta_emit(m, specta_emit_struct_idx, bytes) {
            findings.push(f);
            continue;
        }
        if let Some(f) = process_struct(m, struct_name_idx, struct_item_idx, bytes, content) {
            findings.push(f);
            continue;
        }
        if let Some(f) = process_fn(m, fn_name_idx, fn_item_idx, bytes, content) {
            findings.push(f);
            continue;
        }
        match process_event_call(m, method_name_idx, event_name_idx, bytes, &local_constants) {
            EventCallOutcome::Found(f) => findings.push(f),
            EventCallOutcome::Dynamic(behavior) => dynamic_usages.record(behavior),
            EventCallOutcome::NoMatch => {}
        }
    }

    Ok(RustParse {
        findings,
        dynamic_usages,
    })
}

fn process_specta_emit(
    m: &tree_sitter::QueryMatch<'_, '_>,
    specta_emit_struct_idx: Option<u32>,
    bytes: &[u8],
) -> Option<Finding> {
    let cap = find_capture(m, specta_emit_struct_idx)?;
    let struct_name = cap.node.utf8_text(bytes).unwrap_or_default();
    if !struct_name.starts_with(|c: char| c.is_ascii_uppercase()) {
        return None;
    }
    let kebab_name = crate::utils::camel_to_kebab(struct_name);
    Some(Finding::new(
        kebab_name,
        EntityType::Event,
        Behavior::Emit,
        Range {
            start: point_to_position(cap.node.start_position()),
            end: point_to_position(cap.node.end_position()),
        },
    ))
}

fn process_struct(
    m: &tree_sitter::QueryMatch<'_, '_>,
    struct_name_idx: Option<u32>,
    struct_item_idx: Option<u32>,
    bytes: &[u8],
    content: &str,
) -> Option<Finding> {
    let name_cap = find_capture(m, struct_name_idx)?;
    let item_cap = find_capture(m, struct_item_idx)?;

    if !crate::parser::rust::attrs::has_specta_event_derive(item_cap.node, content) {
        return None;
    }

    let struct_name = name_cap.node.utf8_text(bytes).unwrap_or_default();
    let kebab_name = crate::utils::camel_to_kebab(struct_name);
    Some(Finding::new(
        kebab_name,
        EntityType::Event,
        Behavior::Definition,
        Range {
            start: point_to_position(name_cap.node.start_position()),
            end: point_to_position(name_cap.node.end_position()),
        },
    ))
}

fn process_fn(
    m: &tree_sitter::QueryMatch<'_, '_>,
    fn_name_idx: Option<u32>,
    fn_item_idx: Option<u32>,
    bytes: &[u8],
    content: &str,
) -> Option<Finding> {
    let name_cap = find_capture(m, fn_name_idx)?;
    let item_cap = find_capture(m, fn_item_idx)?;

    if !crate::parser::rust::attrs::has_tauri_command_attr(item_cap.node, content) {
        return None;
    }

    let name = name_cap.node.utf8_text(bytes).unwrap_or_default();
    Some(Finding::new(
        name.to_string(),
        EntityType::Command,
        Behavior::Definition,
        Range {
            start: point_to_position(name_cap.node.start_position()),
            end: point_to_position(name_cap.node.end_position()),
        },
    ))
}

/// What a query match yielded for a Rust `emit`/`listen`-style call.
enum EventCallOutcome {
    /// The call names a known event.
    Found(Finding),
    /// The call is an event call whose name could not be resolved to a literal.
    Dynamic(Behavior),
    /// The match is not an event call at all.
    NoMatch,
}

fn process_event_call(
    m: &tree_sitter::QueryMatch<'_, '_>,
    method_name_idx: Option<u32>,
    event_name_idx: Option<u32>,
    bytes: &[u8],
    constants: &std::collections::HashMap<String, String>,
) -> EventCallOutcome {
    let (Some(method_cap), Some(event_cap)) = (
        find_capture(m, method_name_idx),
        find_capture(m, event_name_idx),
    ) else {
        return EventCallOutcome::NoMatch;
    };

    let method_name = method_cap.node.utf8_text(bytes).unwrap_or_default();
    let Some((entity, behavior)) = RUST_EVENT_PATTERNS.get(method_name) else {
        return EventCallOutcome::NoMatch;
    };

    let raw_event_name = event_cap.node.utf8_text(bytes).unwrap_or_default();
    let is_literal = event_cap.node.kind() == "string_literal";

    let literal_content = is_literal
        .then(|| {
            event_cap
                .node
                .child_by_field_name("content")
                .or_else(|| event_cap.node.named_child(0))
        })
        .flatten();

    let resolved_name = if is_literal {
        literal_content.map_or_else(
            || {
                raw_event_name
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(raw_event_name)
                    .to_string()
            },
            |node| node.utf8_text(bytes).unwrap_or_default().to_string(),
        )
    } else {
        let lookup_key = raw_event_name.rsplit("::").next().unwrap_or(raw_event_name);

        match constants
            .get(raw_event_name)
            .or_else(|| constants.get(lookup_key))
        {
            Some(value) => value.clone(),
            None => return EventCallOutcome::Dynamic(*behavior),
        }
    };

    // `app.emit("", ..)` names nothing, but it is not unknown either: an empty
    // literal must not weaken diagnostics the way a dynamic name does.
    if resolved_name.is_empty() {
        return EventCallOutcome::NoMatch;
    }

    let name_node = literal_content.unwrap_or(event_cap.node);
    let range = Range {
        start: point_to_position(name_node.start_position()),
        end: point_to_position(name_node.end_position()),
    };

    EventCallOutcome::Found(Finding::new(resolved_name, *entity, *behavior, range))
}
