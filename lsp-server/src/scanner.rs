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

/// List of extensions that are supported by extensions
const TARGET_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "vue"];

/// Filter: Returns true if this is a folder to be IGNORED
fn is_ignored_dir(entry: &DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .map(|s| EXCLUDED_DIRS.contains(&s))
        .unwrap_or(false)
}

/// Make sure it is a Tauri project by searching for the configuration file
pub fn is_tauri_project(root: &Path) -> bool {
    WalkDir::new(root)
        .follow_links(false) // Avoid symlinks for security and speed
        .into_iter()
        .filter_entry(|e| !is_ignored_dir(e))
        .filter_map(|e| e.ok()) // Ignoring file access errors
        .any(|e| {
            if let Some(name) = e.file_name().to_str() {
                // https://v2.tauri.app/reference/config/#file-formats
                return name.to_lowercase().starts_with("tauri");
            }
            false
        })
}

/// Basic scan of files in the working directory
/// Returns a list of all files to be indexed
pub fn scan_workspace_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| !is_ignored_dir(e))
        .filter_map(|e| e.ok())
        .filter(|e| {
            if !e.file_type().is_file() {
                return false;
            }

            let path = e.path();

            // Exclude vite.config.ts
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name == "vite.config.ts" {
                    return false;
                }
            }

            // Check the extension
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                return TARGET_EXTENSIONS.contains(&ext);
            }

            false
        })
        .map(|e| e.into_path())
        .collect()
}
