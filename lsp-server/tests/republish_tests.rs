//! Which files get fresh diagnostics after one file is re-indexed.

mod common_paths;

use common_paths::test_path;
use lsp_server::indexer::{DiscoveredGenerator, GeneratorKind, ProjectIndex};
use lsp_server::pipeline::{self, IndexSnapshot};
use lsp_server::syntax::EntityType;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

    let files = pipeline::refresh_after(&backend, &before, &index, |_: &Path| None);
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

    let files = pipeline::refresh_after(&backend, &before, &index, |_: &Path| None);
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

    let mut files = pipeline::refresh_after(&backend, &before, &index, |_: &Path| None);
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

    let files = pipeline::refresh_after(&backend, &before, &index, |_: &Path| None);
    assert!(files.contains(&listener), "{files:?}");
    assert!(!files.contains(&caller), "{files:?}");
}

// ===========================================================================
// Files whose parse depended on a changed constant or helper
// ===========================================================================

/// A workspace held in memory: what `refresh_after` reads dependents from.
struct Workspace {
    index: ProjectIndex,
    files: HashMap<PathBuf, String>,
}

impl Workspace {
    fn new(files: &[(&str, &str)]) -> Self {
        let workspace = Self {
            index: ProjectIndex::new(),
            files: files
                .iter()
                .map(|(path, content)| (test_path(path), (*content).to_string()))
                .collect(),
        };

        // The two passes the server runs: constants and helpers first, then parsing.
        for (path, content) in &workspace.files {
            pipeline::process_file_content(path, content, &workspace.index);
        }
        for (path, content) in &workspace.files {
            pipeline::process_file_content(path, content, &workspace.index);
        }

        workspace
    }

    /// Edit one file the way the server does and return the files it republishes.
    fn edit(&mut self, path: &str, content: &str) -> Vec<PathBuf> {
        let path = test_path(path);
        self.files.insert(path.clone(), content.to_string());

        let files = &self.files;
        pipeline::reindex(&path, content, &self.index, |file: &Path| {
            files.get(file).cloned()
        })
    }

    /// Run a re-index of `path` that was queued earlier and only gets to run now,
    /// after the editor's text moved on: it must index the text as it is now.
    fn late_reindex(&self, path: &str) -> Vec<PathBuf> {
        let files = &self.files;
        pipeline::reindex_current(&test_path(path), &self.index, |file: &Path| {
            files
                .get(file)
                .cloned()
                .ok_or_else(|| std::io::ErrorKind::NotFound.into())
        })
    }

    /// Delete a file the way the server does and return the files it republishes.
    fn delete(&mut self, path: &str) -> Vec<PathBuf> {
        let path = test_path(path);
        self.files.remove(&path);

        let files = &self.files;
        pipeline::forget(&path, &self.index, |file: &Path| files.get(file).cloned())
    }

    /// The file disappears from the editor and the disk without the server being
    /// told: opened, deleted on disk, then its tab closed.
    fn vanish(&mut self, path: &str) {
        self.files.remove(&test_path(path));
    }

    /// Change a file in the editor without indexing it yet: an edit still waiting
    /// on its own debounce.
    fn type_unindexed(&mut self, path: &str, content: &str) {
        self.files.insert(test_path(path), content.to_string());
    }

    fn event_files(&self, name: &str) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self
            .index
            .get_locations(EntityType::Event, name)
            .into_iter()
            .map(|location| location.path)
            .collect();
        files.sort();
        files.dedup();
        files
    }
}

#[test]
fn changing_a_rust_constant_reparses_the_files_that_use_it() {
    let mut workspace = Workspace::new(&[
        (
            "src-tauri/src/events.rs",
            r#"pub const EVENT_RUN: &str = "run://a";"#,
        ),
        (
            "src-tauri/src/run.rs",
            "fn start(app: &AppHandle) { app.emit(crate::events::EVENT_RUN, ()).ok(); }",
        ),
        ("src-tauri/src/other.rs", "fn other() {}"),
    ]);
    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src-tauri/src/run.rs")]
    );

    let files = workspace.edit(
        "src-tauri/src/events.rs",
        r#"pub const EVENT_RUN: &str = "run://b";"#,
    );

    assert!(workspace.event_files("run://a").is_empty());
    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src-tauri/src/run.rs")]
    );
    assert!(
        files.contains(&test_path("src-tauri/src/run.rs")),
        "{files:?}"
    );
    assert!(
        !files.contains(&test_path("src-tauri/src/other.rs")),
        "{files:?}"
    );
}

