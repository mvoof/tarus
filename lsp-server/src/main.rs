#![warn(clippy::all, clippy::pedantic)]

use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;
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

    let keys: Vec<IndexKey> = match project_index.file_map.get(path) {
        Some(k) => k.value().clone(),
        None => return diagnostics,
    };

    for key in &keys {
        let info: DiagnosticInfo = project_index.get_diagnostic_info(key);
        let locations = project_index.get_locations(key.entity, &key.name);

        for loc in locations.iter().filter(|l| l.path == *path) {
            let msg = match loc.behavior {
                Behavior::Call if !info.has_definition => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Command '{}' is not defined in Rust backend", key.name),
                )),
                Behavior::Definition if !info.has_calls => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Command '{}' is defined but never invoked", key.name),
                )),
                Behavior::Listen if !info.has_emitters => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Event '{}' is listened for but never emitted", key.name),
                )),
                Behavior::Emit if !info.has_listeners => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Event '{}' is emitted but never listened to", key.name),
                )),
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

    let file_index = tree_parser::parse(path, content);
    project_index.add_file(file_index);

    true
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

    let file_index = tree_parser::parse(&path, &content);
    project_index.add_file(file_index);

    true
}

#[derive(Debug)]
struct Backend {
    client: Client,
    workspace_root: OnceCell<PathBuf>,
    project_index: Arc<ProjectIndex>,
    is_developer_mode_active: Arc<AtomicBool>,
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

            let (definitions, references): (Vec<&LocationInfo>, Vec<&LocationInfo>) =
                locations.iter().partition(|l| match key.entity {
                    EntityType::Command => l.behavior == Behavior::Definition,
                    EntityType::Event => l.behavior == Behavior::Listen,
                });

            // Create Markdown Text
            let mut md_text = String::new();

            // Header
            let icon = match key.entity {
                EntityType::Command => "🔧",
                EntityType::Event => "📡",
            };

            md_text.push_str(&format!(
                "### {} {:?}: `{}`\n\n",
                icon, key.entity, key.name
            ));

            // Definitions Section
            if !definitions.is_empty() {
                md_text.push_str("Definitions:\n");

                for def in definitions {
                    let file_icon = if def.path.extension().map_or(false, |e| e == "rs") {
                        "🦀"
                    } else {
                        "⚡️"
                    };

                    let filename = def.path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

                    md_text.push_str(&format!(
                        "* {} `{} : {}`\n",
                        file_icon,
                        filename,
                        def.range.start.line + 1
                    ));
                }

                md_text.push_str("\n");
            }

            // Links Section
            if !references.is_empty() {
                md_text.push_str(&format!("**References ({})**:\n", references.len()));
                for (i, rf) in references.iter().enumerate() {
                    if i >= 5 {
                        md_text.push_str(&format!("* *...and {} more*\n", references.len() - 5));
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
                        "* {} `[{}] {} : {}`\n",
                        file_icon,
                        behavior_badge,
                        filename,
                        rf.range.start.line + 1
                    ));
                }
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
                if process_file_content(&path, &change.text, &self.project_index) {
                    self.publish_diagnostics_for_file(&path).await;
                }
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
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
