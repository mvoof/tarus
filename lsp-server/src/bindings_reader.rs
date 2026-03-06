//! Parsers for generated TypeScript binding files
//!
//! Supports three generators:
//! - tauri-specta: `export const commands = { async methodName(...): Promise<...> { ... } }`
//! - ts-rs: `export type Name = { ... };`
//! - tauri-typegen: `export type Name = { ... };`

use crate::indexer::{CommandSchema, EventSchema, GeneratorKind, ParamSchema};
use crate::ts_tree_utils::parse_ts;
use crate::utils::camel_to_snake;
use std::collections::HashMap;
use std::path::Path;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Query, QueryCursor};

/// Parse a Specta-generated bindings file and return a list of `CommandSchema`.
///
/// Expects lines like:
/// ```text
/// async getUserProfile(id: number): Promise<Result<UserProfile, string>> {
/// ```
/// inside an `export const commands = { ... }` block.
#[must_use]
pub fn parse_specta_bindings(content: &str, source_path: &Path) -> Vec<CommandSchema> {
    let Some(tree) = parse_ts(content) else {
        return Vec::new();
    };

    let ts_lang: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();

    let query_str = include_str!("queries/bindings_specta_commands.scm");

    let Ok(query) = Query::new(&ts_lang, query_str) else {
        return Vec::new();
    };

    let var_name_idx = query.capture_index_for_name("var_name");
    let method_name_idx = query.capture_index_for_name("method_name");
    let params_idx = query.capture_index_for_name("params");
    let return_type_idx = query.capture_index_for_name("return_type");

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
    let mut schemas = Vec::new();

    while let Some(m) = matches.next() {
        // Only match `commands` variable
        let var_name = var_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        if var_name != "commands" {
            continue;
        }

        let method_name = method_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        if method_name.is_empty() {
            continue;
        }

        let params = params_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .map(|cap| extract_params_from_node(cap.node, content))
            .unwrap_or_default();

        let return_type_node = return_type_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .map(|cap| cap.node);

        let return_type = return_type_node.map_or_else(
            || "void".to_string(),
            |node| unwrap_return_type_node(node, content),
        );

        schemas.push(CommandSchema {
            command_name: camel_to_snake(method_name),
            params,
            return_type,
            source_path: source_path.to_path_buf(),
            generator: GeneratorKind::Specta,
        });
    }

    schemas
}

/// Extract parameters from a `formal_parameters` node.
fn extract_params_from_node(params_node: tree_sitter::Node<'_>, content: &str) -> Vec<ParamSchema> {
    let mut params = Vec::new();
    let mut cursor = params_node.walk();

    for child in params_node.children(&mut cursor) {
        if child.kind() == "required_parameter" || child.kind() == "optional_parameter" {
            let name = child
                .child_by_field_name("pattern")
                .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                .unwrap_or("")
                .to_string();

            let ts_type = child
                .child_by_field_name("type")
                .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                .map_or("", |t| t.strip_prefix(": ").unwrap_or(t))
                .trim()
                .to_string();

            if !name.is_empty() && !ts_type.is_empty() {
                params.push(ParamSchema { name, ts_type });
            }
        }
    }

    params
}

