//! The language server: [`Backend`] and its `LanguageServer` impl. `main.rs` only
//! serves it over stdin/stdout, so tests can drive it in-process through
//! `LspService`.

use dashmap::DashMap;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::OnceCell;
use tower_lsp_server::jsonrpc::Result;
use tower_lsp_server::lsp_types::{
    CodeActionParams, CodeActionResponse, CodeLens, CodeLensParams, CompletionParams,
    CompletionResponse, ConfigurationItem, ConfigurationParams, DidChangeTextDocumentParams,
    DidChangeWatchedFilesParams, DidChangeWatchedFilesRegistrationOptions,
    DidCloseTextDocumentParams, DidOpenTextDocumentParams, DidSaveTextDocumentParams,
    DocumentSymbolParams, DocumentSymbolResponse, ExecuteCommandParams, FileChangeType,
    FileSystemWatcher, GlobPattern, GotoDefinitionParams, GotoDefinitionResponse, Hover,
    HoverParams, InitializeParams, InitializeResult, InitializedParams, LSPAny, Location, OneOf,
    ReferenceParams, Registration, ServerCapabilities, SymbolInformation, Uri, WorkspaceSymbol,
    WorkspaceSymbolParams,
};
use tower_lsp_server::{Client, LanguageServer, UriExt};

