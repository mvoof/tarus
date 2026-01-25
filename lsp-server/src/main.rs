#![warn(clippy::all, clippy::pedantic)]

use dashmap::DashMap;
use serde_json::json;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
use tokio::time::Duration;
use tower_lsp_server::{
    jsonrpc::Result, lsp_types::*, Client, LanguageServer, LspService, Server, UriExt,
};

mod indexer;
mod scanner;
mod syntax;
mod tree_parser;

use crate::indexer::{DiagnosticInfo, IndexKey, LocationInfo, ProjectIndex};
use scanner::{is_tauri_project, scan_workspace_files};
use std::sync::atomic::{AtomicBool, Ordering};
use syntax::{Behavior, EntityType};

/// Trigger function names for autocompletion
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

fn compute_file_diagnostics(path: &PathBuf, project_index: &ProjectIndex) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // If file has parse errors, skip diagnostic generation
    // (errors are logged in developer mode only, not shown to user)
    // TS/Rust analyzer already shows syntax errors
    if project_index.get_parse_error(path).is_some() {
        return diagnostics;
    }

    let keys: Vec<IndexKey> = match project_index.file_map.get(path) {
        Some(k) => k.value().clone(),
        None => return diagnostics,
    };

    for key in &keys {
        let info: DiagnosticInfo = project_index.get_diagnostic_info(key);
        let locations = project_index.get_locations(key.entity, &key.name);

        // Filter locations to only those in current file
        let local_locations: Vec<_> = locations.iter().filter(|l| l.path == *path).collect();

        // Find first occurrence of each behavior type
        let first_call = local_locations
            .iter()
            .find(|l| matches!(l.behavior, Behavior::Call))
            .map(|l| l.range);
        let first_emit = local_locations
            .iter()
            .find(|l| matches!(l.behavior, Behavior::Emit))
            .map(|l| l.range);

        for loc in local_locations {
            // Determine if we should show diagnostic for this location
            let msg = match loc.behavior {
                // Show on Definition if command never called
                Behavior::Definition if !info.has_calls => Some((
                    DiagnosticSeverity::WARNING,
                    format!(
                        "Command '{}' is defined but never invoked in frontend",
                        key.name
                    ),
                )),
                // Show on FIRST Call only if command not defined
                Behavior::Call if !info.has_definition => {
                    if first_call == Some(loc.range) {
                        Some((
                            DiagnosticSeverity::WARNING,
                            format!("Command '{}' is not defined in Rust backend", key.name),
                        ))
                    } else {
                        None // Skip subsequent calls
                    }
                }
                // Show on Listen if event never emitted
                Behavior::Listen if !info.has_emitters => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Event '{}' is listened for but never emitted", key.name),
                )),
                // Show on FIRST Emit only if event never listened
                Behavior::Emit if !info.has_listeners => {
                    if first_emit == Some(loc.range) {
                        Some((
                            DiagnosticSeverity::WARNING,
                            format!("Event '{}' is emitted but no listeners found", key.name),
                        ))
                    } else {
                        None // Skip subsequent emits
                    }
                }
                _ => None,
            };

            if let Some((severity, message)) = msg {
                diagnostics.push(Diagnostic {
                    range: loc.range,
                    severity: Some(severity),
                    source: Some("tarus".to_string()),
                    message,
                    ..Default::default()
                });
            }
        }
    }

    diagnostics
}

/// Supported file extensions for parsing
const SUPPORTED_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "vue", "svelte"];

fn is_supported_file(path: &PathBuf) -> bool {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    // Check for Angular component files (.component.ts)
    let is_angular = path
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.ends_with(".component.ts"))
        .unwrap_or(false);

    SUPPORTED_EXTENSIONS.contains(&ext) || is_angular
}

