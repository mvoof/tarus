//! Source code parsers for Rust and frontend languages
//!
//! ## Structure
//! - `rust/`       — Rust `#[tauri::command]`, events, struct/enum types, attributes
//! - `typescript/` — TS/JS/Vue/Svelte invoke/emit/listen + bindings files
//! - `lang_config` — language detection and tree-sitter query routing

pub mod lang_config;
pub mod rust;
pub mod typescript;

pub use lang_config::LangType;

use crate::indexer::{CommandSchema, EventSchema, FileIndex};
use crate::syntax::{ParseError, ParseResult};
use lang_config::is_angular_file;
use rust::commands::extract_rust_findings;
use typescript::frontend::parse_frontend;
use typescript::sfc::extract_script_blocks;

use std::collections::HashMap;
use std::path::Path;
use tree_sitter::{Language, Parser};

/// Main parse entry point for all supported file types.
///
/// # Errors
///
/// Returns error if tree-sitter fails to parse the file or query execution fails.
#[allow(clippy::implicit_hasher)]
pub fn parse(
    path: &Path,
    content: &str,
    global_constants: &HashMap<String, String>,
) -> ParseResult<FileIndex> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    let is_angular = ext == "ts" && is_angular_file(content);

    let lang = if is_angular {
        Some(LangType::Angular)
    } else {
        LangType::from_extension(ext)
    };

    let findings = match lang {
        Some(LangType::Rust) => {
            let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
            let mut parser = Parser::new();
            parser.set_language(&ts_lang).map_err(|e| {
                ParseError::LanguageError(format!("Failed to set Rust language: {e}"))
            })?;
            let tree = parser
                .parse(content, None)
                .ok_or_else(|| ParseError::SyntaxError("Failed to parse Rust file".to_string()))?;
            extract_rust_findings(tree.root_node(), content, &ts_lang, global_constants)?
        }
        Some(lang_val @ (LangType::TypeScript | LangType::JavaScript | LangType::Angular)) => {
            parse_frontend(content, lang_val, 0, global_constants)?
        }
        Some(LangType::Vue | LangType::Svelte) => {
            let blocks = extract_script_blocks(content);
            let mut all_findings = Vec::new();

            for (script_content, line_offset) in blocks {
                let findings = parse_frontend(
                    &script_content,
                    LangType::TypeScript,
                    line_offset,
                    global_constants,
                )?;
                all_findings.extend(findings);
            }

            all_findings
        }
        None => Vec::new(),
    };

    Ok(FileIndex {
        path: path.to_path_buf(),
        findings,
    })
}

/// Combined result of parsing a Rust file: findings + schemas from a single parse pass.
pub struct RustFileIndex {
    pub file_index: FileIndex,
    pub command_schemas: Vec<CommandSchema>,
    pub event_schemas: Vec<EventSchema>,
}

/// Parse a Rust file in a single pass, extracting findings, command schemas, and event schemas.
///
/// # Errors
///
/// Returns error if tree-sitter fails to parse the file or query execution fails.
#[allow(clippy::implicit_hasher)]
pub fn parse_rust_full(
    content: &str,
    path: &Path,
    global_constants: &HashMap<String, String>,
) -> ParseResult<RustFileIndex> {
    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .map_err(|e| ParseError::LanguageError(format!("Failed to set Rust language: {e}")))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError("Failed to parse Rust file".to_string()))?;

    let root = tree.root_node();

    let findings = extract_rust_findings(root, content, &ts_lang, global_constants)?;
    let command_schemas = rust::types::extract_command_schemas_from_tree(root, content, path);
    let event_schemas = rust::types::extract_event_schemas_from_tree(root, content, path);

    Ok(RustFileIndex {
        file_index: FileIndex {
            path: path.to_path_buf(),
            findings,
        },
        command_schemas,
        event_schemas,
    })
}
