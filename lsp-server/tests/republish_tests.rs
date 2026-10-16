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
            "import { listen } from \"@tauri-apps/api/event\";\nimport { PREFIX } from \"./base\";\nexport const RUN = \"run://a\";\nlisten(PREFIX, () => {});\n",
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
        "import { listen } from \"@tauri-apps/api/event\";\nimport { PREFIX } from \"./base\";\nexport const RUN = \"run://b\";\nlisten(PREFIX, () => {});\n",
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
    // `use.ts` and `helpers.ts` both use `RUN` (changed in `names.ts`), and `use.ts`
    // calls `send` from `helpers.ts`, whose editor text holds an unindexed edit
    // turning `send` into a forwarding helper. Whichever of the two is re-parsed
    // first, `use.ts` must end up with the call through `send` resolved.
    let mut workspace = Workspace::new(&[
        ("src/names.ts", "export const RUN = \"run://a\";\n"),
        (
            "src/helpers.ts",
            "import { RUN } from \"./names\";\nimport { emit } from \"@tauri-apps/api/event\";\nexport const send = (event: string) => event;\nemit(RUN);\n",
        ),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nimport { send } from \"./helpers\";\nlisten(RUN, () => {});\nsend(\"run://sent\");\n",
        ),
    ]);
    workspace.type_unindexed(
        "src/helpers.ts",
        "import { RUN } from \"./names\";\nimport { emit } from \"@tauri-apps/api/event\";\nexport const send = (event: string) => emit(event);\nemit(RUN);\n",
    );

    workspace.edit("src/names.ts", "export const RUN = \"run://b\";\n");

    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src/helpers.ts"), test_path("src/use.ts")]
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

#[test]
fn a_rust_file_that_does_not_parse_keeps_its_constants_until_it_does() {
    // A half-typed string must not replace the constant every file using it
    // resolves through. Unlike the frontend, the rest of the file is still indexed.
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

    workspace.edit(
        "src-tauri/src/events.rs",
        "pub const EVENT_RUN: &str = \"run://c\";\npub const EVENT_STOP: &str = \"run://b",
    );

    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src-tauri/src/run.rs")]
    );
    assert!(workspace
        .index
        .get_all_rust_constants()
        .contains_key("EVENT_RUN"));
}

#[test]
fn a_broken_edit_republishes_nothing() {
    // Nothing in the index changed, so neither the file nor those sharing a name
    // with it need fresh diagnostics.
    let mut workspace = Workspace::new(&[
        (
            "src/emit.ts",
            "import { emit } from \"@tauri-apps/api/event\";\nemit(\"run://a\");\n",
        ),
        (
            "src/listen.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nlisten(\"run://a\", () => {});\n",
        ),
    ]);

    let files = workspace.edit(
        "src/emit.ts",
        "import { emit } from \"@tauri-apps/api/event\";\nemit(\"run://a\");\nemit(\"x);\n",
    );

    assert!(files.is_empty(), "{files:?}");
}

#[test]
fn deleting_the_file_whose_command_schema_is_in_use_falls_back_to_another() {
    // Both files define `greet`; `old.rs` was indexed last, so its schema is in use.
    let command = "#[tauri::command]\nfn greet(name: String) -> String { name }\n";
    let mut workspace = Workspace::new(&[("src-tauri/src/new.rs", command)]);

    workspace.edit("src-tauri/src/old.rs", command);
    workspace.delete("src-tauri/src/old.rs");

    let schema = workspace.index.get_schema("greet");
    assert_eq!(
        schema.map(|schema| schema.source_path),
        Some(test_path("src-tauri/src/new.rs"))
    );
}

#[test]
fn a_file_that_does_not_parse_keeps_its_same_named_helpers_ambiguous() {
    // `relay` both listens to and emits its parameter: two helpers under one name,
    // which no call site can be attributed to. A broken edit must restore both.
    let helper = "import { emit, listen } from \"@tauri-apps/api/event\";\nexport function relay(e: string) {\n  listen(e, () => {});\n  emit(e);\n}\n";
    let mut workspace = Workspace::new(&[("src/relay.ts", helper)]);
    assert!(!workspace.index.get_all_forwarders().contains_key("relay"));

    workspace.edit("src/relay.ts", &format!("{helper}emit(\"x);\n"));

    assert!(!workspace.index.get_all_forwarders().contains_key("relay"));
}

