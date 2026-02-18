//! Bindings reader for Tauri Specta and Typegen integration
//!
//! Handles auto-discovery of bindings files and parsing them to extract
//! command signatures and types.

use crate::indexer::{BindingEntry, BindingSource, ExternalTypeEntry, Parameter, ProjectIndex};
use crate::tree_parser::utils::ParseContext;
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

/// Result of loading bindings files
#[derive(Debug, Clone)]
pub struct LoadResult {
    /// Number of successfully loaded files
    pub loaded: usize,
    /// Errors encountered during loading (path, error message)
    pub errors: Vec<(PathBuf, String)>,
}

impl Default for BindingsConfig {
    fn default() -> Self {
        Self {
            type_bindings_paths: None,
            type_safety_enabled: true,
        }
    }
}

/// Load all bindings files for a project
///
/// This unified function handles bindings discovery and loading with comprehensive
/// error tracking. It's used by initialization, hot-reload, and manual sync commands.
///
/// # Arguments
/// * `project_root` - Root directory of the project
/// * `config` - Bindings configuration (paths, enabled state)
/// * `project_index` - Index to populate with bindings
/// * `clear_first` - Whether to clear existing bindings before loading
///
/// # Returns
/// `LoadResult` containing count of loaded files and any errors encountered
pub fn load_all_bindings(
    project_root: &Path,
    config: &BindingsConfig,
    project_index: &ProjectIndex,
    clear_first: bool,
) -> LoadResult {
    // Early return if type safety is disabled
    if !config.type_safety_enabled {
        return LoadResult {
            loaded: 0,
            errors: vec![],
        };
    }

    // Clear existing bindings if requested
    if clear_first {
        project_index.clear_bindings_registry();
    }

    // Discover bindings files
    let files = find_bindings_files(project_root, config);
    let mut result = LoadResult {
        loaded: 0,
        errors: vec![],
    };

    // Load each file
    for path in files {
        match read_bindings(&path, project_index) {
            Ok(()) => result.loaded += 1,
            Err(e) => result.errors.push((path, e.to_string())),
        }
    }

    result
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
    // Use scanner's find_tauri_config to avoid hardcoded filename/path
    let tauri_conf = crate::scanner::find_tauri_config(project_root)?;

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
    // Use scanner's find_src_tauri_dir to avoid hardcoded path
    let src_tauri_dir = crate::scanner::find_src_tauri_dir(project_root)?;
    let src_tauri = src_tauri_dir.join("src");
    if !src_tauri.exists() {
        return None;
    }

    let walker = WalkDir::new(&src_tauri).max_depth(3);
    for entry in walker.into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            if let Ok(content) = std::fs::read_to_string(path) {
                // Look for `ts::export(..., "path/to/bindings.ts")`
                // or `export(..., "path/to/bindings.ts")`
                // or `specta_builder.export(..., "path")`
                if let Some(bindings_path) = extract_export_path(&content) {
                    // Resolve relative path from src-tauri base
                    let resolved = src_tauri_dir.join(&bindings_path);
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

/// Captured values from a single tree-sitter match
struct BindingCaptures<'a> {
    func_name: Option<String>,
    params_node: Option<tree_sitter::Node<'a>>,
    return_type_node: Option<tree_sitter::Node<'a>>,
    type_name: Option<String>,
    type_value_node: Option<tree_sitter::Node<'a>>,
    iface_name: Option<String>,
    iface_body_node: Option<tree_sitter::Node<'a>>,
    specta_object_name: Option<String>,
    specta_method_name: Option<String>,
    specta_method_name_node: Option<tree_sitter::Node<'a>>,
    specta_params_node: Option<tree_sitter::Node<'a>>,
    specta_return_node: Option<tree_sitter::Node<'a>>,
}

impl<'a> BindingCaptures<'a> {
    /// Create empty captures
    fn new() -> Self {
        Self {
            func_name: None,
            params_node: None,
            return_type_node: None,
            type_name: None,
            type_value_node: None,
            iface_name: None,
            iface_body_node: None,
            specta_object_name: None,
            specta_method_name: None,
            specta_method_name_node: None,
            specta_params_node: None,
            specta_return_node: None,
        }
    }

    /// Extract captures from a match result
    fn from_match(
        match_result: &tree_sitter::QueryMatch<'a, 'a>,
        content: &str,
        func_name_idx: Option<u32>,
        func_params_idx: Option<u32>,
        return_type_idx: Option<u32>,
        type_name_idx: Option<u32>,
        type_value_idx: Option<u32>,
        interface_name_idx: Option<u32>,
        interface_body_idx: Option<u32>,
        specta_object_name_idx: Option<u32>,
        specta_method_name_idx: Option<u32>,
        specta_method_params_idx: Option<u32>,
        specta_method_return_idx: Option<u32>,
    ) -> Self {
        let mut captures = Self::new();

        for capture in match_result.captures {
            if Some(capture.index) == func_name_idx {
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    captures.func_name = Some(text.to_string());
                }
            } else if Some(capture.index) == func_params_idx {
                captures.params_node = Some(capture.node);
            } else if Some(capture.index) == return_type_idx {
                captures.return_type_node = Some(capture.node);
            } else if Some(capture.index) == type_name_idx {
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    captures.type_name = Some(text.to_string());
                }
            } else if Some(capture.index) == type_value_idx {
                captures.type_value_node = Some(capture.node);
            } else if Some(capture.index) == interface_name_idx {
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    captures.iface_name = Some(text.to_string());
                }
            } else if Some(capture.index) == interface_body_idx {
                captures.iface_body_node = Some(capture.node);
            } else if Some(capture.index) == specta_object_name_idx {
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    captures.specta_object_name = Some(text.to_string());
                }
            } else if Some(capture.index) == specta_method_name_idx {
                captures.specta_method_name_node = Some(capture.node);
                if let Ok(text) = capture.node.utf8_text(content.as_bytes()) {
                    captures.specta_method_name = Some(text.to_string());
                }
            } else if Some(capture.index) == specta_method_params_idx {
                captures.specta_params_node = Some(capture.node);
            } else if Some(capture.index) == specta_method_return_idx {
                captures.specta_return_node = Some(capture.node);
            }
        }

        captures
    }
}

