//! Extract parameter and return type information from Rust #[`tauri::command`] functions

use crate::indexer::{CommandSchema, EventSchema, GeneratorKind, ParamSchema};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

const RUST_PARAMS_QUERY: &str = include_str!("queries/rust_params.scm");

/// Map a Rust type string to its TypeScript equivalent.
///
/// Examples:
/// - `u32` → `"number"`
/// - `String` → `"string"`
/// - `bool` → `"boolean"`
/// - `()` → `"void"`
/// - `Result<T, E>` → `rust_type_to_ts(T)`
/// - `Option<T>` → `rust_type_to_ts(T) + " | null"`
/// - `Vec<T>` → `rust_type_to_ts(T) + "[]"`
/// - unknown → pass through
#[must_use]
pub fn rust_type_to_ts(rust_type: &str) -> String {
    let t = rust_type.trim();

    // Primitives
    match t {
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" | "f32" | "f64" => return "number".to_string(),
        "String" | "&str" => return "string".to_string(),
        "bool" => return "boolean".to_string(),
        "()" => return "void".to_string(),
        _ => {}
    }

    // Result<T, E> → T
    if let Some(inner) = t.strip_prefix("Result<") {
        if let Some(ok_type) = extract_first_generic_arg(inner) {
            return rust_type_to_ts(ok_type);
        }
    }

    // Option<T> → T | null
    if let Some(inner) = t.strip_prefix("Option<") {
        if let Some(inner_str) = inner.strip_suffix('>') {
            return format!("{} | null", rust_type_to_ts(inner_str));
        }
        // Handle nested generics
        if let Some(inner_type) = extract_first_generic_arg(inner) {
            return format!("{} | null", rust_type_to_ts(inner_type));
        }
    }

    // Vec<T> → T[]
    if let Some(inner) = t.strip_prefix("Vec<") {
        if let Some(inner_str) = inner.strip_suffix('>') {
            return format!("{}[]", rust_type_to_ts(inner_str));
        }
        if let Some(inner_type) = extract_first_generic_arg(inner) {
            return format!("{}[]", rust_type_to_ts(inner_type));
        }
    }

    // Unknown type - pass through (user-defined struct)
    t.to_string()
}

/// Extract the first generic argument from a string like `T, E>` or `T>`.
fn extract_first_generic_arg(s: &str) -> Option<&str> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            '>' => {
                // End of the outer generic
                return Some(s[..i].trim());
            }
            ',' if depth == 0 => {
                return Some(s[..i].trim());
            }
            _ => {}
        }
    }
    None
}

/// Extract command schemas from a Rust source file.
///
/// Only extracts functions that have `#[tauri::command]` attribute.
/// Does NOT modify queries/rust.scm.
///
/// Prefer `extract_command_schemas_from_tree` when a parsed tree is already available.
#[must_use]
#[allow(dead_code)]
pub fn extract_command_schemas(content: &str, source_path: &Path) -> Vec<CommandSchema> {
    let Ok(schemas) = try_extract_command_schemas(content, source_path) else {
        return Vec::new();
    };
    schemas
}

/// Extract command schemas from a pre-parsed tree root node.
///
/// Use this when you already have a parsed tree (e.g. from `parse_rust_full`).
#[must_use]
pub fn extract_command_schemas_from_tree(
    root: tree_sitter::Node<'_>,
    content: &str,
    source_path: &Path,
) -> Vec<CommandSchema> {
    let Ok(schemas) = try_extract_command_schemas_from_node(root, content, source_path) else {
        return Vec::new();
    };
    schemas
}

#[allow(dead_code)]
fn try_extract_command_schemas(
    content: &str,
    source_path: &Path,
) -> Result<Vec<CommandSchema>, Box<dyn std::error::Error>> {
    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&ts_lang)?;

    let tree = parser
        .parse(content, None)
        .ok_or("Failed to parse Rust file")?;

    try_extract_command_schemas_from_node(tree.root_node(), content, source_path)
}

