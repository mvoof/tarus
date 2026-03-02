//! Extract parameter and return type information from Rust #[`tauri::command`] functions

use crate::indexer::{CommandSchema, EventSchema, GeneratorKind, ParamSchema};
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// Embedded query for extracting Rust command parameter and return types.
/// Uses a local query, NOT modifying queries/rust.scm.
const RUST_PARAMS_QUERY: &str = r"
(function_item
  name: (identifier) @fn_name
  parameters: (parameters) @fn_params
  return_type: (_)? @fn_return
) @fn_item
";

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
#[must_use]
pub fn extract_command_schemas(content: &str, source_path: &Path) -> Vec<CommandSchema> {
    let Ok(schemas) = try_extract_command_schemas(content, source_path) else {
        return Vec::new();
    };
    schemas
}

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

    let query = Query::new(&ts_lang, RUST_PARAMS_QUERY)?;
    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

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
                let params_text = cap
                    .node
                    .utf8_text(content.as_bytes())
                    .unwrap_or("()")
                    .to_string();
                parse_rust_params(&params_text)
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

/// Parse a Rust parameter list string like `(app: AppHandle, id: u32)` into `Vec<ParamSchema>`.
fn parse_rust_params(params_str: &str) -> Vec<ParamSchema> {
    // Remove surrounding parentheses
    let inner = params_str
        .trim()
        .strip_prefix('(')
        .and_then(|s| s.strip_suffix(')'))
        .unwrap_or(params_str);

    if inner.trim().is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();

    for ch in inner.chars() {
        match ch {
            '<' => {
                depth += 1;
                current.push(ch);
            }
            '>' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                if let Some(param) = parse_rust_param(current.trim()) {
                    result.push(param);
                }
                current = String::new();
            }
            _ => current.push(ch),
        }
    }

    if let Some(param) = parse_rust_param(current.trim()) {
        result.push(param);
    }

    result
}

/// Parse a single Rust parameter like `id: u32` or `app: AppHandle`.
///
/// Skips Tauri-injected parameters: `AppHandle`, `State<_>`, `Window`.
fn parse_rust_param(s: &str) -> Option<ParamSchema> {
    let colon_pos = s.find(':')?;
    let name = s[..colon_pos].trim().to_string();
    let rust_type = s[colon_pos + 1..].trim();

    // Skip Tauri-injected parameters (not user data)
    let skip_types = ["AppHandle", "Window", "WebviewWindow", "Webview"];
    if skip_types.contains(&rust_type) || rust_type.starts_with("State<") {
        return None;
    }

    // Skip `self` parameter
    if name == "self" || name == "&self" || name == "&mut self" {
        return None;
    }

    let ts_type = rust_type_to_ts(rust_type);

    Some(ParamSchema { name, ts_type })
}

// ─── Event schema extraction from Rust source ────────────────────────────────

/// Embedded query for extracting `emit("event-name", payload)` calls from Rust source.
const RUST_EMIT_QUERY: &str = r#"
(call_expression
  function: (field_expression
    field: (field_identifier) @method_name)
  arguments: (arguments
    (string_literal (string_content) @event_name)
    . (_) @payload_arg)
  (#any-of? @method_name "emit" "emit_filter" "emit_to")
)
"#;

/// Extract event schemas from a Rust source file by finding `emit("event", payload)` calls.
///
/// For each emit call, attempts to resolve the payload variable's type from the enclosing
/// function's parameters.
#[must_use]
pub fn extract_event_schemas(content: &str, source_path: &Path) -> Vec<EventSchema> {
    let Ok(schemas) = try_extract_event_schemas(content, source_path) else {
        return Vec::new();
    };
    schemas
}

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

    let query = Query::new(&ts_lang, RUST_EMIT_QUERY)?;
    let mut cursor = QueryCursor::new();
    let root = tree.root_node();

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
                resolve_emit_payload_type(node, content, &tree)
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
/// Handles simple cases:
/// - String literal → "string"
/// - Numeric literal → "number"
/// - Boolean literal → "boolean"
/// - Variable reference → look up in enclosing function params
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
        "identifier" => {
            // Look up variable name in enclosing function parameters
            let var_name = text;
            let fn_node = find_enclosing_function(node, tree)?;
            let params_node = fn_node.child_by_field_name("parameters")?;
            let params_text = params_node.utf8_text(content.as_bytes()).ok()?;

            // Parse params to find the type of var_name
            for param in parse_rust_params(params_text) {
                if param.name == var_name {
                    return Some(param.ts_type);
                }
            }
            None
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
