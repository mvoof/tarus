//! Drives the language server in-process through `LspService`, playing the editor:
//! it sends requests and notifications, answers the server's own requests, and
//! collects what the server publishes and logs.
//!
//! Tests run with tokio time paused (`#[tokio::test(start_paused = true)]`). The
//! clock then only moves when the test advances it, or by itself once nothing is
//! left to run — work in `spawn_blocking` holds it still until it is done. So a
//! wait below elapses only when the server has gone idle, never on a slow machine.
//! [`TestServer::initialize`] panics when time is not paused.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::{SinkExt, StreamExt};
use lsp_server::server::Backend;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tower::{Service, ServiceExt};
use tower_lsp_server::jsonrpc::{Request, Response};
use tower_lsp_server::lsp_types::{
    Diagnostic, FileChangeType, FileEvent, LogMessageParams, PublishDiagnosticsParams, Uri,
};
use tower_lsp_server::{ExitedError, LspService, UriExt};

/// How long [`TestServer::next_event`] waits for a message. With time paused it
/// elapses only when the server is idle with nothing left to send.
const TIMEOUT: Duration = Duration::from_secs(60);

/// How long the server must stay idle for [`TestServer::events_until_idle`] to
/// stop: longer than any timer the server sets, so the debounce fires first.
const QUIET: Duration = Duration::from_secs(5);

/// The log line the startup scan ends with.
pub const INDEXING_COMPLETE: &str = "Indexing complete";

/// A message the server sent to the editor.
#[derive(Debug, Clone)]
pub enum Event {
    Published {
        path: PathBuf,
        diagnostics: Vec<Diagnostic>,
    },
    Log(String),
}

/// A temporary Tauri project on disk.
pub struct Workspace {
    /// Removes the project when the test ends.
    _dir: TempDir,
    root: PathBuf,
}

impl Workspace {
    /// A project with `tauri.conf.json` and `files`, given as (relative path, text).
    pub fn new(files: &[(&str, &str)]) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("tarus-e2e")
            .tempdir()
            .expect("create temp workspace");
        let root = dir.path().canonicalize().expect("canonicalize workspace");
        let workspace = Self { _dir: dir, root };

        workspace.write("src-tauri/tauri.conf.json", "{}");

        for (path, text) in files {
            workspace.write(path, text);
        }

        workspace
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    pub fn write(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().expect("file has a parent")).expect("create dir");
        std::fs::write(path, text).expect("write file");
    }

    pub fn remove(&self, relative: &str) {
        std::fs::remove_file(self.path(relative)).expect("remove file");
    }
}

/// The server, with the editor side of the connection.
pub struct TestServer {
    service: LspService<Backend>,
    next_id: i64,
    events: mpsc::UnboundedReceiver<Event>,
    /// Holds back the answer to `workspace/configuration` while false. The server
    /// asks for it in `initialized` before it starts the startup scan, so holding it
    /// keeps the scan from starting.
    configuration: watch::Sender<bool>,
    /// The `initialized` handler, run on its own task by
    /// [`TestServer::send_initialized`]. Checked when a wait times out, so a panic
    /// in it fails the test with that panic.
    initialized: Option<JoinHandle<Result<Option<Response>, ExitedError>>>,
}

impl TestServer {
    /// Send `initialize` for `root` and wait for its result. Nothing is indexed yet:
    /// `initialized` is up to the test, see [`TestServer::start`].
    pub async fn initialize(root: &Path) -> Self {
        // Panics with "time is not frozen" unless the test runs with
        // `#[tokio::test(start_paused = true)]`: without it every wait below would
        // take its full timeout in real time.
        tokio::time::advance(Duration::ZERO).await;

        let (service, socket) = LspService::new(Backend::new);
        let (events_tx, events) = mpsc::unbounded_channel();
        let (configuration, gate) = watch::channel(false);

        tokio::spawn(play_editor(socket, events_tx, gate));

        let mut server = Self {
            service,
            next_id: 0,
            events,
            configuration,
            initialized: None,
        };

        let root_uri = uri(root);
        let response = server
            .request(
                "initialize",
                json!({
                    "capabilities": {},
                    "workspaceFolders": [{ "uri": root_uri, "name": "workspace" }],
                }),
            )
            .await;
        assert!(
            response["capabilities"]["textDocumentSync"].is_number()
                || response["capabilities"]["textDocumentSync"].is_object(),
            "not detected as a Tauri project: {response}"
        );

        server
    }

