//! Bindings reader for Tauri Specta and Typegen integration
//!
//! Handles auto-discovery of bindings files and parsing them to extract
//! command signatures and types.

use crate::indexer::{BindingEntry, BindingSource, ExternalTypeEntry, Parameter, ProjectIndex};
use serde_json::Value;
use std::path::{Path, PathBuf};
use streaming_iterator::StreamingIterator;
use walkdir::WalkDir;

/// Configuration for bindings integration
#[derive(Debug, Clone)]
pub struct BindingsConfig {
    /// Custom paths to bindings files (overrides auto-discovery if non-empty)
    pub type_bindings_paths: Option<Vec<String>>,
    /// Whether type safety features using bindings are enabled
    pub type_safety_enabled: bool,
}

impl Default for BindingsConfig {
    fn default() -> Self {
        Self {
            type_bindings_paths: None,
            type_safety_enabled: true,
        }
    }
}

/// Find all bindings files using enhanced auto-discovery
///
/// Priority order:
/// 1. User-specified paths from config (if provided)
/// 2. `tauri.conf.json` `plugins.tauri-typegen.output_path` (tauri-plugin-typegen)
/// 3. Scan src-tauri for `ts::export()` calls (tauri-specta)
/// 4. Common fallback locations
#[must_use]
pub fn find_bindings_files(project_root: &Path, config: &BindingsConfig) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // 1. User-specified paths (highest priority)
    if let Some(ref paths) = config.type_bindings_paths {
        if !paths.is_empty() {
            for path_str in paths {
                let path = if Path::new(path_str).is_absolute() {
                    PathBuf::from(path_str)
                } else {
                    project_root.join(path_str)
                };

                if path.exists() {
                    files.push(path);
                }
            }
            // If user provided paths and they exist, don't auto-discover
            if !files.is_empty() {
                return files;
            }
        }
    }

    // 2. Check tauri.conf.json for tauri-plugin-typegen config
    if let Some(typegen_path) = read_typegen_config(project_root) {
        let path = if Path::new(&typegen_path).is_absolute() {
            PathBuf::from(typegen_path)
        } else {
            project_root.join(typegen_path)
        };

        if path.exists() {
            files.push(path);
        }
    }

    // 3. Scan src-tauri for Specta exports
    if let Some(specta_path) = find_specta_bindings(project_root) {
        if !files.contains(&specta_path) {
            files.push(specta_path);
        }
    }

    // 4. Scan for ts-rs bindings directory
    find_ts_rs_bindings(project_root, &mut files);

    // 5. Fallback to common locations
    let common_paths = [
        "src/bindings.ts",
        "bindings.ts",
        "src/types/bindings.ts",
        "src/lib/bindings.ts",
        "src/tauri/bindings.ts",
    ];

    for p in common_paths {
        let path = project_root.join(p);
        if path.exists() {
            if !files.contains(&path) {
                files.push(path);
            }
        }
    }
    files
}

/// Read tauri-plugin-typegen configuration from tauri.conf.json
fn read_typegen_config(project_root: &Path) -> Option<String> {
    let tauri_conf = project_root.join("src-tauri/tauri.conf.json");
    if !tauri_conf.exists() {
        return None;
    }

    let content = std::fs::read_to_string(&tauri_conf).ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;

    // Navigate: plugins -> tauri-typegen -> output_path
    json.get("plugins")
        .and_then(|p| p.get("tauri-typegen"))
        .and_then(|tg| tg.get("output_path"))
        .and_then(|op| op.as_str())
        .map(String::from)
}

/// Find Specta bindings by scanning Rust files for `ts::export()` calls
fn find_specta_bindings(project_root: &Path) -> Option<PathBuf> {
    let src_tauri = project_root.join("src-tauri/src");
    if !src_tauri.exists() {
        return None;
    }

    let walker = WalkDir::new(src_tauri).max_depth(3);
    for entry in walker.into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            if let Ok(content) = std::fs::read_to_string(path) {
                // Look for `ts::export(..., "path/to/bindings.ts")`
                // or `export(..., "path/to/bindings.ts")`
                // or `specta_builder.export(..., "path")`
                if let Some(bindings_path) = extract_export_path(&content) {
                    // Resolve relative path from src-tauri base
                    let resolved = project_root.join("src-tauri").join(&bindings_path);
                    if let Ok(canon) = resolved.canonicalize() {
                        return Some(canon);
                    }
                    // Try resolving from project root
                    let resolved_root = project_root.join(&bindings_path);
                    if let Ok(canon) = resolved_root.canonicalize() {
                        return Some(canon);
                    }
                }
            }
        }
    }

    None
}