fn try_extract_command_schemas_from_node(
    root: tree_sitter::Node<'_>,
    content: &str,
    source_path: &Path,
) -> Result<Vec<CommandSchema>, Box<dyn std::error::Error>> {
    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&ts_lang, RUST_PARAMS_QUERY)?;
    let mut cursor = QueryCursor::new();

    let fn_name_idx = query.capture_index_for_name("fn_name");
    let fn_params_idx = query.capture_index_for_name("fn_params");
    let fn_return_idx = query.capture_index_for_name("fn_return");
    let fn_item_idx = query.capture_index_for_name("fn_item");

    let mut schemas = Vec::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Check that fn_item has a #[tauri::command] attribute
        if let Some(item_idx) = fn_item_idx {
            if let Some(item_cap) = m.captures.iter().find(|c| c.index == item_idx) {
                let fn_node = item_cap.node;
                if !has_tauri_command_attr(fn_node, content) {
                    continue;
                }
            }
        }

        let fn_name = fn_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("")
            .to_string();

        if fn_name.is_empty() {
            continue;
        }

        // Extract params
        let params = if let Some(params_idx) = fn_params_idx {
            if let Some(cap) = m.captures.iter().find(|c| c.index == params_idx) {
                parse_rust_params_from_node(cap.node, content)
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        // Extract return type
        let return_type = if let Some(ret_idx) = fn_return_idx {
            if let Some(cap) = m.captures.iter().find(|c| c.index == ret_idx) {
                let ret_text = cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                rust_type_to_ts(&ret_text)
            } else {
                "void".to_string()
            }
        } else {
            "void".to_string()
        };

        schemas.push(CommandSchema {
            command_name: fn_name,
            params,
            return_type,
            source_path: source_path.to_path_buf(),
            generator: GeneratorKind::RustSource,
        });
    }

    Ok(schemas)
}

/// Check if a struct has a derive attribute containing `Event` (covers both
/// `tauri_specta::Event` and its common alias `SpectaEvent`).
#[must_use]
pub fn has_specta_event_derive(struct_node: tree_sitter::Node<'_>, content: &str) -> bool {
    let Some(parent) = struct_node.parent() else {
        return false;
    };

    let mut cursor = parent.walk();
    let children: Vec<_> = parent.children(&mut cursor).collect();

    let Some(struct_idx) = children.iter().position(|n| n.id() == struct_node.id()) else {
        return false;
    };

    for sibling in children[..struct_idx].iter().rev() {
        let kind = sibling.kind();
        if kind == "attribute_item" {
            let text = sibling.utf8_text(content.as_bytes()).unwrap_or("");
            if text.contains("derive") && text.contains("Event") {
                return true;
            }
        } else if kind == "line_comment" || kind == "block_comment" {
            // Skip comments between attributes
        } else {
            break;
        }
    }

    false
}

/// Check if a function node has a `#[tauri::command]` or `#[command]` attribute
/// among its immediately-preceding siblings, skipping other attribute items and comments.
#[must_use]
pub fn has_tauri_command_attr(fn_node: tree_sitter::Node<'_>, content: &str) -> bool {
    // Walk through the siblings BEFORE this function node in its parent
    let Some(parent) = fn_node.parent() else {
        return false;
    };

    let mut cursor = parent.walk();
    let children: Vec<_> = parent.children(&mut cursor).collect();

    // Find the index of fn_node among siblings
    let Some(fn_idx) = children.iter().position(|n| n.id() == fn_node.id()) else {
        return false;
    };

    // Walk backwards from fn_idx, collecting only consecutive attribute_item nodes
    // Stop at the first non-attribute sibling (ignoring comments/whitespace)
    for sibling in children[..fn_idx].iter().rev() {
        let kind = sibling.kind();
        if kind == "attribute_item" {
            let text = sibling.utf8_text(content.as_bytes()).unwrap_or("");
            if text.contains("tauri::command") {
                return true;
            }
            // Another attribute, keep going back
        } else if kind == "line_comment" || kind == "block_comment" {
            // Skip comments between attributes and function
        } else {
            // Hit a real statement — stop searching
            break;
        }
    }

    false
}

