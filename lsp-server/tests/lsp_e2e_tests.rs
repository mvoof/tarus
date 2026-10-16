//! End-to-end tests of the server wiring in `server.rs`: the order in which the
//! startup scan, editor notifications, debounced re-indexes and publishes run.
//! Each test covers one flow that has broken before. What the index and the
//! diagnostics compute is covered by the other integration tests.

mod lsp_harness;

use std::time::Duration;

use lsp_harness::{published, published_paths, TestServer, Workspace};
use lsp_server::constants::DEBOUNCE_MS;

const BACKEND: &str = "src-tauri/src/lib.rs";
const APP: &str = "src/app.ts";

const GREET_COMMAND: &str = "#[tauri::command]\nfn greet() {}\n";

/// A frontend file that invokes `command`.
fn invoking(command: &str) -> String {
    format!("import {{ invoke }} from \"@tauri-apps/api/core\";\ninvoke(\"{command}\");\n")
}

fn not_defined(command: &str) -> String {
    format!("Command '{command}' is not defined in Rust backend")
}

/// Less than the debounce, so an edit after this wait restarts it.
fn within_debounce() -> Duration {
    Duration::from_millis(DEBOUNCE_MS * 2 / 3)
}

#[tokio::test(start_paused = true)]
async fn a_file_edited_while_the_server_starts_is_indexed_from_the_editor_text() {
    let workspace = Workspace::new(&[(BACKEND, GREET_COMMAND), (APP, &invoking("missing"))]);
    let app = workspace.path(APP);

    let mut server = TestServer::initialize(workspace.root()).await;
    server.send_initialized().await;

    // The scan waits for the configuration answer, so the file is opened and its
    // edit indexed before the scan reads any file. The disk keeps the old text.
    server.open(&app, &invoking("missing")).await;
    server.change(&app, 2, &invoking("greet")).await;
    server.events_until_idle().await;

    server.answer_configuration();
    let events = server
        .events_until_log(lsp_harness::INDEXING_COMPLETE)
        .await;

    assert_eq!(published(&events, &app), vec![Vec::<String>::new()]);
}

#[tokio::test(start_paused = true)]
async fn nothing_is_published_before_the_startup_scan_finishes() {
    let workspace = Workspace::new(&[(BACKEND, GREET_COMMAND), (APP, &invoking("greet"))]);
    let backend = workspace.path(BACKEND);
    let app = workspace.path(APP);

    let mut server = TestServer::initialize(workspace.root()).await;
    server.send_initialized().await;

    // A re-index before the scan has indexed `greet`: diagnostics computed now
    // would warn that it is not defined.
    server.open(&app, &invoking("greet")).await;
    server
        .change(&app, 2, &format!("{}\n", invoking("greet")))
        .await;
    let before_scan = server.events_until_idle().await;

    assert!(
        published_paths(&before_scan).is_empty(),
        "published before the scan: {before_scan:?}"
    );

    server.answer_configuration();
    let events = server
        .events_until_log(lsp_harness::INDEXING_COMPLETE)
        .await;

    let mut indexed = vec![backend, app.clone()];
    indexed.sort();

    assert_eq!(published_paths(&events), indexed);
    assert_eq!(published(&events, &app), vec![Vec::<String>::new()]);
}

#[tokio::test(start_paused = true)]
async fn edits_within_the_debounce_publish_once_for_the_final_text() {
    let workspace = Workspace::new(&[(BACKEND, GREET_COMMAND), (APP, &invoking("greet"))]);
    let app = workspace.path(APP);

    let (mut server, _) = TestServer::start(workspace.root()).await;
    server.open(&app, &invoking("greet")).await;
    server.events_until_idle().await;

    server.change(&app, 2, &invoking("one")).await;
    tokio::time::advance(within_debounce()).await;
    server.change(&app, 3, &invoking("two")).await;
    tokio::time::advance(within_debounce()).await;
    server.change(&app, 4, &invoking("three")).await;
    let events = server.events_until_idle().await;

    assert_eq!(published(&events, &app), vec![vec![not_defined("three")]]);
}

#[tokio::test(start_paused = true)]
async fn a_file_deleted_while_open_has_no_diagnostics_once_closed() {
    let workspace = Workspace::new(&[(BACKEND, GREET_COMMAND), (APP, &invoking("missing"))]);
    let app = workspace.path(APP);

    let (mut server, startup) = TestServer::start(workspace.root()).await;
    assert_eq!(
        published(&startup, &app),
        vec![vec![not_defined("missing")]]
    );

    server.open(&app, &invoking("missing")).await;
    workspace.remove(APP);
    server.watched_file_deleted(&app).await;
    server.close(&app).await;
    let events = server.events_until_idle().await;

    // Opening republishes the warning; closing clears it, and nothing follows.
    assert_eq!(
        published(&events, &app),
        vec![vec![not_defined("missing")], Vec::new()],
        "{events:?}"
    );
}

#[tokio::test(start_paused = true)]
async fn changing_a_constant_republishes_the_file_that_uses_it() {
    const NAMES: &str = "src/names.ts";
    const LISTENER: &str = "src/listener.ts";

    let workspace = Workspace::new(&[
        (
            BACKEND,
            "fn start(app: &AppHandle) {\n    app.emit(\"run://a\", ()).ok();\n}\n",
        ),
        (NAMES, "export const RUN = \"run://a\";\n"),
        (
            LISTENER,
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\n",
        ),
    ]);
    let names = workspace.path(NAMES);
    let listener = workspace.path(LISTENER);

    let (mut server, startup) = TestServer::start(workspace.root()).await;
    assert_eq!(published(&startup, &listener), vec![Vec::<String>::new()]);

    server
        .open(&names, "export const RUN = \"run://a\";\n")
        .await;
    server
        .change(&names, 2, "export const RUN = \"run://b\";\n")
        .await;
    let events = server.events_until_idle().await;

    assert_eq!(
        published(&events, &listener).last(),
        Some(&vec![
            "Event 'run://b' is listened for but never emitted".to_string()
        ]),
        "{events:?}"
    );
}
