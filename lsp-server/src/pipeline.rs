//! File processing utilities

use crate::indexer::{DynamicUsages, Forwarder, GeneratorKind, IndexKey, ProjectIndex};
use crate::parser;
use crate::parser::typescript::bindings as bindings_reader;
use crate::syntax::{Behavior, EntityType};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What the index held before one file is re-indexed: enough to tell afterwards
/// whose diagnostics the change can reach, and whose parse it invalidated.
#[derive(Debug, Clone)]
pub struct IndexSnapshot {
    file: FileState,
    names: NameState,
}

impl IndexSnapshot {
    /// Take the snapshot; call it right before re-indexing or removing `path`.
    #[must_use]
    pub fn before(path: &Path, project_index: &ProjectIndex) -> Self {
        Self {
            file: FileState::of(path, project_index),
            names: NameState::of(project_index),
        }
    }
}

/// One file's part of the index: the names it holds and the workspace-wide
/// unresolved-call flags, which is what a change to it can reach.
#[derive(Debug, Clone)]
struct FileState {
    keys: Vec<IndexKey>,
    dynamic: DynamicUsages,
}

impl FileState {
    fn of(path: &Path, project_index: &ProjectIndex) -> Self {
        Self {
            keys: project_index.get_file_keys(path),
            dynamic: project_index.dynamic_usages(),
        }
    }
}

/// Every constant and forwarding helper in the workspace: what a parse resolves
/// names through.
#[derive(Debug, Clone)]
struct NameState {
    rust_constants: HashMap<String, String>,
    js_constants: HashMap<String, String>,
    forwarders: HashMap<String, Forwarder>,
}

impl NameState {
    fn of(project_index: &ProjectIndex) -> Self {
        Self {
            rust_constants: project_index.get_all_rust_constants(),
            js_constants: project_index.get_all_js_constants(),
            forwarders: project_index.get_all_forwarders(),
        }
    }

    /// Rust and frontend names whose value differs in `after`: constants, and for
    /// the frontend also forwarding helpers.
    fn changed_to(&self, after: &Self) -> (Vec<String>, Vec<String>) {
        let rust_names = changed_names(&self.rust_constants, &after.rust_constants);
        let mut frontend_names = changed_names(&self.js_constants, &after.js_constants);
        frontend_names.extend(changed_names(&self.forwarders, &after.forwarders));

        (rust_names, frontend_names)
    }
}

/// Re-index `path` from `content`, re-parse what that invalidated, and return every
/// file whose diagnostics to publish.
///
/// The file's constants and helpers are stored before the file itself is parsed,
/// so they change even when the parse fails on a half-typed line. Their users are
/// refreshed either way — once the typo is fixed there is no change left to see.
/// A file that failed to parse is left out of the result: it keeps its last good
/// diagnostics rather than losing them mid-edit.
pub fn reindex(
    path: &Path,
    content: &str,
    project_index: &ProjectIndex,
    read_file: impl Fn(&Path) -> Option<String>,
) -> Vec<PathBuf> {
    let before = IndexSnapshot::before(path, project_index);
    let parsed = process_file_content(path, content, project_index);
    let mut files = refresh_after(path, &before, project_index, read_file);

    if !parsed {
        files.retain(|file| file != path);
    }

    files
}

/// [`reindex`] `path` from its text as it is when this runs, read through
/// `read_file` — the editor's copy for an open file, the disk's otherwise.
///
/// Re-index jobs for one file can queue up and run in any order; each indexing the
/// text it was created with would let an older edit land after a newer one. Read
/// at run time, a late job indexes the newest text, so the order stops mattering.
pub fn reindex_current(
    path: &Path,
    project_index: &ProjectIndex,
    read_file: impl Fn(&Path) -> std::io::Result<String>,
) -> Vec<PathBuf> {
    if !is_supported_file(path) {
        return Vec::new();
    }

    let read_dependent = |file: &Path| read_file(file).ok();

    match read_file(path) {
        Ok(content) => reindex(path, &content, project_index, read_dependent),

        // No text in the editor or on disk: the file is gone (deleted on disk, then
        // its tab closed). Indexing nothing would keep its old entries forever, and
        // nothing else clears its diagnostics, so it is published too — empty now.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let mut files = forget(path, project_index, read_dependent);
            files.push(path.to_path_buf());

            files
        }

        // The file is there but its text is not: invalid UTF-8, no permission, a
        // save in progress. That says nothing about what it declares, so its entries
        // stay until a read succeeds, and so do its diagnostics.
        Err(error) => {
            project_index
                .set_parse_error(path.to_path_buf(), format!("Failed to read file: {error}"));

            Vec::new()
        }
    }
}

