use crate::indexer::IndexKey;
use crate::indexer::ProjectIndex;
use crate::syntax::{camel_to_snake, map_ts_type_to_rust, Behavior, EntityType};
use std::path::{Path, PathBuf};
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    DocumentChanges, NumberOrString, OneOf, OptionalVersionedTextDocumentIdentifier, Position,
    Range, TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
};

/// Rust file candidate for command insertion
#[derive(Debug, Clone)]
pub struct RustFileCandidate {
    pub path: PathBuf,
    pub priority: u8,
    pub insertion_line: usize,
}

/// Handle code action request (pure function)
///
/// `src_tauri_dir` is the pre-computed src-tauri directory (parent of tauri.conf.json).
/// Pass `None` if workspace root is unavailable; code actions requiring Rust file candidates
/// will be skipped.
pub fn handle_code_action(
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    src_tauri_dir: Option<&Path>,
) -> Option<CodeActionResponse> {
    let path = super::uri_to_path(&params.text_document.uri)?;
    let mut actions = Vec::new();

    // --- Rename suggestion: TypeScript type name differs from Rust type name ---
    // Triggered by the "tarus/return-type-name" hint diagnostic (set in diagnostics.rs).
    // The diagnostic carries the replacement text in its `data` field.
    for diag in &params.context.diagnostics {
        if matches!(&diag.code, Some(NumberOrString::String(c)) if c == "tarus/return-type-name") {
            if let Some(replacement) = diag
                .data
                .as_ref()
                .and_then(|d| d.get("replacement"))
                .and_then(|v| v.as_str())
            {
                let rust_type = diag
                    .data
                    .as_ref()
                    .and_then(|d| d.get("rustType"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(replacement);

                // `data.tsType` is the base TS type name (without `[]`) stored by
                // `type_name_hint`. We use it to locate the generic argument `<TsType...>`
                // in the source file so the edit targets the type arg, not the command string.
                let ts_type = diag
                    .data
                    .as_ref()
                    .and_then(|d| d.get("tsType"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                let edit_range =
                    find_generic_type_range(&path, diag.range.start.line, ts_type, replacement)
                        .unwrap_or(diag.range);

                if let Some(uri) = Uri::from_file_path(&path) {
                    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                        title: format!("Rename to '{replacement}' (match Rust type '{rust_type}')"),
                        kind: Some(CodeActionKind::QUICKFIX),
                        diagnostics: Some(vec![diag.clone()]),
                        edit: Some(WorkspaceEdit {
                            document_changes: Some(DocumentChanges::Edits(vec![
                                TextDocumentEdit {
                                    text_document: OptionalVersionedTextDocumentIdentifier {
                                        uri,
                                        version: None,
                                    },
                                    edits: vec![OneOf::Left(TextEdit {
                                        range: edit_range,
                                        new_text: replacement.to_string(),
                                    })],
                                },
                            ])),
                            ..Default::default()
                        }),
                        ..Default::default()
                    }));
                }
            }
        }
    }

    // --- Rename suggestion: TypeScript field name doesn't match serde serialized name ---
    handle_field_renames(&path, &params.context.diagnostics, &mut actions);

    // --- Key-based actions (generate command, event handler) ---
    let Some((key, loc)) = project_index.get_key_at_position(&path, params.range.start) else {
        return (!actions.is_empty()).then_some(actions);
    };

    match (key.entity, loc.behavior) {
        (EntityType::Command, Behavior::Call) => {
            handle_command_call(
                &key,
                &loc,
                src_tauri_dir,
                params,
                project_index,
                &mut actions,
            );
        }

        (EntityType::Event, Behavior::Call) => {
            handle_event_call(
                &key,
                &loc,
                src_tauri_dir,
                params,
                project_index,
                &mut actions,
            );
        }
        _ => {}
    }

    (!actions.is_empty()).then_some(actions)
}

/// Find the source range covering the content of a generic type argument `<TsType>` near `line`.
///
/// `ts_type` is the base type name without `[]` (e.g., `"SimpleUser"`).
/// `replacement` is the full replacement text (e.g., `"SimpleUser1[]"`), used to
/// determine how many characters of the generic content to select.
///
/// Searches `line` and up to 2 lines before it (the `<T>` is usually on the invoke keyword line,
/// while the string literal — and thus the diagnostic — may be on the same or a later line).
/// Returns `None` if the pattern is not found.
fn find_generic_type_range(
    path: &Path,
    diag_line: u32,
    ts_type: &str,
    replacement: &str,
) -> Option<Range> {
    if ts_type.is_empty() {
        return None;
    }

    let content = std::fs::read_to_string(path).ok()?;
    let lines: Vec<&str> = content.lines().collect();
    let diag_line_idx = usize::try_from(diag_line).ok()?;

    // Search from 2 lines before the diagnostic up to the diagnostic line itself.
    // The generic `<T>` and the string literal are almost always on the same line.
    let search_start = diag_line_idx.saturating_sub(2);
    let search_end = diag_line_idx + 1;

    let pattern = format!("<{ts_type}");

    for abs_idx in (search_start..search_end.min(lines.len())).rev() {
        let line_text = lines[abs_idx];
        let Some(bracket_col) = line_text.find(&pattern) else {
            continue;
        };

        // Start is the character right after `<`
        let start_col = bracket_col + 1; // skip `<`

        // End: scan forward from start_col to find where the generic content ends.
        // Stop at the matching `>`, or at `,` / whitespace that closes the type arg.
        let after_open = &line_text[start_col..];
        let content_len = after_open
            .find(['>', ','])
            .unwrap_or(after_open.len());

        // Sanity: the content we found must be at least as long as ts_type itself.
        if content_len < ts_type.len() {
            continue;
        }

        // Also verify: replacement must fit (avoid replacing unrelated `<` tokens).
        // The current generic text must start with ts_type.
        if !after_open[..content_len].starts_with(ts_type) {
            continue;
        }

        let _ = replacement; // replacement length doesn't constrain the selection

        let abs_line_u32 = u32::try_from(abs_idx).ok()?;
        let start_col_u32 = u32::try_from(start_col).ok()?;
        let end_col_u32 = u32::try_from(start_col + content_len).ok()?;

        return Some(Range {
            start: Position { line: abs_line_u32, character: start_col_u32 },
            end: Position { line: abs_line_u32, character: end_col_u32 },
        });
    }

    None
}

/// Handle `tarus/rename-field` diagnostics.
///
/// For each diagnostic, reads the file to locate the exact `wrong_name:` key and
/// generates a `TextEdit` renaming it to `correct_name`.
fn handle_field_renames(
    path: &Path,
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
    actions: &mut Vec<CodeActionOrCommand>,
) {
    for diag in diagnostics {
        if !matches!(&diag.code, Some(NumberOrString::String(c)) if c == "tarus/rename-field") {
            continue;
        }

        let Some(wrong_name) = diag
            .data
            .as_ref()
            .and_then(|d| d.get("wrongName"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };

        let Some(correct_name) = diag
            .data
            .as_ref()
            .and_then(|d| d.get("correctName"))
            .and_then(|v| v.as_str())
        else {
            continue;
        };

        // Search from the invoke call line downward (up to 30 lines).
        let start_line = usize::try_from(
            diag.data
                .as_ref()
                .and_then(|d| d.get("line"))
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(u64::from(diag.range.start.line)),
        )
        .unwrap_or(0);

        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };

        let lines: Vec<&str> = content.lines().collect();
        let search_end = (start_line + 30).min(lines.len());
        let key_pattern = format!("{wrong_name}:");

        let mut found_range: Option<Range> = None;

        for (idx, line_text) in lines[start_line..search_end].iter().enumerate() {
            if let Some(col) = line_text.find(key_pattern.as_str()) {
                let abs_line = u32::try_from(start_line + idx).unwrap_or(u32::MAX);
                let col_start = u32::try_from(col).unwrap_or(u32::MAX);
                let col_end = u32::try_from(col + wrong_name.len()).unwrap_or(u32::MAX);

                found_range = Some(Range {
                    start: Position {
                        line: abs_line,
                        character: col_start,
                    },
                    end: Position {
                        line: abs_line,
                        character: col_end,
                    },
                });

                break;
            }
        }

        let Some(edit_range) = found_range else {
            continue;
        };

        let Some(uri) = Uri::from_file_path(path) else {
            continue;
        };

        actions.push(CodeActionOrCommand::CodeAction(CodeAction {
            title: format!("Rename '{wrong_name}' to '{correct_name}'"),
            kind: Some(CodeActionKind::QUICKFIX),
            diagnostics: Some(vec![diag.clone()]),
            edit: Some(WorkspaceEdit {
                document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
                    text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
                    edits: vec![OneOf::Left(TextEdit {
                        range: edit_range,
                        new_text: correct_name.to_string(),
                    })],
                }])),
                ..Default::default()
            }),
            ..Default::default()
        }));
    }
}

