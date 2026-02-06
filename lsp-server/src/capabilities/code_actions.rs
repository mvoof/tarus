use crate::indexer::IndexKey;
use crate::indexer::ProjectIndex;
use crate::scanner::find_src_tauri_dir;
use crate::syntax::{
    camel_to_snake, map_rust_type_to_ts, map_ts_type_to_rust, snake_to_camel, Behavior, EntityType,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tower_lsp_server::lsp_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range,
    TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
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
    workspace_root: Option<&Path>,
) -> Option<CodeActionResponse> {
    let path = params.text_document.uri.to_file_path()?.to_path_buf();
    let root = workspace_root?;
    let (key, loc) = project_index.get_key_at_position(&path, params.range.start)?;

    let mut actions = Vec::new();

    // Handle each entity type with dedicated functions
    match (key.entity, loc.behavior) {
        (EntityType::Command, Behavior::Call) => {
            handle_undefined_command(&key, root, params, project_index, &mut actions);
        }
        (EntityType::Struct, Behavior::Definition) => {
            handle_struct_definition(&key, &loc, root, params, &mut actions);
        }
        (EntityType::Enum, Behavior::Definition) => {
            handle_enum_definition(&key, &loc, root, params, &mut actions);
        }
        (EntityType::Interface, Behavior::Definition) => {
            handle_interface_definition(
                &key,
                &loc,
                &path,
                root,
                params,
                project_index,
                &mut actions,
            );
        }
        _ => {} // No actions for other combinations
    }

    (!actions.is_empty()).then_some(actions)
}

/// Handle undefined command call - offer to create Rust command
fn handle_undefined_command(
    key: &IndexKey,
    root: &Path,
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let info = project_index.get_diagnostic_info(key);

    if info.has_definition() {
        return;
    }

    let candidates = find_rust_file_candidates(root);

    for candidate in rank_and_limit(candidates) {
        actions.push(create_rust_command_action(
            &key.name,
            &candidate,
            &params.context.diagnostics,
        ));
    }
}

/// Handle struct definition - offer to sync to .d.ts
fn handle_struct_definition(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    root: &Path,
    params: &CodeActionParams,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let Some(fields) = &loc.fields else {
        return;
    };

    if let Some(action) =
        create_sync_to_dts_action(&key.name, fields, root, &params.context.diagnostics)
    {
        actions.push(action);
    }
}

/// Handle enum definition - offer to sync to .d.ts
fn handle_enum_definition(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    root: &Path,
    params: &CodeActionParams,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let Some(variants) = &loc.fields else {
        return;
    };

    if let Some(action) =
        create_sync_enum_to_dts_action(&key.name, variants, root, &params.context.diagnostics)
    {
        actions.push(action);
    }
}

/// Handle interface definition - offer to sync to .d.ts and create Rust struct
fn handle_interface_definition(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    path: &Path,
    root: &Path,
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let Some(fields) = &loc.fields else {
        return;
    };

    // A. Copy to tauri-commands.d.ts (if not already there)
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    if file_name != "tauri-commands.d.ts" {
        if let Some(action) = create_copy_interface_to_dts_action(
            &key.name,
            fields,
            root,
            &params.context.diagnostics,
        ) {
            actions.push(action);
        }
    }

    // B. Create Rust struct (if doesn't exist in Rust)
    let rust_struct_exists = !project_index
        .get_locations(EntityType::Struct, &key.name)
        .is_empty();

    if rust_struct_exists {
        return;
    }

    let candidates = find_rust_file_candidates(root);

    for candidate in rank_and_limit(candidates) {
        if let Some(action) =
            create_rust_struct_action(&key.name, fields, &candidate, &params.context.diagnostics)
        {
            actions.push(action);
        }
    }
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

/// Create "Sync to tauri-commands.d.ts" action for Rust struct
fn create_sync_to_dts_action(
    name: &str,
    fields: &[crate::indexer::Parameter],
    workspace_root: &Path,
    diagnostics: &[tower_lsp_server::lsp_types::Diagnostic],
) -> Option<CodeActionOrCommand> {
    let dts_path = find_or_create_dts_path(workspace_root);

    // Generate TypeScript interface from Rust struct
    let mut ts_interface = format!("\nexport interface {name} {{\n");

    for field in fields {
        let ts_type = map_rust_type_to_ts(&field.type_name);
        let ts_name = snake_to_camel(&field.name);
        let _ = writeln!(ts_interface, "  {ts_name}: {ts_type};");
    }

    ts_interface.push_str("}\n");

    let (target_uri, insertion_line) = prepare_dts_edit(&dts_path)?;

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Sync '{name}' to tauri-commands.d.ts"),
        kind: Some(CodeActionKind::REFACTOR),
        diagnostics: Some(diagnostics.to_vec()),
        edit: Some(create_workspace_edit(
            target_uri,
            u32::try_from(insertion_line).unwrap_or(u32::MAX),
            ts_interface,
        )),
        ..Default::default()
    }))
}

