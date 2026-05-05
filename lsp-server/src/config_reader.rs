//! Config-based discovery of generator output paths.
//!
//! Reads project configuration files to find where each binding generator places its output,
//! rather than sniffing file headers at scan time.

use crate::constants::{SPECTA_EXPORT_METHOD, SPECTA_EXPORT_TO_METHOD};
use crate::indexer::{DiscoveredGenerator, GeneratorKind};
use crate::ts_tree_utils::parse_rust;
use std::path::{Component, Path, PathBuf};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Query, QueryCursor};
use walkdir::WalkDir;

// ────────────────────────────────────────────────────────────────────────────
// Public API
// ────────────────────────────────────────────────────────────────────────────

/// Discover all type generators configured in the project and return their output paths.
#[must_use]
pub fn discover_generators(workspace_root: &Path) -> Vec<DiscoveredGenerator> {
    // find_tauri_config also gives us the exact config file path, which we pass to discover_typegen
    // so it doesn't need to hardcode a filename.
    let Some(tauri_config_path) = crate::scanner::find_tauri_config(workspace_root) else {
        return Vec::new();
    };

    let Some(src_tauri_dir) = tauri_config_path.parent() else {
        return Vec::new();
    };

    let mut results = Vec::new();

    results.extend(discover_specta_generators(src_tauri_dir));

    if let Some(g) = discover_typegen(&tauri_config_path, src_tauri_dir) {
        results.push(g);
    }

    if let Some(g) = discover_ts_rs(workspace_root, src_tauri_dir) {
        results.push(g);
    }

    results
}

// ────────────────────────────────────────────────────────────────────────────
// Individual generator discovery
// ────────────────────────────────────────────────────────────────────────────

/// Discover the tauri-specta and standalone specta-typescript output file by scanning Rust sources.
fn discover_specta_generators(src_tauri_dir: &Path) -> Vec<DiscoveredGenerator> {
    let rust_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let query_str = include_str!("queries/rust_specta_discovery.scm");

    let Ok(query) = Query::new(&rust_lang, query_str) else {
        return Vec::new();
    };

    let Some(method_name_idx) = query.capture_index_for_name("method_name") else {
        return Vec::new();
    };
    let Some(path_arg_idx) = query.capture_index_for_name("path_arg") else {
        return Vec::new();
    };

    let rust_files = scan_rust_files(src_tauri_dir);
    let mut generators = Vec::new();

    for (path, content) in rust_files {
        match_specta_patterns(
            &content,
            &path,
            src_tauri_dir,
            &query,
            method_name_idx,
            path_arg_idx,
            &mut generators,
        );
    }

    generators
}

/// Iterate `src_tauri_dir` recursively and return `(path, content)` for every `.rs` file
/// that mentions `tauri_specta` or `specta_typescript`.
fn scan_rust_files(src_tauri_dir: &Path) -> Vec<(PathBuf, String)> {
    WalkDir::new(src_tauri_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| {
            e.file_type().is_file()
                && e.path().extension().and_then(|s| s.to_str()) == Some("rs")
        })
        .filter_map(|e| {
            let content = std::fs::read_to_string(e.path()).ok()?;
            if content.contains("tauri_specta") || content.contains("specta_typescript") {
                Some((e.into_path(), content))
            } else {
                None
            }
        })
        .collect()
}

/// Run the specta discovery query on one file and push any found generators into `out`.
fn match_specta_patterns(
    content: &str,
    _file_path: &Path,
    src_tauri_dir: &Path,
    query: &Query,
    method_name_idx: u32,
    path_arg_idx: u32,
    out: &mut Vec<DiscoveredGenerator>,
) {
    let Some(tree) = parse_rust(content) else {
        return;
    };

    let bytes = content.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), bytes);

    while let Some(m) = matches.next() {
        let method = m
            .captures
            .iter()
            .find(|c| c.index == method_name_idx)
            .and_then(|cap| cap.node.utf8_text(bytes).ok())
            .unwrap_or("");

        if method != SPECTA_EXPORT_METHOD && method != SPECTA_EXPORT_TO_METHOD {
            continue;
        }

        let path_str = m
            .captures
            .iter()
            .find(|c| c.index == path_arg_idx)
            .and_then(|cap| cap.node.utf8_text(bytes).ok())
            .unwrap_or("");

        let ext_ok = Path::new(path_str)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("ts") || e.eq_ignore_ascii_case("js"));

        if ext_ok {
            let resolved = normalize_path(&src_tauri_dir.join(path_str));
            let kind = if method == SPECTA_EXPORT_METHOD {
                GeneratorKind::Specta
            } else {
                GeneratorKind::TsRs
            };
            out.push(DiscoveredGenerator {
                kind,
                output_path: resolved,
                is_directory: false,
            });
        }
    }
}

