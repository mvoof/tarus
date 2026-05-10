//! Data-driven test helpers using expect-test (rust-analyzer style)
//!
//! ## Fixture format
//!
//! ```text
//! //- /backend.rs
//! #[tauri::command]
//! fn greet() {}
//!
//! //- /frontend.ts
//! invoke("gre$0et")
//! ```
//!
//! - `//- /path` — file separator, extension determines language
//! - `$0` — cursor position (removed before parsing)
//! - `//- /bindings.ts [specta]` — routes file through bindings reader
//! - `$SCHEMA greet(name: string): string` — inject CommandSchema (bindings generator)
//! - `$RUST_SCHEMA greet(name: string): string` — inject CommandSchema (RustSource generator)
//! - `$EVENT_SCHEMA user-updated(UserPayload)` — inject EventSchema
//! - `$TYPE_ALIAS UserPayload = { id: number; name: string }` — inject type alias

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dashmap::DashMap;
use expect_test::Expect;
use lsp_server::capabilities::{
    code_actions, code_lens, completion, definition, diagnostics, hover, references, symbols,
};
use lsp_server::indexer::{CommandSchema, EventSchema, GeneratorKind, ParamSchema, ProjectIndex};
use lsp_server::syntax::{Behavior, EntityType};
use lsp_server::tree_parser;

use tower_lsp_server::lsp_types::*;
use tower_lsp_server::UriExt;

// ---------------------------------------------------------------------------
// Fixture data
// ---------------------------------------------------------------------------

pub struct FixtureData {
    pub index: ProjectIndex,
    pub cursor_file: Option<PathBuf>,
    pub cursor_position: Option<Position>,
    pub contents: HashMap<PathBuf, String>,
}

// ---------------------------------------------------------------------------
// Fixture parsing
// ---------------------------------------------------------------------------