#[test]
fn changing_a_typescript_constant_reparses_the_files_that_use_it() {
    let mut workspace = Workspace::new(&[
        ("src/names.ts", r#"export const RUN = "run://a";"#),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\n",
        ),
    ]);
    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src/use.ts")]
    );

    workspace.edit("src/names.ts", r#"export const RUN = "run://b";"#);

    assert!(workspace.event_files("run://a").is_empty());
    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn adding_a_forwarding_helper_reparses_its_call_sites() {
    let mut workspace = Workspace::new(&[
        (
            "src/events.ts",
            "export const listenTo = (event: string) => event;\n",
        ),
        (
            "src/use.ts",
            "import { listenTo } from \"./events\";\nlistenTo(\"run://a\");\n",
        ),
    ]);
    assert!(workspace.event_files("run://a").is_empty());

    workspace.edit(
        "src/events.ts",
        "import { listen } from \"@tauri-apps/api/event\";\nexport const listenTo = (event: string) => listen(event, () => {});\n",
    );

    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn an_edit_that_changes_no_constant_reparses_nothing_else() {
    let mut workspace = Workspace::new(&[
        (
            "src-tauri/src/events.rs",
            r#"pub const EVENT_RUN: &str = "run://a";"#,
        ),
        (
            "src-tauri/src/run.rs",
            "fn start(app: &AppHandle) { app.emit(crate::events::EVENT_RUN, ()).ok(); }",
        ),
    ]);

    let files = workspace.edit(
        "src-tauri/src/events.rs",
        "// a comment\npub const EVENT_RUN: &str = \"run://a\";",
    );

    assert_eq!(files, vec![test_path("src-tauri/src/events.rs")]);
}

#[test]
fn regenerated_bindings_refresh_every_typed_call_site() {
    let bindings = test_path("src/bindings.ts");
    let mut workspace = Workspace::new(&[
        (
            "src/use.ts",
            "import { invoke } from \"@tauri-apps/api/core\";\ninvoke(\"greet\");\n",
        ),
        ("src/plain.ts", "export const nothing = 1;\n"),
    ]);
    workspace
        .index
        .set_generator_bindings(vec![DiscoveredGenerator {
            kind: GeneratorKind::TsRs,
            output_path: bindings.clone(),
            is_directory: false,
        }]);

    let files = workspace.edit("src/bindings.ts", "export type User = { name: string };\n");

    assert!(files.contains(&test_path("src/use.ts")), "{files:?}");
    assert!(!files.contains(&test_path("src/plain.ts")), "{files:?}");
}

#[test]
fn a_file_that_does_not_parse_keeps_its_constants_until_it_does() {
    // The edit changes `RUN` while a half-typed call breaks the rest of the file.
    // Read from a broken tree, constants can be dropped or mangled, so the old one
    // stays and its users keep resolving; the fix then passes the new value on.
    let mut workspace = Workspace::new(&[
        ("src/names.ts", "export const RUN = \"run://a\";\n"),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\n",
        ),
    ]);

    let files = workspace.edit(
        "src/names.ts",
        "import { listen } from \"@tauri-apps/api/event\";\nexport const RUN = \"run://b\";\nlisten(\"x, () => {});\n",
    );

    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src/use.ts")]
    );
    // The broken file keeps its last good diagnostics rather than being cleared.
    assert!(!files.contains(&test_path("src/names.ts")), "{files:?}");

    let files = workspace.edit(
        "src/names.ts",
        "import { listen } from \"@tauri-apps/api/event\";\nexport const RUN = \"run://b\";\nlisten(\"x\", () => {});\n",
    );

    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src/use.ts")]
    );
    assert!(files.contains(&test_path("src/use.ts")), "{files:?}");
}

#[test]
fn deleting_a_file_keeps_a_command_schema_another_file_took_over() {
    // A rename the watcher reports as CREATED(new.rs) before DELETED(old.rs).
    let command = "#[tauri::command]\nfn greet(name: String) -> String { name }\n";
    let mut workspace = Workspace::new(&[("src-tauri/src/old.rs", command)]);

    workspace.edit("src-tauri/src/new.rs", command);
    workspace.delete("src-tauri/src/old.rs");

    let schema = workspace.index.get_schema("greet");
    assert_eq!(
        schema.map(|schema| schema.source_path),
        Some(test_path("src-tauri/src/new.rs"))
    );
}