/// Read and index bindings from the specified file using tree-sitter
/// # Errors
/// Returns an error if file reading or parsing fails.
pub fn read_bindings(path: &Path, project_index: &ProjectIndex) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;

    // Canonicalize path for consistent comparison
    let canonical_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    // Parse bindings and get findings
    let findings = parse_bindings_with_tree_sitter(&canonical_path, &content, project_index)?;

    // Register this file as a bindings file (to skip normal processing)
    project_index.register_bindings_file(canonical_path.clone());

    // Add findings to index (if any)
    if !findings.is_empty() {
        use crate::indexer::FileIndex;
        project_index.add_file(FileIndex {
            path: canonical_path,
            findings,
        });
    }

    Ok(())
}

/// Process function binding (function declaration)
fn process_function_binding(
    captures: &BindingCaptures,
    content: &str,
    project_index: &ProjectIndex,
) {
    if let Some(name) = &captures.func_name {
        let args = if let Some(params) = captures.params_node {
            extract_function_parameters(params, content)
        } else {
            vec![]
        };

        let return_type = if let Some(ret_node) = captures.return_type_node {
            ret_node
                .utf8_text(content.as_bytes())
                .ok()
                .map(unwrap_promise_type)
        } else {
            None
        };

        let entry = BindingEntry { args, return_type };
        project_index.bindings_cache.insert(name.clone(), entry);
    }
}

/// Process type alias declaration
fn process_type_alias(
    captures: &BindingCaptures,
    content: &str,
    source: &BindingSource,
    project_index: &ProjectIndex,
) {
    if let Some(name) = &captures.type_name {
        if let Some(value_node) = captures.type_value_node {
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
            project_index.types_cache.insert(name.clone(), entry);
        }
    }
}

/// Process interface declaration
fn process_interface_def(
    captures: &BindingCaptures,
    content: &str,
    source: &BindingSource,
    project_index: &ProjectIndex,
) {
    if let Some(name) = &captures.iface_name {
        if let Some(body_node) = captures.iface_body_node {
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
            project_index.types_cache.insert(name.clone(), entry);
        }
    }
}

/// Process Specta/Typegen method binding
fn process_method_binding(
    captures: &BindingCaptures,
    content: &str,
    source: &BindingSource,
    project_index: &ProjectIndex,
) -> Option<crate::indexer::Finding> {
    let obj_name = captures.specta_object_name.as_ref()?;
    if obj_name != "commands" {
        return None;
    }

    let method_name_str = captures.specta_method_name.as_ref()?;
    let snake_case_name = crate::syntax::camel_to_snake(method_name_str);

    // Extract parameters
    let args = if let Some(params) = captures.specta_params_node {
        extract_function_parameters(params, content)
    } else {
        vec![]
    };

    // Extract return type
    let return_type = if let Some(ret_node) = captures.specta_return_node {
        ret_node
            .utf8_text(content.as_bytes())
            .ok()
            .map(unwrap_promise_type)
    } else {
        None
    };

    // Store in bindings_cache under snake_case name
    let entry = BindingEntry {
        args: args.clone(),
        return_type: return_type.clone(),
    };
    project_index
        .bindings_cache
        .insert(snake_case_name.clone(), entry);

    // Store in unified method_map: camelCase → (snake_case, source)
    project_index.method_map.insert(
        method_name_str.clone(),
        (snake_case_name.clone(), source.clone()),
    );

    // Create a Finding for this method (to enable CodeLens navigation)
    let method_node = captures.specta_method_name_node?;
    let start = method_node.start_position();
    let end = method_node.end_position();

    use crate::indexer::Finding;
    use crate::syntax::{Behavior, EntityType};
    use tower_lsp_server::ls_types::{Position, Range};

    Some(Finding {
        key: snake_case_name,
        entity: EntityType::Command,
        behavior: Behavior::Definition,
        range: Range {
            start: Position {
                line: start.row as u32,
                character: start.column as u32,
            },
            end: Position {
                line: end.row as u32,
                character: end.column as u32,
            },
        },
        parameters: Some(args),
        return_type,
        fields: None,
        attributes: None,
    })
}

