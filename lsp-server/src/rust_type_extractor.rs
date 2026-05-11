//! Extract parameter and return type information from Rust #[`tauri::command`] functions

use crate::indexer::{CommandSchema, EventSchema, GeneratorKind, ParamSchema};
use crate::utils::{capture_text, find_capture};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Query, QueryCursor};

const RUST_PARAMS_QUERY: &str = include_str!("queries/rust_params.scm");

const TAURI_SELF_PARAMS: &[&str] = &["self", "&self", "&mut self"];
const TAURI_INJECTED_TYPES: &[&str] = &["AppHandle", "Window", "WebviewWindow", "Webview"];

fn setup_rust_query(query_str: &str) -> Option<(Query, QueryCursor)> {
    let lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&lang, query_str).ok()?;
    Some((query, QueryCursor::new()))
}

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

    match classify_rust_type(t) {
        RustType::Number => "number".to_string(),
        RustType::Str => "string".to_string(),
        RustType::Bool => "boolean".to_string(),
        RustType::Unit => "void".to_string(),
        RustType::Result => extract_first_generic_arg_from_type(t)
            .map_or_else(|| t.to_string(), |ok| rust_type_to_ts(&ok)),
        RustType::Option => extract_first_generic_arg_from_type(t).map_or_else(
            || t.to_string(),
            |inner| format!("{} | null", rust_type_to_ts(&inner)),
        ),
        RustType::Vec => extract_first_generic_arg_from_type(t).map_or_else(
            || t.to_string(),
            |inner| format!("{}[]", rust_type_to_ts(&inner)),
        ),
        RustType::Other => t.to_string(),
    }
}

enum RustType {
    Number,
    Str,
    Bool,
    Unit,
    Result,
    Option,
    Vec,
    Other,
}

fn classify_rust_type(t: &str) -> RustType {
    match t {
        "u8" | "u16" | "u32" | "u64" | "u128" | "usize" | "i8" | "i16" | "i32" | "i64" | "i128"
        | "isize" | "f32" | "f64" => RustType::Number,
        "String" | "&str" => RustType::Str,
        "bool" => RustType::Bool,
        "()" => RustType::Unit,
        _ if t.starts_with("Result<") => RustType::Result,
        _ if t.starts_with("Option<") => RustType::Option,
        _ if t.starts_with("Vec<") => RustType::Vec,
        _ => RustType::Other,
    }
}