#[test]
fn a_changed_name_matches_whole_identifiers_only() {
    let mut workspace = Workspace::new(&[
        (
            "src-tauri/src/events.rs",
            r#"pub const ID: &str = "run://a";"#,
        ),
        (
            "src-tauri/src/run.rs",
            "fn start(app: &AppHandle) { app.emit(ID, ()).ok(); }",
        ),
        (
            "src-tauri/src/other.rs",
            "fn VALID_ID() {} fn IDENTITY() {} fn id() {}",
        ),
    ]);

    let files = workspace.edit(
        "src-tauri/src/events.rs",
        r#"pub const ID: &str = "run://b";"#,
    );

    assert!(
        files.contains(&test_path("src-tauri/src/run.rs")),
        "{files:?}"
    );
    assert!(
        !files.contains(&test_path("src-tauri/src/other.rs")),
        "{files:?}"
    );
}

#[test]
fn a_dependent_reparsed_with_a_pending_edit_passes_its_own_change_on() {
    // `names.ts` is re-parsed because `base.ts` changed, and its editor text already
    // holds an unindexed edit to `RUN`. That edit lands in the index now, so its own
    // debounce will find nothing to pass on: `use.ts` must be reached in this pass.
    let mut workspace = Workspace::new(&[
        ("src/base.ts", "export const PREFIX = \"base://a\";\n"),
        (
            "src/names.ts",
            "import { PREFIX } from \"./base\";\nexport const RUN = \"run://a\";\nexport const OTHER = PREFIX;\n",
        ),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\n",
        ),
    ]);
    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src/use.ts")]
    );

    workspace.type_unindexed(
        "src/names.ts",
        "import { PREFIX } from \"./base\";\nexport const RUN = \"run://b\";\nexport const OTHER = PREFIX;\n",
    );
    workspace.edit("src/base.ts", "export const PREFIX = \"base://b\";\n");

    assert!(workspace.event_files("run://a").is_empty());
    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn deleting_a_file_drops_its_constants_and_reparses_their_users() {
    let mut workspace = Workspace::new(&[
        (
            "src-tauri/src/events.rs",
            r#"pub const EVENT_RUN: &str = "run://a";"#,
        ),
        (
            "src-tauri/src/run.rs",
            "fn start(app: &AppHandle) { app.emit(crate::events::EVENT_RUN, ()).ok(); }",
        ),
    ]);
    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src-tauri/src/run.rs")]
    );

    let files = workspace.delete("src-tauri/src/events.rs");

    assert!(workspace.event_files("run://a").is_empty());
    assert!(!workspace
        .index
        .get_all_rust_constants()
        .contains_key("EVENT_RUN"));
    assert!(
        files.contains(&test_path("src-tauri/src/run.rs")),
        "{files:?}"
    );
    assert!(
        !files.contains(&test_path("src-tauri/src/events.rs")),
        "{files:?}"
    );
}

