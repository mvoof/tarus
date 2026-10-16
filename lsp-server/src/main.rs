#![warn(clippy::all, clippy::pedantic)]

use dashmap::DashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::OnceCell;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{
    CodeActionParams, CodeActionResponse, CodeLens, CodeLensParams, CompletionParams,
    CompletionResponse, ConfigurationItem, ConfigurationParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidChangeWatchedFilesRegistrationOptions,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, FileChangeType, FileSystemWatcher, GlobPattern,
    GotoDefinitionParams, GotoDefinitionResponse, Hover, HoverParams, InitializeParams,
    InitializeResult, InitializedParams, Location, MessageType, OneOf, ReferenceParams,
    Registration, ServerCapabilities, SymbolInformation, Uri, WorkspaceSymbol,
    WorkspaceSymbolParams,
};
use tower_lsp_server::{Client, LanguageServer, LspService, Server, UriExt};

// Refactored modules
mod capabilities;
mod constants;
mod discovery;
mod indexer;
mod parser;
mod pipeline;
mod syntax;
mod utils;

use crate::discovery::config_reader;
use crate::discovery::scanner;

use capabilities::{build_server_capabilities, diagnostics};
use indexer::ProjectIndex;
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
    /// Set to true after Pass 1 (constant collection) completes so `did_open/did_change`missing backticks
    /// can use the full global constants map for cross-file resolution.
    constants_ready: Arc<AtomicBool>,
    /// Set to true once the startup scan has parsed every file. Until then a
    /// re-index publishes nothing: diagnostics computed from a half-built index warn
    /// about calls whose other side is not indexed yet. The scan publishes every
    /// file when it is done.
    index_ready: Arc<AtomicBool>,
}

