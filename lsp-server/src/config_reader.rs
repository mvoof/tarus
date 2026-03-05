//! Config-based discovery of generator output paths.
//!
//! Reads project configuration files to find where each binding generator places its output,
//! rather than sniffing file headers at scan time.

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

    if let Some(g) = discover_specta(src_tauri_dir) {
        results.push(g);
    }
    if let Some(g) = discover_typegen(&tauri_config_path, src_tauri_dir) {
        results.push(g);
    }
    if let Some(g) = discover_ts_rs(workspace_root, src_tauri_dir) {
        results.push(g);
    }
    if let Some(g) = discover_specta_typescript(src_tauri_dir) {
        results.push(g);
    }

    results
}

// ────────────────────────────────────────────────────────────────────────────
// Individual generator discovery
// ────────────────────────────────────────────────────────────────────────────

/// Discover the tauri-specta output file by scanning Rust sources for `.export(` calls.
fn discover_specta(src_tauri_dir: &Path) -> Option<DiscoveredGenerator> {
    let query_str = include_str!("queries/rust_specta_discovery.scm");

    let rust_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&rust_lang, query_str).ok()?;
    let method_name_idx = query.capture_index_for_name("method_name")?;
    let path_arg_idx = query.capture_index_for_name("path_arg")?;

    for entry in WalkDir::new(src_tauri_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };

        // Fast pre-filter: skip files that don't use tauri_specta
        if !content.contains("tauri_specta") {
            continue;
        }

        let Some(tree) = parse_rust(&content) else {
            continue;
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        while let Some(m) = matches.next() {
            let method = m
                .captures
                .iter()
                .find(|c| c.index == method_name_idx)
                .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
                .unwrap_or("");

            if method != "export" {
                continue;
            }

            let path_str = m
                .captures
                .iter()
                .find(|c| c.index == path_arg_idx)
                .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
                .unwrap_or("");

            let ext_ok = Path::new(path_str)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ts") || e.eq_ignore_ascii_case("js"));

            if ext_ok {
                let resolved = normalize_path(&src_tauri_dir.join(path_str));
                return Some(DiscoveredGenerator {
                    kind: GeneratorKind::Specta,
                    output_path: resolved,
                    is_directory: false,
                });
            }
        }
    }

    None
}

/// Discover the tauri-typegen output directory from the Tauri config file.
///
/// Supports `.json` and `.json5` config formats (parsed with `serde_json`).
/// Returns `None` for `.toml` configs or if no `plugins.typegen` section is present.
fn discover_typegen(tauri_config_path: &Path, src_tauri_dir: &Path) -> Option<DiscoveredGenerator> {
    // Only attempt JSON parsing; TOML configs are not supported by serde_json
    let ext = tauri_config_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    if ext == "toml" {
        return None;
    }

    let content = std::fs::read_to_string(tauri_config_path).ok()?;
    let json: serde_json::Value = serde_json::from_str(&content).ok()?;

    // Section must exist; if absent typegen is not configured
    let typegen = json.get("plugins")?.get("typegen")?;

    let output_path = if let Some(p) = typegen.get("outputPath").and_then(|v| v.as_str()) {
        normalize_path(&src_tauri_dir.join(p))
    } else {
        normalize_path(&src_tauri_dir.join("../src/generated"))
    };

    Some(DiscoveredGenerator {
        kind: GeneratorKind::Typegen,
        output_path,
        is_directory: true,
    })
}

/// Discover the ts-rs output directory from `.cargo/config.toml` or `Cargo.toml`.
fn discover_ts_rs(workspace_root: &Path, src_tauri_dir: &Path) -> Option<DiscoveredGenerator> {
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

/// Discover standalone specta-typescript output by scanning for `export_to("path", ...)` calls.
///
/// Unlike `tauri-specta` which uses `.export(format, "path")`, standalone `specta-typescript`
/// uses `Typescript::default().export_to("path", &types)` where the path is the **first** arg.
/// The output format is identical to ts-rs (`export type Name = ...`), so we use `GeneratorKind::TsRs`.
fn discover_specta_typescript(src_tauri_dir: &Path) -> Option<DiscoveredGenerator> {
    let query_str = include_str!("queries/rust_specta_discovery.scm");

    let rust_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&rust_lang, query_str).ok()?;
    let method_name_idx = query.capture_index_for_name("method_name")?;
    let path_arg_idx = query.capture_index_for_name("path_arg")?;

    for entry in WalkDir::new(src_tauri_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("rs"))
    {
        let Ok(content) = std::fs::read_to_string(entry.path()) else {
            continue;
        };

        if !content.contains("specta_typescript") {
            continue;
        }

        let Some(tree) = parse_rust(&content) else {
            continue;
        };

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        while let Some(m) = matches.next() {
            let method = m
                .captures
                .iter()
                .find(|c| c.index == method_name_idx)
                .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
                .unwrap_or("");

            if method != "export_to" {
                continue;
            }

            let path_str = m
                .captures
                .iter()
                .find(|c| c.index == path_arg_idx)
                .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
                .unwrap_or("");

            let ext_ok = Path::new(path_str)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("ts") || e.eq_ignore_ascii_case("js"));

            if ext_ok {
                let resolved = normalize_path(&src_tauri_dir.join(path_str));
                return Some(DiscoveredGenerator {
                    kind: GeneratorKind::TsRs,
                    output_path: resolved,
                    is_directory: false,
                });
            }
        }
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
                components.pop();
            }
            Component::CurDir => {}
            c => components.push(c),
        }
    }
    components.iter().collect()
}
