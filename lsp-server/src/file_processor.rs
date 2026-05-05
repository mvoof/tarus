//! File processing utilities

use crate::bindings_reader;
use crate::indexer::{GeneratorKind, ProjectIndex};
use crate::tree_parser;
use std::path::{Path, PathBuf};

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
        match tree_parser::parse_rust_full(content, path) {
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
        match tree_parser::parse(path, content) {
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
        }

        GeneratorKind::TsRs => {
            for (name, def) in bindings_reader::parse_ts_rs_types(content) {
                project_index.add_type_alias(name, def, path.to_path_buf());
            }
        }

        GeneratorKind::Typegen => {
            for (name, def) in bindings_reader::parse_typegen_types(content) {
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

/// Process file from disk
pub fn process_file_index(path: PathBuf, project_index: &ProjectIndex) -> bool {
    if !is_supported_file(&path) {
        return false;
    }

    match std::fs::read_to_string(&path) {
        Ok(content) => process_file_content(&path, &content, project_index),
        Err(e) => {
            project_index.set_parse_error(path, format!("Failed to read file: {e}"));
            false
        }
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
