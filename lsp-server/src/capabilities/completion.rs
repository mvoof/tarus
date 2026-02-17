//! Completion capability - autocomplete commands and events

use crate::indexer::ProjectIndex;
use crate::syntax::{
    extract_result_ok_type, get_base_rust_type, is_primitive_rust_type, map_rust_type_to_ts,
    should_rename_to_camel, snake_to_camel, Behavior, EntityType,
};
use dashmap::DashMap;
use std::fmt::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp_server::ls_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
};

/// Format struct fields as "{ field1: type1, field2: type2 }" if the type is a known struct
fn format_struct_fields(project_index: &ProjectIndex, base_type: &str) -> Option<String> {
    if is_primitive_rust_type(base_type) {
        return None;
    }
    let struct_locs = project_index.get_locations(EntityType::Struct, base_type);
    let sd = struct_locs
        .iter()
        .find(|sl| sl.behavior == Behavior::Definition)?;
    let fields = sd.fields.as_ref()?;
    let rename = should_rename_to_camel(sd.attributes.as_ref());
    let field_strs: Vec<String> = fields
        .iter()
        .map(|f| {
            let fname = if rename {
                snake_to_camel(&f.name)
            } else {
                f.name.clone()
            };
            format!("{}: {}", fname, map_rust_type_to_ts(&f.type_name))
        })
        .collect();
    Some(format!("{{ {} }}", field_strs.join(", ")))
}

const COMPLETION_TRIGGERS: &[&str] = &[
    "invoke",
    "emit",
    "emitTo",
    "listen",
    "once",
    "emit_to",
    "emit_str",
    "emit_str_to",
    "emit_filter",
    "emit_str_filter",
    "listen_any",
    "once_any",
];

/// Handle completion request
pub fn handle_completion(
    params: &CompletionParams,
    project_index: &ProjectIndex,
    document_cache: &Arc<DashMap<PathBuf, String>>,
) -> Option<CompletionResponse> {
    let uri = &params.text_document_position.text_document.uri;
    let path = super::uri_to_path(uri)?;

    // Try to get content from cache first, fallback to reading from disk
    let content = document_cache
        .get(&path)
        .map(|entry| entry.value().clone())
        .or_else(|| std::fs::read_to_string(&path).ok())?;

    let lines: Vec<&str> = content.lines().collect();
    let line_idx = params.text_document_position.position.line as usize;

    if line_idx >= lines.len() {
        return None;
    }

    let line = lines[line_idx];
    let col = params.text_document_position.position.character as usize;
    let byte_index = lsp_character_to_byte_index(line, col);
    let prefix = &line[..byte_index];

    // 1. Generic invoke completion: invoke<|>("cmd")
    if let Some(items) = complete_invoke_generic_type(line, prefix, byte_index, project_index) {
        return Some(CompletionResponse::Array(items));
    }

    // 2. Arguments completion: invoke("cmd", { | })
    if let Some(items) = complete_invoke_arguments(prefix, project_index) {
        return Some(CompletionResponse::Array(items));
    }

    // 3. Command/Event name completion
    if let Some(items) = complete_command_event_names(prefix, project_index) {
        return Some(CompletionResponse::Array(items));
    }

    None
}

/// Extract first quoted string from input. Returns `(content, rest_of_string)`.
fn split_at_first_quote(s: &str) -> Option<(&str, &str)> {
    let quote_start = s.find('"').or_else(|| s.find('\''))?;
    let after_quote = &s[quote_start + 1..];
    let quote_end = after_quote.find('"').or_else(|| after_quote.find('\''))?;

    Some((&after_quote[..quote_end], &after_quote[quote_end + 1..]))
}

fn complete_invoke_generic_type(
    line: &str,
    prefix: &str,
    byte_index: usize,
    project_index: &ProjectIndex,
) -> Option<Vec<CompletionItem>> {
    let invoke_pos = prefix.rfind("invoke")?;
    let after_invoke = &line[invoke_pos + 6..];

    if !after_invoke.starts_with('<') {
        return None;
    }

    let prefix_after_invoke = &prefix[invoke_pos + 6..];
    if prefix_after_invoke.contains('>') {
        return None;
    }

    // Find command name in the rest of the line
    let rest_of_line = &line[byte_index..];
    let (cmd_name, _) = split_at_first_quote(rest_of_line)?;

    let locations = project_index.get_locations(EntityType::Command, cmd_name);
    let def = locations
        .iter()
        .find(|l| l.behavior == Behavior::Definition)?;

    let rust_ret = def.return_type.as_ref()?;
    let ts_type = map_rust_type_to_ts(rust_ret);

    Some(vec![CompletionItem {
        label: ts_type,
        kind: Some(CompletionItemKind::TYPE_PARAMETER),
        detail: Some(format!("Return type of {cmd_name}")),
        ..Default::default()
    }])
}