/// Handle event call (emit) - offer to create Rust handler (listen)
fn handle_event_call(
    key: &IndexKey,
    _loc: &crate::indexer::LocationInfo,
    src_tauri_dir: Option<&Path>,
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let info = project_index.get_diagnostic_info(key);

    if !info.has_definition() {
        let candidates = find_rust_file_candidates(src_tauri_dir);

        for candidate in rank_and_limit(candidates) {
            let event_name = &key.name;
            let handler_name = format!("handle_{}", camel_to_snake(event_name));

            let snippet = format!(
                "\n// Generated event handler for '{event_name}'\npub fn {handler_name}<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {{\n    app.listen(\"{event_name}\", |event| {{\n        println!(\"Received event: {{:?}}\", event.payload());\n    }});\n}}\n", 
            );

            let Some(target_uri) = Uri::from_file_path(&candidate.path) else {
                continue;
            };

            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!(
                    "Generate handler '{}' in {}",
                    handler_name,
                    candidate
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(params.context.diagnostics.clone()),
                edit: Some(create_workspace_edit(
                    target_uri,
                    u32::try_from(candidate.insertion_line).unwrap_or(u32::MAX),
                    snippet,
                )),
                ..Default::default()
            }));
        }
    }
}

/// Handle command call - offer to create it (if missing)
fn handle_command_call(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    src_tauri_dir: Option<&Path>,
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let info = project_index.get_diagnostic_info(key);

    if !info.has_definition() {
        // Undefined: Offer to create in Rust
        let candidates = find_rust_file_candidates(src_tauri_dir);
        let rust_args = infer_rust_args(loc);

        for candidate in rank_and_limit(candidates) {
            if let Some(action) = create_rust_command_action(
                &key.name,
                &rust_args,
                &candidate,
                &params.context.diagnostics,
            ) {
                actions.push(action);
            }
        }
    }
}

