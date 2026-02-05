//! Completion capability - autocomplete commands and events

use crate::indexer::ProjectIndex;
use crate::syntax::{map_rust_type_to_ts, snake_to_camel, Behavior, EntityType};
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
};
use tower_lsp_server::UriExt;

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

#[allow(clippy::too_many_lines)]
/// Handle completion request (pure function)
pub fn handle_completion(
    params: &CompletionParams,
    project_index: &ProjectIndex,
    document_cache: &Arc<DashMap<PathBuf, String>>,
) -> Option<CompletionResponse> {
    let uri = &params.text_document_position.text_document.uri;
    let path_cow = uri.to_file_path()?;
    let path: PathBuf = path_cow.to_path_buf();

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

    // Check if we are inside the generic arguments of invoke
    // e.g., invoke<|> ("my_cmd")
    if let Some(invoke_pos) = prefix.rfind("invoke") {
        let after_invoke = &line[invoke_pos + 6..];

        if after_invoke.starts_with('<') {
            let prefix_after_invoke = &prefix[invoke_pos + 6..];

            if !prefix_after_invoke.contains('>') {
                // Find command name in the rest of the line
                let rest_of_line = &line[byte_index..];

                if let Some(quote_start) =
                    rest_of_line.find('\"').or_else(|| rest_of_line.find('\''))
                {
                    let after_quote = &rest_of_line[quote_start + 1..];

                    if let Some(quote_end) =
                        after_quote.find('\"').or_else(|| after_quote.find('\''))
                    {
                        let cmd_name = &after_quote[..quote_end];

                        let mut items = Vec::new();
                        let locations = project_index.get_locations(EntityType::Command, cmd_name);

                        if let Some(def) = locations
                            .iter()
                            .find(|l| l.behavior == Behavior::Definition)
                        {
                            if let Some(rust_ret) = &def.return_type {
                                let ts_type = map_rust_type_to_ts(rust_ret);

                                items.push(CompletionItem {
                                    label: ts_type,
                                    kind: Some(CompletionItemKind::TYPE_PARAMETER),
                                    detail: Some(format!("Return type of {cmd_name}")),
                                    ..Default::default()
                                });

                                return Some(CompletionResponse::Array(items));
                            }
                        }
                    }
                }
            }
        }
    }

    // Check if we are inside the arguments object of invoke
    // e.g., invoke("my_cmd", { | })
    if let Some(invoke_pos) = prefix.rfind("invoke") {
        let rest = &prefix[invoke_pos + 6..];

        if let Some(_open_brace_pos) = rest.rfind('{') {
            // Very basic check: ensure the open brace is after the command name
            let after_invoke = &rest;

            if let Some(quote_start) = after_invoke.find('\"').or_else(|| after_invoke.find('\'')) {
                let after_quote = &after_invoke[quote_start + 1..];

                if let Some(quote_end) = after_quote.find('\"').or_else(|| after_quote.find('\'')) {
                    let cmd_name = &after_quote[..quote_end];

                    // Check if open brace is after the command name quote
                    let after_cmd_name = &after_quote[quote_end + 1..];

                    if after_cmd_name.contains('{') {
                        let mut items = Vec::new();
                        let locations = project_index.get_locations(EntityType::Command, cmd_name);

                        if let Some(def) = locations
                            .iter()
                            .find(|l| l.behavior == Behavior::Definition)
                        {
                            if let Some(rust_params) = &def.parameters {
                                let filtered_rust_params: Vec<_> = rust_params
                                    .iter()
                                    .filter(|p| {
                                        !["State", "AppHandle", "Window"]
                                            .iter()
                                            .any(|&s| p.type_name.contains(s))
                                    })
                                    .collect();

                                for rp in filtered_rust_params {
                                    let camel_name = snake_to_camel(&rp.name);

                                    items.push(CompletionItem {
                                        label: camel_name.clone(),
                                        kind: Some(CompletionItemKind::PROPERTY),
                                        detail: Some(format!(
                                            ": {}",
                                            map_rust_type_to_ts(&rp.type_name)
                                        )),
                                        insert_text: Some(format!("{camel_name}: ")),
                                        ..Default::default()
                                    });
                                }

                                if !items.is_empty() {
                                    return Some(CompletionResponse::Array(items));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Check if in completion context
    // Support both direct calls: invoke("...") and generic calls: invoke<Type>("...")
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

    let mut items = Vec::new();

    // Add commands
    for (name, def_loc) in project_index.get_all_names(EntityType::Command) {
        let detail = def_loc.as_ref().map(|l| {
            let filename = l
                .path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");
            format!("Command defined in {filename}")
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
        return None;
    }

    Some(CompletionResponse::Array(items))
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