#[test]
fn a_rust_file_with_syntax_errors_is_indexed_from_what_parsed() {
    // A typo, or syntax tree-sitter-rust does not know, must not hide every command
    // in the file: not when it is first indexed, nor when an edit adds one.
    let mut workspace = Workspace::new(&[(
        "src-tauri/src/commands.rs",
        "#[tauri::command]\nfn greet(name: String) -> String { name }\nfn broken( {}\n",
    )]);
    assert!(workspace.index.get_schema("greet").is_some());

    workspace.edit(
        "src-tauri/src/commands.rs",
        "#[tauri::command]\nfn greet(name: String) -> String { name }\n#[tauri::command]\nfn bye() {}\nfn broken( {}\n",
    );

    assert!(workspace.index.get_schema("bye").is_some());
}

#[test]
fn a_refresh_keeps_the_edited_file_s_value_of_a_shared_constant_name() {
    // Both files declare `EVENT` and use it. Re-parsing the other file as a
    // dependent must not hand the name back to its value, wave after wave.
    let mut workspace = Workspace::new(&[
        (
            "src/a.ts",
            "import { emit } from \"@tauri-apps/api/event\";\nexport const EVENT = \"run://a\";\nemit(EVENT);\n",
        ),
        (
            "src/b.ts",
            "import { emit } from \"@tauri-apps/api/event\";\nexport const EVENT = \"run://b\";\nemit(EVENT);\n",
        ),
    ]);

    let edited = "import { emit } from \"@tauri-apps/api/event\";\nexport const EVENT = \"run://c\";\nemit(EVENT);\n";
    workspace.type_unindexed("src/a.ts", edited);

    let reads = std::cell::Cell::new(0);
    let files = &workspace.files;
    pipeline::reindex(
        &test_path("src/a.ts"),
        edited,
        &workspace.index,
        |file: &Path| {
            reads.set(reads.get() + 1);
            files.get(file).cloned()
        },
    );

    // One wave re-parses `b.ts`, the next finds nothing changed.
    assert!(reads.get() <= 2, "{} reads", reads.get());
    assert_eq!(
        workspace.index.get_all_js_constants().get("EVENT"),
        Some(&"run://c".to_string())
    );
}

#[test]
fn a_file_that_does_not_parse_does_not_take_over_a_shared_constant_name() {
    // `new.ts` was indexed last, so its `EVENT` is in use; a broken edit of
    // `old.ts` stores nothing and must not change that.
    let mut workspace = Workspace::new(&[("src/old.ts", "export const EVENT = \"run://old\";\n")]);
    workspace.edit("src/new.ts", "export const EVENT = \"run://new\";\n");

    workspace.edit(
        "src/old.ts",
        "import { emit } from \"@tauri-apps/api/event\";\nexport const EVENT = \"run://old\";\nemit(\"x);\n",
    );

    assert_eq!(
        workspace.index.get_all_js_constants().get("EVENT"),
        Some(&"run://new".to_string())
    );
}

/// Re-index `path` in `workspace` and return the files the refresh read.
fn files_read_by_edit(workspace: &mut Workspace, path: &str, content: &str) -> Vec<PathBuf> {
    workspace.type_unindexed(path, content);

    let read = std::cell::RefCell::new(Vec::new());
    let files = &workspace.files;
    pipeline::reindex(
        &test_path(path),
        content,
        &workspace.index,
        |file: &Path| {
            read.borrow_mut().push(file.to_path_buf());
            files.get(file).cloned()
        },
    );

    let mut read = read.into_inner();
    read.sort();
    read.dedup();
    read
}