/// Unwrap `Promise<Result<T, E>>` → `T`, `Promise<T>` → `T` using tree-sitter node walking.
///
/// Navigates `generic_type` → `name` + `type_arguments` children instead of manual bracket tracking.
fn unwrap_return_type_node(node: tree_sitter::Node<'_>, content: &str) -> String {
    let mut current = node;

    loop {
        if current.kind() != "generic_type" {
            break;
        }

        let type_name = current
            .child_by_field_name("name")
            .and_then(|n| n.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        match type_name {
            "Promise" | "Result" => {
                let Some(type_args) = current.child_by_field_name("type_arguments") else {
                    break;
                };
                let mut cursor = type_args.walk();
                let first_arg = type_args
                    .children(&mut cursor)
                    .find(tree_sitter::Node::is_named);
                let Some(arg) = first_arg else {
                    break;
                };
                current = arg;
            }
            _ => break,
        }
    }

    current
        .utf8_text(content.as_bytes())
        .unwrap_or("void")
        .trim()
        .to_string()
}

/// Parse a ts-rs generated file and return a map of `TypeName -> definition`.
///
/// Looks for lines like:
/// ```text
/// export type UserProfile = { id: number; name: string };
/// ```
#[must_use]
pub fn parse_ts_rs_types(content: &str) -> HashMap<String, String> {
    parse_type_aliases_and_interfaces(content, false)
}

/// Parse a typegen-generated file and return a map of `TypeName -> definition`.
///
/// Handles both:
/// - `export type Name = ...;`  (inline type aliases)
/// - `export interface Name { field: type; ... }` (multi-line interface blocks)
#[must_use]
pub fn parse_typegen_types(content: &str) -> HashMap<String, String> {
    parse_type_aliases_and_interfaces(content, true)
}

/// Unified parser for `export type` and `export interface` declarations using tree-sitter.
fn parse_type_aliases_and_interfaces(
    content: &str,
    include_interfaces: bool,
) -> HashMap<String, String> {
    let Some(tree) = parse_ts(content) else {
        return HashMap::new();
    };

    let ts_lang: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    let mut aliases = HashMap::new();

    let type_query_str = include_str!("queries/bindings_type_aliases.scm");

    if let Ok(query) = Query::new(&ts_lang, type_query_str) {
        let name_idx = query.capture_index_for_name("type_name");
        let value_idx = query.capture_index_for_name("type_value");

        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

        while let Some(m) = matches.next() {
            let name = name_idx
                .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
                .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
                .unwrap_or("")
                .to_string();

            let def = value_idx
                .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
                .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
                .unwrap_or("")
                .to_string();

            if !name.is_empty() && !def.is_empty() {
                aliases.insert(name, def);
            }
        }
    }

    // Query for interface declarations (typegen only)
    if include_interfaces {
        let iface_query_str = include_str!("queries/bindings_interfaces.scm");

        if let Ok(query) = Query::new(&ts_lang, iface_query_str) {
            let name_idx = query.capture_index_for_name("iface_name");
            let body_idx = query.capture_index_for_name("iface_body");

            let mut cursor = QueryCursor::new();
            let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

            while let Some(m) = matches.next() {
                let name = name_idx
                    .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
                    .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
                    .unwrap_or("")
                    .to_string();

                let body_node = body_idx
                    .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
                    .map(|cap| cap.node);

                if let Some(body) = body_node {
                    if !name.is_empty() {
                        let def = extract_interface_fields(body, content);
                        if !def.is_empty() {
                            aliases.insert(name, def);
                        }
                    }
                }
            }
        }
    }

    aliases
}

/// Extract fields from an `interface_body` node into a compact inline object string.
///
/// Skips index signatures like `[key: string]: unknown`.
fn extract_interface_fields(body_node: tree_sitter::Node<'_>, content: &str) -> String {
    let mut fields = Vec::new();
    let mut cursor = body_node.walk();

    for child in body_node.children(&mut cursor) {
        if child.kind() == "property_signature" {
            let name = child
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                .unwrap_or("");

            let type_text = child
                .child_by_field_name("type")
                .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                .map_or("", |t| t.strip_prefix(": ").unwrap_or(t).trim());

            if !name.is_empty() && !type_text.is_empty() {
                fields.push(format!("{name}: {type_text}"));
            }
        }
        // index_signature nodes are automatically skipped
    }

    if fields.is_empty() {
        String::new()
    } else {
        format!("{{ {} }}", fields.join("; "))
    }
}

// ─── Event schema parsers ────────────────────────────────────────────────────

/// Parse event schemas from a Specta-generated bindings file.
///
/// Looks for the `__makeEvents__` block:
/// ```text
/// export const events = __makeEvents__<{
///     DemoEvent: string,
///     UserUpdated: UserProfile,
/// }>({
///     DemoEvent: "demo-event",
///     UserUpdated: "user-updated",
/// })
/// ```
///
/// The type parameter maps TS names to payload types.
/// The value object maps TS names to actual event name strings.
#[must_use]
pub fn parse_specta_events(content: &str, source_path: &Path) -> Vec<EventSchema> {
    let Some(tree) = parse_ts(content) else {
        return Vec::new();
    };

    let ts_lang: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();

    let query_str = include_str!("queries/bindings_specta_events.scm");

    let Ok(query) = Query::new(&ts_lang, query_str) else {
        return Vec::new();
    };

    let fn_name_idx = query.capture_index_for_name("fn_name");
    let type_obj_idx = query.capture_index_for_name("type_obj");
    let value_obj_idx = query.capture_index_for_name("value_obj");

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
    let mut schemas = Vec::new();

    while let Some(m) = matches.next() {
        let fn_name = fn_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        if fn_name != "__makeEvents__" {
            continue;
        }

        // Extract type map: {TypeName: PayloadType, ...}
        let type_map = type_obj_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .map(|cap| extract_type_object_entries(cap.node, content))
            .unwrap_or_default();

        // Extract value map: {TypeName: "event-name", ...}
        let value_node = value_obj_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .map(|cap| cap.node);

        if let Some(vnode) = value_node {
            let mut vcursor = vnode.walk();
            for child in vnode.children(&mut vcursor) {
                if child.kind() == "pair" {
                    let key = child
                        .child_by_field_name("key")
                        .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                        .unwrap_or("");

                    let value = child
                        .child_by_field_name("value")
                        .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                        .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
                        .unwrap_or_default();

                    if !value.is_empty() {
                        if let Some(payload_type) = type_map.get(key) {
                            schemas.push(EventSchema {
                                event_name: value,
                                payload_type: payload_type.clone(),
                                source_path: source_path.to_path_buf(),
                                generator: GeneratorKind::Specta,
                            });
                        }
                    }
                }
            }
        }
    }

    schemas
}

/// Extract entries from a type object like `{ Name: Type, ... }` (inside `<{...}>`).
fn extract_type_object_entries(
    node: tree_sitter::Node<'_>,
    content: &str,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "property_signature" {
            let name = child
                .child_by_field_name("name")
                .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                .unwrap_or("");

            let type_text = child
                .child_by_field_name("type")
                .and_then(|n| n.utf8_text(content.as_bytes()).ok())
                .map(|t| t.strip_prefix(": ").unwrap_or(t).trim().to_string())
                .unwrap_or_default();

            if !name.is_empty() && !type_text.is_empty() {
                map.insert(name.to_string(), type_text);
            }
        }
    }

    map
}