use crate::capabilities::{self, build_server_capabilities, diagnostics};
use crate::constants;
use crate::discovery::config_reader;
use crate::discovery::scanner::{
    self, is_tauri_project, scan_workspace_files, WorkspaceFilter, LARGE_FILE_BYTES,
};
use crate::indexer::{DiscoveredGenerator, ProjectIndex};
use crate::logging::{self, format_duration, Log};
use crate::pipeline;
use crate::scheduling::{PublishOrder, ReindexQueue};
use crate::utils;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The TARUS language server, one per editor session.
#[derive(Debug)]
pub struct Backend {
    client: Client,
    workspace_root: OnceCell<PathBuf>,
    /// Which files the editor or the file watcher hands over get indexed, set once
    /// the generators are known.
    workspace_filter: OnceCell<WorkspaceFilter>,
    project_index: Arc<ProjectIndex>,
    log: Log,
    debounce_tasks: Arc<DashMap<PathBuf, tokio::task::JoinHandle<()>>>,
    /// Orders this server's re-indexes and publishes, see [`Jobs`].
    jobs: Arc<Jobs>,
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
    /// A server that has not been initialized yet, talking to the editor through `client`.
    #[must_use]
    pub fn new(client: Client) -> Self {
        Self {
            log: Log::new(client.clone()),
            client,
            workspace_root: OnceCell::new(),
            workspace_filter: OnceCell::new(),
            project_index: Arc::new(ProjectIndex::new()),
            debounce_tasks: Arc::new(DashMap::new()),
            jobs: Arc::new(Jobs::default()),
            document_cache: Arc::new(DashMap::new()),
            constants_ready: Arc::new(AtomicBool::new(false)),
            index_ready: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Helper: Checks if the server is fully initialized (workspace root set)
    fn is_ready(&self) -> bool {
        self.workspace_root.get().is_some()
    }

    /// Like `uri_to_path`, but None for files the workspace scan excludes (ignore
    /// files, `node_modules`, `.d.ts`, ...), so opening or changing one never
    /// indexes it.
    fn indexable_path(&self, uri: &Uri) -> Option<PathBuf> {
        let path = uri_to_path(uri)?;
        let is_ignored = match self.workspace_filter.get() {
            Some(filter) => filter.is_ignored(&path),
            None => scanner::is_ignored_path(&path),
        };

        (!is_ignored).then_some(path)
    }

    /// Re-index `path` from its current text and publish what that reaches.
    async fn reindex_and_publish(&self, path: PathBuf) {
        let started = Instant::now();
        let job_path = path.clone();
        let files = in_background(
            &self.project_index,
            &self.document_cache,
            &self.log,
            &self.jobs,
            move |index, read| pipeline::reindex_current(&job_path, index, read),
        )
        .await;

        report_reindex(&self.log, &self.project_index, &path, files.len(), started).await;

        publish_diagnostics_for(
            files,
            &self.project_index,
            &self.client,
            &self.index_ready,
            &self.jobs,
        )
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

        if let Some(is_enabled) = iter.next().and_then(|setting| setting.as_bool()) {
            self.log.set_debug(is_enabled);
        }

        let limit = iter.next().and_then(|setting| setting.as_u64());

        if let Some(limit) = limit {
            self.project_index
                .set_reference_limit(usize::try_from(limit).unwrap_or(3));
        }

        self.log
            .debug(|| {
                let limit = limit.map_or_else(|| "default".to_string(), |limit| limit.to_string());

                format!("Settings: developerMode on, referenceLimit {limit}")
            })
            .await;
    }

    /// Spawn background task that scans workspace files, indexes them, and publishes diagnostics.
    fn spawn_indexing(&self, root: PathBuf, generators: Vec<DiscoveredGenerator>) {
        let project_index = self.project_index.clone();
        let document_cache = self.document_cache.clone();
        let client = self.client.clone();
        let log = self.log.clone();
        let constants_ready = self.constants_ready.clone();
        let index_ready = self.index_ready.clone();
        let jobs = self.jobs.clone();

        tokio::spawn(async move {
            let indexing_started = Instant::now();
            let files = scan_and_report(&log, root, generators).await;
            let pass_started = Instant::now();

            // Both passes take the lock editor re-indexes take, one file at a time: an
            // edit indexed while the scan runs must not be overwritten by it, and must
            // not wait for the whole scan either. Pass 1 collects constants so
            // cross-file references are resolvable.
            let files = {
                let project_index = project_index.clone();
                let document_cache = document_cache.clone();
                let jobs = jobs.clone();

                tokio::task::spawn_blocking(move || {
                    for path in &files {
                        let _guard = jobs.lock_reindex();

                        let Ok(content) = utils::read_current_text(path, &document_cache) else {
                            continue;
                        };

                        let content = pipeline::startup_names_text(
                            path,
                            content,
                            document_cache.contains_key(path),
                            || std::fs::read_to_string(path).ok(),
                        );

                        pipeline::collect_constants_from_file(path, &content, &project_index);
                    }

                    files
                })
                .await
                .unwrap_or_default()
            };

            report_constants(&log, &project_index, pass_started).await;

            // Signal that the global constants map is fully populated so that
            // did_open / did_change handlers can use it for cross-file resolution.
            constants_ready.store(true, Ordering::Release);

            // Pass 2: full parse using the complete constants map, from each file's
            // current text — a file opened during the scan has the editor's copy.
            let pass_started = Instant::now();
            let files = {
                let project_index = project_index.clone();
                let jobs = jobs.clone();

                tokio::task::spawn_blocking(move || {
                    for path in &files {
                        let _guard = jobs.lock_reindex();

                        let Ok(content) = utils::read_current_text(path, &document_cache) else {
                            continue;
                        };

                        // An open file mid-edit that no edit has indexed yet has no
                        // earlier good index to keep, so the saved text stands in until
                        // the edit parses. One an edit already indexed keeps that.
                        if !pipeline::process_file_content(path, &content, &project_index)
                            && document_cache.contains_key(path)
                            && !project_index.is_indexed(path)
                        {
                            if let Ok(saved) = std::fs::read_to_string(path) {
                                pipeline::process_file_content(path, &saved, &project_index);
                            }
                        }
                    }

                    files
                })
                .await
                .unwrap_or_default()
            };

            report_full_parse(&log, &project_index, &files, pass_started).await;

            // Set before publishing: a re-index finishing from here on publishes its
            // own files, and one that finished before is covered by the loop below.
            index_ready.store(true, Ordering::Release);

            publish_diagnostics_for(
                project_index.get_indexed_paths(),
                &project_index,
                &client,
                &index_ready,
                &jobs,
            )
            .await;

            // Trigger VS Code to re-request CodeLens for all open editors now that
            // the index is fully populated with resolved constants.
            let _ = client.code_lens_refresh().await;

            let counts = project_index.name_counts();
            log.info(format!(
                "Indexing complete in {}: {} commands, {} events",
                format_duration(indexing_started.elapsed()),
                counts.commands,
                counts.events,
            ))
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
                self.log.set_root(root.clone());

                self.log
                    .info(format!(
                        "TARUS {}, workspace {}",
                        env!("CARGO_PKG_VERSION"),
                        root.display()
                    ))
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
            self.log
                .info("Generators: none found, bindings are detected by content")
                .await;
        } else {
            let found: Vec<String> = generators
                .iter()
                .map(|generator| {
                    format!(
                        "{:?} → {}",
                        generator.kind,
                        self.log.path(&generator.output_path)
                    )
                })
                .collect();

            self.log
                .info(format!("Generators: {}", found.join(", ")))
                .await;
        }

        let _ = self
            .workspace_filter
            .set(WorkspaceFilter::new(root, generators.clone()));
        self.project_index
            .set_generator_bindings(generators.clone());
        self.spawn_indexing(root.clone(), generators);

        let registration_options = DidChangeWatchedFilesRegistrationOptions {
            watchers: vec![FileSystemWatcher {
                glob_pattern: GlobPattern::String("**/*.{rs,ts,tsx,js,jsx,vue,svelte}".to_string()),
                kind: None,
            }],
        };
        let register_options = match serde_json::to_value(registration_options) {
            Ok(v) => Some(v),
            Err(e) => {
                self.log
                    .warn(format!(
                        "File watcher not registered, its options did not serialize: {e}"
                    ))
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
            self.log
                .warn(format!(
                    "File watcher not registered, the editor rejected it: {e}"
                ))
                .await;
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let started = Instant::now();
        let target = self.log.location(
            &params.text_document_position_params.text_document.uri,
            Some(params.text_document_position_params.position),
        );

        let result = capabilities::definition::handle_goto_definition(params, &self.project_index);

        let count = result.as_ref().and_then(|r| {
            if let GotoDefinitionResponse::Link(links) = r {
                Some(links.len())
            } else {
                None
            }
        });
        self.log
            .request(
                "Definition",
                &target,
                &logging::outcome(count, "links"),
                started,
            )
            .await;

        Ok(result)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let started = Instant::now();
        let target = self.log.location(
            &params.text_document_position.text_document.uri,
            Some(params.text_document_position.position),
        );

        let result = capabilities::references::handle_references(params, &self.project_index);

        let outcome = logging::outcome(result.as_ref().map(Vec::len), "references");
        self.log
            .request("References", &target, &outcome, started)
            .await;

        Ok(result)
    }

    async fn code_lens(&self, params: CodeLensParams) -> Result<Option<Vec<CodeLens>>> {
        let started = Instant::now();
        let target = self.log.location(&params.text_document.uri, None);

        let result = capabilities::code_lens::handle_code_lens(params, &self.project_index);

        let outcome = logging::outcome(result.as_ref().map(Vec::len), "lenses");
        self.log
            .request("CodeLens", &target, &outcome, started)
            .await;

        Ok(result)
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let started = Instant::now();
        let target = self.log.location(
            &params.text_document_position_params.text_document.uri,
            Some(params.text_document_position_params.position),
        );

        let result = capabilities::hover::handle_hover(params, &self.project_index);

        let outcome = if result.is_some() { "found" } else { "none" };
        self.log.request("Hover", &target, outcome, started).await;

        Ok(result)
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let started = Instant::now();
        let target = self
            .log
            .location(&params.text_document.uri, Some(params.range.start));

        let workspace_root = self.workspace_root.get().cloned();
        let result = capabilities::code_actions::handle_code_action(
            &params,
            &self.project_index,
            workspace_root.as_ref(),
        );

        let outcome = logging::outcome(result.as_ref().map(Vec::len), "actions");
        self.log
            .request("CodeAction", &target, &outcome, started)
            .await;

        Ok(result)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let started = Instant::now();
        let target = self.log.location(&params.text_document.uri, None);

        let result = capabilities::symbols::handle_document_symbol(params, &self.project_index);

        let outcome = logging::outcome(result.as_ref().map(document_symbol_len), "symbols");
        self.log
            .request("Symbols", &target, &outcome, started)
            .await;

        Ok(result)
    }

    async fn symbol(
        &self,
        params: WorkspaceSymbolParams,
    ) -> Result<Option<OneOf<Vec<SymbolInformation>, Vec<WorkspaceSymbol>>>> {
        let started = Instant::now();
        let target = format!("\"{}\"", params.query);

        let result = capabilities::symbols::handle_workspace_symbol(&params, &self.project_index);

        let outcome = logging::outcome(result.as_ref().map(one_of_len), "symbols");
        self.log
            .request("Workspace", &target, &outcome, started)
            .await;

        Ok(result)
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let started = Instant::now();
        let target = self.log.location(
            &params.text_document_position.text_document.uri,
            Some(params.text_document_position.position),
        );

        let result = capabilities::completion::handle_completion(
            &params,
            &self.project_index,
            &self.document_cache,
        );

        let outcome = logging::outcome(result.as_ref().map(completion_response_len), "items");
        self.log
            .request("Completion", &target, &outcome, started)
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

        if let Some(path) = self.indexable_path(&params.text_document.uri) {
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

        if let Some(path) = self.indexable_path(&params.text_document.uri) {
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
                let log = self.log.clone();
                let index_ready = self.index_ready.clone();
                let jobs = self.jobs.clone();

                // Only the wait is cancellable. Once it has elapsed, the re-index runs
                // as its own task: aborting it half-way would leave the index updated
                // and the diagnostics that should follow it never published. A job
                // already queued for this file will read the newest text when it
                // runs, so no second one is queued behind it.
                let task = tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(constants::DEBOUNCE_MS)).await;

                    if !jobs.queued.try_queue(&path_clone) {
                        return;
                    }

                    tokio::spawn(async move {
                        process_debounced_change(
                            &path_clone,
                            &project_index,
                            &document_cache,
                            &client,
                            &log,
                            &index_ready,
                            &jobs,
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

        if let Some(path) = self.indexable_path(&params.text_document.uri) {
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

        if let Some(path) = self.indexable_path(&params.text_document.uri) {
            self.reindex_and_publish(path).await;
        }
    }

    async fn did_change_watched_files(&self, params: DidChangeWatchedFilesParams) {
        if !self.is_ready() {
            return;
        }

        for event in params.changes {
            let Some(path) = self.indexable_path(&event.uri) else {
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
                    &self.log,
                    &self.jobs,
                    move |index, read| pipeline::forget(&job_path, index, |file| read(file).ok()),
                )
                .await;

                // Cleared once the file is out of the index: a publish computed from
                // here on finds nothing for it either, so none can bring them back.
                clear_diagnostics(&self.client, &self.jobs, path).await;

                publish_diagnostics_for(
                    others,
                    &self.project_index,
                    &self.client,
                    &self.index_ready,
                    &self.jobs,
                )
                .await;
            } else {
                // CREATED or CHANGED
                self.reindex_and_publish(path).await;
            }
        }
    }

    async fn execute_command(&self, params: ExecuteCommandParams) -> Result<Option<LSPAny>> {
        if params.command == constants::DUMP_INDEX_COMMAND {
            let report = self.project_index.index_report(&|path| self.log.path(path));
            self.log.info(report).await;
        }

        Ok(None)
    }

    #[allow(unknown_lints, clippy::unused_async_trait_impl)]
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    uri.to_file_path().map(std::borrow::Cow::into_owned)
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
    log: &Log,
    index_ready: &AtomicBool,
    jobs: &Arc<Jobs>,
) {
    let started = Instant::now();
    let job_path = path.to_path_buf();
    let job_queue = jobs.clone();
    let files = in_background(
        project_index,
        document_cache,
        log,
        jobs,
        move |index, read| {
            job_queue.queued.started(&job_path);

            pipeline::reindex_current(&job_path, index, read)
        },
    )
    .await;

    report_reindex(log, project_index, path, files.len(), started).await;

    publish_diagnostics_for(files, project_index, client, index_ready, jobs).await;
}

/// Keeps one server's background jobs from stepping on each other. Held per
/// server rather than in statics, so servers sharing a process never wait on each
/// other.
#[derive(Debug, Default)]
struct Jobs {
    /// Re-indexes run one at a time, so a dependent is never re-parsed against
    /// half-updated constants. It does not order them: each job reads the file's
    /// current text when it runs (`pipeline::reindex_current`), so their order does
    /// not matter.
    reindex_lock: std::sync::Mutex<()>,
    /// Debounced re-index jobs queued but not started, see [`ReindexQueue`].
    queued: ReindexQueue,
    /// Keeps publishes that finish out of order from sending stale diagnostics.
    publishes: PublishOrder,
}

impl Jobs {
    /// Take the re-index lock, also after a job panicked holding it: the next job
    /// reads every file's current text afresh rather than trusting what the last
    /// one left.
    fn lock_reindex(&self) -> std::sync::MutexGuard<'_, ()> {
        self.reindex_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Run a re-index `job` off the async runtime, under the re-index lock: it reads files
/// from disk and parses them, which would otherwise stall the language server. The
/// job gets the index and a reader for any file's current text.
async fn in_background<F>(
    project_index: &Arc<ProjectIndex>,
    document_cache: &Arc<DashMap<PathBuf, String>>,
    log: &Log,
    jobs: &Arc<Jobs>,
    job: F,
) -> Vec<PathBuf>
where
    F: FnOnce(&ProjectIndex, &dyn Fn(&Path) -> std::io::Result<String>) -> Vec<PathBuf>
        + Send
        + 'static,
{
    let project_index = project_index.clone();
    let document_cache = document_cache.clone();
    let jobs = jobs.clone();

    tokio::task::spawn_blocking(move || {
        let _guard = jobs.lock_reindex();
        let read = |file: &Path| utils::read_current_text(file, &document_cache);

        job(&project_index, &read)
    })
    .await
    .unwrap_or_else(|error| {
        let message = format!("Re-index failed: {error}");
        let log = log.clone();
        tokio::spawn(async move { log.error(message).await });

        Vec::new()
    })
}

/// Compute and publish diagnostics for each of `files` — none before `index_ready`,
/// see [`Backend::index_ready`].
///
/// They are computed off the async runtime and sent as each is ready. Publishes
/// for one file can run concurrently, so a result older than the one already sent
/// is dropped rather than sent over it.
async fn publish_diagnostics_for(
    files: Vec<PathBuf>,
    project_index: &Arc<ProjectIndex>,
    client: &Client,
    index_ready: &AtomicBool,
    jobs: &Arc<Jobs>,
) {
    if !index_ready.load(Ordering::Acquire) || files.is_empty() {
        return;
    }

    let (sender, mut computed) = tokio::sync::mpsc::unbounded_channel();
    let project_index = project_index.clone();
    let worker_jobs = jobs.clone();

    let worker = tokio::task::spawn_blocking(move || {
        for file in files {
            let sequence = worker_jobs.publishes.start();
            let diagnostics = diagnostics::compute_file_diagnostics(&file, &project_index);

            if sender.send((file, sequence, diagnostics)).is_err() {
                break;
            }
        }
    });

    while let Some((file, sequence, diagnostics)) = computed.recv().await {
        send_if_newest(client, jobs, file, sequence, diagnostics).await;
    }

    let _ = worker.await;
}

/// Clear `path`'s diagnostics, unless a newer computation already published them.
/// Scan the workspace for the files to index, saying how many and where, and
/// warning about each one over the size source files rarely reach: it is usually
/// build output that no ignore file covers yet.
async fn scan_and_report(
    log: &Log,
    root: PathBuf,
    generators: Vec<DiscoveredGenerator>,
) -> Vec<PathBuf> {
    let scan_root = root.clone();
    let scan = tokio::task::spawn_blocking(move || scan_workspace_files(&scan_root, &generators))
        .await
        .unwrap_or_default();

    log.info(format!(
        "Indexing {} files ({})",
        scan.files.len(),
        files_by_folder(&root, &scan.files)
    ))
    .await;

    for (path, size) in &scan.large_files {
        log.warn(format!(
            "Large file indexed: {} ({} KB, over {} KB). If it is build output, add it to .gitignore. If Tarus should skip such files by default, open an issue: {}",
            log.path(path),
            size / 1024,
            LARGE_FILE_BYTES / 1024,
            constants::NEW_ISSUE_URL,
        ))
        .await;
    }

    scan.files
}

/// How many folders `files_by_folder` names before summing up the rest
const FOLDERS_SHOWN: usize = 4;

/// `src 816, src-tauri 125, 2 more 13`: file counts per top-level folder of
/// `root`, largest first.
fn files_by_folder(root: &Path, files: &[PathBuf]) -> String {
    let mut counts: HashMap<String, usize> = HashMap::new();

    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(file);
        let folder = match relative.components().next() {
            Some(first) if relative.components().count() > 1 => {
                first.as_os_str().to_string_lossy().into_owned()
            }
            _ => "root".to_string(),
        };

        *counts.entry(folder).or_default() += 1;
    }

    let mut counts: Vec<(String, usize)> = counts.into_iter().collect();
    counts.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    let mut parts: Vec<String> = counts
        .iter()
        .take(FOLDERS_SHOWN)
        .map(|(folder, count)| format!("{folder} {count}"))
        .collect();

    let rest = &counts[counts.len().min(FOLDERS_SHOWN)..];

    if !rest.is_empty() {
        let rest_files: usize = rest.iter().map(|(_, count)| count).sum();
        parts.push(format!("{} more {rest_files}", rest.len()));
    }

    parts.join(", ")
}

/// Pass 1 summary; the constants themselves in developer mode.
async fn report_constants(log: &Log, project_index: &ProjectIndex, started: Instant) {
    let rust_constants = project_index.get_all_rust_constants();
    let js_constants = project_index.get_all_js_constants();

    log.info(format!(
        "Pass 1, constants and helpers: {} Rust, {} JS/TS constants ({})",
        rust_constants.len(),
        js_constants.len(),
        format_duration(started.elapsed())
    ))
    .await;

    for (language, constants) in [("Rust", &rust_constants), ("JS/TS", &js_constants)] {
        log.debug(|| {
            let mut pairs: Vec<String> = constants
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            pairs.sort();

            format!("{language} constants: {}", pairs.join(", "))
        })
        .await;
    }
}

/// Pass 2 summary, and a warning for each file that did not parse.
async fn report_full_parse(
    log: &Log,
    project_index: &ProjectIndex,
    files: &[PathBuf],
    started: Instant,
) {
    let not_parsed: Vec<(&PathBuf, String)> = files
        .iter()
        .filter_map(|path| Some((path, project_index.get_parse_error(path)?)))
        .collect();

    log.info(format!(
        "Pass 2, full parse: {} files, {} not parsed ({})",
        files.len(),
        not_parsed.len(),
        format_duration(started.elapsed())
    ))
    .await;

    for (path, error) in not_parsed {
        log.warn(format!("Not parsed: {}: {error}", log.path(path)))
            .await;
    }
}

/// One developer-mode line for a re-index of `path` that republished
/// `republished` files. A file the job dropped, because it is gone, has none.
async fn report_reindex(
    log: &Log,
    project_index: &ProjectIndex,
    path: &Path,
    republished: usize,
    started: Instant,
) {
    if !log.is_debug() {
        return;
    }

    let outcome = match project_index.get_parse_error(path) {
        Some(error) => format!("not parsed: {error}"),
        None if project_index.is_indexed(path) => format!(
            "{} names, {republished} files republished",
            project_index.get_file_keys(path).len()
        ),
        None => return,
    };

    log.request("Re-index", &log.path(path), &outcome, started)
        .await;
}

async fn clear_diagnostics(client: &Client, jobs: &Jobs, path: PathBuf) {
    let sequence = jobs.publishes.start();
    send_if_newest(client, jobs, path, sequence, Vec::new()).await;
}

/// Send `diagnostics` for `file` unless diagnostics from a later computation were
/// sent already.
async fn send_if_newest(
    client: &Client,
    jobs: &Jobs,
    file: PathBuf,
    sequence: u64,
    diagnostics: Vec<tower_lsp_server::lsp_types::Diagnostic>,
) {
    let Some(uri) = Uri::from_file_path(&file) else {
        return;
    };

    jobs.publishes
        .send_if_newest(&file, sequence, || {
            client.publish_diagnostics(uri, diagnostics, None)
        })
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(root: &Path, relative: &[&str]) -> Vec<PathBuf> {
        relative.iter().map(|file| root.join(file)).collect()
    }

    #[test]
    fn files_by_folder_lists_the_largest_folders_first() {
        let root = Path::new("/proj");
        let scanned = files(
            root,
            &[
                "src/a.ts",
                "src/b.ts",
                "src/c.ts",
                "src-tauri/src/main.rs",
                "src-tauri/build.rs",
            ],
        );

        assert_eq!(files_by_folder(root, &scanned), "src 3, src-tauri 2");
    }

    #[test]
    fn files_by_folder_counts_files_at_the_root_and_sums_up_the_rest() {
        let root = Path::new("/proj");
        let scanned = files(
            root,
            &[
                "vite.ts", "a/1.ts", "a/2.ts", "b/1.ts", "c/1.ts", "d/1.ts", "e/1.ts",
            ],
        );

        assert_eq!(
            files_by_folder(root, &scanned),
            "a 2, b 1, c 1, d 1, 2 more 2"
        );
    }
}