/// Discover the tauri-typegen output directory from the Tauri config file.
///
/// Supports `.json`, `.json5`, and `.toml` config formats.
/// Returns `None` if no `plugins.typegen` section is present.
fn discover_typegen(tauri_config_path: &Path, src_tauri_dir: &Path) -> Option<DiscoveredGenerator> {
    let content = std::fs::read_to_string(tauri_config_path).ok()?;
    let ext = tauri_config_path.extension().and_then(|e| e.to_str()).unwrap_or("");

    // Returns None if plugins.typegen section doesn't exist; Some(None) if section exists but no outputPath
    let output_path_str = read_typegen_section_output_path(&content, ext)?;

    let output_path = match output_path_str {
        Some(p) => normalize_path(&src_tauri_dir.join(p)),
        None => normalize_path(&src_tauri_dir.join("../src/generated")),
    };

    Some(DiscoveredGenerator {
        kind: GeneratorKind::Typegen,
        output_path,
        is_directory: true,
    })
}

/// Read `plugins.typegen.outputPath` from a Tauri config file.
///
/// Returns:
/// - `None` if there is no `plugins.typegen` section (caller should skip typegen)
/// - `Some(None)` if the section exists but has no `outputPath` (use default)
/// - `Some(Some(path))` if `outputPath` is set explicitly
#[allow(clippy::option_option)] // three-way: absent section / present without path / present with path
fn read_typegen_section_output_path(content: &str, ext: &str) -> Option<Option<String>> {
    if ext == "toml" {
        let val: toml::Value = content.parse().ok()?;
        let typegen = val.get("plugins")?.get("typegen")?;
        Some(typegen.get("outputPath").and_then(|v| v.as_str()).map(ToString::to_string))
    } else {
        let val: serde_json::Value = serde_json::from_str(content).ok()?;
        let typegen = val.get("plugins")?.get("typegen")?;
        Some(typegen.get("outputPath").and_then(|v| v.as_str()).map(ToString::to_string))
    }
}

/// Discover the ts-rs output directory from `.cargo/config.toml` or `Cargo.toml`.
fn discover_ts_rs(workspace_root: &Path, src_tauri_dir: &Path) -> Option<DiscoveredGenerator> {
    // By default, ts-rs generates types in the `bindings/` folder next to the `Cargo.toml` file.
    // This path can be overridden using the `TS_RS_EXPORT_DIR` environment variable.
    // In the context of Rust projects, this variable is often set in the `.cargo/config.toml` file.
    // https://docs.rs/ts-rs/latest/ts_rs/#configuration
    let candidates = [
        workspace_root.join(".cargo/config.toml"),
        src_tauri_dir.join(".cargo/config.toml"),
    ];

    for config_path in &candidates {
        if !config_path.exists() {
            continue;
        }

        let Ok(content) = std::fs::read_to_string(config_path) else {
            continue;
        };

        if let Some(output_path) = parse_ts_rs_export_dir(&content, config_path, src_tauri_dir) {
            return Some(DiscoveredGenerator {
                kind: GeneratorKind::TsRs,
                output_path,
                is_directory: true,
            });
        }
    }

    // No TS_RS_EXPORT_DIR found; fall back to checking Cargo.toml for a ts-rs dependency
    let cargo_toml_path = src_tauri_dir.join("Cargo.toml");
    let cargo_content = std::fs::read_to_string(cargo_toml_path).ok()?;
    let cargo_toml: toml::Value = cargo_content.parse().ok()?;

    let has_ts_rs = ["dependencies", "dev-dependencies", "build-dependencies"]
        .iter()
        .any(|section| {
            cargo_toml
                .get(section)
                .and_then(|v| v.as_table())
                .is_some_and(|t| t.contains_key("ts-rs"))
        });

    if has_ts_rs {
        let default_path = normalize_path(&src_tauri_dir.join("bindings"));
        return Some(DiscoveredGenerator {
            kind: GeneratorKind::TsRs,
            output_path: default_path,
            is_directory: true,
        });
    }

    None
}

// ────────────────────────────────────────────────────────────────────────────
// Parsing helpers
// ────────────────────────────────────────────────────────────────────────────