/// Drop a deleted file from the index, re-parse what used its constants or helpers,
/// and return the other files whose diagnostics to publish.
pub fn forget(
    path: &Path,
    project_index: &ProjectIndex,
    read_file: impl Fn(&Path) -> Option<String>,
) -> Vec<PathBuf> {
    let before = IndexSnapshot::before(path, project_index);

    project_index.remove_file(path);
    // `remove_file` keeps what re-indexing replaces anyway — constants, schemas, type
    // aliases; a deleted file has no replacement, so they go here.
    project_index.remove_rust_constants_for_file(path);
    project_index.remove_js_constants_for_file(path);
    project_index.remove_schemas_for_file(path);
    project_index.remove_event_schemas_for_file(path);
    project_index.remove_type_aliases_for_file(path);

    refresh_after(path, &before, project_index, read_file)
        .into_iter()
        .filter(|file| file != path)
        .collect()
}

/// Most waves one refresh runs. Each file can change constants or helpers on its
/// first re-parse only, so real chains end far sooner; this only bounds a surprise.
const MAX_REFRESH_WAVES: usize = 64;

/// Bring the index up to date after `path` was re-indexed or removed, and return
/// every file whose diagnostics to publish.
///
/// A file that calls `emit(EVENT_X)` stores the event name `EVENT_X` resolved to
/// when it was parsed, and a call through a forwarding helper is resolved the same
/// way. So when `path` changes a constant's value or a helper, the files using it
/// hold stale names until they are parsed again: each of them is re-parsed here.
/// Only files of the same language whose text mentions a changed name are
/// touched, which keeps an ordinary edit from re-parsing anything else.
///
/// `read_file` returns a file's current text — the editor's copy for an open file.
pub fn refresh_after(
    path: &Path,
    before: &IndexSnapshot,
    project_index: &ProjectIndex,
    read_file: impl Fn(&Path) -> Option<String>,
) -> Vec<PathBuf> {
    let mut files = files_reached_from(path, &before.file, project_index);

    // A re-parsed dependent reads the editor's text, which may hold an edit still
    // waiting on its own debounce. That edit lands in the index here, so its debounce
    // will find nothing to pass on: whatever it changed is passed on in this pass,
    // wave after wave — to files already re-parsed in an earlier wave too.
    //
    // Each wave compares the names against one copy taken as it starts, so a wave
    // sees exactly what the one before it changed. This ends on its own: a file's
    // constants and helpers come from its own text alone, which does not change
    // during the pass. The wave limit is a guard, not the stopping rule.
    let mut names_before = before.names.clone();

    for wave in 0..MAX_REFRESH_WAVES {
        let names_now = NameState::of(project_index);
        let (rust_names, frontend_names) = names_before.changed_to(&names_now);

        if rust_names.is_empty() && frontend_names.is_empty() {
            break;
        }

        names_before = names_now;

        for dependent in project_index.get_indexed_paths() {
            // The changed file was just parsed; in the first wave it only mentions
            // its own new names.
            if wave == 0 && dependent == path {
                continue;
            }

            let names = if dependent.extension().is_some_and(|ext| ext == "rs") {
                &rust_names
            } else {
                &frontend_names
            };

            if names.is_empty() {
                continue;
            }

            let Some(content) = read_file(&dependent) else {
                continue;
            };

            if !names.iter().any(|name| mentions_identifier(&content, name)) {
                continue;
            }

            let dependent_before = FileState::of(&dependent, project_index);

            // A dependent mid-edit keeps its last good diagnostics, as the edited
            // file does in `reindex`.
            if !process_file_content(&dependent, &content, project_index) {
                continue;
            }

            files.extend(files_reached_from(
                &dependent,
                &dependent_before,
                project_index,
            ));
        }
    }

    // Type checks read the generated bindings when diagnostics are computed, so a
    // regenerated bindings file only needs every typed call site refreshed.
    if project_index.get_generator_for_file(path).is_some() {
        files.extend(files_with_call_sites(project_index));
    }

    files.into_iter().collect()
}

