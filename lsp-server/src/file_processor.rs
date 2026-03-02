//! File processing utilities

use crate::bindings_reader;
use crate::indexer::{GeneratorKind, ProjectIndex};
use crate::rust_type_extractor;
use crate::scanner::detect_generator_kind;
use crate::tree_parser;
use std::path::{Path, PathBuf};

/// Supported file extensions
pub const SUPPORTED_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "vue", "svelte"];

/// Check if file extension is supported
#[must_use]
pub fn is_supported_file(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        SUPPORTED_EXTENSIONS.contains(&ext)
    } else {
        false
    }
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

    // Check if this is a generated bindings file before running the normal parser.
    // Prefer config-based routing; fall back to content-based detection only when no
    // generator configs have been discovered (e.g. during tests or on first open).
    let generator_kind = project_index.get_generator_for_file(path).or_else(|| {
        if project_index.generator_bindings.read().unwrap().is_empty() {
            detect_generator_kind(content)
        } else {
            None
        }
    });

    if let Some(kind) = generator_kind {
        process_bindings_file(path, content, kind, project_index);
        return true;
    }

    match tree_parser::parse(path, content) {
        Ok(file_index) => {
            project_index.add_file(file_index);

            // For Rust files: also extract command schemas (as RustSource)
            // Only store if no higher-priority schema already exists for this command
            if path.extension().and_then(|s| s.to_str()) == Some("rs") {
                let schemas = rust_type_extractor::extract_command_schemas(content, path);
                for schema in schemas {
                    // Only store RustSource if no bindings-derived schema exists yet
                    let existing = project_index.get_schema(&schema.command_name);
                    let is_higher_priority = existing.as_ref().is_some_and(|e| {
                        matches!(
                            e.generator,
                            GeneratorKind::Specta | GeneratorKind::TsRs | GeneratorKind::Typegen
                        )
                    });
                    if !is_higher_priority {
                        project_index.add_schema(schema);
                    }
                }
            }

            true
        }
        Err(e) => {
            project_index.set_parse_error(path.to_path_buf(), format!("{e:?}"));
            false
        }
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

    match kind {
        GeneratorKind::Specta => {
            let schemas = bindings_reader::parse_specta_bindings(content, &path_buf);
            for schema in schemas {
                project_index.add_schema(schema);
            }
        }
        GeneratorKind::TsRs => {
            let aliases = bindings_reader::parse_ts_rs_types(content);
            for (name, def) in aliases {
                project_index.add_type_alias(name, def, path.to_path_buf());
            }
        }
        GeneratorKind::Typegen => {
            let aliases = bindings_reader::parse_typegen_types(content);
            for (name, def) in aliases {
                project_index.add_type_alias(name, def, path.to_path_buf());
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

    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            project_index.set_parse_error(path, format!("Failed to read file: {e}"));
            return false;
        }
    };

    process_file_content(&path, &content, project_index)
}