/// Extract TypeScript export path from Rust code
fn extract_export_path(content: &str) -> Option<String> {
    // Look for patterns like:
    // - ts::export(..., "path")
    // - export(..., "path")
    // - .export(..., "path")

    for pattern in ["export(", ".export(", "ts::export("] {
        if let Some(idx) = content.find(pattern) {
            let rest = &content[idx..];
            if let Some(quote_start) = rest.find('"') {
                let rest_quoted = &rest[quote_start + 1..];
                if let Some(quote_end) = rest_quoted.find('"') {
                    let path_str = &rest_quoted[..quote_end];
                    if Path::new(path_str).extension().is_some_and(|ext| {
                        ext.eq_ignore_ascii_case("ts") || ext.eq_ignore_ascii_case("js")
                    }) {
                        return Some(path_str.to_string());
                    }
                }
            }
        }
    }

    None
}

/// Try to find the bindings file path automatically (legacy single file version)
///
/// Strategies:
/// 1. Check `tauri.conf.json` for `plugins.tauri-typegen.output_path`
/// 2. Scan `src-tauri/src/*.rs` for `ts::export` calls (Specta)
/// 3. Fallback to common locations
///
/// Deprecated: Use `find_bindings_files` for multi-file support
pub fn find_bindings_file(project_root: &Path) -> Option<PathBuf> {
    // 1. Check tauri.conf.json (simplified check for now)
    // In a real implementation we would parse the JSON, but for MVP we can check string content
    let tauri_conf = project_root.join("src-tauri/tauri.conf.json");
    if tauri_conf.exists() {
        if let Ok(content) = std::fs::read_to_string(&tauri_conf) {
            // Very naive check for typegen config
            if content.contains("tauri-typegen") && content.contains("output_path") {
                // TODO: Parse JSON properly to get exact path
                // For now, let's proceed to other strategies as this requires JSON parsing
            }
        }
    }

    // 2. Scan src-tauri for Specta exports
    let src_tauri = project_root.join("src-tauri/src");
    if src_tauri.exists() {
        let walker = WalkDir::new(src_tauri).max_depth(3);
        for entry in walker.into_iter().filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "rs") {
                if let Ok(content) = std::fs::read_to_string(path) {
                    // Look for `ts::export(..., "path/to/bindings.ts")`
                    // or `export(..., "path/to/bindings.ts")`
                    if let Some(idx) = content.find("export(") {
                        let rest = &content[idx..];
                        if let Some(quote_start) = rest.find('"') {
                            let rest_quoted = &rest[quote_start + 1..];
                            if let Some(quote_end) = rest_quoted.find('"') {
                                let path_str = &rest_quoted[..quote_end];
                                if Path::new(path_str).extension().is_some_and(|ext| {
                                    ext.eq_ignore_ascii_case("ts") || ext.eq_ignore_ascii_case("js")
                                }) {
                                    // Resolve relative path from src-tauri base
                                    // Usually it's like "../src/bindings.ts"
                                    let resolved = project_root.join("src-tauri").join(path_str);
                                    if let Ok(canon) = resolved.canonicalize() {
                                        return Some(canon);
                                    }
                                    // Try resolving from project root if absolute-ish
                                    let resolved_root = project_root.join(path_str);
                                    if let Ok(canon) = resolved_root.canonicalize() {
                                        return Some(canon);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Fallback common locations
    let common_paths = [
        "src/bindings.ts",
        "bindings.ts",
        "src/tauri-commands.d.ts", // If using our own typegen output as source? Unlikely but possible
    ];

    for p in common_paths {
        let path = project_root.join(p);
        if path.exists() {
            return Some(path);
        }
    }

    None
}

/// Read and index bindings from the specified file using tree-sitter
/// # Errors
/// Returns an error if file reading or parsing fails.
pub fn read_bindings(path: &Path, project_index: &ProjectIndex) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    parse_bindings_with_tree_sitter(path, &content, project_index)
}

/// Parse bindings file with tree-sitter and extract function signatures + type definitions
/// # Errors
/// Returns an error if tree-sitter parsing fails
pub fn parse_bindings_with_tree_sitter(
    path: &Path,
    content: &str,
    project_index: &ProjectIndex,
) -> std::io::Result<()> {
    // Initialize TypeScript parser
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();

    parser
        .set_language(&language)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "Failed to parse"))?;

    // Load TypeScript query for bindings
    let query_source = include_str!("queries/typescript.scm");
    let query = tree_sitter::Query::new(&language, query_source)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let mut cursor = tree_sitter::QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

    // Find capture indices for function bindings
    let func_name_idx = query.capture_index_for_name("binding_func_name");
    let func_params_idx = query.capture_index_for_name("binding_func_params");
    let return_type_idx = query.capture_index_for_name("binding_return_type");

    // Find capture indices for type definitions
    let type_name_idx = query.capture_index_for_name("binding_type_name");
    let type_value_idx = query.capture_index_for_name("binding_type_value");
    let interface_name_idx = query.capture_index_for_name("binding_interface_name");
    let interface_body_idx = query.capture_index_for_name("binding_interface_body");

    // Determine binding source from file path
    let source = detect_binding_source(path);

    while let Some(match_result) = matches.next() {
        let mut func_name = None;
        let mut params_node = None;
        let mut return_type_node = None;
        let mut type_name = None;
        let mut type_value_node = None;
        let mut iface_name = None;
        let mut iface_body_node = None;

        for capture in match_result.captures {
            if Some(capture.index) == func_name_idx {
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    func_name = Some(text.to_string());
                }
            } else if Some(capture.index) == func_params_idx {
                params_node = Some(capture.node);
            } else if Some(capture.index) == return_type_idx {
                return_type_node = Some(capture.node);
            } else if Some(capture.index) == type_name_idx {
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    type_name = Some(text.to_string());
                }
            } else if Some(capture.index) == type_value_idx {
                type_value_node = Some(capture.node);
            } else if Some(capture.index) == interface_name_idx {
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    iface_name = Some(text.to_string());
                }
            } else if Some(capture.index) == interface_body_idx {
                iface_body_node = Some(capture.node);
            }
        }

        // Process function bindings (existing logic)
        if let Some(name) = func_name {
            let args = if let Some(params) = params_node {
                extract_function_parameters(params, content)
            } else {
                vec![]
            };

            let return_type = if let Some(ret_node) = return_type_node {
                if let Ok(ret_text) = ret_node.utf8_text(content.as_bytes()) {
                    Some(unwrap_promise_type(ret_text))
                } else {
                    None
                }
            } else {
                None
            };

            let entry = BindingEntry { args, return_type };
            project_index.bindings_cache.insert(name.to_string(), entry);
        }

        // Process type alias declarations: export type Foo = { ... } or export type Status = "a" | "b"
        if let Some(name) = type_name {
            if let Some(value_node) = type_value_node {
                let raw_body = value_node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .to_string();

                let (fields, variants) = parse_type_body(&raw_body);

                let entry = ExternalTypeEntry {
                    source: source.clone(),
                    ts_name: name.clone(),
                    fields,
                    variants,
                    raw_ts_body: Some(raw_body),
                };
                project_index.types_cache.insert(name, entry);
            }
        }

        // Process interface declarations: export interface Foo { ... }
        if let Some(name) = iface_name {
            if let Some(body_node) = iface_body_node {
                let raw_body = body_node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("{}")
                    .to_string();

                let fields = parse_interface_fields(&raw_body);

                let entry = ExternalTypeEntry {
                    source: source.clone(),
                    ts_name: name.clone(),
                    fields: Some(fields),
                    variants: None,
                    raw_ts_body: Some(raw_body),
                };
                project_index.types_cache.insert(name, entry);
            }
        }
    }

    Ok(())
}

/// Extract function parameters from `formal_parameters` node
fn extract_function_parameters(
    params_node: tree_sitter::Node,
    content: &str,
) -> Vec<crate::indexer::Parameter> {
    let mut params = Vec::new();
    let mut cursor = params_node.walk();

    for child in params_node.children(&mut cursor) {
        if child.kind() == "required_parameter" || child.kind() == "optional_parameter" {
            let pattern = child.child_by_field_name("pattern");
            let type_node = child.child_by_field_name("type");

            if let (Some(pat), Some(ty)) = (pattern, type_node) {
                let name = pat.utf8_text(content.as_bytes()).unwrap_or("_").to_string();

                // Extract type annotation (skip the colon)
                let type_text = ty.utf8_text(content.as_bytes()).unwrap_or("any");
                // Type annotation includes colon, extract actual type
                let type_name = type_text.trim_start_matches(':').trim().to_string();

                params.push(crate::indexer::Parameter { name, type_name });
            }
        }
    }

    params
}

/// Unwrap Promise<T> to get T, or return the type as-is
fn unwrap_promise_type(type_str: &str) -> String {
    let trimmed = type_str.trim().trim_start_matches(':').trim();

    if trimmed.starts_with("Promise<") && trimmed.ends_with('>') {
        trimmed[8..trimmed.len() - 1].trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Find ts-rs generated bindings from the `bindings/` directory
/// ts-rs outputs one `.ts` file per type (e.g. `bindings/MyType.ts`)
/// The directory can be customized via `TS_RS_EXPORT_DIR` env variable
fn find_ts_rs_bindings(project_root: &Path, files: &mut Vec<PathBuf>) {
    // Check TS_RS_EXPORT_DIR env variable first
    let ts_rs_dir = std::env::var("TS_RS_EXPORT_DIR")
        .ok()
        .map(PathBuf::from)
        .unwrap_or_else(|| project_root.join("bindings"));

    if !ts_rs_dir.is_dir() {
        return;
    }

    // Scan for .ts files in the ts-rs output directory
    if let Ok(entries) = std::fs::read_dir(&ts_rs_dir) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "ts") && !files.contains(&path) {
                files.push(path);
            }
        }
    }
}