fn complete_invoke_arguments(
    prefix: &str,
    project_index: &ProjectIndex,
) -> Option<Vec<CompletionItem>> {
    let invoke_pos = prefix.rfind("invoke")?;
    let rest = &prefix[invoke_pos + 6..];

    // Check for open brace of args object
    let _open_brace_pos = rest.rfind('{')?;

    // Very basic check: ensure the open brace is after the command name
    // Found duplication here in original code: extracting command name logic
    let (cmd_name, after_cmd_name) = split_at_first_quote(rest)?;

    // Check if open brace is after the command name quote
    if !after_cmd_name.contains('{') {
        return None;
    }

    // 1. Try bindings first
    if let Some(binding) = project_index.bindings_cache.get(cmd_name) {
        let items: Vec<_> = binding
            .args
            .iter()
            .map(|bp| CompletionItem {
                label: bp.name.clone(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(format!(": {}", bp.type_name)),
                insert_text: Some(format!("{}: ", bp.name)),
                ..Default::default()
            })
            .collect();
        // Return bindings results if any (or empty list if binding exists but no args)
        return Some(items);
    }

    let locations = project_index.get_locations(EntityType::Command, cmd_name);
    let def = locations
        .iter()
        .find(|l| l.behavior == Behavior::Definition)?;

    let rust_params = def.parameters.as_ref()?;

    let items: Vec<_> = rust_params
        .iter()
        .filter(|p| {
            !["State", "AppHandle", "Window"]
                .iter()
                .any(|&s| p.type_name.contains(s))
        })
        .map(|rp| {
            let camel_name = snake_to_camel(&rp.name);
            let ts_type = map_rust_type_to_ts(&rp.type_name);
            let mut detail = format!(": {ts_type}");

            let base = get_base_rust_type(&rp.type_name);
            if let Some(fields_str) = format_struct_fields(project_index, &base) {
                let _ = write!(detail, " {fields_str}");
            }

            CompletionItem {
                label: camel_name.clone(),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(detail),
                insert_text: Some(format!("{camel_name}: ")),
                ..Default::default()
            }
        })
        .collect();

    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn complete_command_event_names(
    prefix: &str,
    project_index: &ProjectIndex,
) -> Option<Vec<CompletionItem>> {
    // Check if in completion context
    let in_context = COMPLETION_TRIGGERS.iter().any(|name| {
        if let Some(pos) = prefix.rfind(name) {
            let rest = &prefix[pos + name.len()..];
            rest.starts_with('(') || rest.starts_with('<')
        } else {
            false
        }
    });

    if !in_context {
        return None;
    }

    // Add commands
    let mut items = Vec::new();

    // Add commands from bindings
    for entry in &project_index.bindings_cache {
        let name = entry.key();
        let binding = entry.value();
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail: Some(format!(
                "Command (binding) → {}",
                binding.return_type.as_deref().unwrap_or("void")
            )),
            ..Default::default()
        });
    }

    // Add commands from Rust index
    for (name, def_loc) in project_index.get_all_names(EntityType::Command) {
        // Avoid duplicates if binding exists
        if project_index.bindings_cache.contains_key(&name) {
            continue;
        }
        let detail = def_loc.as_ref().map(|l| {
            let filename = l
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            // Build rich detail with return type info
            let mut detail = format!("Command defined in {filename}");

            if let Some(rt) = &l.return_type {
                let inner = extract_result_ok_type(rt);
                let ts_type = map_rust_type_to_ts(inner);

                let _ = write!(detail, " → {ts_type}");

                let base = get_base_rust_type(rt);
                if let Some(fields_str) = format_struct_fields(project_index, &base) {
                    detail = format!("Command → {base} {fields_str}");
                }
            }

            detail
        });

        items.push(CompletionItem {
            label: name,
            kind: Some(CompletionItemKind::FUNCTION),
            detail,
            ..Default::default()
        });
    }

    // Add events
    for (name, _) in project_index.get_all_names(EntityType::Event) {
        items.push(CompletionItem {
            label: name,
            kind: Some(CompletionItemKind::EVENT),
            detail: Some("Event".to_string()),
            ..Default::default()
        });
    }

    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

#[must_use]
pub fn lsp_character_to_byte_index(line: &str, character: usize) -> usize {
    let mut byte_index = 0;
    let mut char_count = 0;

    for (i, c) in line.char_indices() {
        if char_count == character {
            return i;
        }
        // LSP 'character' is usually based on UTF-16 code units.
        // Most editors (VS Code) use UTF-16.
        // Rust's char is a Unicode Scalar Value.
        // We need to count how many UTF-16 code units this char takes.
        char_count += c.len_utf16();
        byte_index = i + c.len_utf8();
    }

    // If we overshoot or match exactly at the end
    if char_count <= character {
        return byte_index;
    }

    // Fallback? Ideally shouldn't happen if character is valid.
    // If we haven't returned yet, it might mean we are strictly inside the last char
    // (unlikely if loop finishes) OR the requested character is beyond string length.
    // Just return the length of the string to be safe.
    line.len()
}
