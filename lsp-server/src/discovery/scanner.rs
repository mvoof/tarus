use crate::constants::SUPPORTED_EXTENSIONS;
use crate::indexer::DiscoveredGenerator;
use ignore::{DirEntry, IncrementalIgnore, WalkBuilder};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};

/// Ignored folder list, applied on top of the ignore files: it still covers a
/// project that has none.
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

/// A scanned file above this size is reported: source files are rarely this big,
/// build output that no ignore file covers usually is.
pub const LARGE_FILE_BYTES: u64 = 512 * 1024;

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
    // The root itself is never skipped, whatever the workspace folder is called.
    if entry.depth() == 0 {
        return false;
    }

    let name = entry.file_name().to_str().unwrap_or("");
    let is_dir = entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir());

    is_ignored_entry_name(name, is_dir)
}

/// A walk over `root` that skips what git skips — `.gitignore` in every folder,
/// `.ignore`, `.git/info/exclude`, the global excludes file — and the excluded
/// names above.
pub(crate) fn workspace_walk(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);

    builder
        // Hidden folders were always scanned: only the ignore files decide.
        .hidden(false)
        // A `.gitignore` already holds before `git init`.
        .require_git(false)
        .filter_entry(|entry| !should_skip(entry));

    builder
}

fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| SUPPORTED_EXTENSIONS.contains(&ext))
}

fn is_generator_output(path: &Path, generators: &[DiscoveredGenerator]) -> bool {
    generators.iter().any(|generator| {
        if generator.is_directory {
            path.starts_with(&generator.output_path)
        } else {
            path == generator.output_path
        }
    })
}

/// The files the generators wrote, whatever the ignore files say: generated
/// bindings are often ignored by git, and they are where command types come from.
fn generator_output_files(generators: &[DiscoveredGenerator]) -> Vec<PathBuf> {
    let mut files = Vec::new();

    for generator in generators {
        if generator.is_directory {
            let in_directory = WalkBuilder::new(&generator.output_path)
                .standard_filters(false)
                .build()
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry
                        .file_type()
                        .is_some_and(|file_type| file_type.is_file())
                })
                .map(DirEntry::into_path)
                .filter(|path| has_supported_extension(path));

            files.extend(in_directory);
        } else if generator.output_path.is_file() {
            files.push(generator.output_path.clone());
        }
    }

    files
}

/// Returns true if the path lies inside an excluded directory or is an excluded file.
///
/// Applies the same rules as the workspace scan to a single path, so a file the
/// editor opens by itself (Go to Definition into `node_modules`, a search result)
/// is not indexed when the scan would never have reached it.
#[must_use]
pub fn is_ignored_path(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };

    let in_excluded_dir = parent
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .any(|name| is_ignored_entry_name(name, true));

    let is_excluded_file = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| is_ignored_entry_name(name, false));

    in_excluded_dir || is_excluded_file
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
    workspace_walk(root)
        .build()
        .filter_map(Result::ok)
        .find(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
                && is_tauri_config_path(entry.path())
        })
        .map(DirEntry::into_path)
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

/// What the startup scan found
#[derive(Debug, Default)]
pub struct WorkspaceScan {
    /// Files to index
    pub files: Vec<PathBuf>,
    /// Files among them above [`LARGE_FILE_BYTES`], with their size in bytes
    pub large_files: Vec<(PathBuf, u64)>,
}

/// Scan the working directory for the files to index
#[must_use]
pub fn scan_workspace_files(root: &Path, generators: &[DiscoveredGenerator]) -> WorkspaceScan {
    let mut files: Vec<PathBuf> = workspace_walk(root)
        .build()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        })
        .map(DirEntry::into_path)
        .filter(|path| has_supported_extension(path))
        .collect();

    for path in generator_output_files(generators) {
        if !files.contains(&path) {
            files.push(path);
        }
    }

    let large_files = files
        .iter()
        .filter_map(|path| {
            let size = std::fs::metadata(path).ok()?.len();

            (size > LARGE_FILE_BYTES).then(|| (path.clone(), size))
        })
        .collect();

    WorkspaceScan { files, large_files }
}

/// Whether a single file is left out of the index, by the rules of
/// [`scan_workspace_files`]: for the files the editor opens and the file watcher
/// reports, which the scan did not pick.
///
/// Ignore files are read once per folder, on first use: an edit to one takes
/// effect after a server restart.
#[derive(Debug)]
pub struct WorkspaceFilter {
    // `IncrementalIgnore` fills its per-folder cache through `&mut self`.
    ignore_files: Mutex<IncrementalIgnore>,
    generators: Vec<DiscoveredGenerator>,
}

impl WorkspaceFilter {
    /// A filter for the workspace at `root`, keeping the files `generators` write.
    ///
    /// # Panics
    ///
    /// Never: a walk over one root builds exactly one matcher.
    #[must_use]
    pub fn new(root: &Path, generators: Vec<DiscoveredGenerator>) -> Self {
        let ignore_files = workspace_walk(root)
            .build_matchers()
            .pop()
            .expect("a walk over one root builds one matcher");

        Self {
            ignore_files: Mutex::new(ignore_files),
            generators,
        }
    }