/// Extract parameters from a tree-sitter `parameters` node.
///
/// Iterates `parameter` children, extracting name and type.
/// Skips Tauri-injected parameters: `AppHandle`, `State<_>`, `Window`.
fn parse_rust_params_from_node(
    params_node: tree_sitter::Node<'_>,
    content: &str,
) -> Vec<ParamSchema> {
    let mut result = Vec::new();
    let mut cursor = params_node.walk();

    for child in params_node.children(&mut cursor) {
        if child.kind() != "parameter" {
            continue;
        }

        let name = child
            .child_by_field_name("pattern")
            .and_then(|n| n.utf8_text(content.as_bytes()).ok())
            .unwrap_or("")
            .to_string();

        // Skip `self` parameter
        if name == "self" || name == "&self" || name == "&mut self" {
            continue;
        }

        let rust_type = child
            .child_by_field_name("type")
            .and_then(|n| n.utf8_text(content.as_bytes()).ok())
            .unwrap_or("")
            .trim();

        if name.is_empty() || rust_type.is_empty() {
            continue;
        }

        // Skip Tauri-injected parameters (not user data)
        let skip_types = ["AppHandle", "Window", "WebviewWindow", "Webview"];
        if skip_types.contains(&rust_type) || rust_type.starts_with("State<") {
            continue;
        }

        let ts_type = rust_type_to_ts(rust_type);
        result.push(ParamSchema { name, ts_type });
    }

    result
}

// ─── Event schema extraction from Rust source ────────────────────────────────

const RUST_EMIT_QUERY: &str = include_str!("queries/rust_emit.scm");

/// Extract event schemas from a Rust source file by finding `emit("event", payload)` calls.
///
/// For each emit call, attempts to resolve the payload variable's type from the enclosing
/// function's parameters.
#[must_use]
#[allow(dead_code)]
pub fn extract_event_schemas(content: &str, source_path: &Path) -> Vec<EventSchema> {
    let Ok(schemas) = try_extract_event_schemas(content, source_path) else {
        return Vec::new();
    };
    schemas
}

/// Extract event schemas from a pre-parsed tree root node.
///
/// Use this when you already have a parsed tree (e.g. from `parse_rust_full`).
#[must_use]
pub fn extract_event_schemas_from_tree(
    root: tree_sitter::Node<'_>,
    content: &str,
    tree: &tree_sitter::Tree,
    source_path: &Path,
) -> Vec<EventSchema> {
    let Ok(schemas) = try_extract_event_schemas_from_node(root, content, tree, source_path) else {
        return Vec::new();
    };
    schemas
}

#[allow(dead_code)]
fn try_extract_event_schemas(
    content: &str,
    source_path: &Path,
) -> Result<Vec<EventSchema>, Box<dyn std::error::Error>> {
    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&ts_lang)?;

    let tree = parser
        .parse(content, None)
        .ok_or("Failed to parse Rust file")?;

    try_extract_event_schemas_from_node(tree.root_node(), content, &tree, source_path)
}

fn try_extract_event_schemas_from_node(
    root: tree_sitter::Node<'_>,
    content: &str,
    tree: &tree_sitter::Tree,
    source_path: &Path,
) -> Result<Vec<EventSchema>, Box<dyn std::error::Error>> {
    let ts_lang: Language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&ts_lang, RUST_EMIT_QUERY)?;
    let mut cursor = QueryCursor::new();

    let event_name_idx = query.capture_index_for_name("event_name");
    let payload_arg_idx = query.capture_index_for_name("payload_arg");

    let mut schemas = Vec::new();
    let mut seen_events = std::collections::HashSet::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        let event_name = event_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        if event_name.is_empty() || !seen_events.insert(event_name.to_string()) {
            continue;
        }

        let payload_type = payload_arg_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| {
                let node = cap.node;
                // Try to resolve type from the payload expression
                resolve_emit_payload_type(node, content, tree)
            })
            .unwrap_or_else(|| "unknown".to_string());

        if payload_type != "unknown" {
            schemas.push(EventSchema {
                event_name: event_name.to_string(),
                payload_type,
                source_path: source_path.to_path_buf(),
                generator: GeneratorKind::RustSource,
            });
        }
    }

    Ok(schemas)
}

