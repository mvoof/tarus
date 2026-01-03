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

/// List of extensions that are supported by extensions
const TARGET_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "vue"];

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

/// Make sure it is a Tauri project by searching for the configuration file
pub fn is_tauri_project(root: &Path) -> bool {
    WalkDir::new(root)
        .follow_links(false) // Avoid symlinks for security and speed
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(|e| e.ok()) // Ignoring file access errors
        .any(|e| {
            if !e.file_type().is_file() {
                return false;
            }

            e.file_name()
                .to_str()
                .map(|name| name.to_lowercase().starts_with("tauri")) // https://v2.tauri.app/reference/config/#file-formats
                .unwrap_or(false)
        })
}

/// Basic scan of files in the working directory
/// Returns a list of all files to be indexed
pub fn scan_workspace_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(|e| e.ok())
        .filter(|e| {
            if !e.file_type().is_file() {
                return false;
            }

            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| TARGET_EXTENSIONS.contains(&ext))
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect()
}
