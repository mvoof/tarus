//! Which files get fresh diagnostics after one file is re-indexed.

mod common_paths;

use common_paths::test_path;
use lsp_server::indexer::ProjectIndex;
use lsp_server::pipeline::{self, IndexSnapshot};

const LISTENER: &str = r#"
import { listen } from "@tauri-apps/api/event";
listen("run://typo", () => {});
"#;

const UNRESOLVED_EMIT: &str = r#"
const EVENT_RUN: &str = concat!("run", "://x");
fn start(app: &AppHandle) {
    app.emit(EVENT_RUN, ()).ok();
}
"#;

const RESOLVED_EMIT: &str = r#"
const EVENT_RUN: &str = "run://x";
fn start(app: &AppHandle) {
    app.emit(EVENT_RUN, ()).ok();
}
"#;

#[test]
fn a_file_that_stops_being_dynamic_reaches_the_files_it_silenced() {
    // `listener.ts` shares no event with `run.rs`, yet its "never emitted" warning
    // was silenced by the unresolved emit there; fixing that emit must reach it.
    let index = ProjectIndex::new();
    let listener = test_path("src/listener.ts");
    let backend = test_path("src-tauri/src/run.rs");

    pipeline::process_file_content(&listener, LISTENER, &index);
    pipeline::process_file_content(&backend, UNRESOLVED_EMIT, &index);

    let before = IndexSnapshot::before(&backend, &index);
    pipeline::process_file_content(&backend, RESOLVED_EMIT, &index);

    let files = pipeline::files_to_republish(&backend, &before, &index);
    assert!(files.contains(&listener), "{files:?}");
    assert!(files.contains(&backend), "{files:?}");
}

#[test]
fn a_file_that_becomes_dynamic_reaches_the_files_it_silences() {
    let index = ProjectIndex::new();
    let listener = test_path("src/listener.ts");
    let backend = test_path("src-tauri/src/run.rs");

    pipeline::process_file_content(&listener, LISTENER, &index);
    pipeline::process_file_content(&backend, RESOLVED_EMIT, &index);

    let before = IndexSnapshot::before(&backend, &index);
    pipeline::process_file_content(&backend, UNRESOLVED_EMIT, &index);

    let files = pipeline::files_to_republish(&backend, &before, &index);
    assert!(files.contains(&listener), "{files:?}");
}

#[test]
fn an_unrelated_edit_republishes_only_files_sharing_a_name() {
    let index = ProjectIndex::new();
    let listener = test_path("src/listener.ts");
    let backend = test_path("src-tauri/src/run.rs");
    let partner = test_path("src/partner.ts");

    pipeline::process_file_content(&listener, LISTENER, &index);
    pipeline::process_file_content(&backend, RESOLVED_EMIT, &index);
    pipeline::process_file_content(
        &partner,
        "import { listen } from \"@tauri-apps/api/event\";\nlisten(\"run://x\", () => {});\n",
        &index,
    );

    let before = IndexSnapshot::before(&backend, &index);
    pipeline::process_file_content(&backend, RESOLVED_EMIT, &index);

    let mut files = pipeline::files_to_republish(&backend, &before, &index);
    files.sort();
    let mut expected = vec![backend, partner];
    expected.sort();
    assert_eq!(files, expected);
}

#[test]
fn a_flip_skips_files_with_no_diagnostic_that_depends_on_it() {
    // Unresolved emits gate only "never emitted" (on listeners) and "defined but
    // never used" (on event definitions). A file that only invokes a command has
    // neither, so it does not need fresh diagnostics.
    let index = ProjectIndex::new();
    let listener = test_path("src/listener.ts");
    let backend = test_path("src-tauri/src/run.rs");
    let caller = test_path("src/caller.ts");

    pipeline::process_file_content(&listener, LISTENER, &index);
    pipeline::process_file_content(
        &caller,
        "import { invoke } from \"@tauri-apps/api/core\";\ninvoke(\"greet\");\n",
        &index,
    );
    pipeline::process_file_content(&backend, UNRESOLVED_EMIT, &index);

    let before = IndexSnapshot::before(&backend, &index);
    pipeline::process_file_content(&backend, RESOLVED_EMIT, &index);

    let files = pipeline::files_to_republish(&backend, &before, &index);
    assert!(files.contains(&listener), "{files:?}");
    assert!(!files.contains(&caller), "{files:?}");
}
