//! Rust source code parsing for Tauri commands and events

use crate::indexer::Finding;
use crate::syntax::{Behavior, EntityType, ParseError, ParseResult};
use crate::utils::{find_capture, point_to_position};
use std::collections::HashMap;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::Range;
use tree_sitter::{Language, Query, QueryCursor};

use super::lang_config::RUST_QUERY;

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

/// Extract findings from a pre-parsed Rust tree root node.
#[allow(clippy::too_many_lines)] // reason: struct+fn+event patterns in single pass
pub(super) fn extract_rust_findings(
    root: tree_sitter::Node<'_>,
    content: &str,
    ts_lang: &Language,
) -> ParseResult<Vec<Finding>> {
    let mut findings = Vec::new();

    let query = Query::new(ts_lang, RUST_QUERY)
        .map_err(|e| ParseError::QueryError(format!("Failed to create Rust query: {e}")))?;

    let mut cursor = QueryCursor::new();

    let fn_name_idx = query.capture_index_for_name("fn_name");
    let fn_item_idx = query.capture_index_for_name("fn_item");
    let method_name_idx = query.capture_index_for_name("method_name");
    let event_name_idx = query.capture_index_for_name("event_name");
    let struct_name_idx = query.capture_index_for_name("struct_name");
    let struct_item_idx = query.capture_index_for_name("struct_item");
    let specta_emit_struct_idx = query.capture_index_for_name("specta_emit_struct");

    let rust_event_patterns = get_rust_event_patterns();

    let mut matches = cursor.matches(&query, root, content.as_bytes());
    while let Some(m) = matches.next() {
        // Process specta typed event emit: GlobalEvent(payload).emit_to(&app)
        if let Some(emit_idx) = specta_emit_struct_idx {
            if let Some(cap) = find_capture(m, Some(emit_idx)) {
                let struct_name = cap.node.utf8_text(content.as_bytes()).unwrap_or_default();
                if struct_name.starts_with(|c: char| c.is_ascii_uppercase()) {
                    let kebab_name = crate::utils::camel_to_kebab(struct_name);
                    findings.push(Finding::new(
                        kebab_name,
                        EntityType::Event,
                        Behavior::Emit,
                        Range {
                            start: point_to_position(cap.node.start_position()),
                            end: point_to_position(cap.node.end_position()),
                        },
                    ));
                }
                continue;
            }
        }

        // Process struct_item — check if it has #[derive(...Event...)]
        if let (Some(sname_idx), Some(sitem_idx)) = (struct_name_idx, struct_item_idx) {
            let name_cap = find_capture(m, Some(sname_idx));
            let item_cap = find_capture(m, Some(sitem_idx));

            if let (Some(name_cap), Some(item_cap)) = (name_cap, item_cap) {
                if crate::rust_attr::has_specta_event_derive(item_cap.node, content) {
                    let struct_name = name_cap
                        .node
                        .utf8_text(content.as_bytes())
                        .unwrap_or_default();
                    let kebab_name = crate::utils::camel_to_kebab(struct_name);
                    findings.push(Finding::new(
                        kebab_name,
                        EntityType::Event,
                        Behavior::Definition,
                        Range {
                            start: point_to_position(name_cap.node.start_position()),
                            end: point_to_position(name_cap.node.end_position()),
                        },
                    ));
                }
                continue;
            }
        }

        if let (Some(name_idx), Some(item_idx)) = (fn_name_idx, fn_item_idx) {
            let name_cap = find_capture(m, Some(name_idx));
            let item_cap = find_capture(m, Some(item_idx));

            if let (Some(name_cap), Some(item_cap)) = (name_cap, item_cap) {
                if crate::rust_attr::has_tauri_command_attr(item_cap.node, content) {
                    let name = name_cap
                        .node
                        .utf8_text(content.as_bytes())
                        .unwrap_or_default();
                    findings.push(Finding::new(
                        name.to_string(),
                        EntityType::Command,
                        Behavior::Definition,
                        Range {
                            start: point_to_position(name_cap.node.start_position()),
                            end: point_to_position(name_cap.node.end_position()),
                        },
                    ));
                }
                continue;
            }
        }

        if let (Some(method_idx), Some(event_idx)) = (method_name_idx, event_name_idx) {
            let method_capture = find_capture(m, Some(method_idx));
            let event_capture = find_capture(m, Some(event_idx));

            if let (Some(method_cap), Some(event_cap)) = (method_capture, event_capture) {
                let method_name = method_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();
                let event_name = event_cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or_default();

                if let Some((entity, behavior)) = rust_event_patterns.get(method_name) {
                    findings.push(Finding::new(
                        event_name.to_string(),
                        *entity,
                        *behavior,
                        Range {
                            start: point_to_position(event_cap.node.start_position()),
                            end: point_to_position(event_cap.node.end_position()),
                        },
                    ));
                }
            }
        }
    }

    Ok(findings)
}
