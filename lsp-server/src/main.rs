#![warn(clippy::all, clippy::pedantic)]

use dashmap::DashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::OnceCell;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::ls_types::{
    CodeActionParams, CodeActionResponse, CodeLens, CodeLensParams, CompletionParams,
    CompletionResponse, DidChangeTextDocumentParams, DidOpenTextDocumentParams,
    DidSaveTextDocumentParams, DocumentSymbolParams, DocumentSymbolResponse, ExecuteCommandParams,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams, InitializeParams,
    InitializeResult, InitializedParams, Location, MessageType, ReferenceParams,
    ServerCapabilities, Uri, WorkspaceSymbolParams, WorkspaceSymbolResponse,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server};

// Refactored modules
mod bindings_reader;
mod capabilities;
mod config;
mod file_processor;
mod indexer;
mod initialization;
mod scanner;
mod syntax;
mod tree_parser;
mod typegen;

use bindings_reader::BindingsConfig;
use capabilities::{build_server_capabilities, diagnostics};
use indexer::{IndexKey, ProjectIndex};
use scanner::is_tauri_project;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use typegen::TypegenConfig;

#[derive(Debug)]
struct Backend {
    client: Client,
    workspace_roots: OnceCell<Vec<PathBuf>>,
    project_index: Arc<ProjectIndex>,
    is_developer_mode_active: Arc<AtomicBool>,
    debounce_tasks: Arc<DashMap<PathBuf, tokio::task::JoinHandle<()>>>,
    /// Cache of open document contents for completion and other features
    document_cache: Arc<DashMap<PathBuf, String>>,
    /// Type generation configuration (loaded from client settings)
    typegen_config: Arc<tokio::sync::RwLock<TypegenConfig>>,
    /// Bindings configuration (loaded from client settings)
    bindings_config: Arc<tokio::sync::RwLock<BindingsConfig>>,
}

impl Backend {
    /// Helper: Checks if the server is fully initialized (workspace root set)
    fn is_ready(&self) -> bool {
        self.workspace_roots.get().is_some()
    }

    async fn on_change(&self, path: PathBuf) {
        if !self.is_ready() {
            return;
        }

        let is_rust_file = path.extension().is_some_and(|ext| ext == "rs");

        if file_processor::process_file_index(path.clone(), &self.project_index) {
            let report = self.project_index.file_report(&path);
            self.log_dev_info(&report).await;

            // Regenerate type definitions when Rust files change
            if is_rust_file {
                if let Some(roots) = self.workspace_roots.get() {
                    // Use the first root (usually the primary one) to store generated types
                    if let Some(primary_root) = roots.first() {
                        let config = self.typegen_config.read().await.clone();
                        if let Err(e) = typegen::write_types_file_with_config(
                            &self.project_index,
                            primary_root,
                            &config,
                        ) {
                            self.log_dev_info(&format!("Failed to regenerate types: {e}"))
                                .await;
                        }
                    }
                }
            }
        }
    }

    async fn log_dev_info(&self, message: &str) {
        if self.is_developer_mode_active.load(Ordering::Relaxed) {
            self.client.log_message(MessageType::INFO, message).await;
        }
    }

    async fn publish_diagnostics_for_file(&self, path: &PathBuf) {
        let Some(uri) = Uri::from_file_path(path) else {
            return;
        };

        let diagnostics = diagnostics::compute_file_diagnostics(path, &self.project_index);
        self.client
            .publish_diagnostics(uri, diagnostics, None)
            .await;
    }
}

