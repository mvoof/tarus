use crate::indexer::IndexKey;
use crate::indexer::ProjectIndex;
use crate::scanner::find_src_tauri_dir;
use crate::syntax::{
    camel_to_snake, map_rust_type_to_ts, map_ts_type_to_rust, snake_to_camel, Behavior, EntityType,
};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams, CodeActionResponse,
    DocumentChanges, OneOf, OptionalVersionedTextDocumentIdentifier, Position, Range,
    TextDocumentEdit, TextEdit, Uri, WorkspaceEdit,
};

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
    let path = params.text_document.uri.to_file_path()?.into_owned();
    let root = workspace_root?;
    let (key, loc) = project_index.get_key_at_position(&path, params.range.start)?;

    let mut actions = Vec::new();

    // Handle each entity type with dedicated functions
    // Always offer "Sync all types" if strict type safety is enabled or just generally
    // It's a useful utility.
    actions.push(CodeActionOrCommand::CodeAction(CodeAction {
        title: "Sync all types (Regenerate .d.ts)".to_string(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS), // Or Empty/QuickFix? SOURCE is good for regeneration
        command: Some(tower_lsp_server::ls_types::Command {
            title: "Sync all types".to_string(),
            command: "tarus.syncTypes".to_string(),
            arguments: None,
        }),
        ..Default::default()
    }));

    match (key.entity, loc.behavior) {
        (EntityType::Command, Behavior::Call) => {
            handle_command_call(&key, &loc, root, params, project_index, &mut actions);
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
        (EntityType::Event, Behavior::Call) => {
            handle_event_call(&key, &loc, root, params, project_index, &mut actions);
        }
        _ => {} // No actions for other combinations
    }

    (!actions.is_empty()).then_some(actions)
}

/// Handle event call (emit) - offer to create Rust handler (listen)
fn handle_event_call(
    key: &IndexKey,
    _loc: &crate::indexer::LocationInfo,
    root: &Path,
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    // Check if any Rust file "listens" to this event
    // We assume Rust parser extracts "listen" as Behavior::Definition or similar for Events?
    // Actually patterns.rs defines behavior.
    // If we assume Definition means "handler exists" or "defined", we check that.

    let info = project_index.get_diagnostic_info(key);

    // If no definition found (or no Rust files usage that looks like definition)
    // We offer to create a handler.
    if !info.has_definition() {
        let candidates = find_rust_file_candidates(root);
        for candidate in rank_and_limit(candidates) {
            let event_name = &key.name;
            let handler_name = format!("handle_{}", crate::syntax::camel_to_snake(event_name));

            // snippet to append
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
                    u32::MAX, // Append
                    snippet,
                )),
                ..Default::default()
            }));
        }
    }
}

/// Handle command call - offer to create it (if missing) or wrap it (if exists)
fn handle_command_call(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    root: &Path,
    params: &CodeActionParams,
    project_index: &ProjectIndex,
    actions: &mut Vec<CodeActionOrCommand>,
) {
    let info = project_index.get_diagnostic_info(key);

    if info.has_definition() {
        // Defined: Offer to generate TS wrapper
        let locations = project_index.get_locations(EntityType::Command, &key.name);
        if let Some(def) = locations
            .iter()
            .find(|l| l.behavior == Behavior::Definition)
        {
            let ts_candidates = find_ts_file_candidates(root);
            for candidate in rank_and_limit_ts(ts_candidates) {
                if let Some(action) = create_ts_wrapper_action(
                    &key.name,
                    def,
                    &candidate,
                    &params.context.diagnostics,
                ) {
                    actions.push(action);
                }
            }
        }
    } else {
        // Undefined: Offer to create in Rust
        let candidates = find_rust_file_candidates(root);
        let rust_args = infer_rust_args(loc);

        for candidate in rank_and_limit(candidates) {
            actions.push(create_rust_command_action(
                &key.name,
                &rust_args,
                &candidate,
                &params.context.diagnostics,
            ));
        }
    }
}