/// Create "Sync enum to tauri-commands.d.ts" action for Rust enum
fn create_sync_enum_to_dts_action(
    name: &str,
    variants: &[crate::indexer::Parameter],
    workspace_root: &Path,
    diagnostics: &[tower_lsp_server::lsp_types::Diagnostic],
) -> Option<CodeActionOrCommand> {
    let dts_path = find_or_create_dts_path(workspace_root);

    // Generate TypeScript type from Rust enum
    let mut ts_type = format!("\nexport type {name} = ");

    for (i, variant) in variants.iter().enumerate() {
        if i > 0 {
            ts_type.push_str(" | ");
        }

        let _ = write!(ts_type, "'{}'", variant.name);
    }

    ts_type.push_str(";\n");

    let (target_uri, insertion_line) = prepare_dts_edit(&dts_path)?;

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Sync enum '{name}' to tauri-commands.d.ts"),
        kind: Some(CodeActionKind::REFACTOR),
        diagnostics: Some(diagnostics.to_vec()),
        edit: Some(create_workspace_edit(
            target_uri,
            u32::try_from(insertion_line).unwrap_or(u32::MAX),
            ts_type,
        )),
        ..Default::default()
    }))
}

/// Create "Copy interface to tauri-commands.d.ts" action for TS interface
fn create_copy_interface_to_dts_action(
    name: &str,
    fields: &[crate::indexer::Parameter],
    workspace_root: &Path,
    diagnostics: &[tower_lsp_server::lsp_types::Diagnostic],
) -> Option<CodeActionOrCommand> {
    let dts_path = find_or_create_dts_path(workspace_root);

    // Generate interface (keep TS types as-is)
    let mut ts_interface = format!("\nexport interface {name} {{\n");

    for field in fields {
        let _ = writeln!(ts_interface, "  {}: {};", field.name, field.type_name);
    }

    ts_interface.push_str("}\n");

    let (target_uri, insertion_line) = prepare_dts_edit(&dts_path)?;

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Copy '{name}' to tauri-commands.d.ts"),
        kind: Some(CodeActionKind::REFACTOR),
        diagnostics: Some(diagnostics.to_vec()),
        edit: Some(create_workspace_edit(
            target_uri,
            u32::try_from(insertion_line).unwrap_or(u32::MAX),
            ts_interface,
        )),
        ..Default::default()
    }))
}

/// Find existing tauri-commands.d.ts or return default path
fn find_or_create_dts_path(workspace_root: &Path) -> PathBuf {
    let possible_locations = [
        workspace_root.join("src/tauri-commands.d.ts"),
        workspace_root.join("src/types/tauri-commands.d.ts"),
        workspace_root.join("tauri-commands.d.ts"),
    ];

    for path in &possible_locations {
        if path.exists() {
            return path.clone();
        }
    }

    // Default to src/tauri-commands.d.ts
    let src_dir = workspace_root.join("src");

    if src_dir.exists() {
        src_dir.join("tauri-commands.d.ts")
    } else {
        workspace_root.join("tauri-commands.d.ts")
    }
}

/// Prepare edit target for .d.ts file
fn prepare_dts_edit(dts_path: &Path) -> Option<(Uri, usize)> {
    let target_uri = Uri::from_file_path(dts_path)?;

    let insertion_line = if dts_path.exists() {
        let content = std::fs::read_to_string(dts_path).unwrap_or_default();
        find_dts_insertion_line(&content)
    } else {
        0
    };

    Some((target_uri, insertion_line))
}

/// Find insertion line for .d.ts files (at the end, after existing interfaces)
fn find_dts_insertion_line(content: &str) -> usize {
    let lines: Vec<&str> = content.lines().collect();

    // Find the last non-empty line
    for (i, line) in lines.iter().enumerate().rev() {
        if !line.trim().is_empty() {
            return i + 1;
        }
    }

    lines.len()
}

/// Create "Create Rust struct" action from TS interface
fn create_rust_struct_action(
    name: &str,
    fields: &[crate::indexer::Parameter],
    candidate: &RustFileCandidate,
    diagnostics: &[tower_lsp_server::lsp_types::Diagnostic],
) -> Option<CodeActionOrCommand> {
    let mut rust_struct =
        format!("\n#[derive(serde::Deserialize, serde::Serialize)]\npub struct {name} {{\n");

    for field in fields {
        let rust_type = map_ts_type_to_rust(&field.type_name);
        let rust_name = camel_to_snake(&field.name);
        let _ = writeln!(rust_struct, "    pub {rust_name}: {rust_type},");
    }
    rust_struct.push_str("}\n");

    let target_uri = Uri::from_file_path(&candidate.path)?;
    let file_name = candidate.path.file_name()?.to_str()?;

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Create Rust struct '{name}' in {file_name}"),
        kind: Some(CodeActionKind::REFACTOR),
        diagnostics: Some(diagnostics.to_vec()),
        edit: Some(create_workspace_edit(
            target_uri,
            u32::try_from(candidate.insertion_line).unwrap_or(u32::MAX),
            rust_struct,
        )),
        ..Default::default()
    }))
}

/// Helper to create "Create Rust Command" action
fn create_rust_command_action(
    name: &str,
    candidate: &RustFileCandidate,
    diagnostics: &[tower_lsp_server::lsp_types::Diagnostic],
) -> CodeActionOrCommand {
    let command_template = format!(
        "\n#[tauri::command]\nfn {name}() -> Result<String, String> {{\n    Ok(\"Not implemented\".to_string())\n}}\n"
    );

    let target_uri = Uri::from_file_path(&candidate.path).unwrap();
    let file_name = candidate.path.file_name().and_then(|n| n.to_str()).unwrap();

    CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Create Rust command '{name}' in {file_name}"),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(diagnostics.to_vec()),
        edit: Some(create_workspace_edit(
            target_uri,
            u32::try_from(candidate.insertion_line).unwrap_or(u32::MAX),
            command_template,
        )),
        ..Default::default()
    })
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