/// Parse an inline multi-file fixture into a `FixtureData`.
pub fn parse_fixture(input: &str) -> FixtureData {
    let index = ProjectIndex::new();
    let mut contents = HashMap::new();
    let mut cursor_file: Option<PathBuf> = None;
    let mut cursor_position: Option<Position> = None;

    // Collect directives (can appear before any file block or inside file blocks)
    let mut schemas: Vec<String> = Vec::new();
    let mut rust_schemas: Vec<String> = Vec::new();
    let mut event_schemas: Vec<String> = Vec::new();
    let mut type_aliases: Vec<String> = Vec::new();

    // Scan for directives BEFORE any file block
    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//- ") {
            break; // Stop at first file block
        }
        if let Some(rest) = trimmed.strip_prefix("$SCHEMA ") {
            schemas.push(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("$RUST_SCHEMA ") {
            rust_schemas.push(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("$EVENT_SCHEMA ") {
            event_schemas.push(rest.to_string());
        } else if let Some(rest) = trimmed.strip_prefix("$TYPE_ALIAS ") {
            type_aliases.push(rest.to_string());
        }
    }

    // Split into file blocks
    let blocks = split_fixture_blocks(input);

    for (raw_path, generator_tag, mut content) in blocks {
        // Extract directives from content
        let mut clean_lines = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("$SCHEMA ") {
                schemas.push(rest.to_string());
            } else if let Some(rest) = trimmed.strip_prefix("$RUST_SCHEMA ") {
                rust_schemas.push(rest.to_string());
            } else if let Some(rest) = trimmed.strip_prefix("$EVENT_SCHEMA ") {
                event_schemas.push(rest.to_string());
            } else if let Some(rest) = trimmed.strip_prefix("$TYPE_ALIAS ") {
                type_aliases.push(rest.to_string());
            } else {
                clean_lines.push(line);
            }
        }
        content = clean_lines.join("\n");

        // Find and remove $0 cursor marker
        if let Some(pos) = find_cursor(&content) {
            let path = abs_test_path(&raw_path);
            cursor_file = Some(path);
            cursor_position = Some(pos);
            content = content.replace("$0", "");
        }

        let path = abs_test_path(&raw_path);

        if let Some(ref tag) = generator_tag {
            // Route through bindings reader
            let gen_kind = match tag.as_str() {
                "specta" => GeneratorKind::Specta,
                "ts-rs" => GeneratorKind::TsRs,
                "typegen" => GeneratorKind::Typegen,
                _ => panic!("Unknown generator tag: {tag}"),
            };

            // Parse command schemas (specta only)
            if gen_kind == GeneratorKind::Specta {
                let cmd_schemas =
                    lsp_server::bindings_reader::parse_specta_bindings(&content, &path);
                for schema in cmd_schemas {
                    index.add_schema(schema);
                }

                let evt_schemas = lsp_server::bindings_reader::parse_specta_events(&content, &path);
                for schema in evt_schemas {
                    index.add_event_schema(schema);
                }
            }

            // Parse type aliases (Specta, ts-rs, and typegen)
            let aliases = match gen_kind {
                GeneratorKind::Specta | GeneratorKind::TsRs | GeneratorKind::Typegen => {
                    lsp_server::bindings_reader::parse_typescript_types(&content)
                }
                _ => std::collections::HashMap::new(),
            };
            for (alias_name, alias_def) in aliases {
                index.add_type_alias(alias_name, alias_def, path.clone());
            }

            // Parse typegen events
            if gen_kind == GeneratorKind::Typegen {
                let evt_schemas =
                    lsp_server::bindings_reader::parse_typegen_events(&content, &path);
                for schema in evt_schemas {
                    index.add_event_schema(schema);
                }
            }
        }

        // Always parse with tree_parser (even bindings files get normal parsing too)
        let parse_result = tree_parser::parse(&path, &content);
        match parse_result {
            Ok(file_index) => {
                index.add_file(file_index);
            }
            Err(e) => {
                index.set_parse_error(path.clone(), format!("{e:?}"));
            }
        }

        contents.insert(path, content);
    }

    // Apply $SCHEMA directives
    for schema_str in schemas {
        let schema = parse_schema_directive(&schema_str);
        let source_path = PathBuf::from("/test/__directives__");
        index.add_schema(CommandSchema {
            source_path: source_path.clone(),
            ..schema
        });
    }

    // Apply $RUST_SCHEMA directives (RustSource generator — NOT from bindings)
    for schema_str in rust_schemas {
        let mut schema = parse_schema_directive(&schema_str);
        schema.generator = GeneratorKind::RustSource;
        schema.source_path = PathBuf::from("/test/__rust_source__");
        index.add_schema(schema);
    }

    // Apply $EVENT_SCHEMA directives
    for schema_str in event_schemas {
        let schema = parse_event_schema_directive(&schema_str);
        let source_path = PathBuf::from("/test/__directives__");
        index.add_event_schema(EventSchema {
            source_path: source_path.clone(),
            ..schema
        });
    }

    // Apply $TYPE_ALIAS directives
    for alias_str in type_aliases {
        let (name, def) = parse_type_alias_directive(&alias_str);
        let source_path = PathBuf::from("/test/__directives__");
        index.add_type_alias(name, def, source_path);
    }

    FixtureData {
        index,
        cursor_file,
        cursor_position,
        contents,
    }
}

fn abs_test_path(relative: &str) -> PathBuf {
    PathBuf::from("/test").join(relative.strip_prefix('/').unwrap_or(relative))
}

fn find_cursor(content: &str) -> Option<Position> {
    for (line_idx, line) in content.lines().enumerate() {
        if let Some(col) = line.find("$0") {
            return Some(Position {
                line: line_idx as u32,
                character: col as u32,
            });
        }
    }
    None
}

/// Split fixture input into (path, optional_generator_tag, content) blocks
fn split_fixture_blocks(input: &str) -> Vec<(String, Option<String>, String)> {
    let mut blocks = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_tag: Option<String> = None;
    let mut current_lines: Vec<&str> = Vec::new();

    for line in input.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("//- ") {
            // Flush previous block
            if let Some(path) = current_path.take() {
                blocks.push((path, current_tag.take(), current_lines.join("\n")));
                current_lines.clear();
            }

            // Parse path and optional tag: "/path [tag]"
            let (path, tag) = if let Some(bracket_start) = rest.find('[') {
                let bracket_end = rest.find(']').unwrap_or(rest.len());
                let path = rest[..bracket_start].trim().to_string();
                let tag = rest[bracket_start + 1..bracket_end].trim().to_string();
                (path, Some(tag))
            } else {
                (rest.trim().to_string(), None)
            };

            current_path = Some(path);
            current_tag = tag;
        } else if current_path.is_some() {
            current_lines.push(line);
        }
    }

    // Flush last block
    if let Some(path) = current_path {
        blocks.push((path, current_tag, current_lines.join("\n")));
    }

    blocks
}