impl Backend {
    /// Spawn a debounced file processing task
    fn spawn_debounced_processing(
        &self,
        path: PathBuf,
        content: String,
    ) -> tokio::task::JoinHandle<()> {
        let project_index = self.project_index.clone();
        let client = self.client.clone();
        let is_dev_mode = self.is_developer_mode_active.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;

            // Get OLD keys before processing (will be removed)
            let old_keys: Vec<IndexKey> = project_index
                .file_map
                .get(&path)
                .map(|keys| keys.value().clone())
                .unwrap_or_default();

            if file_processor::process_file_content(&path, &content, &project_index) {
                // Log parse errors in developer mode (check AFTER processing)
                if is_dev_mode.load(Ordering::Relaxed) {
                    if let Some(error_msg) = project_index.get_parse_error(&path) {
                        let filename = path
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
                    .get(&path)
                    .map(|keys| keys.value().clone())
                    .unwrap_or_default();

                // Combine old and new keys to find all affected commands/events
                let mut all_keys = HashSet::new();
                for key in old_keys.iter().chain(new_keys.iter()) {
                    all_keys.insert(key.clone());
                }

                // Collect all files that contain these commands/events
                let mut affected_files = HashSet::new();
                affected_files.insert(path.clone());

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
        })
    }

    /// Load and apply configuration settings from the client
    async fn load_configuration(&self) {
        config::load_configuration(
            &self.client,
            &self.is_developer_mode_active,
            &self.project_index,
            &self.typegen_config,
            &self.bindings_config,
        )
        .await;

        self.reload_bindings().await;
    }

    /// Reload bindings based on configuration and discovery
    async fn reload_bindings(&self) {
        let roots = self.workspace_roots.get();
        let Some(root) = roots.and_then(|r| r.first()) else {
            return;
        };

        let config = self.bindings_config.read().await;
        if !config.type_safety_enabled {
            return;
        }

        // Determine path to bindings
        let bindings_path = if let Some(paths) = &config.type_bindings_paths {
            paths.first().map(PathBuf::from)
        } else {
            bindings_reader::find_bindings_file(root)
        };

        if let Some(path) = bindings_path {
            self.log_dev_info(&format!("Loading bindings from {}", path.display()))
                .await;
            if let Err(e) = bindings_reader::read_bindings(&path, &self.project_index) {
                self.log_dev_info(&format!("Failed to read bindings: {e}"))
                    .await;
            } else {
                let count = self.project_index.bindings_cache.len();
                self.log_dev_info(&format!("Loaded {count} bindings")).await;
            }
        } else {
            self.log_dev_info("No bindings file found").await;
        }
    }

    /// Spawn background indexing task for all roots
    fn spawn_background_indexing(&self, roots: &[PathBuf]) {
        initialization::spawn_background_indexing(
            roots,
            self.project_index.clone(),
            self.client.clone(),
            self.is_developer_mode_active.clone(),
        );
    }
}

/// Helper: Extract path and content from document change params
fn extract_change_params(params: DidChangeTextDocumentParams) -> Option<(PathBuf, String)> {
    let path = params.text_document.uri.to_file_path()?.into_owned();
    // With TextDocumentSyncKind::FULL, content_changes[0].text contains the full document
    let content = params.content_changes.into_iter().next()?.text;
    Some((path, content))
}

impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Collect all potential workspace roots
        let mut roots = Vec::new();

        if let Some(folders) = &params.workspace_folders {
            for folder in folders {
                if let Some(path) = folder.uri.to_file_path().map(|p| p.to_path_buf()) {
                    roots.push(path);
                }
            }
        }

        // Fallback to root_uri
        if roots.is_empty() {
            // root_uri is deprecated, but we still support it for backward compatibility
            #[allow(deprecated)]
            if let Some(path) = params
                .root_uri
                .as_ref()
                .and_then(|uri| uri.to_file_path().map(std::borrow::Cow::into_owned))
            {
                roots.push(path);
            }
        }

        // Check if ANY root contains a Tauri project
        let is_tauri = roots.iter().any(|r| is_tauri_project(r));