/// Parse bindings file with tree-sitter and extract function signatures + type definitions
/// Returns a list of Findings for indexing (e.g., Specta method definitions)
/// # Errors
/// Returns an error if tree-sitter parsing fails
pub fn parse_bindings_with_tree_sitter(
    path: &Path,
    content: &str,
    project_index: &ProjectIndex,
) -> std::io::Result<Vec<crate::indexer::Finding>> {
    // Initialize TypeScript parser with ParseContext
    let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let query_source = include_str!("queries/typescript.scm");

    let ctx = ParseContext::new(&language, query_source, content, path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;

    let mut cursor = ctx.cursor();
    let mut matches = cursor.matches(&ctx.query, ctx.root_node(), content.as_bytes());

    // Find capture indices for function bindings
    let func_name_idx = ctx.query.capture_index_for_name("binding_func_name");
    let func_params_idx = ctx.query.capture_index_for_name("binding_func_params");
    let return_type_idx = ctx.query.capture_index_for_name("binding_return_type");

    // Find capture indices for type definitions
    let type_name_idx = ctx.query.capture_index_for_name("binding_type_name");
    let type_value_idx = ctx.query.capture_index_for_name("binding_type_value");
    let interface_name_idx = ctx.query.capture_index_for_name("binding_interface_name");
    let interface_body_idx = ctx.query.capture_index_for_name("binding_interface_body");

    // Find capture indices for Specta object methods
    let specta_object_name_idx = ctx.query.capture_index_for_name("specta_object_name");
    let specta_method_name_idx = ctx.query.capture_index_for_name("specta_method_name");
    let specta_method_params_idx = ctx.query.capture_index_for_name("specta_method_params");
    let specta_method_return_idx = ctx.query.capture_index_for_name("specta_method_return");

    // Determine binding source from file path
    let source = detect_binding_source(path, content);

    // Accumulate findings for indexing
    let mut findings = Vec::new();

    while let Some(match_result) = matches.next() {
        // Extract all captures from the match
        let captures = BindingCaptures::from_match(
            match_result,
            content,
            func_name_idx,
            func_params_idx,
            return_type_idx,
            type_name_idx,
            type_value_idx,
            interface_name_idx,
            interface_body_idx,
            specta_object_name_idx,
            specta_method_name_idx,
            specta_method_params_idx,
            specta_method_return_idx,
        );

        // Process each type of binding
        process_function_binding(&captures, content, project_index);
        process_type_alias(&captures, content, &source, project_index);
        process_interface_def(&captures, content, &source, project_index);

        if let Some(finding) = process_method_binding(&captures, content, &source, project_index)
        {
            findings.push(finding);
        }
    }

    Ok(findings)
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

/// Detect the binding source tool based on file path heuristics and content analysis
fn detect_binding_source(path: &Path, content: &str) -> BindingSource {
    let path_str = path.to_string_lossy();
    let file_name = path.file_name().map_or("", |n| n.to_str().unwrap_or(""));

    // ts-rs typically outputs in bindings/ directory
    // Verify by checking for ts-rs signature or lack of tool-specific imports
    if path_str.contains("/bindings/") || path_str.contains("\\bindings\\") {
        // ts-rs signature: generated comment or no @tauri-apps imports
        if content.contains("// This file was generated by [ts-rs]")
            || content.contains("ts-rs")
            || (!content.contains("@tauri-apps/api") && !content.contains("TAURI_INVOKE"))
        {
            return BindingSource::TsRs;
        }
        // Default for bindings/ directory
        return BindingSource::TsRs;
    }

    // tauri-plugin-typegen typically outputs in src/generated/ or other configured paths
    if path_str.contains("/generated/") || path_str.contains("\\generated\\") {
        return BindingSource::Typegen;
    }

    // Check file content to distinguish between Specta, Typegen, and ts-rs
    // Priority: explicit tool signatures first (ts-rs comment, TAURI_INVOKE), then imports

    // ts-rs signature has highest priority: explicit generated comment
    if content.contains("// This file was generated by [ts-rs]") || content.contains("[ts-rs]") {
        return BindingSource::TsRs;
    }

    // tauri-specta signature: uses TAURI_INVOKE or custom wrapper
    if content.contains("TAURI_INVOKE") || content.contains("__TAURI_INVOKE__") {
        return BindingSource::Specta;
    }

    // tauri-plugin-typegen signature: imports from @tauri-apps/api (lowest priority)
    if content.contains("from '@tauri-apps/api/core'")
        || content.contains("from '@tauri-apps/api/tauri'")
    {
        return BindingSource::Typegen;
    }

    // Fallback: if filename is bindings.ts without clear indicators, assume Specta
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