// ---------------------------------------------------------------------------
// Directive parsers
// ---------------------------------------------------------------------------

/// Parse `greet(name: string, age: number): string` into CommandSchema
fn parse_schema_directive(s: &str) -> CommandSchema {
    let (name_and_params, return_type) = s
        .split_once("):")
        .map(|(a, b)| (format!("{a})"), b.trim().to_string()))
        .unwrap_or_else(|| {
            let (a, _) = s.split_once(')').unwrap_or((s, ""));
            (format!("{a})"), "void".to_string())
        });

    let paren_start = name_and_params.find('(').unwrap_or(name_and_params.len());
    let command_name = name_and_params[..paren_start].trim().to_string();
    let params_str = &name_and_params[paren_start + 1..name_and_params.len() - 1];

    let params = if params_str.trim().is_empty() {
        vec![]
    } else {
        params_str
            .split(',')
            .map(|p| {
                let parts: Vec<&str> = p.trim().splitn(2, ':').collect();
                ParamSchema {
                    name: parts[0].trim().to_string(),
                    ts_type: parts
                        .get(1)
                        .map(|s| s.trim().to_string())
                        .unwrap_or_default(),
                }
            })
            .collect()
    };

    CommandSchema {
        command_name,
        params,
        return_type,
        source_path: PathBuf::new(),
        generator: GeneratorKind::Specta,
    }
}

/// Parse `user-updated(UserPayload)` into EventSchema
fn parse_event_schema_directive(s: &str) -> EventSchema {
    let paren_start = s.find('(').unwrap_or(s.len());
    let event_name = s[..paren_start].trim().to_string();
    let payload_type = if paren_start < s.len() {
        let end = s.find(')').unwrap_or(s.len());
        s[paren_start + 1..end].trim().to_string()
    } else {
        "void".to_string()
    };

    EventSchema {
        event_name,
        payload_type,
        source_path: PathBuf::new(),
        generator: GeneratorKind::Specta,
    }
}

/// Parse `UserPayload = { id: number; name: string }` into (name, definition)
fn parse_type_alias_directive(s: &str) -> (String, String) {
    let (name, def) = s.split_once('=').expect("$TYPE_ALIAS must contain '='");
    (name.trim().to_string(), def.trim().to_string())
}

// ---------------------------------------------------------------------------
// Output formatters
// ---------------------------------------------------------------------------

fn format_range(r: Range) -> String {
    format!(
        "{}:{}..{}:{}",
        r.start.line, r.start.character, r.end.line, r.end.character
    )
}

fn short_path(p: &Path) -> String {
    // Strip "/test" prefix for readability
    let s = p.to_string_lossy();
    s.strip_prefix("/test").unwrap_or(&s).to_string()
}

fn format_behavior(b: &Behavior) -> &'static str {
    match b {
        Behavior::Definition => "Definition",
        Behavior::Call => "Call",
        Behavior::SpectaCall => "SpectaCall",
        Behavior::Emit => "Emit",
        Behavior::Listen => "Listen",
    }
}

fn format_entity(e: &EntityType) -> &'static str {
    match e {
        EntityType::Command => "Command",
        EntityType::Event => "Event",
    }
}

// ---------------------------------------------------------------------------
// check_* functions
// ---------------------------------------------------------------------------

/// Check parsing results for all files in fixture
pub fn check_parse(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let mut out = String::new();

    // Collect findings per file from the index
    let mut files: Vec<PathBuf> = data.contents.keys().cloned().collect();
    files.sort();

    for file_path in &files {
        let content = &data.contents[file_path];
        let parse_result = tree_parser::parse(file_path, content);

        match parse_result {
            Ok(file_index) => {
                if file_index.findings.is_empty() {
                    continue;
                }
                writeln!(out, "{}:", short_path(file_path)).unwrap();
                let mut findings = file_index.findings;
                findings.sort_by_key(|f| (f.range.start.line, f.range.start.character));
                for f in &findings {
                    let mut line = format!(
                        "  {} {} \"{}\" {}",
                        format_entity(&f.entity),
                        format_behavior(&f.behavior),
                        f.key,
                        format_range(f.range),
                    );
                    if let Some(keys) = &f.call_param_keys {
                        write!(line, " params=[{}]", keys.join(", ")).unwrap();
                    }
                    if let Some(rt) = &f.return_type {
                        write!(line, " return_type={rt}").unwrap();
                    }
                    if let Some(count) = f.call_arg_count {
                        write!(line, " args={count}").unwrap();
                    }
                    writeln!(out, "{line}").unwrap();
                }
            }
            Err(e) => {
                writeln!(out, "{}: ERROR {e:?}", short_path(file_path)).unwrap();
            }
        }
    }

    expect.assert_eq(out.trim_end());
}