        if is_tauri {
            let _ = self.workspace_roots.set(roots);

            self.client
                .log_message(
                    MessageType::INFO,
                    "✅ Tauri project detected. Indexing workspace...",
                )
                .await;
        } else {
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

        // Load configuration settings
        self.load_configuration().await;

        // Start background indexing
        let Some(roots) = self.workspace_roots.get() else {
            return;
        };
        self.spawn_background_indexing(roots);
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = &params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        let Some(path) = uri.to_file_path() else {
            return Ok(None);
        };

        if scanner::is_ignored(&path) {
            return Ok(None);
        }

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

        let Some(path) = uri.to_file_path() else {
            return Ok(None);
        };

        if scanner::is_ignored(&path) {
            return Ok(None);
        }

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
        let Some(path) = uri.to_file_path() else {
            return Ok(None);
        };

        if scanner::is_ignored(&path) {
            return Ok(None);
        }

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

        let Some(path) = uri.to_file_path() else {
            return Ok(None);
        };

        if scanner::is_ignored(&path) {
            return Ok(None);
        }

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

        let Some(path) = uri.to_file_path() else {
            return Ok(None);
        };

        if scanner::is_ignored(&path) {
            return Ok(None);
        }

        self.log_dev_info(&format!(
            "➡️ Request: CodeAction at {:?} line: {}, char: {}",
            uri, position.line, position.character
        ))
        .await;

        let result = capabilities::code_actions::handle_code_action(
            &params,
            &self.project_index,
            self.workspace_roots
                .get()
                .and_then(|r| r.first().map(PathBuf::as_path)),
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

        if let Some(path) = uri.to_file_path() {
            if scanner::is_ignored(&path) {
                return Ok(None);
            }
        }

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
    ) -> Result<Option<WorkspaceSymbolResponse>> {
        self.log_dev_info(&format!(
            "➡️ Request: WorkspaceSymbol query: '{}'",
            params.query
        ))
        .await;

        let result = capabilities::symbols::handle_workspace_symbol(&params, &self.project_index);

        if let Some(ref response) = result {
            match response {
                WorkspaceSymbolResponse::Flat(syms) => {
                    self.log_dev_info(&format!("✅ Found {} workspace symbols", syms.len()))
                        .await;
                }
                WorkspaceSymbolResponse::Nested(syms) => {
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
        let uri = &params.text_document_position.text_document.uri;
        if let Some(path) = uri.to_file_path() {
            if scanner::is_ignored(&path) {
                return Ok(None);
            }
        }

        self.log_dev_info("➡️ Request: Completion").await;

        let result = capabilities::completion::handle_completion(
            &params,
            &self.project_index,
            &self.document_cache,
        );

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
    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path_cow) = params.text_document.uri.to_file_path() {
            let path: PathBuf = path_cow.into_owned();
            let content = params.text_document.text.clone();

            // Cache document content for completion
            self.document_cache.insert(path.clone(), content.clone());

            if file_processor::process_file_content(&path, &content, &self.project_index) {
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

        let Some((path, content)) = extract_change_params(params) else {
            return;
        };

        // Cache document content immediately for completion (before debounce)
        self.document_cache.insert(path.clone(), content.clone());

        // Cancel existing debounce task for this file
        if let Some((_key, old_task)) = self.debounce_tasks.remove(&path) {
            old_task.abort();
        }

        // Spawn new debounced task
        let task = self.spawn_debounced_processing(path.clone(), content);
        self.debounce_tasks.insert(path, task);
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

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        self.log_dev_info(&format!("➡️ Request: ExecuteCommand {}", params.command))
            .await;

        let roots = self.workspace_roots.get().cloned().unwrap_or_default();
        let config = self.typegen_config.read().await;

        match capabilities::commands::handle_execute_command(
            &params,
            &self.project_index,
            &roots,
            &config,
        ) {
            Ok(res) => {
                self.log_dev_info("✅ Command executed successfully").await;
                Ok(res)
            }
            Err(e) => {
                self.log_dev_info(&format!("❌ Command execution failed: {e:?}"))
                    .await;
                Err(e)
            }
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
    let typegen_config = Arc::new(tokio::sync::RwLock::new(TypegenConfig::default()));
    let bindings_config = Arc::new(tokio::sync::RwLock::new(BindingsConfig::default()));

    let (service, socket) = LspService::new(|client| Backend {
        client,
        workspace_roots: OnceCell::new(),
        project_index,
        is_developer_mode_active: initial_dev_mode_state.clone(),
        debounce_tasks: Arc::new(DashMap::new()),
        document_cache: Arc::new(DashMap::new()),
        typegen_config,
        bindings_config,
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