/// Detect the binding source tool based on file path heuristics
fn detect_binding_source(path: &Path) -> BindingSource {
    let path_str = path.to_string_lossy();
    let file_name = path.file_name().map_or("", |n| n.to_str().unwrap_or(""));

    // ts-rs typically outputs in bindings/ directory, one file per type
    if path_str.contains("/bindings/") || path_str.contains("\\bindings\\") {
        return BindingSource::TsRs;
    }

    // tauri-typegen typically outputs in src/generated/
    if path_str.contains("/generated/") || path_str.contains("\\generated\\") {
        return BindingSource::Typegen;
    }

    // tauri-specta typically outputs bindings.ts
    if file_name == "bindings.ts" || file_name == "bindings.js" {
        return BindingSource::Specta;
    }

    BindingSource::Custom
}

/// Parse a TypeScript type alias body to extract fields (for object types) or variants (for union types)
///
/// Examples:
/// - `{ name: string; age: number }` → fields
/// - `"active" | "inactive" | "pending"` → variants
/// - `{ tag: "success"; value: T } | { tag: "error"; message: string }` → raw body only
fn parse_type_body(body: &str) -> (Option<Vec<Parameter>>, Option<Vec<String>>) {
    let trimmed = body.trim();

    // Check if it's an object type: { ... }
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        let fields = parse_interface_fields(trimmed);
        if fields.is_empty() {
            return (None, None);
        }
        return (Some(fields), None);
    }

    // Check if it's a string literal union: "a" | "b" | "c"
    if trimmed.contains('|') {
        let parts: Vec<&str> = trimmed.split('|').map(str::trim).collect();
        let all_string_literals = parts.iter().all(|p| {
            (p.starts_with('"') && p.ends_with('"')) || (p.starts_with('\'') && p.ends_with('\''))
        });

        if all_string_literals {
            let variants: Vec<String> = parts
                .iter()
                .map(|p| p.trim_matches(|c: char| c == '"' || c == '\'').to_string())
                .collect();
            return (None, Some(variants));
        }
    }

    (None, None)
}

/// Parse interface/object type fields from a body string like `{ name: string; age: number }`
fn parse_interface_fields(body: &str) -> Vec<Parameter> {
    let trimmed = body.trim();
    let inner = if trimmed.starts_with('{') && trimmed.ends_with('}') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };

    let mut fields = Vec::new();

    // Split by semicolons or newlines
    for part in inner.split(|c: char| c == ';' || c == '\n') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }

        // Parse "name: type" or "name?: type"
        if let Some(colon_idx) = part.find(':') {
            let name = part[..colon_idx]
                .trim()
                .trim_end_matches('?')
                .trim()
                .to_string();
            let type_name = part[colon_idx + 1..].trim().to_string();

            if !name.is_empty() && !type_name.is_empty() {
                fields.push(Parameter { name, type_name });
            }
        }
    }

    fields
}