fn infer_rust_args(loc: &crate::indexer::LocationInfo) -> String {
    if let Some(params) = &loc.parameters {
        if let Some(first_param) = params.first() {
            if first_param.type_name.starts_with('{') {
                let fields = crate::syntax::parse_ts_object_string(&first_param.type_name);

                if fields.is_empty() {
                    return String::new();
                }

                let mut args_strs = Vec::new();
                let mut sorted_keys: Vec<_> = fields.keys().collect();

                sorted_keys.sort();

                for key in sorted_keys {
                    let ts_type = fields.get(key).unwrap();
                    let rust_type = map_ts_type_to_rust(ts_type);
                    let rust_name = camel_to_snake(key);
                    args_strs.push(format!("{rust_name}: {rust_type}"));
                }

                return args_strs.join(", ");
            }
        }
    }
    String::new()
}

/// Helper to create a workspace edit with a single text insertion
fn create_workspace_edit(uri: Uri, line: u32, text: String) -> WorkspaceEdit {
    let text_document_edit = TextDocumentEdit {
        text_document: OptionalVersionedTextDocumentIdentifier { uri, version: None },
        edits: vec![OneOf::Left(TextEdit {
            range: Range {
                start: Position { line, character: 0 },
                end: Position { line, character: 0 },
            },
            new_text: text,
        })],
    };

    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(vec![text_document_edit])),
        ..Default::default()
    }
}

/// Helper to create "Create Rust Command" action
fn create_rust_command_action(
    name: &str,
    args: &str,
    candidate: &RustFileCandidate,
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
) -> Option<CodeActionOrCommand> {
    let command_template = format!(
        "\n#[tauri::command]\nfn {name}({args}) -> Result<String, String> {{\n    Ok(\"Not implemented\".to_string())\n}}\n"
    );

    let target_uri = Uri::from_file_path(&candidate.path)?;
    let file_name = candidate.path.file_name().and_then(|n| n.to_str())?;

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Create Rust command '{name}' in {file_name}"),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(diagnostics.to_vec()),
        edit: Some(create_workspace_edit(
            target_uri,
            u32::try_from(candidate.insertion_line).unwrap_or(u32::MAX),
            command_template,
        )),
        ..Default::default()
    }))
}

fn find_rust_file_candidates(src_tauri_dir: Option<&Path>) -> Vec<RustFileCandidate> {
    let Some(dir) = src_tauri_dir else {
        return Vec::new();
    };

    let src_dir = dir.join("src");

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