/// Parse `TS_RS_EXPORT_DIR` from a `.cargo/config.toml` `[env]` section.
///
/// Handles both plain-string and inline-table forms:
/// - `TS_RS_EXPORT_DIR = "some/path"`
/// - `TS_RS_EXPORT_DIR = { value = "some/path", relative = true }`
fn parse_ts_rs_export_dir(
    content: &str,
    config_path: &Path,
    src_tauri_dir: &Path,
) -> Option<PathBuf> {
    let table: toml::Value = content.parse().ok()?;
    let env = table.get("env")?;
    let entry = env.get("TS_RS_EXPORT_DIR")?;

    match entry {
        toml::Value::String(value) => Some(normalize_path(&src_tauri_dir.join(value))),
        toml::Value::Table(t) => {
            let value = t.get("value")?.as_str()?;
            let is_relative = t
                .get("relative")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false);

            let base: &Path = if is_relative {
                config_path
                    .parent()
                    .and_then(|p| p.parent())
                    .unwrap_or(src_tauri_dir)
            } else {
                src_tauri_dir
            };

            Some(normalize_path(&base.join(value)))
        }
        _ => None,
    }
}

/// Resolve `..` and `.` path components without requiring the path to exist on disk.
fn normalize_path(path: &Path) -> PathBuf {
    let mut components: Vec<Component<'_>> = Vec::new();

    for component in path.components() {
        match component {
            Component::ParentDir => {
                if matches!(components.last(), Some(Component::Normal(_))) {
                    components.pop();
                } else {
                    components.push(component);
                }
            }
            Component::CurDir => {}
            c => components.push(c),
        }
    }

    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::GeneratorKind;
    use std::fs;
    use tempfile::TempDir;

    fn assert_discovery(files: &[(&str, &str)], expected: &[(GeneratorKind, &str)]) {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let src_tauri = root.join("src-tauri");
        fs::create_dir_all(&src_tauri).unwrap();

        if !files.iter().any(|(p, _)| *p == "tauri.conf.json") {
            fs::write(
                src_tauri.join("tauri.conf.json"),
                r#"{ "identifier": "com.test" }"#,
            )
            .unwrap();
        }

        for (path_str, content) in files {
            let path = src_tauri.join(path_str);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, content).unwrap();
        }

        let gens = discover_generators(root);

        for (kind, expected_suffix) in expected {
            let found = gens
                .iter()
                .find(|g| g.kind == *kind && g.output_path.ends_with(expected_suffix));

            assert!(
                found.is_some(),
                "Failed to find {kind:?} ending with '{expected_suffix}'"
            );
        }

        if !expected.is_empty() {
            assert_eq!(
                gens.len(),
                expected.len(),
                "Found extra unexpected generators"
            );
        }
    }

    // ─── Specta ──────────────────────────────────────────────────────────────

    #[test]
    fn test_specta_single_line_export() {
        assert_discovery(
            &[(
                "src/lib.rs",
                r#"use tauri_specta::Builder; fn run() { builder.export(Typescript::default(), "../src/bindings.ts"); }"#,
            )],
            &[(GeneratorKind::Specta, "src/bindings.ts")],
        );
    }

    #[test]
    fn test_specta_multiline_export_with_nested_parens() {
        assert_discovery(
            &[(
                "src/lib.rs",
                r#"use tauri_specta::Builder; fn run() { specta_builder.export(Typescript::default().bigint(BigIntExportBehavior::Number), "../src/types/specta/bindings.ts"); }"#,
            )],
            &[(GeneratorKind::Specta, "src/types/specta/bindings.ts")],
        );
    }

    #[test]
    fn test_specta_jsdoc_export() {
        assert_discovery(
            &[(
                "src/lib.rs",
                r#"use tauri_specta::Builder; fn run() { builder.export(JSDoc::default(), "../src/bindings.js"); }"#,
            )],
            &[(GeneratorKind::Specta, "src/bindings.js")],
        );
    }

    #[test]
    fn test_specta_not_found_without_tauri_specta_import() {
        assert_discovery(
            &[(
                "src/lib.rs",
                r#"fn run() { something.export(Default::default(), "../src/bindings.ts"); }"#,
            )],
            &[],
        );
    }

    #[test]
    fn test_specta_cfg_guarded_export() {
        assert_discovery(
            &[(
                "src/lib.rs",
                r#"use tauri_specta::Builder; fn run() { #[cfg(feature = "specta")] builder.export(Typescript::default(), "../src/types/specta/output.ts"); }"#,
            )],
            &[(GeneratorKind::Specta, "src/types/specta/output.ts")],
        );
    }

    #[test]
    fn test_specta_multiple_exports_discovered() {
        assert_discovery(
            &[(
                "src/lib.rs",
                r#"use tauri_specta::Builder; use specta_typescript::Typescript; fn run() {
                builder.export(Typescript::default(), "../src/admin.ts");
                builder.export(Typescript::default(), "../src/client.ts");
                Typescript::default().export_to("../src/shared.ts", &types);
            }"#,
            )],
            &[
                (GeneratorKind::Specta, "src/admin.ts"),
                (GeneratorKind::Specta, "src/client.ts"),
                (GeneratorKind::TsRs, "src/shared.ts"),
            ],
        );
    }

    // ─── Standalone specta-typescript ─────────────────────────────────────────

    #[test]
    fn test_specta_typescript_export_to() {
        assert_discovery(
            &[(
                "src/main.rs",
                r#"use specta_typescript::Typescript; fn main() { Typescript::default().export_to("../src/bindings.ts", &types); }"#,
            )],
            &[(GeneratorKind::TsRs, "src/bindings.ts")],
        );
    }

    #[test]
    fn test_specta_typescript_not_found_without_import() {
        assert_discovery(
            &[(
                "src/main.rs",
                r#"fn main() { something.export_to("../src/bindings.ts", &types); }"#,
            )],
            &[],
        );
    }

    // ─── ts-rs ───────────────────────────────────────────────────────────────

    #[test]
    fn test_ts_rs_plain_string_env() {
        assert_discovery(
            &[(
                ".cargo/config.toml",
                r#"[env]
TS_RS_EXPORT_DIR = "../src/types/ts-rs""#,
            )],
            &[(GeneratorKind::TsRs, "src/types/ts-rs")],
        );
    }

    #[test]
    fn test_ts_rs_inline_table_relative() {
        assert_discovery(
            &[(
                ".cargo/config.toml",
                r#"[env]
TS_RS_EXPORT_DIR = { value = "./src/bindings_type", relative = true }"#,
            )],
            &[(GeneratorKind::TsRs, "src/bindings_type")],
        );
    }

    #[test]
    fn test_ts_rs_cargo_toml_fallback() {
        assert_discovery(
            &[("Cargo.toml", "[dependencies]\nts-rs = \"1\"")],
            &[(GeneratorKind::TsRs, "src-tauri/bindings")],
        );
    }

    #[test]
    fn test_ts_rs_not_found_without_dep_or_config() {
        assert_discovery(
            &[("Cargo.toml", "[dependencies]\nserde = \"1\"")],
            &[],
        );
    }

    // ─── Typegen ─────────────────────────────────────────────────────────────

    #[test]
    fn test_typegen_camel_case_output_path() {
        assert_discovery(
            &[(
                "tauri.conf.json",
                r#"{ "plugins": { "typegen": { "outputPath": "../src/types/typegen" } } }"#,
            )],
            &[(GeneratorKind::Typegen, "src/types/typegen")],
        );
    }

    #[test]
    fn test_typegen_default_output_path() {
        assert_discovery(
            &[(
                "tauri.conf.json",
                r#"{ "plugins": { "typegen": { "projectPath": "." } } }"#,
            )],
            &[(GeneratorKind::Typegen, "src/generated")],
        );
    }

    #[test]
    fn test_typegen_not_found_without_plugin_section() {
        assert_discovery(&[], &[]);
    }

    // ─── Combined ────────────────────────────────────────────────────────────

    #[test]
    fn test_all_three_generators_discovered() {
        assert_discovery(
            &[
                (
                    "src/lib.rs",
                    r#"use tauri_specta::Builder; fn run() { builder.export(Typescript::default(), "../src/specta.ts"); }"#,
                ),
                (
                    "tauri.conf.json",
                    r#"{ "plugins": { "typegen": { "outputPath": "../src/typegen" } } }"#,
                ),
                (
                    ".cargo/config.toml",
                    r#"[env]
TS_RS_EXPORT_DIR = "../src/ts-rs""#,
                ),
            ],
            &[
                (GeneratorKind::Specta, "src/specta.ts"),
                (GeneratorKind::Typegen, "src/typegen"),
                (GeneratorKind::TsRs, "src/ts-rs"),
            ],
        );
    }

    #[test]
    fn test_no_generators_without_tauri_config() {
        let tmp = TempDir::new().unwrap();
        assert!(discover_generators(tmp.path()).is_empty());
    }
}