#[test]
fn a_late_reindex_indexes_the_current_text_not_the_one_it_was_queued_with() {
    // Edits v2 and v3 both waited on the lock; v3 ran first and v2 runs last. If v2
    // stored its own text, the index would end up behind the editor.
    let mut workspace = Workspace::new(&[(
        "src/use.ts",
        "import { listen } from \"@tauri-apps/api/event\";\nlisten(\"run://v1\", () => {});\n",
    )]);

    workspace.type_unindexed(
        "src/use.ts",
        "import { listen } from \"@tauri-apps/api/event\";\nlisten(\"run://v3\", () => {});\n",
    );
    workspace.late_reindex("src/use.ts");

    assert!(workspace.event_files("run://v1").is_empty());
    assert_eq!(
        workspace.event_files("run://v3"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn a_file_reparsed_in_one_wave_is_reparsed_again_when_a_later_wave_affects_it() {
    // `use.ts` mentions `RUN` (changed in `names.ts`) and calls `send` from
    // `helpers.ts`, whose editor text holds an unindexed edit turning `send` into a
    // forwarding helper. Whichever of the two is re-parsed first, `use.ts` must end
    // up with the call through `send` resolved.
    let mut workspace = Workspace::new(&[
        ("src/names.ts", "export const RUN = \"run://a\";\n"),
        ("src/helpers.ts", "export const send = (event: string) => event;\n"),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nimport { send } from \"./helpers\";\nlisten(RUN, () => {});\nsend(\"run://sent\");\n",
        ),
        (
            "src/mentions.ts",
            "import { RUN } from \"./names\";\nexport const again = RUN;\n",
        ),
    ]);
    workspace.type_unindexed(
        "src/helpers.ts",
        "import { RUN } from \"./names\";\nimport { emit } from \"@tauri-apps/api/event\";\nexport const send = (event: string) => emit(event);\nexport const unused = RUN;\n",
    );

    workspace.edit("src/names.ts", "export const RUN = \"run://b\";\n");

    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src/use.ts")]
    );
    assert_eq!(
        workspace.event_files("run://sent"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn deleting_a_file_drops_its_command_schemas() {
    // Hover, completion and argument checks read schemas; a deleted command must
    // not keep offering its old signature.
    let mut workspace = Workspace::new(&[(
        "src-tauri/src/commands.rs",
        "#[tauri::command]\nfn greet(name: String) -> String { name }\n",
    )]);
    assert!(workspace.index.get_schema("greet").is_some());

    workspace.delete("src-tauri/src/commands.rs");

    assert!(workspace.index.get_schema("greet").is_none());
}

#[test]
fn a_reindex_of_a_file_that_no_longer_exists_drops_it() {
    // Closing the tab of a file already deleted on disk re-indexes it from a text
    // that is gone: it must leave the index, constants included.
    let mut workspace = Workspace::new(&[
        (
            "src-tauri/src/events.rs",
            r#"pub const EVENT_RUN: &str = "run://a";"#,
        ),
        (
            "src-tauri/src/run.rs",
            "fn start(app: &AppHandle) { app.emit(crate::events::EVENT_RUN, ()).ok(); }",
        ),
    ]);

    workspace.vanish("src-tauri/src/events.rs");
    let files = workspace.late_reindex("src-tauri/src/events.rs");

    assert!(!workspace
        .index
        .get_all_rust_constants()
        .contains_key("EVENT_RUN"));
    assert!(workspace.event_files("run://a").is_empty());
    assert!(
        files.contains(&test_path("src-tauri/src/run.rs")),
        "{files:?}"
    );
}

#[test]
fn a_dependent_that_does_not_parse_keeps_its_last_good_diagnostics() {
    // Same rule as for the edited file itself: a file mid-edit is not republished.
    let mut workspace = Workspace::new(&[
        ("src/names.ts", "export const RUN = \"run://a\";\n"),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\n",
        ),
    ]);
    workspace.type_unindexed(
        "src/use.ts",
        "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\nlisten(\"x, () => {});\n",
    );

    let files = workspace.edit("src/names.ts", "export const RUN = \"run://b\";\n");

    assert!(!files.contains(&test_path("src/use.ts")), "{files:?}");
}

#[test]
fn a_reindex_of_a_file_that_no_longer_exists_clears_its_diagnostics() {
    // Nothing else publishes for a file deleted on disk while open: its own entry in
    // the result is what clears its old warnings.
    let mut workspace = Workspace::new(&[(
        "src/use.ts",
        "import { listen } from \"@tauri-apps/api/event\";\nlisten(\"run://a\", () => {});\n",
    )]);

    workspace.vanish("src/use.ts");
    let files = workspace.late_reindex("src/use.ts");

    assert!(files.contains(&test_path("src/use.ts")), "{files:?}");
}

#[test]
fn a_file_that_cannot_be_read_keeps_its_index_entries() {
    // A read that fails for any reason but "not found" — invalid UTF-8, no
    // permission, caught mid-save — says nothing about the file's contents: its
    // commands and events stay until a read succeeds.
    let workspace = Workspace::new(&[(
        "src-tauri/src/commands.rs",
        "#[tauri::command]\nfn greet(name: String) -> String { name }\n",
    )]);
    let path = test_path("src-tauri/src/commands.rs");

    let files = pipeline::reindex_current(&path, &workspace.index, |_: &Path| {
        Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
    });

    assert!(files.is_empty(), "{files:?}");
    assert!(workspace.index.get_schema("greet").is_some());
    assert!(workspace.index.get_parse_error(&path).is_some());
}
