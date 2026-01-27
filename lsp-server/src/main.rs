#![warn(clippy::all, clippy::pedantic)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::mutable_key_type)]
#![allow(clippy::ptr_arg)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::type_complexity)]
#![allow(clippy::struct_excessive_bools)]
#![allow(clippy::too_many_lines)]
#![allow(clippy::enum_variant_names)]
#![allow(clippy::question_mark)]

use dashmap::DashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::OnceCell;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{MessageType, Uri, InitializeParams, InitializeResult, ServerCapabilities, InitializedParams, ConfigurationParams, ConfigurationItem, GotoDefinitionParams, GotoDefinitionResponse, ReferenceParams, Location, CodeLensParams, CodeLens, HoverParams, Hover, CodeActionParams, CodeActionResponse, DocumentSymbolParams, DocumentSymbolResponse, WorkspaceSymbolParams, OneOf, SymbolInformation, WorkspaceSymbol, CompletionParams, CompletionResponse, DidOpenTextDocumentParams, DidChangeTextDocumentParams, DidSaveTextDocumentParams};
use tower_lsp_server::{Client, LanguageServer, LspService, Server, UriExt};

// Refactored modules
mod capabilities;
mod file_processor;
mod indexer;
mod scanner;
mod syntax;
mod tree_parser;

use capabilities::{build_server_capabilities, diagnostics};
use indexer::{IndexKey, ProjectIndex};
use scanner::{is_tauri_project, scan_workspace_files};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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

        if file_processor::process_file_index(path.clone(), &self.project_index) {
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

        let diagnostics = diagnostics::compute_file_diagnostics(path, &self.project_index);
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
            capabilities: build_server_capabilities(),
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
                            &format!("Developer Mode initialized to: {is_enabled}"),
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
                file_processor::process_file_index(path, &project_index_clone);
            }

            // Publish diagnostics for all indexed files
            for entry in &project_index_clone.file_map {
                let path = entry.key().clone();
                if let Some(uri) = Uri::from_file_path(&path) {
                    let diagnostics =
                        diagnostics::compute_file_diagnostics(&path, &project_index_clone);
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
                .log_message(MessageType::INFO, "🏁 Indexing complete".to_string())
                .await;
        });
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        self.log_dev_info(&format!(
            "➡️ Request: Definition at {:?} line: {}, char: {}",
            uri, position.line, position.character
        ))
        .await;

        let result = capabilities::definition::handle_goto_definition(params, &self.project_index);

        if let Some(GotoDefinitionResponse::Link(ref links)) = result {
            self.log_dev_info(&format!("✅ Found {} definition links", links.len()))
                .await;
        } else {
            self.log_dev_info("⚠️ No definitions found").await;
        }

        Ok(result)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = &params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        self.log_dev_info(&format!(
            "➡️ Request: References at {:?} line: {}, char: {}",
            uri, position.line, position.character
        ))
        .await;

        let result = capabilities::references::handle_references(params, &self.project_index);

        if let Some(ref locations) = result {
            self.log_dev_info(&format!("✅ Found {} references", locations.len()))
                .await;
        } else {
            self.log_dev_info("⚠️ No references found").await;
        }

        Ok(result)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;

        self.log_dev_info(&format!("➡️ Request: CodeLens for {uri:?}"))
            .await;

        let result = capabilities::code_lens::handle_code_lens(params, &self.project_index);

        if let Some(ref lenses) = result {
            self.log_dev_info(&format!("✅ Generated {} code lenses", lenses.len()))
                .await;
        } else {
            self.log_dev_info("⚠️ No code lenses found").await;
        }

        Ok(result)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        self.log_dev_info(&format!(
            "➡️ Request: Hover at {:?} line: {}, char: {}",
            uri, position.line, position.character
        ))
        .await;

        let result = capabilities::hover::handle_hover(params, &self.project_index);

        if result.is_some() {
            self.log_dev_info("✅ Generated hover tooltip").await;
        } else {
            self.log_dev_info("⚠️ No hover info available").await;
        }

        Ok(result)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = &params.text_document.uri;
        let position = params.range.start;

        self.log_dev_info(&format!(
            "➡️ Request: CodeAction at {:?} line: {}, char: {}",
            uri, position.line, position.character
        ))
        .await;

        let workspace_root = self.workspace_root.get().cloned();
        let result = capabilities::code_actions::handle_code_action(
            &params,
            &self.project_index,
            workspace_root.as_ref(),
        );

        if let Some(ref actions) = result {
            self.log_dev_info(&format!("✅ Generated {} code actions", actions.len()))
                .await;
        } else {
            self.log_dev_info("⚠️ No code actions available").await;
        }

        Ok(result)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = &params.text_document.uri;

        self.log_dev_info(&format!("➡️ Request: DocumentSymbol for {uri:?}"))
            .await;

        let result = capabilities::symbols::handle_document_symbol(params, &self.project_index);

        if let Some(ref response) = result {
            match response {
                DocumentSymbolResponse::Flat(syms) => {
                    self.log_dev_info(&format!("✅ Found {} document symbols", syms.len()))
                        .await;
                }
                DocumentSymbolResponse::Nested(syms) => {
                    self.log_dev_info(&format!("✅ Found {} nested document symbols", syms.len()))
                        .await;
                }
            }
        } else {
            self.log_dev_info("⚠️ No document symbols found").await;
        }

        Ok(result)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<OneOf<Vec<SymbolInformation>, Vec<WorkspaceSymbol>>>> {
        self.log_dev_info(&format!(
            "➡️ Request: WorkspaceSymbol query: '{}'",
            params.query
        ))
        .await;

        let result = capabilities::symbols::handle_workspace_symbol(&params, &self.project_index);

        if let Some(ref response) = result {
            match response {
                OneOf::Left(syms) => {
                    self.log_dev_info(&format!("✅ Found {} workspace symbols", syms.len()))
                        .await;
                }
                OneOf::Right(syms) => {
                    self.log_dev_info(&format!("✅ Found {} workspace symbols", syms.len()))
                        .await;
                }
            }
        } else {
            self.log_dev_info("⚠️ No workspace symbols found").await;
        }

        Ok(result)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.log_dev_info("➡️ Request: Completion").await;

        let result = capabilities::completion::handle_completion(&params, &self.project_index);

        if let Some(ref response) = result {
            match response {
                CompletionResponse::Array(items) => {
                    self.log_dev_info(&format!("✅ Generated {} completion items", items.len()))
                        .await;
                }
                CompletionResponse::List(list) => {
                    self.log_dev_info(&format!(
                        "✅ Generated {} completion items",
                        list.items.len()
                    ))
                    .await;
                }
            }
        } else {
            self.log_dev_info("⚠️ No completions available").await;
        }

        Ok(result)
    }

    // =============================================================================
    // Text Document Synchronization
    // =============================================================================
    // NOTE: These handlers are kept inline (not extracted to a module) because
    // they require complex orchestration with Backend's state (client,
    // debounce_tasks, developer_mode) and cross-file diagnostic propagation.
    // Extracting them would duplicate Backend's responsibilities.

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path_cow) = params.text_document.uri.to_file_path() {
            let path: PathBuf = path_cow.into_owned();
            let content = &params.text_document.text;

            if file_processor::process_file_content(&path, content, &self.project_index) {
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

                    if file_processor::process_file_content(&path_clone, &content, &project_index) {
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
                                        format!("Parse error in {filename}: {error_msg}"),
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
                                let diagnostics =
                                    diagnostics::compute_file_diagnostics(&file, &project_index);
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