/// Whether `name` occurs in `content` as a whole identifier: `ID` in `emit(ID)`,
/// not in `VALID_ID` or `IDENTITY`. A short constant name would otherwise match
/// nearly every file and re-parse the workspace on each change.
fn mentions_identifier(content: &str, name: &str) -> bool {
    let is_identifier_char = |c: char| c.is_alphanumeric() || c == '_' || c == '$';

    content.match_indices(name).any(|(start, _)| {
        let before = content[..start].chars().next_back();
        let after = content[start + name.len()..].chars().next();

        !before.is_some_and(is_identifier_char) && !after.is_some_and(is_identifier_char)
    })
}

/// Names added, removed or given a different value between two snapshots.
fn changed_names<V: PartialEq>(
    before: &HashMap<String, V>,
    after: &HashMap<String, V>,
) -> Vec<String> {
    before
        .keys()
        .chain(after.keys())
        .filter(|name| before.get(*name) != after.get(*name))
        .cloned()
        .collect::<HashSet<String>>()
        .into_iter()
        .collect()
}

/// Files holding an `invoke`, `emit` or `listen` — the call sites type checks run on.
fn files_with_call_sites(project_index: &ProjectIndex) -> HashSet<PathBuf> {
    project_index
        .map
        .iter()
        .flat_map(|entry| {
            entry
                .value()
                .iter()
                .filter(|location| location.behavior != Behavior::Definition)
                .map(|location| location.path.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Files whose diagnostics may have changed after `path` was re-indexed: the file
/// itself and every file sharing a command or event with it, before or after.
///
/// An unresolved call has no name to share, yet it switches "never emitted" /
/// "no listeners" / "never invoked" on or off for every file at once. So when a
/// workspace-wide [`DynamicUsages`] flag differs from the snapshot, every file
/// holding a diagnostic gated by that flag is added too. Edits are debounced, so a
/// flag can still flip on each pause while a name is being typed; limiting the
/// refresh to the gated files keeps that from touching the whole workspace.
fn files_reached_from(
    path: &Path,
    before: &FileState,
    project_index: &ProjectIndex,
) -> HashSet<PathBuf> {
    let mut files: HashSet<PathBuf> = HashSet::from([path.to_path_buf()]);
    let after = project_index.get_file_keys(path);

    for key in before.keys.iter().chain(after.iter()) {
        for location in project_index.get_locations_for_key(key) {
            files.insert(location.path);
        }
    }

    let now = project_index.dynamic_usages();
    let flipped = DynamicUsages {
        invokes: now.invokes != before.dynamic.invokes,
        emitters: now.emitters != before.dynamic.emitters,
        listeners: now.listeners != before.dynamic.listeners,
    };

    if !flipped.is_empty() {
        files.extend(files_gated_by(flipped, project_index));
    }

    files
}

/// Files holding a location whose diagnostic depends on one of the `flipped` flags,
/// mirroring the `dynamic.*` gates in `compute_structural_diagnostics`.
fn files_gated_by(flipped: DynamicUsages, project_index: &ProjectIndex) -> HashSet<PathBuf> {
    let is_gated = |entity: EntityType, behavior: Behavior| match (entity, behavior) {
        (EntityType::Command, Behavior::Definition) => flipped.invokes,
        (EntityType::Event, Behavior::Definition) => flipped.emitters || flipped.listeners,
        (_, Behavior::Listen) => flipped.emitters,
        (_, Behavior::Emit) => flipped.listeners,
        _ => false,
    };

    project_index
        .map
        .iter()
        .flat_map(|entry| {
            let entity = entry.key().entity;

            entry
                .value()
                .iter()
                .filter(|location| is_gated(entity, location.behavior))
                .map(|location| location.path.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Check if file extension is supported
#[must_use]
pub fn is_supported_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| crate::constants::SUPPORTED_EXTENSIONS.contains(&ext))
}

/// Process file content from editor buffer.
///
/// Returns `true` if the file was successfully routed and processed.
///
/// # Panics
///
/// Panics if the `generator_bindings` lock is poisoned (only if another thread panicked
/// while holding the write lock).
pub fn process_file_content(path: &Path, content: &str, project_index: &ProjectIndex) -> bool {
    if !is_supported_file(path) {
        return false;
    }

    // Check if this is a generated bindings file via config-based discovery.
    if let Some(kind) = project_index.get_generator_for_file(path) {
        process_bindings_file(path, content, kind, project_index);

        return true;
    }

    if path.extension().is_some_and(|s| s == "rs") {
        // Extract constants from this file and store globally, then get all global Rust constants
        let local_constants = crate::utils::extract_rust_constant_decls_from_content(content);
        project_index.add_rust_constants(path.to_path_buf(), local_constants);
        let global_constants = project_index.get_all_rust_constants();

        match parser::parse_rust_full(content, path, &global_constants) {
            Ok(rust_index) => {
                let path_buf = path.to_path_buf();

                project_index.remove_schemas_for_file(&path_buf);
                project_index.remove_event_schemas_for_file(&path_buf);

                project_index.add_file(rust_index.file_index);

                for schema in rust_index.command_schemas {
                    add_command_schema_if_higher_priority(schema, project_index);
                }

                for schema in rust_index.event_schemas {
                    add_event_schema_if_higher_priority(schema, project_index);
                }

                true
            }

            Err(e) => {
                project_index.set_parse_error(path.to_path_buf(), format!("{e:?}"));
                false
            }
        }
    } else {
        // Extract constants from this file and store globally, then get all global JS/TS constants
        let is_js = path.extension().is_some_and(|e| e == "js" || e == "jsx");
        let local_constants = crate::utils::extract_js_constants_from_content(content, is_js);
        project_index.add_js_constants(path.to_path_buf(), local_constants);
        let global_constants = project_index.get_all_js_constants();

        // Re-register this file's helpers before the main pass reads the global set:
        // an edit that declares a helper and calls it in the same file would otherwise
        // be parsed against the pre-edit snapshot and leave the call site unresolved.
        project_index.add_forwarders(path.to_path_buf(), extract_file_forwarders(path, content));

        let known_forwarders = project_index.get_all_forwarders();

        match parser::parse(path, content, &global_constants, &known_forwarders) {
            Ok(file_index) => {
                project_index.add_file(file_index);

                true
            }

            Err(e) => {
                project_index.set_parse_error(path.to_path_buf(), format!("{e:?}"));

                false
            }
        }
    }
}

/// Add a command schema only if no higher-priority (non-RustSource) schema already exists.
fn add_command_schema_if_higher_priority(
    schema: crate::indexer::CommandSchema,
    project_index: &ProjectIndex,
) {
    if !project_index
        .get_schema(&schema.command_name)
        .is_some_and(|e| e.generator != GeneratorKind::RustSource)
    {
        project_index.add_schema(schema);
    }
}

/// Add an event schema only if no higher-priority (non-RustSource) schema already exists.
fn add_event_schema_if_higher_priority(
    schema: crate::indexer::EventSchema,
    project_index: &ProjectIndex,
) {
    if !project_index
        .get_event_schema(&schema.event_name)
        .is_some_and(|e| e.generator != GeneratorKind::RustSource)
    {
        project_index.add_event_schema(schema);
    }
}

/// Route a generated bindings file to the appropriate reader and store results.
fn process_bindings_file(
    path: &Path,
    content: &str,
    kind: GeneratorKind,
    project_index: &ProjectIndex,
) {
    let path_buf = path.to_path_buf();

    // Clear stale data for this file first
    project_index.remove_schemas_for_file(&path_buf);
    project_index.remove_type_aliases_for_file(&path_buf);
    project_index.remove_event_schemas_for_file(&path_buf);

    match kind {
        GeneratorKind::Specta => {
            for schema in bindings_reader::parse_specta_bindings(content, &path_buf) {
                project_index.add_schema(schema);
            }

            for schema in bindings_reader::parse_specta_events(content, &path_buf) {
                project_index.add_event_schema(schema);
            }

            for (name, def) in bindings_reader::parse_typescript_types(content) {
                project_index.add_type_alias(name, def, path.to_path_buf());
            }
        }

        GeneratorKind::TsRs => {
            for (name, def) in bindings_reader::parse_typescript_types(content) {
                project_index.add_type_alias(name, def, path.to_path_buf());
            }
        }

        GeneratorKind::Typegen => {
            for (name, def) in bindings_reader::parse_typescript_types(content) {
                project_index.add_type_alias(name, def, path.to_path_buf());
            }

            for schema in bindings_reader::parse_typegen_events(content, &path_buf) {
                project_index.add_event_schema(schema);
            }
        }

        GeneratorKind::RustSource => {
            // Not a valid kind for generated TS files — ignore
        }
    }
}

/// First-pass: extract and store constants from a file without full parsing.
///
/// Called during the initial workspace scan before full indexing so that
/// cross-file constant references are available when the main parse pass runs.
pub fn collect_constants_from_file(path: &Path, project_index: &ProjectIndex) {
    if !is_supported_file(path) {
        return;
    }
    // Skip generated bindings files — they don't contain constants we need to resolve
    if project_index.get_generator_for_file(path).is_some() {
        return;
    }
    let Ok(content) = std::fs::read_to_string(path) else {
        return;
    };
    if path.extension().is_some_and(|e| e == "rs") {
        let constants = crate::utils::extract_rust_constant_decls_from_content(&content);
        project_index.add_rust_constants(path.to_path_buf(), constants);
    } else {
        let is_js = path.extension().is_some_and(|e| e == "js" || e == "jsx");
        let constants = crate::utils::extract_js_constants_from_content(&content, is_js);
        project_index.add_js_constants(path.to_path_buf(), constants);

        // Forwarding helpers are cross-file: a call site is only resolvable once the
        // helper that owns it is known, so they must be collected before the main pass.
        project_index.add_forwarders(path.to_path_buf(), extract_file_forwarders(path, &content));
    }
}

/// Forwarding helpers declared by a single frontend file.
///
/// Returns an empty list for anything that is not a frontend language. SFCs are
/// unwrapped to their script blocks first, since the helper scan reads plain
/// TypeScript and would otherwise see the surrounding template markup.
fn extract_file_forwarders(path: &Path, content: &str) -> Vec<crate::indexer::types::Forwarder> {
    let Some(lang) = path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(parser::LangType::from_extension)
    else {
        return Vec::new();
    };

    match lang {
        parser::LangType::Rust => Vec::new(),

        parser::LangType::Vue | parser::LangType::Svelte => {
            parser::typescript::sfc::extract_script_blocks(content)
                .into_iter()
                .filter_map(|(script, _)| {
                    parser::typescript::frontend::extract_forwarders(
                        &script,
                        parser::LangType::TypeScript,
                    )
                    .ok()
                })
                .flatten()
                .collect()
        }

        _ => parser::typescript::frontend::extract_forwarders(content, lang).unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{DiscoveredGenerator, GeneratorKind, ProjectIndex};
    use std::path::Path;

    fn load_fixture(relative_path: &str) -> String {
        let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        std::fs::read_to_string(fixtures_dir.join(relative_path))
            .unwrap_or_else(|e| panic!("Failed to load fixture {relative_path}: {e}"))
    }

    #[test]
    fn test_helper_declared_and_called_in_same_edit_is_indexed() {
        // Simulates `didChange`: the file introduces the helper and a call to it at
        // once, so nothing else in the workspace has registered the forwarder yet.
        let index = ProjectIndex::new();
        let path = PathBuf::from("app.ts");

        let content = r#"
import { emit } from "@tauri-apps/api/event";

const broadcast = (event: string, payload: unknown) => emit(event, payload);

broadcast("units-changed", "metric");
"#;

        assert!(process_file_content(&path, content, &index));

        let key = crate::indexer::types::IndexKey {
            entity: crate::syntax::EntityType::Event,
            name: "units-changed".to_string(),
        };

        assert!(
            index.map.contains_key(&key),
            "call site of a helper declared in the same edit should be indexed"
        );
    }

    #[test]
    fn test_process_ts_rs_file_populates_type_aliases() {
        let index = ProjectIndex::new();
        let path = PathBuf::from("ts_rs_types.ts");

        index.set_generator_bindings(vec![DiscoveredGenerator {
            kind: GeneratorKind::TsRs,
            output_path: path.clone(),
            is_directory: false,
        }]);

        let content = load_fixture("bindings/ts_rs_types.ts");
        process_file_content(&path, &content, &index);

        assert!(
            index.type_aliases.contains_key("UserProfile"),
            "UserProfile alias should be in index"
        );
        assert!(
            index.type_aliases.contains_key("TaskState"),
            "TaskState alias should be in index"
        );
    }
}