#[test]
fn a_file_that_only_mentions_a_changed_name_is_not_reparsed() {
    // `RUN` in a comment, a string and an unrelated local is not a use of the
    // constant: only `use.ts`, which resolved a call through it, is read.
    let mut workspace = Workspace::new(&[
        ("src/names.ts", "export const RUN = \"run://a\";\n"),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\n",
        ),
        (
            "src/mentions.ts",
            "// RUN is documented here\nconst label = \"RUN\";\nfunction f(RUN: string) { return RUN; }\n",
        ),
    ]);

    let read = files_read_by_edit(
        &mut workspace,
        "src/names.ts",
        "export const RUN = \"run://b\";\n",
    );

    assert_eq!(read, vec![test_path("src/use.ts")]);
    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn a_constant_declared_later_reaches_the_call_that_could_not_resolve_it() {
    let mut workspace = Workspace::new(&[
        ("src/names.ts", "export const OTHER = \"x\";\n"),
        (
            "src/use.ts",
            "import { listen } from \"@tauri-apps/api/event\";\nimport { RUN } from \"./names\";\nlisten(RUN, () => {});\n",
        ),
    ]);
    assert!(workspace.event_files("run://a").is_empty());

    workspace.edit(
        "src/names.ts",
        "export const OTHER = \"x\";\nexport const RUN = \"run://a\";\n",
    );

    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn a_helper_declared_later_reaches_the_calls_to_it() {
    let mut workspace = Workspace::new(&[
        (
            "src/helpers.ts",
            "export const send = (event: string) => event;\n",
        ),
        (
            "src/use.ts",
            "import { send } from \"./helpers\";\nsend(\"run://sent\");\n",
        ),
    ]);
    assert!(workspace.event_files("run://sent").is_empty());

    workspace.edit(
        "src/helpers.ts",
        "import { emit } from \"@tauri-apps/api/event\";\nexport const send = (event: string) => emit(event);\n",
    );

    assert_eq!(
        workspace.event_files("run://sent"),
        vec![test_path("src/use.ts")]
    );
}

#[test]
fn a_changed_rust_constant_reaches_a_call_through_a_reference_to_it() {
    // `run.rs` emits `RUN`, which is `BASE` from another file: changing `BASE`
    // changes `RUN` though `run.rs` never names `BASE`.
    let mut workspace = Workspace::new(&[
        (
            "src-tauri/src/base.rs",
            r#"pub const BASE: &str = "run://a";"#,
        ),
        (
            "src-tauri/src/names.rs",
            "pub const RUN: &str = crate::base::BASE;",
        ),
        (
            "src-tauri/src/run.rs",
            "fn start(app: &AppHandle) { app.emit(crate::names::RUN, ()).ok(); }",
        ),
        ("src-tauri/src/other.rs", "fn other() {}"),
    ]);
    assert_eq!(
        workspace.event_files("run://a"),
        vec![test_path("src-tauri/src/run.rs")]
    );

    let read = files_read_by_edit(
        &mut workspace,
        "src-tauri/src/base.rs",
        r#"pub const BASE: &str = "run://b";"#,
    );

    assert_eq!(read, vec![test_path("src-tauri/src/run.rs")]);
    assert_eq!(
        workspace.event_files("run://b"),
        vec![test_path("src-tauri/src/run.rs")]
    );
}

#[test]
fn renaming_one_constant_does_not_hand_a_shared_name_to_the_edited_file() {
    // Both files declare `EVENT`; `b.ts` was indexed last, so its value is in use.
    // Editing another constant in `a.ts` must leave `EVENT` with `b.ts`.
    let mut workspace = Workspace::new(&[(
        "src/a.ts",
        "export const EVENT = \"run://a\";\nexport const X = \"x\";\n",
    )]);
    workspace.edit("src/b.ts", "export const EVENT = \"run://b\";\n");

    workspace.edit(
        "src/a.ts",
        "export const EVENT = \"run://a\";\nexport const X = \"y\";\n",
    );

    assert_eq!(
        workspace.index.get_all_js_constants().get("EVENT"),
        Some(&"run://b".to_string())
    );
}
