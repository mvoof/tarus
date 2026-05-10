//! Unified Tree-sitter based parser for Rust and frontend languages
//!
//! This module provides a single entry point for parsing all supported file types
//! using Tree-sitter queries defined in external .scm files.
//!
//! ## Submodules
//! - `lang_config` — language detection, query routing, Angular detection
//! - `sfc_parser` — Vue/Svelte `<script>` block extraction
//! - `rust_parser` — Rust `#[tauri::command]` and event parsing
//! - `frontend_parser` — TypeScript/JavaScript invoke/emit/listen parsing
//! - `extractors` — type argument and call argument extraction helpers

mod extractors;
mod frontend_parser;
mod lang_config;
mod rust_parser;
mod sfc_parser;

pub use lang_config::LangType;

use crate::indexer::{CommandSchema, EventSchema, FileIndex};
use crate::rust_type_extractor;
use crate::syntax::{ParseError, ParseResult};
use std::path::Path;
use tree_sitter::{Language, Parser};

use frontend_parser::parse_frontend;
use lang_config::is_angular_file;
use rust_parser::extract_rust_findings;
use sfc_parser::extract_script_blocks;

/// Main parsing function - entry point for all file types
///
/// # Errors
///
/// Returns error if tree-sitter fails to parse the file or query execution fails
///
/// # Panics
///
/// Panics if language detection succeeds but lang is None (should never happen due to match guards)
pub fn parse(path: &Path, content: &str) -> ParseResult<FileIndex> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");

    // Check for Angular: content-based detection for .ts files
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
            extract_rust_findings(tree.root_node(), content, &ts_lang)?
        }
        Some(lang_val @ (LangType::TypeScript | LangType::JavaScript | LangType::Angular)) => {
            parse_frontend(content, lang_val, 0)?
        }
        Some(LangType::Vue | LangType::Svelte) => {
            let blocks = extract_script_blocks(content);
            let mut all_findings = Vec::new();

            for (script_content, line_offset) in blocks {
                let findings = parse_frontend(&script_content, LangType::TypeScript, line_offset)?;
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

/// Parse a Rust file in a single pass: one `Parser::new()` + `parser.parse()`,
/// then run the findings query, command schema query, and event schema query
/// sequentially on the same tree.
///
/// # Errors
///
/// Returns error if tree-sitter fails to parse the file or query execution fails
pub fn parse_rust_full(content: &str, path: &Path) -> ParseResult<RustFileIndex> {
    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser
        .set_language(&ts_lang)
        .map_err(|e| ParseError::LanguageError(format!("Failed to set Rust language: {e}")))?;

    let tree = parser
        .parse(content, None)
        .ok_or_else(|| ParseError::SyntaxError("Failed to parse Rust file".to_string()))?;

    let root = tree.root_node();

    // 1. Extract findings (commands + events) using the main query
    let findings = extract_rust_findings(root, content, &ts_lang)?;

    // 2. Extract command schemas
    let command_schemas =
        rust_type_extractor::extract_command_schemas_from_tree(root, content, path);

    // 3. Extract event schemas
    let event_schemas = rust_type_extractor::extract_event_schemas_from_tree(root, content, path);

    Ok(RustFileIndex {
        file_index: FileIndex {
            path: path.to_path_buf(),
            findings,
        },
        command_schemas,
        event_schemas,
    })
}
