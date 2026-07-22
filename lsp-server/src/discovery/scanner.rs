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

use crate::constants::SUPPORTED_EXTENSIONS;

/// Determines if a file or directory name matches the ignore rules
fn is_ignored_entry_name(name: &str, is_dir: bool) -> bool {
    if is_dir {
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

/// Filter: Returns true if this is a folder or file to be IGNORED
fn should_skip(entry: &DirEntry) -> bool {
    let name = entry.file_name().to_str().unwrap_or("");
    let is_dir = entry.file_type().is_dir();

    is_ignored_entry_name(name, is_dir)
}

/// List of valid Tauri configuration file names (in lowercase).
///
/// Kept aligned with the documented Tauri config file formats:
/// <https://v2.tauri.app/reference/config/> — base `tauri.conf.json`,
/// `tauri.conf.json5`, `Tauri.toml`, plus the `tauri.<platform>.conf.json` and
/// `Tauri.<platform>.toml` overrides (no platform-specific JSON5 exists).
///
/// The same set is maintained by hand on the extension side and must be updated
/// together whenever this list changes:
///   - `TAURI_CONFIG_FILES` / `TAURI_CONFIG_GLOB` in extension/src/extension.ts
///   - `activationEvents` in extension/package.json
const TAURI_CONFIG_FILES: &[&str] = &[
    "tauri.conf.json",
    "tauri.conf.json5",
    "tauri.toml",
    "tauri.linux.conf.json",
    "tauri.windows.conf.json",
    "tauri.macos.conf.json",
    "tauri.android.conf.json",
    "tauri.ios.conf.json",
    "tauri.linux.toml",
    "tauri.windows.toml",
    "tauri.macos.toml",
    "tauri.android.toml",
    "tauri.ios.toml",
];

/// Helper: Check if a path points to a Tauri configuration file
///
/// This uses a case-insensitive, explicit filename list, so it can disagree with
/// the extension's case-sensitive wildcard globs in two opposite directions —
/// both benign:
/// - Looser on case (`TAURI.CONF.JSON` matches here but not the glob): harmless,
///   because the server only runs after the extension's glob already matched, so
///   such names never reach this function.
/// - Stricter on the middle segment: the extension's `tauri.*.conf.json` glob
///   also matches non-platform names like `tauri.staging.conf.json`, which this
///   list rejects. That is intentional — a standalone non-platform config is not
///   a real Tauri project, so reporting "not Tauri" (empty capabilities) is the
///   correct outcome; the extension merely over-activates and spawns an idle
///   server in that rare case.
fn is_tauri_config_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| {
            let name_lc = name.to_lowercase();
            TAURI_CONFIG_FILES.contains(&name_lc.as_str())
        })
}

/// Helper: Find the first Tauri configuration file in the directory tree
pub fn find_tauri_config(root: &Path) -> Option<PathBuf> {
    WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip(e))
        .filter_map(std::result::Result::ok)
        .find(|e| e.file_type().is_file() && is_tauri_config_path(e.path()))
        .map(walkdir::DirEntry::into_path)
}

/// Make sure it is a Tauri project by searching for the configuration file
#[must_use]
pub fn is_tauri_project(root: &Path) -> bool {
    find_tauri_config(root).is_some()
}

/// Find the src-tauri directory (recursively, respecting ignores)
/// Returns the parent directory of the found tauri configuration file
#[must_use]
pub fn find_src_tauri_dir(root: &Path) -> Option<PathBuf> {
    find_tauri_config(root).and_then(|p| p.parent().map(std::path::Path::to_path_buf))
}