/// Process file content directly (for did_change - content from editor buffer)
fn process_file_content(path: &PathBuf, content: &str, project_index: &Arc<ProjectIndex>) -> bool {
    if !is_supported_file(path) {
        return false;
    }

    match tree_parser::parse(path, content) {
        Ok(file_index) => {
            // Parse succeeded - clear any previous errors and add file
            project_index.clear_parse_error(path);
            project_index.add_file(file_index);
            true
        }
        Err(parse_error) => {
            // Parse failed - store error and remove file from index
            project_index.set_parse_error(path.clone(), parse_error.to_string());
            project_index.remove_file(path);
            true // Still return true to indicate we processed it (with error)
        }
    }
}

/// Process file from disk (for initial scan and did_save)
async fn process_file_index(path: PathBuf, project_index: &Arc<ProjectIndex>) -> bool {
    if !is_supported_file(&path) {
        return false;
    }

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    match tree_parser::parse(&path, &content) {
        Ok(file_index) => {
            // Parse succeeded - clear any previous errors and add file
            project_index.clear_parse_error(&path);
            project_index.add_file(file_index);
            true
        }
        Err(parse_error) => {
            // Parse failed - store error and remove file from index
            project_index.set_parse_error(path.clone(), parse_error.to_string());
            project_index.remove_file(&path);
            true // Still return true to indicate we processed it (with error)
        }
    }
}

#[derive(Debug)]
struct Backend {
    client: Client,
    workspace_root: OnceCell<PathBuf>,
    project_index: Arc<ProjectIndex>,
    is_developer_mode_active: Arc<AtomicBool>,
    debounce_tasks: Arc<DashMap<PathBuf, tokio::task::JoinHandle<()>>>,
}

impl Backend {
    /// Helper: Checks if the server is fully initialized (workspace root set)
    fn is_ready(&self) -> bool {
        self.workspace_root.get().is_some()
    }

    async fn on_change(&self, path: PathBuf) {
        if !self.is_ready() {
            return;
        }

        if process_file_index(path.clone(), &self.project_index).await {
            let report = self.project_index.file_report(&path);
            self.log_dev_info(&report).await;
        }
    }

    async fn log_dev_info(&self, message: &str) {
        if self.is_developer_mode_active.load(Ordering::Relaxed) {
            self.client.log_message(MessageType::INFO, message).await;
        }
    }