/// Check Go to Definition results (cursor at $0)
pub fn check_definition(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let (file, pos) = cursor(&data);
    let params = make_definition_params(&file, pos);
    let result = definition::handle_goto_definition(params, &data.index);

    let out = match result {
        None => "(none)".to_string(),
        Some(GotoDefinitionResponse::Link(links)) => {
            let mut lines: Vec<String> = links
                .iter()
                .filter_map(|link| {
                    let path = link.target_uri.to_file_path()?;
                    Some(format!(
                        "{} {}",
                        short_path(&path),
                        format_range(link.target_selection_range),
                    ))
                })
                .collect();
            lines.sort();
            lines.join("\n")
        }
        Some(GotoDefinitionResponse::Array(locs)) => {
            let mut lines: Vec<String> = locs
                .iter()
                .filter_map(|loc| {
                    let path = loc.uri.to_file_path()?;
                    Some(format!("{} {}", short_path(&path), format_range(loc.range)))
                })
                .collect();
            lines.sort();
            lines.join("\n")
        }
        Some(GotoDefinitionResponse::Scalar(loc)) => {
            if let Some(path) = loc.uri.to_file_path() {
                format!("{} {}", short_path(&path), format_range(loc.range))
            } else {
                "(uri error)".to_string()
            }
        }
    };

    expect.assert_eq(&out);
}

/// Check Find References results (cursor at $0)
pub fn check_references(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let (file, pos) = cursor(&data);
    let params = make_reference_params(&file, pos);
    let result = references::handle_references(params, &data.index);

    let out = match result {
        None => "(none)".to_string(),
        Some(locs) => {
            let mut lines: Vec<String> = locs
                .iter()
                .filter_map(|loc| {
                    let path = loc.uri.to_file_path()?;
                    Some(format!("{} {}", short_path(&path), format_range(loc.range)))
                })
                .collect();
            lines.sort();
            lines.join("\n")
        }
    };

    expect.assert_eq(&out);
}

/// Check CodeLens results ($0 marks the target file)
pub fn check_code_lens(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let file = data.cursor_file.as_ref().unwrap_or_else(|| {
        data.contents
            .keys()
            .next()
            .expect("fixture must have files")
    });
    let params = make_code_lens_params(file);
    let result = code_lens::handle_code_lens(params, &data.index);

    let out = match result {
        None => "(none)".to_string(),
        Some(lenses) => {
            let mut lines: Vec<String> = lenses
                .iter()
                .map(|lens| {
                    let title = lens
                        .command
                        .as_ref()
                        .map(|c| c.title.as_str())
                        .unwrap_or("(no command)");
                    format!(
                        "{}:{} \"{}\"",
                        lens.range.start.line, lens.range.start.character, title
                    )
                })
                .collect();
            lines.sort();
            lines.join("\n")
        }
    };

    expect.assert_eq(&out);
}

