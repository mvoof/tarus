//! Rust source code parsing for Tauri commands and events

use crate::indexer::Finding;
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
) -> ParseResult<Vec<Finding>> {
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
        if let Some(f) =
            process_event_call(m, method_name_idx, event_name_idx, bytes, &local_constants)
        {
            findings.push(f);
        }
    }

    Ok(findings)
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

fn process_event_call(
    m: &tree_sitter::QueryMatch<'_, '_>,
    method_name_idx: Option<u32>,
    event_name_idx: Option<u32>,
    bytes: &[u8],
    constants: &std::collections::HashMap<String, String>,
) -> Option<Finding> {
    let method_cap = find_capture(m, method_name_idx)?;
    let event_cap = find_capture(m, event_name_idx)?;

    let method_name = method_cap.node.utf8_text(bytes).unwrap_or_default();
    let raw_event_name = event_cap.node.utf8_text(bytes).unwrap_or_default();

    let mut resolved_name = raw_event_name.to_string();
    if event_cap.node.kind() == "string_literal" {
        if let Some(content_node) = event_cap.node.child_by_field_name("content") {
            resolved_name = content_node
                .utf8_text(bytes)
                .unwrap_or_default()
                .to_string();
        } else if let Some(content_node) = event_cap.node.named_child(0) {
            resolved_name = content_node
                .utf8_text(bytes)
                .unwrap_or_default()
                .to_string();
        } else if resolved_name.starts_with('"')
            && resolved_name.ends_with('"')
            && resolved_name.len() >= 2
        {
            resolved_name = resolved_name[1..resolved_name.len() - 1].to_string();
        }
    } else {
        let mut resolved = false;
        if let Some(resolved_val) = constants.get(&resolved_name) {
            resolved_name.clone_from(resolved_val);
            resolved = true;
        } else {
            let lookup_key = if resolved_name.contains("::") {
                resolved_name
                    .split("::")
                    .last()
                    .unwrap_or(resolved_name.as_str())
            } else {
                resolved_name.as_str()
            };
            if let Some(resolved_val) = constants.get(lookup_key) {
                resolved_name.clone_from(resolved_val);
                resolved = true;
            }
        }
        if !resolved {
            resolved_name.clear();
        }
    }

    if resolved_name.is_empty() {
        return None;
    }

    let mut range = Range {
        start: point_to_position(event_cap.node.start_position()),
        end: point_to_position(event_cap.node.end_position()),
    };
    if event_cap.node.kind() == "string_literal" {
        if let Some(content_node) = event_cap.node.child_by_field_name("content") {
            range = Range {
                start: point_to_position(content_node.start_position()),
                end: point_to_position(content_node.end_position()),
            };
        } else if let Some(content_node) = event_cap.node.named_child(0) {
            range = Range {
                start: point_to_position(content_node.start_position()),
                end: point_to_position(content_node.end_position()),
            };
        }
    }

    let (entity, behavior) = RUST_EVENT_PATTERNS.get(method_name)?;
    Some(Finding::new(resolved_name, *entity, *behavior, range))
}