    /// Initialize, run the startup scan to the end, and return what it sent.
    pub async fn start(root: &Path) -> (Self, Vec<Event>) {
        let mut server = Self::initialize(root).await;
        server.send_initialized().await;
        server.answer_configuration();
        let events = server.events_until_log(INDEXING_COMPLETE).await;

        (server, events)
    }

    /// Send `initialized` without waiting for it: the server asks for the
    /// configuration first, which [`TestServer::answer_configuration`] answers.
    pub async fn send_initialized(&mut self) {
        let future = self.call(notification("initialized", json!({}))).await;
        self.initialized = Some(tokio::spawn(future));
    }

    /// Let the server's `workspace/configuration` request be answered, and so the
    /// startup scan begin.
    pub fn answer_configuration(&self) {
        self.configuration.send_replace(true);
    }

    pub async fn open(&mut self, path: &Path, text: &str) {
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri(path),
                    "languageId": language_id(path),
                    "version": 1,
                    "text": text,
                }
            }),
        )
        .await;
    }

    pub async fn change(&mut self, path: &Path, version: i32, text: &str) {
        self.notify(
            "textDocument/didChange",
            json!({
                "textDocument": { "uri": uri(path), "version": version },
                "contentChanges": [{ "text": text }],
            }),
        )
        .await;
    }

    pub async fn close(&mut self, path: &Path) {
        self.notify(
            "textDocument/didClose",
            json!({ "textDocument": { "uri": uri(path) } }),
        )
        .await;
    }

    pub async fn watched_file_deleted(&mut self, path: &Path) {
        let event = FileEvent {
            uri: Uri::from_file_path(path).expect("file uri"),
            typ: FileChangeType::DELETED,
        };

        self.notify(
            "workspace/didChangeWatchedFiles",
            json!({ "changes": [event] }),
        )
        .await;
    }

    /// The next message from the server. Panics if the server goes idle first.
    pub async fn next_event(&mut self) -> Event {
        if let Ok(event) = tokio::time::timeout(TIMEOUT, self.events.recv()).await {
            return event.expect("the server closed the connection");
        }

        self.check_initialized().await;
        panic!("the server went idle without sending anything");
    }

    /// Re-raise a panic or error from the `initialized` handler, if it has finished.
    async fn check_initialized(&mut self) {
        let Some(handle) = self.initialized.take_if(|handle| handle.is_finished()) else {
            return;
        };

        match handle.await {
            Ok(Ok(_)) => {}
            Ok(Err(error)) => panic!("initialized failed: {error}"),
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(error) => panic!("initialized was cancelled: {error}"),
        }
    }

    /// Messages up to and including the first log line containing `needle`.
    pub async fn events_until_log(&mut self, needle: &str) -> Vec<Event> {
        let mut events = Vec::new();

        loop {
            let event = self.next_event().await;
            let done = matches!(&event, Event::Log(message) if message.contains(needle));
            events.push(event);

            if done {
                return events;
            }
        }
    }

    /// Messages until the server has nothing left to run or send. Pending debounce
    /// timers fire first, as the clock skips ahead to them.
    pub async fn events_until_idle(&mut self) -> Vec<Event> {
        let mut events = Vec::new();

        while let Ok(event) = tokio::time::timeout(QUIET, self.events.recv()).await {
            events.push(event.expect("the server closed the connection"));
        }

        self.check_initialized().await;

        events
    }

    async fn request(&mut self, method: &'static str, params: Value) -> Value {
        self.next_id += 1;
        let request = Request::build(method)
            .id(self.next_id)
            .params(params)
            .finish();

        let response = self
            .call(request)
            .await
            .await
            .expect("server is running")
            .expect("a request gets a response");
        let (_, result) = response.into_parts();

        result.unwrap_or_else(|error| panic!("{method} failed: {error:?}"))
    }

    /// Send a notification and wait until the server has handled it.
    async fn notify(&mut self, method: &'static str, params: Value) {
        self.call(notification(method, params))
            .await
            .await
            .expect("server is running");
    }

    async fn call(
        &mut self,
        request: Request,
    ) -> <LspService<Backend> as Service<Request>>::Future {
        self.service
            .ready()
            .await
            .expect("server is running")
            .call(request)
    }
}

