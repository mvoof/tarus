//! Code Actions capability - generate Rust command templates

use crate::indexer::{GeneratorKind, LocationInfo, ProjectIndex};
use crate::scanner::find_src_tauri_dir;
use crate::syntax::{Behavior, EntityType};
use std::path::{Path, PathBuf};
use tower_lsp_server::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    Position, Range, TextEdit, Uri, WorkspaceEdit,
};
use tower_lsp_server::UriExt;

/// Rust file candidate for command insertion
#[derive(Debug, Clone)]
pub struct RustFileCandidate {
    pub path: PathBuf,
    pub priority: u8,
    pub insertion_line: usize,
}

/// Handle code action request (pure function)
pub fn handle_code_action(
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    workspace_root: Option<&PathBuf>,
) -> Option<CodeActionResponse> {
    let path_cow = params.text_document.uri.to_file_path()?;
    let path = path_cow.to_path_buf();

    let position = params.range.start;

    // Check if cursor is on a command
    if let Some((key, loc)) = project_index.get_key_at_position(&path, position) {
        if key.entity != EntityType::Command {
            return None;
        }

        // --- Return type code action ---
        if let Some(action) = make_return_type_action(&key.name, &loc, project_index, params) {
            return Some(vec![CodeActionOrCommand::CodeAction(action)]);
        }

        // --- Generate Rust command stub (existing) ---
        let info = project_index.get_diagnostic_info(&key);
        if info.has_definition {
            return None;
        }

        let root = workspace_root?;

        let candidates = find_rust_file_candidates(root);
        if candidates.is_empty() {
            return None;
        }

        let ranked = rank_and_limit(candidates);
        let mut actions = Vec::new();

        for candidate in ranked {
            let file_name = candidate
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");

            let command_template = format!(
                "\n#[tauri::command]\nfn {}() -> Result<String, String> {{\n    Ok(\"Not implemented\".to_string())\n}}\n",
                key.name
            );

            let Some(target_uri) = Uri::from_file_path(&candidate.path) else {
                continue;
            };

            // Uri has interior mutability due to caching, but we don't modify after insertion
            #[allow(clippy::mutable_key_type)]
            let mut changes = std::collections::HashMap::new();
            // LSP line numbers won't exceed u32::MAX in practice
            #[allow(clippy::cast_possible_truncation)]
            changes.insert(
                target_uri,
                vec![TextEdit {
                    range: Range {
                        start: Position {
                            line: candidate.insertion_line as u32,
                            character: 0,
                        },
                        end: Position {
                            line: candidate.insertion_line as u32,
                            character: 0,
                        },
                    },
                    new_text: command_template,
                }],
            );

            let workspace_edit = WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            };

            let action = CodeAction {
                title: format!("Create Rust command '{}' in {}", key.name, file_name),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(params.context.diagnostics.clone()),
                edit: Some(workspace_edit),
                ..Default::default()
            };

            actions.push(CodeActionOrCommand::CodeAction(action));
        }

        return Some(actions);
    }

    None
}

fn find_rust_file_candidates(workspace_root: &Path) -> Vec<RustFileCandidate> {
    let Some(src_tauri_dir) = find_src_tauri_dir(workspace_root) else {
        return Vec::new();
    };

    let src_dir = src_tauri_dir.join("src");
    if !src_dir.exists() {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let Ok(entries) = std::fs::read_dir(&src_dir) else {
        return Vec::new();
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };

        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let priority = calculate_file_priority(file_name, &content);
        let insertion_line = find_insertion_line(&content);

        candidates.push(RustFileCandidate {
            path,
            priority,
            insertion_line,
        });
    }

    candidates
}

fn calculate_file_priority(file_name: &str, content: &str) -> u8 {
    if file_name == "lib.rs" {
        return 100;
    }
    if file_name == "main.rs" {
        return 95;
    }
    if content.contains("invoke_handler(") {
        return 85;
    }
    if content.contains("#[tauri::command]") {
        return 80;
    }
    match file_name {
        "commands.rs" | "api.rs" | "handlers.rs" => 70,
        "mod.rs" => 65,
        _ => 50,
    }
}

fn find_insertion_line(content: &str) -> usize {
    let lines: Vec<&str> = content.lines().collect();
    let mut last_use = 0;
    let mut last_mod = 0;

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
            last_use = i + 1;
        }
        if (trimmed.starts_with("mod ") || trimmed.starts_with("pub mod "))
            && !trimmed.contains('{')
        {
            last_mod = i + 1;
        }
    }

    let insert_after = last_use.max(last_mod);
    if insert_after > 0 {
        return insert_after + 1;
    }
    0
}

fn rank_and_limit(mut candidates: Vec<RustFileCandidate>) -> Vec<RustFileCandidate> {
    candidates.sort_by(|a, b| b.priority.cmp(&a.priority));
    candidates.into_iter().take(5).collect()
}

/// Build a code action to fix or insert the return type on an `invoke()` call.
fn make_return_type_action(
    command_name: &str,
    loc: &LocationInfo,
    project_index: &ProjectIndex,
    params: &CodeActionParams,
) -> Option<CodeAction> {
    if !matches!(loc.behavior, Behavior::Call) {
        return None;
    }

    if !project_index.has_bindings_files() {
        return None;
    }

    let schema = project_index.get_schema(command_name)?;

    // RustSource schemas are allowed when the return type has a known binding
    if matches!(schema.generator, GeneratorKind::RustSource)
        && !project_index.type_aliases.contains_key(&schema.return_type)
    {
        return None;
    }

    let expected = &schema.return_type;
    if expected == "void" {
        return None;
    }

    let (title, edit_range, new_text) = match &loc.return_type {
        None => {
            // Missing generic: insert <Expected> after function name
            let insert_pos = loc.call_name_end?;
            (
                format!("Add return type '{expected}'"),
                Range {
                    start: insert_pos,
                    end: insert_pos,
                },
                format!("<{expected}>"),
            )
        }
        Some(ts_type) => {
            if ts_type == "void" || ts_type == "any" {
                return None;
            }
            if super::diagnostics::types_match(ts_type, expected, project_index) {
                return None;
            }
            // Wrong generic: replace <Wrong> with <Expected>
            let type_range = loc.type_arg_range?;
            (
                format!("Fix return type to '{expected}'"),
                type_range,
                format!("<{expected}>"),
            )
        }
    };

    let doc_uri = params.text_document.uri.clone();
    // Uri has interior mutability due to caching
    #[allow(clippy::mutable_key_type)]
    let mut changes = std::collections::HashMap::new();
    changes.insert(doc_uri, vec![TextEdit { range: edit_range, new_text }]);

    Some(CodeAction {
        title,
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(params.context.diagnostics.clone()),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}