impl Backend {
    /// Helper: Checks if the server is fully initialized (workspace root set)
    fn is_ready(&self) -> bool {
        self.workspace_root.get().is_some()
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

    /// Re-index `path` from its current text and publish what that reaches.
    async fn reindex_and_publish(&self, path: PathBuf) {
        let job_path = path.clone();
        let files = in_background(
            &self.project_index,
            &self.document_cache,
            &self.client,
            move |index, read| pipeline::reindex_current(&job_path, index, read),
        )
        .await;
        self.finish_reindex(&path, files).await;
    }

    /// Publish diagnostics for `files` and, in developer mode, report on `path` —
    /// only when it was indexed: a skipped or failed job has nothing to report, and
    /// neither has a job that dropped the file because it is gone.
    async fn finish_reindex(&self, path: &Path, files: Vec<PathBuf>) {
        if !files.is_empty()
            && self.project_index.is_indexed(path)
            && self.project_index.get_parse_error(path).is_none()
        {
            self.log_dev_info(&self.project_index.file_report(path))
                .await;
        }

        publish_diagnostics_for(files, &self.project_index, &self.client, &self.index_ready).await;
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
        let document_cache = self.document_cache.clone();
        let client = self.client.clone();
        let is_dev_mode = self.is_developer_mode_active.clone();
        let constants_ready = self.constants_ready.clone();
        let index_ready = self.index_ready.clone();

        tokio::spawn(async move {
            client
                .log_message(MessageType::INFO, "🚀 Starting background indexing...")
                .await;

            let files = tokio::task::spawn_blocking(move || scan_workspace_files(&root))
                .await
                .unwrap_or_default();

            // Both passes take the lock editor re-indexes take, one file at a time: an
            // edit indexed while the scan runs must not be overwritten by it, and must
            // not wait for the whole scan either. Pass 1 collects constants so
            // cross-file references are resolvable.
            let files = {
                let project_index = project_index.clone();
                let document_cache = document_cache.clone();

                tokio::task::spawn_blocking(move || {
                    for path in &files {
                        let _guard = lock_reindex();

                        if let Ok(content) = utils::read_current_text(path, &document_cache) {
                            pipeline::collect_constants_from_file(path, &content, &project_index);
                        }
                    }

                    files
                })
                .await
                .unwrap_or_default()
            };

            let rust_constants = project_index.get_all_rust_constants();
            let js_constants = project_index.get_all_js_constants();
            let rust_const_count = rust_constants.len();
            let js_const_count = js_constants.len();
            let rust_keys: Vec<String> = rust_constants
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            let js_keys: Vec<String> = js_constants
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            client
                .log_message(
                    MessageType::INFO,
                    format!(
                        "🔑 Constants collected — Rust: {rust_const_count} [{rust_keys}], JS/TS: {js_const_count} [{js_keys}]",
                        rust_keys = rust_keys.join(", "),
                        js_keys = js_keys.join(", "),
                    ),
                )
                .await;

            // Signal that the global constants map is fully populated so that
            // did_open / did_change handlers can use it for cross-file resolution.
            constants_ready.store(true, Ordering::Release);

            // Pass 2: full parse using the complete constants map, from each file's
            // current text — a file opened during the scan has the editor's copy.
            {
                let project_index = project_index.clone();

                let _ = tokio::task::spawn_blocking(move || {
                    for path in files {
                        let _guard = lock_reindex();

                        let Ok(content) = utils::read_current_text(&path, &document_cache) else {
                            continue;
                        };

                        // An open file mid-edit that no edit has indexed yet has no
                        // earlier good index to keep, so the saved text stands in until
                        // the edit parses. One an edit already indexed keeps that.
                        if !pipeline::process_file_content(&path, &content, &project_index)
                            && document_cache.contains_key(&path)
                            && !project_index.is_indexed(&path)
                        {
                            if let Ok(saved) = std::fs::read_to_string(&path) {
                                pipeline::process_file_content(&path, &saved, &project_index);
                            }
                        }
                    }
                })
                .await;
            }

            // Set before publishing: a re-index finishing from here on publishes its
            // own files, and one that finished before is covered by the loop below.
            index_ready.store(true, Ordering::Release);

            publish_diagnostics_for(
                project_index.get_indexed_paths(),
                &project_index,
                &client,
                &index_ready,
            )
            .await;

            // Trigger VS Code to re-request CodeLens for all open editors now that
            // the index is fully populated with resolved constants.
            let _ = client.code_lens_refresh().await;

            let report = project_index.technical_report();
            if is_dev_mode.load(Ordering::Relaxed) {
                client.log_message(MessageType::INFO, report).await;
            }

            client
                .log_message(MessageType::INFO, "🏁 Indexing complete".to_string())
                .await;
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
        let generators = tokio::task::spawn_blocking(move || {
            config_reader::discover_generators(&root_for_generators)
        })
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

        let registration_options = DidChangeWatchedFilesRegistrationOptions {
            watchers: vec![FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.{rs,ts,tsx,js,jsx,vue,svelte}".to_string()),
                kind: None,
            }],
        };
        let register_options = match serde_json::to_value(registration_options) {
            Ok(v) => Some(v),
            Err(e) => {
                self.client
                    .log_message(
                        MessageType::WARNING,
                        format!(
                            "tarus-file-watcher: failed to serialize registration options: {e}"
                        ),
                    )
                    .await;
                return;
            }
        };
        let registration = Registration {
            id: "tarus-file-watcher".to_string(),
            method: "workspace/didChangeWatchedFiles".to_string(),
            register_options,
        };
        if let Err(e) = self.client.register_capability(vec![registration]).await {
            self.client
                .log_message(
                    MessageType::WARNING,
                    format!("tarus-file-watcher: client rejected capability registration: {e}"),
                )
                .await;
        }
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

        self.log_dev_result(result.as_ref().map(Vec::len), "references")
            .await;

        Ok(result)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let uri = &params.text_document.uri;

        self.log_dev_info(&format!("➡️ Request: CodeLens for {uri:?}"))
            .await;

        let result = capabilities::code_lens::handle_code_lens(params, &self.project_index);

        self.log_dev_result(result.as_ref().map(Vec::len), "code lenses")
            .await;

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

        self.log_dev_result(result.as_ref().map(|_| 1), "hover tooltip")
            .await;

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

        self.log_dev_result(result.as_ref().map(Vec::len), "code actions")
            .await;

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

        self.log_dev_result(
            result.as_ref().map(completion_response_len),
            "completion items",
        )
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

        if let Some(path) = indexable_path(&params.text_document.uri) {
            let content = params.text_document.text.clone();

            // Cache document content for completion
            self.document_cache.insert(path.clone(), content.clone());

            // If Pass 1 hasn't finished yet, skip indexing here — spawn_indexing's
            // Pass 2 will index this file with the full global constants map.
            if !self.constants_ready.load(Ordering::Acquire) {
                return;
            }

            self.reindex_and_publish(path).await;
        }
    }

    // The trait requires `async`; the work is debounced into a spawned task, so
    // there is nothing to await here. `unknown_lints` for clippy before 1.98.
    #[allow(unknown_lints, clippy::unused_async_trait_impl)]
    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path) = indexable_path(&params.text_document.uri) {
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
                let document_cache = self.document_cache.clone();
                let client = self.client.clone();
                let path_clone = path.clone();
                let is_dev_mode = self.is_developer_mode_active.clone();
                let index_ready = self.index_ready.clone();

                // Only the wait is cancellable. Once it has elapsed, the re-index runs
                // as its own task: aborting it half-way would leave the index updated
                // and the diagnostics that should follow it never published.
                let task = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(constants::DEBOUNCE_MS)).await;
                    tokio::spawn(async move {
                        process_debounced_change(
                            &path_clone,
                            &project_index,
                            &document_cache,
                            &client,
                            &is_dev_mode,
                            &index_ready,
                        )
                        .await;
                    });
                });

                self.debounce_tasks.insert(path, task);
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path) = indexable_path(&params.text_document.uri) {
            // Closing drops the editor's copy, unsaved edits included: the disk is the
            // file's text again, for this re-index and for any dependent refresh later.
            self.document_cache.remove(&path);

            if let Some((_key, pending)) = self.debounce_tasks.remove(&path) {
                pending.abort();
            }

            // Same as `did_open`: pass 2 reads the file from disk now that its editor
            // copy is gone.
            if !self.constants_ready.load(Ordering::Acquire) {
                return;
            }

            self.reindex_and_publish(path).await;
        }
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        if !self.is_ready() {
            return;
        }

        if let Some(path) = indexable_path(&params.text_document.uri) {
            self.reindex_and_publish(path).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        if !self.is_ready() {
            return;
        }

        for event in params.changes {
            let Some(path) = indexable_path(&event.uri) else {
                continue;
            };

            // Skip files that are open in the editor — did_change already handles them.
            if self.document_cache.contains_key(&path) {
                continue;
            }

            if event.typ == FileChangeType::DELETED {
                let job_path = path.clone();

                let others = in_background(
                    &self.project_index,
                    &self.document_cache,
                    &self.client,
                    move |index, read| pipeline::forget(&job_path, index, |file| read(file).ok()),
                )
                .await;

                // Cleared once the file is out of the index: a publish computed from
                // here on finds nothing for it either, so none can bring them back.
                if let Some(uri) = Uri::from_file_path(&path) {
                    let _guard = PUBLISH_LOCK.lock().await;
                    self.client.publish_diagnostics(uri, vec![], None).await;
                }

                publish_diagnostics_for(
                    others,
                    &self.project_index,
                    &self.client,
                    &self.index_ready,
                )
                .await;
            } else {
                // CREATED or CHANGED
                self.reindex_and_publish(path).await;
            }
        }
    }

    #[allow(unknown_lints, clippy::unused_async_trait_impl)]
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().map(std::borrow::Cow::into_owned)
}