fn infer_rust_args(loc: &crate::indexer::LocationInfo) -> String {
    if let Some(params) = &loc.parameters {
        // Typically Tauri invokation: invoke('cmd', { args })
        // frontend_parser extraction might return 1 param with type "{ key: val }"
        // OR multiple params if using legacy invoke(cmd, arg1, arg2) - but Tarus focuses on object style

        if let Some(first_param) = params.first() {
            if first_param.type_name.starts_with('{') {
                // Object style: { x: 1, y: "s" }
                let fields = crate::syntax::parse_ts_object_string(&first_param.type_name);

                if fields.is_empty() {
                    return String::new();
                }

                let mut args_strs = Vec::new();
                // Sort by key for deterministic output
                let mut sorted_keys: Vec<_> = fields.keys().collect();
                sorted_keys.sort();

                for key in sorted_keys {
                    let ts_type = fields.get(key).unwrap();
                    let rust_type = crate::syntax::map_ts_type_to_rust(ts_type);
                    let rust_name = crate::syntax::camel_to_snake(key);
                    args_strs.push(format!("{rust_name}: {rust_type}"));
                }

                return args_strs.join(", ");
            }
            // Primitive style or otherwise?
            // If implicit args object: invoke('cmd', { val }) -> param name might be 'val'?
            // But extractors.rs usually extracts the whole object as one param "args" or similar.
        }
    }
    String::new()
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
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
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
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
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
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
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
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
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
    args: &str,
    candidate: &RustFileCandidate,
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
) -> CodeActionOrCommand {
    let command_template = format!(
        "\n#[tauri::command]\nfn {name}({args}) -> Result<String, String> {{\n    Ok(\"Not implemented\".to_string())\n}}\n"
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

/// TS file candidate for wrapper insertion
#[derive(Debug, Clone)]
pub struct TsFileCandidate {
    pub path: PathBuf,
    pub priority: u8,
}

fn find_ts_file_candidates(workspace_root: &Path) -> Vec<TsFileCandidate> {
    let src_dir = workspace_root.join("src");
    if !src_dir.exists() {
        return Vec::new();
    }

    let mut candidates = Vec::new();

    // Recursive search or just top-level?
    // Let's do top-level + 1 level deep for now to avoid scanning node_modules
    // Ideally we use `find_src_tauri_dir` equivalent for frontend but frontend structure varies.
    // We assume standard Vite/Tauri structure: src/

    if let Ok(entries) = std::fs::read_dir(&src_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                    if ["ts", "js", "tsx", "jsx"].contains(&ext) {
                        candidates.push(create_ts_candidate(path));
                    }
                }
            } else if path.is_dir() {
                // Check subdir
                if let Ok(sub_entries) = std::fs::read_dir(&path) {
                    for sub in sub_entries.flatten() {
                        let sub_path = sub.path();
                        if sub_path.is_file() {
                            if let Some(ext) = sub_path.extension().and_then(|s| s.to_str()) {
                                if ["ts", "js", "tsx", "jsx"].contains(&ext) {
                                    candidates.push(create_ts_candidate(sub_path));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    candidates
}

fn create_ts_candidate(path: PathBuf) -> TsFileCandidate {
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let priority = if file_name.starts_with("api") || file_name.starts_with("tauri") {
        100
    } else if file_name == "main.ts" || file_name == "index.ts" {
        80
    } else if file_name.contains("command") {
        90
    } else {
        50
    };

    TsFileCandidate { path, priority }
}

fn rank_and_limit_ts(mut candidates: Vec<TsFileCandidate>) -> Vec<TsFileCandidate> {
    candidates.sort_by(|a, b| b.priority.cmp(&a.priority));
    candidates.into_iter().take(5).collect()
}

fn create_ts_wrapper_action(
    cmd_name: &str,
    def: &crate::indexer::LocationInfo,
    candidate: &TsFileCandidate,
    diagnostics: &[tower_lsp_server::ls_types::Diagnostic],
) -> Option<CodeActionOrCommand> {
    let return_type = def.return_type.as_deref().unwrap_or("void");
    let ts_ret = crate::syntax::map_rust_type_to_ts(return_type);

    let params_str = if let Some(params) = &def.parameters {
        let p_strs: Vec<String> = params
            .iter()
            .filter(|p| {
                !["State", "AppHandle", "Window"]
                    .iter()
                    .any(|&s| p.type_name.contains(s))
            })
            .map(|p| {
                let name = crate::syntax::snake_to_camel(&p.name);
                let ty = crate::syntax::map_rust_type_to_ts(&p.type_name);
                format!("{name}: {ty}")
            })
            .collect();
        p_strs.join(", ")
    } else {
        String::new()
    };

    let args_obj_str = if let Some(params) = &def.parameters {
        let p_strs: Vec<String> = params
            .iter()
            .filter(|p| {
                !["State", "AppHandle", "Window"]
                    .iter()
                    .any(|&s| p.type_name.contains(s))
            })
            .map(|p| {
                // Tauri maps { camelName } -> snake_name automatically?
                // Yes, invoke('cmd', { camelName: val }) -> fn cmd(camel_name: Type)
                // So we can just use the name as is (camelCase for TS object key)
                crate::syntax::snake_to_camel(&p.name)
            })
            .collect();
        if p_strs.is_empty() {
            String::new()
        } else {
            format!("{{ {} }}", p_strs.join(", "))
        }
    } else {
        String::new()
    };

    let wrapper_name = crate::syntax::snake_to_camel(cmd_name);
    let sep = if args_obj_str.is_empty() { "" } else { ", " };

    let template = format!(
        "\nexport async function {wrapper_name}({params_str}): Promise<{ts_ret}> {{\n  return await invoke('{cmd_name}'{sep}{args_obj_str});\n}}\n"
    );

    let target_uri = Uri::from_file_path(&candidate.path)?;
    let file_name = candidate.path.file_name()?.to_str()?;

    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Generate wrapper '{wrapper_name}' in {file_name}"),
        kind: Some(CodeActionKind::REFACTOR),
        diagnostics: Some(diagnostics.to_vec()),
        edit: Some(create_workspace_edit(
            target_uri,
            u32::MAX, // Append
            template,
        )),
        ..Default::default()
    }))
}