    /// True when the scan would leave `path` out
    #[must_use]
    pub fn is_ignored(&self, path: &Path) -> bool {
        if is_generator_output(path, &self.generators) {
            return false;
        }

        if is_ignored_path(path) {
            return true;
        }

        let mut ignore_files = self.ignore_files.lock();

        // A file outside the workspace has no ignore files of its own to match.
        let Some(relative) = ignore_files.normalize(path) else {
            return false;
        };

        ignore_files.matched(relative, false).is_ignore()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::indexer::GeneratorKind;
    use std::fs;

    /// Writes `files` (path relative to the root, content) into a fresh folder.
    fn workspace(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();

        for (relative, content) in files {
            let path = root.path().join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        root
    }

    /// Scanned files as sorted root-relative paths with `/` separators.
    fn scanned(root: &Path, generators: &[DiscoveredGenerator]) -> Vec<String> {
        let mut files: Vec<String> = scan_workspace_files(root, generators)
            .files
            .iter()
            .map(|path| {
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();
        files.sort();

        files
    }

    #[test]
    fn scan_skips_what_the_root_gitignore_ignores() {
        let root = workspace(&[
            (".gitignore", "storybook-static\n"),
            ("src/app.ts", ""),
            ("storybook-static/assets/bundle.js", ""),
        ]);

        assert_eq!(scanned(root.path(), &[]), ["src/app.ts"]);
    }

    #[test]
    fn scan_reads_gitignore_files_in_nested_folders() {
        let root = workspace(&[
            ("src-tauri/.gitignore", "/schemas\n"),
            ("src-tauri/src/main.rs", ""),
            ("src-tauri/schemas/generated.rs", ""),
            ("schemas/kept.ts", ""),
        ]);

        assert_eq!(
            scanned(root.path(), &[]),
            ["schemas/kept.ts", "src-tauri/src/main.rs"]
        );
    }

    #[test]
    fn scan_keeps_a_file_a_negated_pattern_whitelists() {
        let root = workspace(&[
            (".gitignore", "generated/*\n!generated/keep.ts\n"),
            ("generated/drop.ts", ""),
            ("generated/keep.ts", ""),
        ]);

        assert_eq!(scanned(root.path(), &[]), ["generated/keep.ts"]);
    }

    #[test]
    fn scan_reads_git_info_exclude() {
        let root = workspace(&[
            (".git/info/exclude", "local-only\n"),
            ("local-only/scratch.ts", ""),
            ("src/app.ts", ""),
        ]);

        assert_eq!(scanned(root.path(), &[]), ["src/app.ts"]);
    }

    #[test]
    fn scan_without_ignore_files_still_skips_excluded_folders() {
        let root = workspace(&[
            ("node_modules/lib/index.js", ""),
            ("src-tauri/target/debug/build/out.rs", ""),
            ("src/types.d.ts", ""),
            ("src/app.ts", ""),
        ]);

        assert_eq!(scanned(root.path(), &[]), ["src/app.ts"]);
    }

    #[test]
    fn scan_keeps_generator_output_the_gitignore_ignores() {
        let root = workspace(&[
            (".gitignore", "src/bindings.ts\nsrc/types/\n"),
            ("src/bindings.ts", ""),
            ("src/types/User.ts", ""),
            ("src/app.ts", ""),
        ]);
        let generators = [
            DiscoveredGenerator {
                kind: GeneratorKind::Specta,
                output_path: root.path().join("src/bindings.ts"),
                is_directory: false,
            },
            DiscoveredGenerator {
                kind: GeneratorKind::TsRs,
                output_path: root.path().join("src/types"),
                is_directory: true,
            },
        ];

        assert_eq!(
            scanned(root.path(), &generators),
            ["src/app.ts", "src/bindings.ts", "src/types/User.ts"]
        );
    }

    #[test]
    fn scan_reports_files_above_the_large_file_size() {
        let bundle = "x".repeat(usize::try_from(LARGE_FILE_BYTES).unwrap() + 1);
        let root = workspace(&[("public/bundle.js", &bundle), ("src/app.ts", "")]);

        let scan = scan_workspace_files(root.path(), &[]);

        assert_eq!(scan.files.len(), 2, "a large file is reported, not skipped");
        assert_eq!(
            scan.large_files,
            [(root.path().join("public/bundle.js"), LARGE_FILE_BYTES + 1)]
        );
    }

    #[test]
    fn workspace_filter_agrees_with_the_scan() {
        let root = workspace(&[
            (".gitignore", "storybook-static\n"),
            ("src-tauri/.gitignore", "/schemas\n"),
        ]);
        let generators = vec![DiscoveredGenerator {
            kind: GeneratorKind::Specta,
            output_path: root.path().join("storybook-static/bindings.ts"),
            is_directory: false,
        }];
        let filter = WorkspaceFilter::new(root.path(), generators);
        let is_ignored = |relative: &str| filter.is_ignored(&root.path().join(relative));

        assert!(is_ignored("storybook-static/assets/bundle.js"));
        assert!(is_ignored("src-tauri/schemas/generated.rs"));
        assert!(is_ignored("node_modules/lib/index.js"));
        assert!(is_ignored("src/types.d.ts"));

        assert!(!is_ignored("src/app.ts"));
        assert!(!is_ignored("src-tauri/src/main.rs"));
        assert!(
            !is_ignored("storybook-static/bindings.ts"),
            "generator output"
        );
        assert!(!filter.is_ignored(Path::new("/elsewhere/app.ts")));
    }

    #[test]
    fn test_is_ignored_path() {
        assert!(is_ignored_path(Path::new(
            "/proj/node_modules/lib/index.js"
        )));
        assert!(is_ignored_path(Path::new(
            "/proj/src-tauri/target/debug/out.rs"
        )));
        assert!(is_ignored_path(Path::new("/proj/src/types.d.ts")));
        assert!(is_ignored_path(Path::new("/proj/vite.config.ts")));

        assert!(!is_ignored_path(Path::new("/proj/src/app.ts")));
        assert!(!is_ignored_path(Path::new("/proj/src-tauri/src/main.rs")));
    }

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