/// Basic scan of files in the working directory
/// Returns a list of all files to be indexed
#[must_use]
pub fn scan_workspace_files(root: &Path) -> Vec<PathBuf> {
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
                .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext))
        })
        .map(walkdir::DirEntry::into_path)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_ignored_entry_name() {
        // Directory exclusions
        assert!(is_ignored_entry_name("node_modules", true));
        assert!(is_ignored_entry_name(".git", true));
        assert!(is_ignored_entry_name("target", true));
        assert!(is_ignored_entry_name("dist", true));

        // Allowed directories
        assert!(!is_ignored_entry_name("src", true));
        assert!(!is_ignored_entry_name("components", true));
        assert!(!is_ignored_entry_name("vite.config.ts", true));

        // File exclusions
        assert!(is_ignored_entry_name("vite.config.ts", false));
        assert!(is_ignored_entry_name("index.d.ts", false));
        assert!(is_ignored_entry_name("types.d.ts", false));

        // Allowed files
        assert!(!is_ignored_entry_name("main.rs", false));
        assert!(!is_ignored_entry_name("app.tsx", false));
        assert!(!is_ignored_entry_name("Component.vue", false));
    }

    #[test]
    fn test_find_tauri_config() {
        use std::env;
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        // Create a unique temporary directory
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let temp_dir = env::temp_dir().join(format!("tarus_test_scanner_{timestamp}"));
        fs::create_dir_all(&temp_dir).unwrap();

        // 1. Initially, no config should be found
        assert!(find_tauri_config(&temp_dir).is_none());

        // 2. Create a dummy file that is NOT a config
        let dummy_file = temp_dir.join("package.json");
        fs::write(&dummy_file, "{}").unwrap();

        assert!(find_tauri_config(&temp_dir).is_none());

        // 3. Create the tauri config inside a subfolder (like src-tauri)
        let tauri_dir = temp_dir.join("src-tauri");
        fs::create_dir_all(&tauri_dir).unwrap();

        let tauri_config_path = tauri_dir.join("tauri.conf.json");
        fs::write(&tauri_config_path, "{}").unwrap();

        // Now it should find the config
        let found = find_tauri_config(&temp_dir);

        assert!(found.is_some());
        assert_eq!(
            found.unwrap().canonicalize().unwrap(),
            tauri_config_path.canonicalize().unwrap()
        );

        // 4. Check if it correctly ignores 'node_modules' (it shouldn't search inside)
        let node_modules_dir = temp_dir.join("node_modules");
        let node_modules_tauri_dir = node_modules_dir.join("src-tauri");
        fs::create_dir_all(&node_modules_tauri_dir).unwrap();

        let fake_config = node_modules_tauri_dir.join("tauri.conf.json");
        fs::write(&fake_config, "{}").unwrap();

        // Let's hide the real one for a moment to make sure it doesn't find the fake one
        fs::rename(&tauri_config_path, tauri_dir.join("hidden.conf.json.bak")).unwrap();
        assert!(find_tauri_config(&temp_dir).is_none());

        // Cleanup
        fs::remove_dir_all(&temp_dir).unwrap();
    }

    #[test]
    fn test_is_tauri_config_path() {
        use std::path::Path;

        // Valid JSON config
        assert!(is_tauri_config_path(Path::new("tauri.conf.json")));
        assert!(is_tauri_config_path(Path::new("src-tauri/tauri.conf.json")));

        // Valid JSON5 config
        assert!(is_tauri_config_path(Path::new("tauri.conf.json5")));

        // Valid TOML config
        assert!(is_tauri_config_path(Path::new("Tauri.toml")));
        assert!(is_tauri_config_path(Path::new("tauri.toml")));

        // Valid platform-specific configs
        assert!(is_tauri_config_path(Path::new("tauri.linux.conf.json")));
        assert!(is_tauri_config_path(Path::new("Tauri.windows.toml")));
        assert!(is_tauri_config_path(Path::new("tauri.android.conf.json")));
        assert!(is_tauri_config_path(Path::new("Tauri.ios.toml")));
        assert!(is_tauri_config_path(Path::new("tauri.macos.conf.json")));

        // Invalid files that should not match
        assert!(!is_tauri_config_path(Path::new("tauri_stuff.json")));
        assert!(!is_tauri_config_path(Path::new("tauri-backup.txt")));
        assert!(!is_tauri_config_path(Path::new("tauri.json"))); // Missing .conf or .toml
        assert!(!is_tauri_config_path(Path::new("Tauri.txt")));
        assert!(!is_tauri_config_path(Path::new("package.json")));

        // Platform-specific JSON5 is not a documented Tauri format — only base
        // JSON5 (`tauri.conf.json5`) and platform JSON/TOML overrides exist.
        assert!(!is_tauri_config_path(Path::new("tauri.linux.conf.json5")));
        assert!(!is_tauri_config_path(Path::new("tauri.windows.conf.json5")));
        assert!(!is_tauri_config_path(Path::new("tauri.macos.conf.json5")));
        assert!(!is_tauri_config_path(Path::new("tauri.android.conf.json5")));
        assert!(!is_tauri_config_path(Path::new("tauri.ios.conf.json5")));
    }
}