/// Try to resolve the type of an emit payload argument.
///
/// Handles:
/// - String literal → "string"
/// - Numeric literal → "number"
/// - Boolean literal → "boolean"
/// - Struct expression → extract struct name
/// - Variable reference → look up in function params, then local `let` bindings
fn resolve_emit_payload_type(
    node: tree_sitter::Node<'_>,
    content: &str,
    tree: &tree_sitter::Tree,
) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?;

    match node.kind() {
        "string_literal" => Some("string".to_string()),
        "integer_literal" | "float_literal" => Some("number".to_string()),
        "boolean_literal" | "true" | "false" => Some("boolean".to_string()),
        "struct_expression" => {
            // Direct struct literal: app.emit("event", Payload { ... })
            // First named child is the type name (type_identifier or scoped_type_identifier)
            let name_node = node.child_by_field_name("name")?;
            let struct_name = name_node.utf8_text(content.as_bytes()).ok()?;
            Some(rust_type_to_ts(struct_name))
        }
        "identifier" => {
            // Look up variable name in enclosing function parameters
            let var_name = text;
            let fn_node = find_enclosing_function(node, tree)?;

            // First try function parameters
            if let Some(params_node) = fn_node.child_by_field_name("parameters") {
                for param in parse_rust_params_from_node(params_node, content) {
                    if param.name == var_name {
                        return Some(param.ts_type);
                    }
                }
            }

            // Fallback: look for local `let` binding in enclosing function body
            resolve_local_variable_type(fn_node, var_name, content)
        }
        _ => None,
    }
}

/// Walk up the tree to find the enclosing `function_item`.
fn find_enclosing_function<'a>(
    node: tree_sitter::Node<'a>,
    _tree: &'a tree_sitter::Tree,
) -> Option<tree_sitter::Node<'a>> {
    let mut current = node.parent();
    while let Some(n) = current {
        if n.kind() == "function_item" {
            return Some(n);
        }
        current = n.parent();
    }
    None
}

/// Resolve the type of a local variable by scanning `let` declarations in the function body.
///
/// Handles:
/// - `let var: Type = ...;` → extract type annotation
/// - `let var = StructName { ... };` → extract struct name from `struct_expression`
fn resolve_local_variable_type(
    fn_node: tree_sitter::Node<'_>,
    var_name: &str,
    content: &str,
) -> Option<String> {
    let body = fn_node.child_by_field_name("body")?;
    let mut cursor = body.walk();

    for child in body.children(&mut cursor) {
        if child.kind() != "let_declaration" {
            continue;
        }

        // Check if the pattern matches our variable name
        let Some(pattern) = child.child_by_field_name("pattern") else {
            continue;
        };
        let Ok(pat_text) = pattern.utf8_text(content.as_bytes()) else {
            continue;
        };
        if pat_text != var_name {
            continue;
        }

        // Try type annotation first: `let payload: Payload = ...`
        if let Some(type_node) = child.child_by_field_name("type") {
            if let Ok(type_text) = type_node.utf8_text(content.as_bytes()) {
                return Some(rust_type_to_ts(type_text));
            }
        }

        // Try value: `let payload = Payload { ... }`
        if let Some(value_node) = child.child_by_field_name("value") {
            if value_node.kind() == "struct_expression" {
                if let Some(name_node) = value_node.child_by_field_name("name") {
                    if let Ok(struct_name) = name_node.utf8_text(content.as_bytes()) {
                        return Some(rust_type_to_ts(struct_name));
                    }
                }
            }
        }

        return None;
    }

    None
}
