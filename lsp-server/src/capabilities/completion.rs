//! Completion capability - autocomplete commands and events

use crate::indexer::ProjectIndex;
use crate::syntax::EntityType;
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tower_lsp_server::lsp_types::{
    CompletionItem, CompletionItemKind, CompletionParams, CompletionResponse,
};
use tower_lsp_server::UriExt;

const COMPLETION_TRIGGERS: &[&str] = &[
    "invoke", "emit", "emitTo", "listen", "once", "emit_to", "emit_str",
    "emit_str_to", "emit_filter", "emit_str_filter", "listen_any", "once_any",
];

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
    let prefix = if col <= line.len() { &line[..col] } else { line };

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
            let filename = l.path.file_name().and_then(|s| s.to_str()).unwrap_or("unknown");
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
