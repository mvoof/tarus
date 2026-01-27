use std::path::{Path, PathBuf};
use walkdir::{DirEntry, WalkDir};

/// Ignored folder list
const EXCLUDED_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".vscode",
    ".github",
    "docs",
    "target",
    "dist",
    "build",
    "gen",
];

/// Ignored files list
const EXCLUDED_FILES: &[&str] = &["vite.config.ts"];

/// Ignored file suffixes list
const EXCLUDED_FILE_SUFFIXES: &[&str] = &[".d.ts"];

/// List of extensions that are supported by the parser
const TARGET_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "vue", "svelte"];

/// Filter: Returns true if this is a folder or file to be IGNORED
fn should_skip(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_str().unwrap_or("");

    if entry.file_type().is_dir() {
        EXCLUDED_DIRS.contains(&name)
    } else {
        let name_lc = name.to_lowercase();

        let is_excluded_file = EXCLUDED_FILES.contains(&name);

        let is_excluded_suffix = EXCLUDED_FILE_SUFFIXES
            .iter()
            .any(|suffix| name_lc.ends_with(suffix));

    is_excluded_file || is_excluded_suffix
    }
}

/// Helper: Check if a path points to a Tauri configuration file
fn is_tauri_config_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name.to_lowercase().starts_with("tauri"))
}

/// Helper: Find the first Tauri configuration file in the directory tree
fn find_tauri_config(root: &Path) -> Option<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(std::result::Result::ok)
        .find(|e| e.file_type().is_file() && is_tauri_config_path(e.path()))
        .map(walkdir::DirEntry::into_path)
}

/// Make sure it is a Tauri project by searching for the configuration file
#[must_use] pub fn is_tauri_project(root: &Path) -> bool {
    find_tauri_config(root).is_some()
}

/// Find the src-tauri directory (recursively, respecting ignores)
/// Returns the parent directory of the found tauri configuration file
#[must_use] pub fn find_src_tauri_dir(root: &Path) -> Option<PathBuf> {
    find_tauri_config(root).and_then(|p| p.parent().map(std::path::Path::to_path_buf))
}

/// Basic scan of files in the working directory
/// Returns a list of all files to be indexed
#[must_use] pub fn scan_workspace_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            if !e.file_type().is_file() {
                return false;
            }

            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| TARGET_EXTENSIONS.contains(&ext))
        })
        .map(walkdir::DirEntry::into_path)
        .collect()
}