/// Parse event schemas from a typegen-generated events file.
///
/// Looks for `listen<T>('event-name', ...)` patterns:
/// ```text
/// export async function onNotificationSent(
///   handler: (payload: types.message) => void
/// ): Promise<UnlistenFn> {
///   return listen<types.message>('notification-sent', (event) => {
///     handler(event.payload);
///   });
/// }
/// ```
#[must_use]
pub fn parse_typegen_events(content: &str, source_path: &Path) -> Vec<EventSchema> {
    let Some(tree) = parse_ts(content) else {
        return Vec::new();
    };

    let ts_lang: Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();

    let query_str = include_str!("queries/bindings_typegen_events.scm");

    let Ok(query) = Query::new(&ts_lang, query_str) else {
        return Vec::new();
    };

    let fn_name_idx = query.capture_index_for_name("fn_name");
    let type_arg_idx = query.capture_index_for_name("type_arg");
    let event_name_idx = query.capture_index_for_name("event_name");

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());
    let mut schemas = Vec::new();

    while let Some(m) = matches.next() {
        let fn_name = fn_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        if fn_name != "listen" {
            continue;
        }

        let type_arg = type_arg_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        let event_name = event_name_idx
            .and_then(|idx| m.captures.iter().find(|c| c.index == idx))
            .and_then(|cap| cap.node.utf8_text(content.as_bytes()).ok())
            .unwrap_or("");

        if event_name.is_empty() || type_arg.is_empty() {
            continue;
        }

        // Strip "types." prefix if present
        let (clean_type, was_prefixed) = if let Some(stripped) = type_arg.strip_prefix("types.") {
            (stripped.to_string(), true)
        } else {
            (type_arg.to_string(), false)
        };

        // Skip lowercase names from types.X — these are variable names, not types
        if !clean_type.is_empty() && (!was_prefixed || clean_type.starts_with(char::is_uppercase)) {
            schemas.push(EventSchema {
                event_name: event_name.to_string(),
                payload_type: clean_type,
                source_path: source_path.to_path_buf(),
                generator: GeneratorKind::Typegen,
            });
        }
    }

    schemas
}
