//! File processing utilities

use crate::indexer::ProjectIndex;
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

/// Process file content from editor buffer
pub fn process_file_content(path: &Path, content: &str, project_index: &ProjectIndex) -> bool {
    if !is_supported_file(path) {
        return false;
    }

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