/// Like `uri_to_path`, but None for files the workspace scan excludes
/// (`node_modules`, `target`, `.d.ts`, ...), so opening one never indexes it.
fn indexable_path(uri: &Uri) -> Option<PathBuf> {
    uri_to_path(uri).filter(|path| !scanner::is_ignored_path(path))
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
    path: &Path,
    project_index: &Arc<ProjectIndex>,
    document_cache: &Arc<DashMap<PathBuf, String>>,
    client: &Client,
    is_dev_mode: &Arc<AtomicBool>,
    index_ready: &AtomicBool,
) {
    let job_path = path.to_path_buf();
    let files = in_background(project_index, document_cache, client, move |index, read| {
        pipeline::reindex_current(&job_path, index, read)
    })
    .await;

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

    publish_diagnostics_for(files, project_index, client, index_ready).await;
}

/// Re-indexes run one at a time, so a dependent is never re-parsed against
/// half-updated constants. It does not order them: each job reads the file's current
/// text when it runs (`pipeline::reindex_current`), so their order does not matter.
static REINDEX_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take [`REINDEX_LOCK`], also after a job panicked holding it: the next job reads
/// every file's current text afresh rather than trusting what the last one left.
fn lock_reindex() -> std::sync::MutexGuard<'static, ()> {
    REINDEX_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run a re-index `job` off the async runtime, under [`REINDEX_LOCK`]: it reads files
/// from disk and parses them, which would otherwise stall the language server. The
/// job gets the index and a reader for any file's current text.
async fn in_background<F>(
    project_index: &Arc<ProjectIndex>,
    document_cache: &Arc<DashMap<PathBuf, String>>,
    client: &Client,
    job: F,
) -> Vec<PathBuf>
where
    F: FnOnce(&ProjectIndex, &dyn Fn(&Path) -> std::io::Result<String>) -> Vec<PathBuf>
        + Send
        + 'static,
{
    let project_index = project_index.clone();
    let document_cache = document_cache.clone();

    tokio::task::spawn_blocking(move || {
        let _guard = lock_reindex();
        let read = |file: &Path| utils::read_current_text(file, &document_cache);

        job(&project_index, &read)
    })
    .await
    .unwrap_or_else(|error| {
        let message = format!("Re-index failed: {error}");
        let client = client.clone();
        tokio::spawn(async move { client.log_message(MessageType::ERROR, message).await });

        Vec::new()
    })
}

/// Held from computing one file's diagnostics until they are sent.
static PUBLISH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Compute and publish diagnostics for each of `files` — none before `index_ready`,
/// see [`Backend::index_ready`].
async fn publish_diagnostics_for(
    files: Vec<PathBuf>,
    project_index: &ProjectIndex,
    client: &Client,
    index_ready: &AtomicBool,
) {
    if !index_ready.load(Ordering::Acquire) {
        return;
    }

    for file in files {
        if let Some(uri) = Uri::from_file_path(&file) {
            // Computed and sent under one lock: a publish computed earlier cannot be
            // sent after one computed later and overwrite it.
            let _guard = PUBLISH_LOCK.lock().await;
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
        constants_ready: Arc::new(AtomicBool::new(false)),
        index_ready: Arc::new(AtomicBool::new(false)),
    });

    Server::new(stdin, stdout, socket).serve(service).await;
}