    async fn publish_diagnostics_for_file(&self, path: &PathBuf) {
        let uri = match Uri::from_file_path(path) {
            Some(u) => u,
            None => return,
        };

        let diagnostics = compute_file_diagnostics(path, &self.project_index);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        let root_path = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .and_then(|folder| folder.uri.to_file_path())
            .map(|path_cow| path_cow.to_path_buf())
            .or_else(|| {
                #[allow(deprecated)]
                params
                    .root_uri
                    .and_then(|uri| uri.to_file_path().map(|path_cow| path_cow.to_path_buf()))
            });

        let mut is_tauri = false;

        if let Some(root) = root_path {
            if is_tauri_project(&root) {
                is_tauri = true;
                let _ = self.workspace_root.set(root.clone());

                self.client
                    .log_message(
                        MessageType::INFO,
                        "✅ Tauri project detected. Tree-sitter parser ready.",
                    )
                    .await;
            }
        }

        if !is_tauri {
            return Ok(InitializeResult {
                capabilities: ServerCapabilities::default(),
                server_info: None,
            });
        }

        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                code_lens_provider: Some(CodeLensOptions {
                    resolve_provider: Some(false),
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                workspace_symbol_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec!["\"".to_string(), "'".to_string()]),
                    resolve_provider: Some(false),
                    ..Default::default()
                }),
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),

                text_document_sync: Some(TextDocumentSyncCapability::Options(
                    TextDocumentSyncOptions {
                        save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                        open_close: Some(true),
                        change: Some(TextDocumentSyncKind::FULL),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        if !self.is_ready() {
            return;
        }

        let ext_config_request = ConfigurationParams {
            items: vec![ConfigurationItem {
                scope_uri: None,
                section: Some("tarus.developerMode".to_string()),
            }],
        };

        if let Ok(response) = self
            .client
            .configuration(vec![ext_config_request.items[0].clone()])
            .await
        {
            if let Some(settings) = response.into_iter().next() {
                if let Some(is_enabled) = settings.as_bool() {
                    self.is_developer_mode_active
                        .store(is_enabled, Ordering::Relaxed);

                    self.client
                        .log_message(
                            MessageType::INFO,
                            &format!("Developer Mode initialized to: {}", is_enabled),
                        )
                        .await;
                }
            }
        }

        let root = match self.workspace_root.get() {
            Some(r) => r,
            None => return,
        };

        let root_clone = root.clone();
        let project_index_clone = self.project_index.clone();
        let client_clone = self.client.clone();

        let is_dev_mode_clone = self.is_developer_mode_active.clone();

        tokio::spawn(async move {
            client_clone
                .log_message(MessageType::INFO, "🚀 Starting background indexing...")
                .await;

            let files = tokio::task::spawn_blocking(move || scan_workspace_files(&root_clone))
                .await
                .unwrap_or_default();

            for path in files {
                let _ = process_file_index(path, &project_index_clone).await;
            }

            // Publish diagnostics for all indexed files
            for entry in project_index_clone.file_map.iter() {
                let path = entry.key().clone();
                if let Some(uri) = Uri::from_file_path(&path) {
                    let diagnostics = compute_file_diagnostics(&path, &project_index_clone);
                    client_clone
                        .publish_diagnostics(uri, diagnostics, None)
                        .await;
                }
            }

            // Report about the indexing process
            let report = project_index_clone.technical_report();
            if is_dev_mode_clone.load(Ordering::Relaxed) {
                client_clone.log_message(MessageType::INFO, report).await;
            }

            client_clone
                .log_message(MessageType::INFO, format!("🏁 Indexing complete"))
                .await;
        });
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        self.log_dev_info(&format!(
            "➡️ Request: Definition at {:?} line: {}, char: {}",
            uri, position.line, position.character
        ))
        .await;

        let Some(path_cow) = uri.to_file_path() else {
            self.client
                .log_message(MessageType::ERROR, "❌ Failed to convert URI to path")
                .await;
            return Ok(None);
        };

        let path: PathBuf = path_cow.to_path_buf();

        if let Some((key, origin_loc)) = self.project_index.get_key_at_position(&path, position) {
            self.log_dev_info(&format!(
                "✅ Found key under cursor: {:?} '{}' (Type: {:?})",
                key.entity, key.name, origin_loc.behavior
            ))
            .await;

            let all_refs = self.project_index.get_locations(key.entity, &key.name);

            let targets: Vec<&LocationInfo> = all_refs
                .iter()
                .filter(|target| {
                    // Exclude the current file
                    if target.range == origin_loc.range && target.path == origin_loc.path {
                        return false;
                    }

                    match origin_loc.behavior {
                        // If on Definition (Rust) -> Look for Call (JS)
                        Behavior::Definition => target.behavior == Behavior::Call,
                        // If on Call (JS) -> Search for Definition (Rust)
                        Behavior::Call => target.behavior == Behavior::Definition,

                        // If on Emit -> Search for Listen
                        Behavior::Emit => target.behavior == Behavior::Listen,
                        // If on Listen -> Search for Emit
                        Behavior::Listen => target.behavior == Behavior::Emit,
                    }
                })
                .collect();

            self.log_dev_info(&format!(
                "🔎 Found {} targets (smart portal)",
                targets.len()
            ))
            .await;

            if targets.is_empty() {
                return Ok(None);
            }

            let links: Vec<LocationLink> = targets
                .into_iter()
                .filter_map(|target| {
                    let target_uri = Uri::from_file_path(&target.path)?;

                    Some(LocationLink {
                        origin_selection_range: Some(origin_loc.range),
                        target_uri,
                        target_range: target.range,
                        target_selection_range: target.range,
                    })
                })
                .collect();

            return Ok(Some(GotoDefinitionResponse::Link(links)));
        } else {
            self.log_dev_info("⚠️ No key found at this position. Check ranges!")
                .await;

            if let Some(keys) = self.project_index.file_map.get(&path) {
                self.log_dev_info(&format!("ℹ️ Keys in this file: {:?}", keys.len()))
                    .await;
            }
        }

        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        let Some(path_cow) = uri.to_file_path() else {
            return Ok(None);
        };

        let path: PathBuf = path_cow.to_path_buf();

        // Find the key under the cursor
        if let Some((key, _)) = self.project_index.get_key_at_position(&path, position) {
            self.log_dev_info(&format!("🔎 Finding references for: {:?}", key))
                .await;

            let refs = self.project_index.get_locations(key.entity, &key.name);

            let locations: Vec<Location> = refs
                .iter()
                .filter_map(|r| {
                    let uri = Uri::from_file_path(&r.path)?;
                    Some(Location {
                        uri,
                        range: r.range,
                    })
                })
                .collect();

            return Ok(Some(locations));
        }

        Ok(None)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document.uri;

        let Some(path_cow) = uri.to_file_path() else {
            return Ok(None);
        };

        let path: PathBuf = path_cow.to_path_buf();

        let lens_data = self.project_index.get_lens_data(&path);

        if lens_data.is_empty() {
            return Ok(None);
        }

        let mut lenses = Vec::new();

        for (range, title, targets) in lens_data {
            // Converting LocationInfo to LSP Location
            let lsp_locations: Vec<Location> = targets
                .iter()
                .filter_map(|t| {
                    let target_uri = Uri::from_file_path(&t.path)?;
                    Some(Location {
                        uri: target_uri,
                        range: t.range,
                    })
                })
                .collect();

            if lsp_locations.is_empty() {
                continue;
            }

            // Generate a click command
            // editor.action.showReferences takes 3 arguments:
            // [Uri (where to open), Position (where the arrow points), List<Location> (results)]
            let arguments = vec![
                json!(uri.to_string()), // pass the URI as a string!
                json!(range.start),
                json!(lsp_locations),
            ];

            let command = Command {
                title,
                command: "tarus.show_references".to_string(),
                arguments: Some(arguments),
            };

            lenses.push(CodeLens {
                range,
                command: Some(command),
                data: None,
            });
        }

        Ok(Some(lenses))
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(path_cow) = uri.to_file_path() else {
            return Ok(None);
        };

        let path: PathBuf = path_cow.to_path_buf();

        if let Some((key, origin_loc)) = self.project_index.get_key_at_position(&path, position) {
            let locations = self.project_index.get_locations(key.entity, &key.name);

            if locations.is_empty() {
                return Ok(None);
            }

            // Get diagnostic info for warnings
            let info = self.project_index.get_diagnostic_info(&key);

            // Count by behavior type
            let calls_count = locations
                .iter()
                .filter(|l| matches!(l.behavior, Behavior::Call))
                .count();
            let emits_count = locations
                .iter()
                .filter(|l| matches!(l.behavior, Behavior::Emit))
                .count();
            let listens_count = locations
                .iter()
                .filter(|l| matches!(l.behavior, Behavior::Listen))
                .count();
            let definitions_count = locations
                .iter()
                .filter(|l| matches!(l.behavior, Behavior::Definition))
                .count();

            let (definitions, references): (Vec<&LocationInfo>, Vec<&LocationInfo>) =
                locations.iter().partition(|l| match key.entity {
                    EntityType::Command => l.behavior == Behavior::Definition,
                    EntityType::Event => l.behavior == Behavior::Listen,
                });

            // Create Markdown Text
            let mut md_text = String::new();

            // Header with emoji
            let icon = match key.entity {
                EntityType::Command => "⚙️",
                EntityType::Event => "📡",
            };

            md_text.push_str(&format!(
                "### {} {:?}: `{}`\n\n",
                icon, key.entity, key.name
            ));

            // Definitions Section
            if !definitions.is_empty() {
                md_text.push_str("**Definition:**\n");

                for def in &definitions {
                    let file_icon = if def.path.extension().map_or(false, |e| e == "rs") {
                        "🦀"
                    } else {
                        "⚡️"
                    };

                    let filename = def.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

                    md_text.push_str(&format!(
                        "- {} `{}:{}`\n",
                        file_icon,
                        filename,
                        def.range.start.line + 1
                    ));
                }

                md_text.push_str("\n");
            }

            // Reference count breakdown
            let total_refs = locations.len();
            md_text.push_str(&format!("**References ({} total)**\n", total_refs));

            if key.entity == EntityType::Command {
                if definitions_count > 0 {
                    md_text.push_str(&format!("- 🦀 {} definition(s)\n", definitions_count));
                }
                if calls_count > 0 {
                    md_text.push_str(&format!("- ⚡ {} call(s)\n", calls_count));
                }
            } else {
                if emits_count > 0 {
                    md_text.push_str(&format!("- 📤 {} emit(s)\n", emits_count));
                }
                if listens_count > 0 {
                    md_text.push_str(&format!("- 👂 {} listener(s)\n", listens_count));
                }
            }

            md_text.push_str("\n");

            // Sample references (first 5)
            if !references.is_empty() {
                md_text.push_str("**Sample References:**\n");
                for (i, rf) in references.iter().enumerate() {
                    if i >= 5 {
                        md_text.push_str(&format!("- *...and {} more*\n", references.len() - 5));
                        break;
                    }

                    let file_icon = if rf.path.extension().map_or(false, |e| e == "rs") {
                        "🦀"
                    } else {
                        "⚡️"
                    };

                    let filename = rf.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
                    let behavior_badge = format!("{:?}", rf.behavior).to_uppercase();

                    md_text.push_str(&format!(
                        "- {} `[{}] {}:{}`\n",
                        file_icon,
                        behavior_badge,
                        filename,
                        rf.range.start.line + 1
                    ));
                }

                md_text.push_str("\n");
            }

            // Add warnings/tips based on diagnostic info
            if key.entity == EntityType::Command && !info.has_definition {
                md_text.push_str("⚠️ *No backend implementation found*\n");
            } else if key.entity == EntityType::Command && !info.has_calls {
                md_text.push_str("💡 *Defined but never called in frontend*\n");
            } else if key.entity == EntityType::Event && !info.has_emitters {
                md_text.push_str("💡 *Event listened for but never emitted*\n");
            } else if key.entity == EntityType::Event && !info.has_listeners {
                md_text.push_str("💡 *Event emitted but no listeners found*\n");
            }

            return Ok(Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: md_text,
                }),

                range: Some(origin_loc.range),
            }));
        }

        Ok(None)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let path = match params.text_document.uri.to_file_path() {
            Some(p) => p.to_path_buf(),
            None => return Ok(None),
        };

        let position = params.range.start;

        // Check if cursor is on an undefined command
        if let Some((key, _loc)) = self.project_index.get_key_at_position(&path, position) {
            // Only offer action for commands (not events)
            if key.entity != EntityType::Command {
                return Ok(None);
            }

            let info = self.project_index.get_diagnostic_info(&key);

            // Only offer action for undefined commands
            if info.has_definition {
                return Ok(None);
            }

            // Find src-tauri/src/main.rs
            let workspace_root = match self.workspace_root.get() {
                Some(root) => root,
                None => return Ok(None),
            };

            let target_file = workspace_root.join("src-tauri").join("src").join("main.rs");

            if !target_file.exists() {
                return Ok(None);
            }

            // Read target file to find insertion point
            let content = match tokio::fs::read_to_string(&target_file).await {
                Ok(c) => c,
                Err(_) => return Ok(None),
            };

            let lines: Vec<&str> = content.lines().collect();

            // Find line before .invoke_handler() to insert command
            let mut insert_line = 0;
            for (i, line) in lines.iter().enumerate() {
                if line.contains(".invoke_handler") {
                    insert_line = i;
                    break;
                }
            }

            if insert_line == 0 {
                // Fallback: insert before fn main()
                for (i, line) in lines.iter().enumerate() {
                    if line.contains("fn main()") {
                        insert_line = i;
                        break;
                    }
                }
            }

            // Generate command template
            let command_template = format!(
                "\n#[tauri::command]\nfn {}() -> Result<String, String> {{\n    Ok(\"Not implemented\".to_string())\n}}\n",
                key.name
            );

            // Create WorkspaceEdit
            let target_uri = match Uri::from_file_path(&target_file) {
                Some(u) => u,
                None => return Ok(None),
            };

            let mut changes = std::collections::HashMap::new();
            changes.insert(
                target_uri,
                vec![TextEdit {
                    range: Range {
                        start: Position {
                            line: insert_line as u32,
                            character: 0,
                        },
                        end: Position {
                            line: insert_line as u32,
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

            // Create CodeAction
            let action = CodeAction {
                title: format!("Create Rust command '{}'", key.name),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(params.context.diagnostics),
                edit: Some(workspace_edit),
                ..Default::default()
            };

            return Ok(Some(vec![CodeActionOrCommand::CodeAction(action)]));
        }

        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = params.text_document.uri;

        let Some(path_cow) = uri.to_file_path() else {
            return Ok(None);
        };

        let path: PathBuf = path_cow.to_path_buf();
        let symbols = self.project_index.get_document_symbols(&path);

        if symbols.is_empty() {
            return Ok(None);
        }

        Ok(Some(DocumentSymbolResponse::Flat(symbols)))
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<OneOf<Vec<SymbolInformation>, Vec<WorkspaceSymbol>>>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let symbols = self.project_index.search_workspace_symbols(&params.query);

        if symbols.is_empty() {
            return Ok(None);
        }

        Ok(Some(OneOf::Left(symbols)))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        if !self.is_ready() {
            return Ok(None);
        }

        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        // Get file path
        let Some(path_cow) = uri.to_file_path() else {
            return Ok(None);
        };
        let path = path_cow.to_path_buf();

        // Read file content
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Ok(None),
        };

        // Get text before cursor on current line
        let lines: Vec<&str> = content.lines().collect();
        let line_idx = position.line as usize;
        if line_idx >= lines.len() {
            return Ok(None);
        }

        let line = lines[line_idx];
        let col = position.character as usize;
        let prefix = if col <= line.len() {
            &line[..col]
        } else {
            line
        };

        // Check if prefix contains any of the trigger patterns (e.g. "invoke(", "emit(", "invoke<T>(")
        let in_context = COMPLETION_TRIGGERS.iter().any(|name| {
            // Match simple call: invoke("
            if prefix.contains(&format!("{}(", name)) {
                return true;
            }
            // Match generic call: invoke<T>(" - check for name followed by < and then (
            if let Some(idx) = prefix.find(name) {
                let after_name = &prefix[idx + name.len()..];
                // Pattern: <...>(
                if after_name.starts_with('<') {
                    if let Some(close_idx) = after_name.find('>') {
                        let after_generic = &after_name[close_idx + 1..];
                        if after_generic.starts_with('(') {
                            return true;
                        }
                    }
                }
            }
            false
        });

        if !in_context {
            return Ok(None);
        }

        // Collect all known commands and events for completion
        let mut items = Vec::new();

        // Add commands
        for (name, def_loc) in self.project_index.get_all_names(EntityType::Command) {
            let detail = def_loc.as_ref().map(|l| {
                let filename = l
                    .path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown");
                format!("Command defined in {}", filename)
            });

            items.push(CompletionItem {
                label: name,
                kind: Some(CompletionItemKind::FUNCTION),
                detail,
                ..Default::default()
            });
        }

        // Add events
        for (name, _) in self.project_index.get_all_names(EntityType::Event) {
            items.push(CompletionItem {
                label: name,
                kind: Some(CompletionItemKind::EVENT),
                detail: Some("Event".to_string()),
                ..Default::default()
            });
        }

        if items.is_empty() {
            return Ok(None);
        }

        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path_cow) = params.text_document.uri.to_file_path() {
            let path: PathBuf = path_cow.into_owned();
            let content = &params.text_document.text;

            if process_file_content(&path, content, &self.project_index) {
                let report = self.project_index.file_report(&path);
                self.log_dev_info(&report).await;
            }

            self.publish_diagnostics_for_file(&path).await;
        }
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path_cow) = params.text_document.uri.to_file_path() {
            let path: PathBuf = path_cow.into_owned();

            // With TextDocumentSyncKind::FULL, content_changes[0].text contains the full document
            if let Some(change) = params.content_changes.into_iter().next() {
                let content = change.text;

                // Cancel existing debounce task for this file
                if let Some((_key, old_task)) = self.debounce_tasks.remove(&path) {
                    old_task.abort();
                }

                // Spawn new debounced task
                let project_index = self.project_index.clone();
                let client = self.client.clone();
                let path_clone = path.clone();
                let is_dev_mode = self.is_developer_mode_active.clone();

                let task = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(300)).await;

                    // Get OLD keys before processing (will be removed)
                    let old_keys: Vec<IndexKey> = project_index
                        .file_map
                        .get(&path_clone)
                        .map(|keys| keys.value().clone())
                        .unwrap_or_default();

                    if process_file_content(&path_clone, &content, &project_index) {
                        // Log parse errors in developer mode (check AFTER processing)
                        if is_dev_mode.load(Ordering::Relaxed) {
                            if let Some(error_msg) = project_index.get_parse_error(&path_clone) {
                                let filename = path_clone
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown");
                                client
                                    .log_message(
                                        MessageType::ERROR,
                                        format!("Parse error in {}: {}", filename, error_msg),
                                    )
                                    .await;
                            }
                        }
                        // Get NEW keys after processing
                        let new_keys: Vec<IndexKey> = project_index
                            .file_map
                            .get(&path_clone)
                            .map(|keys| keys.value().clone())
                            .unwrap_or_default();

                        // Combine old and new keys to find all affected commands/events
                        let mut all_keys = HashSet::new();
                        for key in old_keys.iter().chain(new_keys.iter()) {
                            all_keys.insert(key.clone());
                        }

                        // Collect all files that contain these commands/events
                        let mut affected_files = HashSet::new();
                        affected_files.insert(path_clone.clone());

                        for key in &all_keys {
                            if let Some(locations) = project_index.map.get(key) {
                                for loc in locations.iter() {
                                    affected_files.insert(loc.path.clone());
                                }
                            }
                        }

                        // Publish diagnostics for all affected files
                        for file in affected_files {
                            if let Some(uri) = Uri::from_file_path(&file) {
                                let diagnostics = compute_file_diagnostics(&file, &project_index);
                                client.publish_diagnostics(uri, diagnostics, None).await;
                            }
                        }
                    }
                });

                self.debounce_tasks.insert(path, task);
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path_cow) = params.text_document.uri.to_file_path() {
            let path: PathBuf = path_cow.into_owned();
            self.on_change(path.clone()).await;
            self.publish_diagnostics_for_file(&path).await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let project_index = Arc::new(ProjectIndex::new());
    let initial_dev_mode_state = Arc::new(AtomicBool::new(false));

    let (service, socket) = LspService::new(|client| Backend {
        client,
        workspace_root: OnceCell::new(),
        project_index,
        is_developer_mode_active: initial_dev_mode_state.clone(),
        debounce_tasks: Arc::new(DashMap::new()),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
