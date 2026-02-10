//! Bindings reader for Tauri Specta and Typegen integration
//!
//! Handles auto-discovery of bindings files and parsing them to extract
//! command signatures and types.

use crate::indexer::{BindingEntry, ProjectIndex};
use std::path::{Path, PathBuf};
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

/// Try to find the bindings file path automatically
///
/// Strategies:
/// 1. Check `tauri.conf.json` for `plugins.tauri-typegen.output_path`
/// 2. Scan `src-tauri/src/*.rs` for `ts::export` calls (Specta)
/// 3. Fallback to common locations
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
            if path.extension().map_or(false, |ext| ext == "rs") {
                if let Ok(content) = std::fs::read_to_string(path) {
                    // Look for `ts::export(..., "path/to/bindings.ts")`
                    // or `export(..., "path/to/bindings.ts")`
                    if let Some(idx) = content.find("export(") {
                        let rest = &content[idx..];
                        if let Some(quote_start) = rest.find('"') {
                            let rest_quoted = &rest[quote_start + 1..];
                            if let Some(quote_end) = rest_quoted.find('"') {
                                let path_str = &rest_quoted[..quote_end];
                                if path_str.ends_with(".ts") || path_str.ends_with(".js") {
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

/// Read and index bindings from the specified file
pub fn read_bindings(path: &Path, project_index: &ProjectIndex) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;

    // Parse using existing frontend parser logic, but we might need to adapt it
    // Specta bindings look like:
    // export async function greet(name: string): Promise<string> { ... }

    // Existing frontend_parser looks for Invocations (invoke()).
    // We need definitions.
    // For now, let's use a simplified regex-based approach for MVP if tree-sitter queries aren't ready for this.
    // OR we can rely on the findings from frontend_parser if we extend it.

    // Let's implement a basic regex parser for now to prove the concept,
    // as expanding tree-sitter queries is a larger task.
    // Matches: export async function name(args...): Promise<Ret>

    // TODO: Replace with tree-sitter integration in Stage 4

    let lines: Vec<&str> = content.lines().collect();
    for line in lines {
        let line = line.trim();
        if line.starts_with("export async function") || line.starts_with("export function") {
            // Extract name
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                // export async function name(...)
                // or export function name(...)
                let name_part = if parts[1] == "async" {
                    parts[3]
                } else {
                    parts[2]
                };
                if let Some(name_end) = name_part.find('(') {
                    let name = &name_part[..name_end];

                    // Basic extraction successful
                    let entry = BindingEntry {
                        command_name: name.to_string(),
                        args: vec![],      // TODO: Parse args
                        return_type: None, // TODO: Parse return type
                    };

                    project_index.bindings_cache.insert(name.to_string(), entry);
                }
            }
        }
    }

    Ok(())
}