/// Extract the first generic type argument from a full generic type string
/// (e.g. `Result<fn() -> bool, String>` → `fn() -> bool`).
///
/// Uses tree-sitter to correctly parse complex types including function pointers,
/// nested generics, and other Rust type syntax.
fn extract_first_generic_arg_from_type(full_type: &str) -> Option<String> {
    let wrapper = format!("type _X = {full_type};");
    let tree = crate::ts_tree_utils::parse_rust(&wrapper)?;
    let root = tree.root_node();

    // Navigate: source_file > type_item > type node > type_arguments > first child type
    let type_item = root.named_child(0)?;

    // The type value is the last named child of type_item (after `type`, name, `=`)
    let type_node = type_item.child_by_field_name("type")?;

    // For generic types, type_node is `generic_type` with `type_arguments`
    let type_args = type_node.child_by_field_name("type_arguments")?;

    // First named child of type_arguments (skipping `<` and `,` tokens)
    let first_arg = type_args.named_child(0)?;
    let arg_text = &wrapper[first_arg.byte_range()];

    Some(arg_text.to_string())
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

fn try_extract_command_schemas_from_node(
    root: tree_sitter::Node<'_>,
    content: &str,
    source_path: &Path,
) -> Result<Vec<CommandSchema>, Box<dyn std::error::Error>> {
    let (query, mut cursor) =
        setup_rust_query(RUST_PARAMS_QUERY).ok_or("failed to build rust_params query")?;

    let fn_name_idx = query.capture_index_for_name("fn_name");
    let fn_params_idx = query.capture_index_for_name("fn_params");
    let fn_return_idx = query.capture_index_for_name("fn_return");
    let fn_item_idx = query.capture_index_for_name("fn_item");

    let mut schemas = Vec::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        // Check that fn_item has a #[tauri::command] attribute
        if let Some(item_cap) = find_capture(m, fn_item_idx) {
            if !crate::rust_attr::has_tauri_command_attr(item_cap.node, content) {
                continue;
            }
        }

        let fn_name = capture_text(m, fn_name_idx, content.as_bytes()).to_string();

        if fn_name.is_empty() {
            continue;
        }

        // Extract params
        let params = find_capture(m, fn_params_idx)
            .map(|cap| parse_rust_params_from_node(cap.node, content))
            .unwrap_or_default();

        // Extract return type
        let return_type = find_capture(m, fn_return_idx).map_or_else(
            || "void".to_string(),
            |cap| {
                let ret_text = cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("")
                    .trim()
                    .to_string();
                rust_type_to_ts(&ret_text)
            },
        );

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

// Attribute detection utilities (has_tauri_command_attr, has_specta_event_derive)
// are in the `rust_attr` module.

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

        if TAURI_SELF_PARAMS.contains(&name.as_str()) {
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

        if TAURI_INJECTED_TYPES.contains(&rust_type) || rust_type.starts_with("State<") {
            continue;
        }

        let ts_type = rust_type_to_ts(rust_type);

        result.push(ParamSchema { name, ts_type });
    }

    result
}

// ─── Event schema extraction from Rust source ────────────────────────────────

const RUST_EMIT_QUERY: &str = include_str!("queries/rust_emit.scm");

/// Extract event schemas from a pre-parsed tree root node.
///
/// Use this when you already have a parsed tree (e.g. from `parse_rust_full`).
#[must_use]
pub fn extract_event_schemas_from_tree(
    root: tree_sitter::Node<'_>,
    content: &str,
    source_path: &Path,
) -> Vec<EventSchema> {
    let Ok(schemas) = try_extract_event_schemas_from_node(root, content, source_path) else {
        return Vec::new();
    };

    schemas
}

fn try_extract_event_schemas_from_node(
    root: tree_sitter::Node<'_>,
    content: &str,
    source_path: &Path,
) -> Result<Vec<EventSchema>, Box<dyn std::error::Error>> {
    let (query, mut cursor) =
        setup_rust_query(RUST_EMIT_QUERY).ok_or("failed to build rust_emit query")?;

    let event_name_idx = query.capture_index_for_name("event_name");
    let payload_arg_idx = query.capture_index_for_name("payload_arg");

    let mut schemas = Vec::new();
    let mut seen_events = std::collections::HashSet::new();
    let mut matches = cursor.matches(&query, root, content.as_bytes());

    while let Some(m) = matches.next() {
        let event_name = capture_text(m, event_name_idx, content.as_bytes());
        if event_name.is_empty() || !seen_events.insert(event_name.to_string()) {
            continue;
        }

        let payload_type = find_capture(m, payload_arg_idx)
            .and_then(|cap| resolve_emit_payload_type(cap.node, content))
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
/// - Reference expression (&val) → recurse
/// - Parenthesized expression ((val)) → recurse
fn resolve_emit_payload_type(node: tree_sitter::Node<'_>, content: &str) -> Option<String> {
    let text = node.utf8_text(content.as_bytes()).ok()?;

    match node.kind() {
        "string_literal" => Some("string".to_string()),
        "integer_literal" | "float_literal" => Some("number".to_string()),
        "boolean_literal" | "true" | "false" => Some("boolean".to_string()),
        "struct_expression" => {
            let name_node = node.child_by_field_name("name")?;
            let struct_name = name_node.utf8_text(content.as_bytes()).ok()?;
            Some(rust_type_to_ts(struct_name))
        }
        "identifier" => {
            let var_name = text;
            let fn_node = find_enclosing_function(node)?;

            if let Some(params_node) = fn_node.child_by_field_name("parameters") {
                for param in parse_rust_params_from_node(params_node, content) {
                    if param.name == var_name {
                        return Some(param.ts_type);
                    }
                }
            }

            resolve_local_variable_type(node, var_name, content)
        }
        "reference_expression" => {
            let inner = node.child_by_field_name("value")?;
            resolve_emit_payload_type(inner, content)
        }
        "parenthesized_expression" => {
            let inner = node.named_child(0)?;
            resolve_emit_payload_type(inner, content)
        }
        _ => None,
    }
}

/// Walk up the tree to find the enclosing `function_item`.
fn find_enclosing_function(node: tree_sitter::Node<'_>) -> Option<tree_sitter::Node<'_>> {
    let mut current = node.parent();

    while let Some(n) = current {
        if n.kind() == "function_item" {
            return Some(n);
        }

        current = n.parent();
    }

    None
}

/// Try to extract type info from a `let_declaration` node matching `var_name`.
///
/// Returns `Some(type)` if the binding matches and has a recoverable type,
/// `None` if the binding does not match this variable.
/// Returns `Some("unknown")` would be misleading — we just return `None` to let
/// the caller keep searching.
fn check_let_declaration(
    s: tree_sitter::Node<'_>,
    var_name: &str,
    content: &str,
) -> Option<String> {
    if s.kind() != "let_declaration" {
        return None;
    }

    let pattern = s.child_by_field_name("pattern")?;
    if pattern.utf8_text(content.as_bytes()).ok()? != var_name {
        return None;
    }

    // `let var: Type = ...` → type annotation wins
    if let Some(type_node) = s.child_by_field_name("type") {
        if let Ok(type_text) = type_node.utf8_text(content.as_bytes()) {
            return Some(rust_type_to_ts(type_text));
        }
    }

    // `let var = StructName { ... }` → struct name
    if let Some(value_node) = s.child_by_field_name("value") {
        if value_node.kind() == "struct_expression" {
            if let Some(name_node) = value_node.child_by_field_name("name") {
                if let Ok(struct_name) = name_node.utf8_text(content.as_bytes()) {
                    return Some(rust_type_to_ts(struct_name));
                }
            }
        }
    }

    // Pattern matched but no type info recoverable
    None
}

/// Resolve the type of a local variable by searching backwards from the site of the variable's use.
///
/// Handles:
/// - `let var: Type = ...;` → extract type annotation
/// - `let var = StructName { ... };` → extract struct name from `struct_expression`
fn resolve_local_variable_type(
    usage_node: tree_sitter::Node<'_>,
    var_name: &str,
    content: &str,
) -> Option<String> {
    let mut current = usage_node;

    loop {
        let mut sibling = current.prev_sibling();
        while let Some(s) = sibling {
            if let Some(ty) = check_let_declaration(s, var_name, content) {
                return Some(ty);
            }
            sibling = s.prev_sibling();
        }

        if let Some(parent) = current.parent() {
            if parent.kind() == "function_item" {
                break;
            }
            current = parent;
        } else {
            break;
        }
    }

    None
}
