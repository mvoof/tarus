//! Unified Tree-sitter based parser for Rust and frontend languages
//!
//! This module provides a single entry point for parsing all supported file types
//! using Tree-sitter queries defined in external .scm files.

mod extractors;
pub mod frontend_parser;
pub mod patterns;
pub mod query_helpers;
pub mod rust_parser;
mod sfc;
mod utils;

#[allow(unused_imports)]
pub use extractors::FindingBuilder;
#[allow(unused_imports)]
pub use query_helpers::CaptureIndices;
#[allow(unused_imports)]
pub use utils::{LangType, NodeTextExt};

use crate::indexer::{FileIndex, Finding};
use crate::syntax::ParseResult;
use std::path::Path;

use frontend_parser::{is_angular_file, parse_frontend};
use rust_parser::parse_rust;
use sfc::extract_script_blocks;

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
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();

    let lang = LangType::from_extension(ext);

    let mut findings: Vec<Finding> = match lang {
        Some(LangType::Rust) => parse_rust(path, content)?,
        Some(LangType::TypeScript) => {
            // Check if it's an Angular component
            // Check if it's an Angular component
            if is_angular_file(content) {
                parse_frontend(path, content, LangType::Angular, 0)?
            } else {
                parse_frontend(path, content, LangType::TypeScript, 0)?
            }
        }
        Some(LangType::JavaScript) => parse_frontend(path, content, LangType::JavaScript, 0)?,
        Some(LangType::Vue | LangType::Svelte) => {
            // Extract script blocks and parse each one
            let script_blocks = extract_script_blocks(content);
            let mut all_findings = Vec::new();

            for (script_content, line_offset) in script_blocks {
                let block_findings =
                    parse_frontend(path, &script_content, LangType::TypeScript, line_offset)?;
                all_findings.extend(block_findings);
            }

            all_findings
        }
        Some(LangType::Angular) => parse_frontend(path, content, LangType::Angular, 0)?,
        None => Vec::new(),
    };

    // Sort findings by their range
    findings.sort_by(|a, b| {
        a.range
            .start
            .line
            .cmp(&b.range.start.line)
            .then_with(|| a.range.start.character.cmp(&b.range.start.character))
    });

    Ok(FileIndex {
        path: path.to_path_buf(),
        findings,
    })
}