/// Diagnostics messages from the publishes for `path`, oldest first.
pub fn published(events: &[Event], path: &Path) -> Vec<Vec<String>> {
    events
        .iter()
        .filter_map(|event| match event {
            Event::Published {
                path: published,
                diagnostics,
            } if published == path => Some(
                diagnostics
                    .iter()
                    .map(|diagnostic| diagnostic.message.clone())
                    .collect(),
            ),
            _ => None,
        })
        .collect()
}

/// Every path diagnostics were published for, sorted and deduplicated.
pub fn published_paths(events: &[Event]) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = events
        .iter()
        .filter_map(|event| match event {
            Event::Published { path, .. } => Some(path.clone()),
            Event::Log(_) => None,
        })
        .collect();
    paths.sort();
    paths.dedup();

    paths
}

/// The editor's side of the connection: answers the server's requests and forwards
/// its notifications to the test as [`Event`]s.
async fn play_editor(
    socket: tower_lsp_server::ClientSocket,
    events: mpsc::UnboundedSender<Event>,
    configuration: watch::Receiver<bool>,
) {
    let (mut requests, mut responses) = socket.split();
    let (reply, mut replies) = mpsc::unbounded_channel::<Response>();

    tokio::spawn(async move {
        while let Some(response) = replies.recv().await {
            if responses.send(response).await.is_err() {
                break;
            }
        }
    });

    while let Some(request) = requests.next().await {
        let (method, id, params) = request.into_parts();

        // Answered on its own task, so a held configuration answer does not stop
        // the notifications behind it.
        if let Some(id) = id {
            let reply = reply.clone();
            let mut configuration = configuration.clone();

            tokio::spawn(async move {
                let result = if method == "workspace/configuration" {
                    let _ = configuration.wait_for(|answer| *answer).await;
                    // tarus.developerMode, tarus.referenceLimit
                    json!([false, 3])
                } else {
                    Value::Null
                };

                let _ = reply.send(Response::from_parts(id, Ok(result)));
            });

            continue;
        }

        let params = params.unwrap_or(Value::Null);
        let event = match method.as_ref() {
            "textDocument/publishDiagnostics" => {
                let params: PublishDiagnosticsParams =
                    serde_json::from_value(params).expect("publishDiagnostics params");

                Event::Published {
                    path: params
                        .uri
                        .to_file_path()
                        .expect("published a file uri")
                        .into_owned(),
                    diagnostics: params.diagnostics,
                }
            }
            "window/logMessage" => {
                let params: LogMessageParams =
                    serde_json::from_value(params).expect("logMessage params");

                Event::Log(params.message)
            }
            _ => continue,
        };

        if events.send(event).is_err() {
            break;
        }
    }
}

fn notification(method: &'static str, params: Value) -> Request {
    Request::build(method).params(params).finish()
}

fn uri(path: &Path) -> Uri {
    Uri::from_file_path(path).expect("file uri")
}

fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => "rust",
        Some("ts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js") => "javascript",
        _ => "plaintext",
    }
}