/// Check Hover result (cursor at $0)
pub fn check_hover(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let (file, pos) = cursor(&data);
    let params = make_hover_params(&file, pos);
    let result = hover::handle_hover(params, &data.index);

    let out = match result {
        None => "(none)".to_string(),
        Some(hover) => match hover.contents {
            HoverContents::Markup(markup) => markup.value,
            HoverContents::Scalar(MarkedString::String(s)) => s,
            HoverContents::Scalar(MarkedString::LanguageString(ls)) => ls.value,
            HoverContents::Array(items) => items
                .iter()
                .map(|i| match i {
                    MarkedString::String(s) => s.clone(),
                    MarkedString::LanguageString(ls) => ls.value.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        },
    };

    expect.assert_eq(out.trim_end());
}

/// Check Completion results (cursor at $0)
pub fn check_completion(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let (file, pos) = cursor(&data);
    let params = make_completion_params(&file, pos);
    let doc_cache = make_document_cache(&data.contents);
    let result = completion::handle_completion(&params, &data.index, &doc_cache);

    let out = match result {
        None => "(none)".to_string(),
        Some(CompletionResponse::Array(items)) => {
            let mut labels: Vec<String> = items.iter().map(|i| i.label.clone()).collect();
            labels.sort();
            labels.dedup();
            labels.join("\n")
        }
        Some(CompletionResponse::List(list)) => {
            let mut labels: Vec<String> = list.items.iter().map(|i| i.label.clone()).collect();
            labels.sort();
            labels.dedup();
            labels.join("\n")
        }
    };

    expect.assert_eq(&out);
}

/// Check Diagnostics for the file containing $0
pub fn check_diagnostics(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let file = data.cursor_file.clone().unwrap_or_else(|| {
        data.contents
            .keys()
            .next()
            .expect("fixture must have files")
            .clone()
    });
    let result = diagnostics::compute_file_diagnostics(&file, &data.index);

    let out = if result.is_empty() {
        "(none)".to_string()
    } else {
        let mut lines: Vec<String> = result
            .iter()
            .map(|d| {
                let severity = match d.severity {
                    Some(DiagnosticSeverity::ERROR) => "ERROR",
                    Some(DiagnosticSeverity::WARNING) => "WARNING",
                    Some(DiagnosticSeverity::INFORMATION) => "INFO",
                    Some(DiagnosticSeverity::HINT) => "HINT",
                    _ => "???",
                };
                let code_str = match &d.code {
                    Some(NumberOrString::String(s)) => format!(" [{s}]"),
                    Some(NumberOrString::Number(n)) => format!(" [{n}]"),
                    None => String::new(),
                };
                format!(
                    "{} {} \"{}\"{code_str}",
                    severity,
                    format_range(d.range),
                    d.message,
                )
            })
            .collect();
        lines.sort();
        lines.join("\n")
    };

    expect.assert_eq(&out);
}

/// Check Code Actions at $0
pub fn check_code_actions(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let (file, pos) = cursor(&data);
    let params = make_code_action_params(&file, pos);
    let result = code_actions::handle_code_action(&params, &data.index, None);

    let out = match result {
        None => "(none)".to_string(),
        Some(actions) => {
            let mut lines = Vec::new();
            for action in &actions {
                match action {
                    CodeActionOrCommand::CodeAction(a) => {
                        let kind = a
                            .kind
                            .as_ref()
                            .map(|k| k.as_str().to_string())
                            .unwrap_or_default();
                        lines.push(format!("\"{}\" [{kind}]", a.title));

                        // Show edits
                        if let Some(edit) = &a.edit {
                            if let Some(DocumentChanges::Edits(edits)) = &edit.document_changes {
                                for doc_edit in edits {
                                    let doc_path = doc_edit
                                        .text_document
                                        .uri
                                        .to_file_path()
                                        .as_deref()
                                        .map(short_path)
                                        .unwrap_or_else(|| "(uri)".to_string());
                                    for e in &doc_edit.edits {
                                        if let OneOf::Left(text_edit) = e {
                                            let r = &text_edit.range;
                                            if r.start == r.end {
                                                lines.push(format!(
                                                    "  edit {doc_path} {}:{} insert {:?}",
                                                    r.start.line,
                                                    r.start.character,
                                                    text_edit.new_text,
                                                ));
                                            } else {
                                                lines.push(format!(
                                                    "  edit {doc_path} {} replace {:?}",
                                                    format_range(text_edit.range),
                                                    text_edit.new_text,
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    CodeActionOrCommand::Command(c) => {
                        lines.push(format!("cmd \"{}\"", c.title));
                    }
                }
            }
            lines.join("\n")
        }
    };

    expect.assert_eq(&out);
}

/// Check Document Symbols ($0 marks the target file)
pub fn check_document_symbols(fixture: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let file = data.cursor_file.as_ref().unwrap_or_else(|| {
        data.contents
            .keys()
            .next()
            .expect("fixture must have files")
    });
    let params = make_document_symbol_params(file);
    let result = symbols::handle_document_symbol(params, &data.index);

    let out = match result {
        None => "(none)".to_string(),
        Some(DocumentSymbolResponse::Flat(syms)) => {
            let mut lines: Vec<String> = syms
                .iter()
                .map(|s| {
                    format!(
                        "{:?} \"{}\" {}",
                        s.kind,
                        s.name,
                        format_range(s.location.range),
                    )
                })
                .collect();
            lines.sort();
            lines.join("\n")
        }
        Some(DocumentSymbolResponse::Nested(syms)) => {
            let mut lines: Vec<String> = syms
                .iter()
                .map(|s| format!("{:?} \"{}\" {}", s.kind, s.name, format_range(s.range),))
                .collect();
            lines.sort();
            lines.join("\n")
        }
    };

    expect.assert_eq(&out);
}

/// Check Workspace Symbol search
pub fn check_workspace_symbols(fixture: &str, query: &str, expect: Expect) {
    let data = parse_fixture(fixture);
    let params = make_workspace_symbol_params(query);
    let result = symbols::handle_workspace_symbol(&params, &data.index);

    let out = match result {
        None => "(none)".to_string(),
        Some(tower_lsp_server::lsp_types::OneOf::Left(syms)) => {
            let mut lines: Vec<String> = syms
                .iter()
                .map(|s| {
                    let path = s
                        .location
                        .uri
                        .to_file_path()
                        .as_deref()
                        .map(short_path)
                        .unwrap_or_default();
                    format!("{:?} \"{}\" {path}", s.kind, s.name)
                })
                .collect();
            lines.sort();
            lines.join("\n")
        }
        Some(tower_lsp_server::lsp_types::OneOf::Right(syms)) => {
            let mut lines: Vec<String> = syms
                .iter()
                .map(|s| format!("{:?} \"{}\"", s.kind, s.name))
                .collect();
            lines.sort();
            lines.join("\n")
        }
    };

    expect.assert_eq(&out);
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn cursor(data: &FixtureData) -> (PathBuf, Position) {
    let file = data
        .cursor_file
        .clone()
        .expect("fixture must contain $0 cursor marker");
    let pos = data.cursor_position.expect("cursor position must exist");
    (file, pos)
}

fn make_definition_params(path: &Path, position: Position) -> GotoDefinitionParams {
    GotoDefinitionParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Uri::from_file_path(path).unwrap(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn make_reference_params(path: &Path, position: Position) -> ReferenceParams {
    ReferenceParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Uri::from_file_path(path).unwrap(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: ReferenceContext {
            include_declaration: true,
        },
    }
}

fn make_hover_params(path: &Path, position: Position) -> HoverParams {
    HoverParams {
        text_document_position_params: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Uri::from_file_path(path).unwrap(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
    }
}

fn make_code_lens_params(path: &Path) -> CodeLensParams {
    CodeLensParams {
        text_document: TextDocumentIdentifier {
            uri: Uri::from_file_path(path).unwrap(),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn make_document_symbol_params(path: &Path) -> DocumentSymbolParams {
    DocumentSymbolParams {
        text_document: TextDocumentIdentifier {
            uri: Uri::from_file_path(path).unwrap(),
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn make_workspace_symbol_params(query: &str) -> WorkspaceSymbolParams {
    WorkspaceSymbolParams {
        query: query.to_string(),
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn make_code_action_params(path: &Path, position: Position) -> CodeActionParams {
    CodeActionParams {
        text_document: TextDocumentIdentifier {
            uri: Uri::from_file_path(path).unwrap(),
        },
        range: Range {
            start: position,
            end: position,
        },
        context: CodeActionContext {
            diagnostics: vec![],
            only: None,
            trigger_kind: None,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
    }
}

fn make_completion_params(path: &Path, position: Position) -> CompletionParams {
    CompletionParams {
        text_document_position: TextDocumentPositionParams {
            text_document: TextDocumentIdentifier {
                uri: Uri::from_file_path(path).unwrap(),
            },
            position,
        },
        work_done_progress_params: Default::default(),
        partial_result_params: Default::default(),
        context: None,
    }
}

fn make_document_cache(contents: &HashMap<PathBuf, String>) -> Arc<DashMap<PathBuf, String>> {
    let cache = Arc::new(DashMap::new());
    for (path, content) in contents {
        cache.insert(path.clone(), content.clone());
    }
    cache
}
