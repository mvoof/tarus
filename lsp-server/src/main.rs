#![warn(clippy::all, clippy::pedantic)]

use dashmap::DashMap;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::OnceCell;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{
    CodeActionParams, CodeActionResponse, CodeLens, CodeLensParams, CompletionParams,
    CompletionResponse, ConfigurationItem, ConfigurationParams, DidChangeTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, DocumentSymbolParams,
    DocumentSymbolResponse, GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams,
    InitializeParams, InitializeResult, InitializedParams, Location, MessageType, OneOf,
    ReferenceParams, ServerCapabilities, SymbolInformation, Uri, WorkspaceSymbol,
    WorkspaceSymbolParams,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server, UriExt};

// Refactored modules
mod bindings_reader;
mod capabilities;
mod config_reader;
mod constants;
mod file_processor;
mod indexer;
mod rust_attr;
mod rust_type_extractor;
mod scanner;
mod syntax;
mod tree_parser;
mod ts_tree_utils;
mod utils;

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
    /// Cache of open document contents for completion and other features
    document_cache: Arc<DashMap<PathBuf, String>>,
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

    /// Log a developer-mode result: "✅ Found N <label>" or "⚠️ No <label> found".
    async fn log_dev_result(&self, count: Option<usize>, label: &str) {
        match count {
            Some(n) => self.log_dev_info(&format!("✅ Found {n} {label}")).await,
            None => self.log_dev_info(&format!("⚠️ No {label} found")).await,
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

    /// Load developer mode and reference limit from VS Code configuration.
    async fn load_config(&self) {
        let request = ConfigurationParams {
            items: vec![
                ConfigurationItem {
                    scope_uri: None,
                    section: Some("tarus.developerMode".to_string()),
                },
                ConfigurationItem {
                    scope_uri: None,
                    section: Some("tarus.referenceLimit".to_string()),
                },
            ],
        };

        let Ok(response) = self.client.configuration(request.items).await else {
            return;
        };

        let mut iter = response.into_iter();

        if let Some(settings) = iter.next() {
            if let Some(is_enabled) = settings.as_bool() {
                self.is_developer_mode_active.store(is_enabled, Ordering::Relaxed);
                self.client
                    .log_message(
                        MessageType::INFO,
                        &format!("Developer Mode initialized to: {is_enabled}"),
                    )
                    .await;
            }
        }

        if let Some(settings) = iter.next() {
            if let Some(limit) = settings.as_u64() {
                self.project_index
                    .set_reference_limit(usize::try_from(limit).unwrap_or(3));
                self.client
                    .log_message(
                        MessageType::INFO,
                        &format!("Reference Limit initialized to: {limit}"),
                    )
                    .await;
            }
        }
    }

    /// Spawn background task that scans workspace files, indexes them, and publishes diagnostics.
    fn spawn_indexing(&self, root: PathBuf) {
        let project_index = self.project_index.clone();
        let client = self.client.clone();
        let is_dev_mode = self.is_developer_mode_active.clone();

        tokio::spawn(async move {
            client.log_message(MessageType::INFO, "🚀 Starting background indexing...").await;

            let files =
                tokio::task::spawn_blocking(move || scan_workspace_files(&root))
                    .await
                    .unwrap_or_default();

            for path in files {
                file_processor::process_file_index(path, &project_index);
            }

            for path in project_index.get_indexed_paths() {
                if let Some(uri) = Uri::from_file_path(&path) {
                    let diags = diagnostics::compute_file_diagnostics(&path, &project_index);
                    client.publish_diagnostics(uri, diags, None).await;
                }
            }

            let report = project_index.technical_report();
            if is_dev_mode.load(Ordering::Relaxed) {
                client.log_message(MessageType::INFO, report).await;
            }

            client.log_message(MessageType::INFO, "🏁 Indexing complete".to_string()).await;
        });
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

        self.load_config().await;

        let Some(root) = self.workspace_root.get() else {
            return;
        };

        let root_for_generators = root.clone();
        let generators =
            tokio::task::spawn_blocking(move || config_reader::discover_generators(&root_for_generators))
                .await
                .unwrap_or_default();

        if generators.is_empty() {
            self.client
                .log_message(
                    MessageType::INFO,
                    "TARUS: No type generator configurations found. Using content-based detection as fallback.",
                )
                .await;
        } else {
            for g in &generators {
                self.client
                    .log_message(
                        MessageType::INFO,
                        &format!(
                            "TARUS: Detected {:?} generator → {}",
                            g.kind,
                            g.output_path.display()
                        ),
                    )
                    .await;
            }
        }

        self.project_index.set_generator_bindings(generators);
        self.spawn_indexing(root.clone());
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

        let count = result.as_ref().and_then(|r| {
            if let GotoDefinitionResponse::Link(links) = r {
                Some(links.len())
            } else {
                None
            }
        });
        self.log_dev_result(count, "definition links").await;

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

        self.log_dev_result(result.as_ref().map(Vec::len), "references").await;

        Ok(result)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;

        self.log_dev_info(&format!("➡️ Request: CodeLens for {uri:?}"))
            .await;

        let result = capabilities::code_lens::handle_code_lens(params, &self.project_index);

        self.log_dev_result(result.as_ref().map(Vec::len), "code lenses").await;

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

        self.log_dev_result(result.as_ref().map(|_| 1), "hover tooltip").await;

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

        self.log_dev_result(result.as_ref().map(Vec::len), "code actions").await;

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

        self.log_dev_result(result.as_ref().map(document_symbol_len), "document symbols")
            .await;

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

        self.log_dev_result(result.as_ref().map(one_of_len), "workspace symbols")
            .await;

        Ok(result)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        self.log_dev_info("➡️ Request: Completion").await;

        let result = capabilities::completion::handle_completion(
            &params,
            &self.project_index,
            &self.document_cache,
        );

        self.log_dev_result(result.as_ref().map(completion_response_len), "completion items")
            .await;

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

        if let Some(path) = uri_to_path(&params.text_document.uri) {
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

        if let Some(path) = uri_to_path(&params.text_document.uri) {
            // With TextDocumentSyncKind::FULL, content_changes[0].text contains the full document
            if let Some(change) = params.content_changes.into_iter().next() {
                let content = change.text;

                // Cache document content immediately for completion (before debounce)
                self.document_cache.insert(path.clone(), content.clone());

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
                    tokio::time::sleep(Duration::from_millis(constants::DEBOUNCE_MS)).await;
                    process_debounced_change(
                        &path_clone,
                        &content,
                        &project_index,
                        &client,
                        &is_dev_mode,
                    )
                    .await;
                });

                self.debounce_tasks.insert(path, task);
            }
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path) = uri_to_path(&params.text_document.uri) {
            self.on_change(path.clone()).await;
            self.publish_diagnostics_for_file(&path).await;
        }
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().map(|c| c.into_owned())
}

fn document_symbol_len(response: &DocumentSymbolResponse) -> usize {
    match response {
        DocumentSymbolResponse::Flat(syms) => syms.len(),
        DocumentSymbolResponse::Nested(syms) => syms.len(),
    }
}

fn one_of_len(response: &OneOf<Vec<SymbolInformation>, Vec<WorkspaceSymbol>>) -> usize {
    match response {
        OneOf::Left(syms) => syms.len(),
        OneOf::Right(syms) => syms.len(),
    }
}

fn completion_response_len(response: &CompletionResponse) -> usize {
    match response {
        CompletionResponse::Array(items) => items.len(),
        CompletionResponse::List(list) => list.items.len(),
    }
}

/// Process a file change after debounce: parse, compute affected keys,
/// and publish diagnostics for all impacted files.
async fn process_debounced_change(
    path: &std::path::Path,
    content: &str,
    project_index: &Arc<ProjectIndex>,
    client: &Client,
    is_dev_mode: &Arc<AtomicBool>,
) {
    // Get OLD keys before processing (will be removed)
    let old_keys: Vec<IndexKey> = project_index.get_file_keys(path);

    if !file_processor::process_file_content(path, content, project_index) {
        return;
    }

    // Log parse errors in developer mode (check AFTER processing)
    if is_dev_mode.load(Ordering::Relaxed) {
        if let Some(error_msg) = project_index.get_parse_error(path) {
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
    let new_keys: Vec<IndexKey> = project_index.get_file_keys(path);

    // Combine old and new keys to find all affected commands/events
    let mut all_keys = HashSet::new();
    for key in old_keys.iter().chain(new_keys.iter()) {
        all_keys.insert(key.clone());
    }

    // Collect all files that contain these commands/events
    let mut affected_files = HashSet::new();
    affected_files.insert(path.to_path_buf());

    for key in &all_keys {
        for loc in project_index.get_locations_for_key(key) {
            affected_files.insert(loc.path.clone());
        }
    }

    // Publish diagnostics for all affected files
    for file in affected_files {
        if let Some(uri) = Uri::from_file_path(&file) {
            let diagnostics = diagnostics::compute_file_diagnostics(&file, project_index);
            client.publish_diagnostics(uri, diagnostics, None).await;
        }
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
        document_cache: Arc::new(DashMap::new()),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
