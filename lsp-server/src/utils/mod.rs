//! Shared utility functions

pub mod ts_tree_utils;

use std::collections::HashSet;
use std::sync::LazyLock;
use streaming_iterator::StreamingIterator;
use tower_lsp_server::lsp_types::{Position, Range};
use tree_sitter::{Query, QueryCursor};

static RUST_CONSTANTS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let ts_lang = tree_sitter_rust::LANGUAGE.into();
    Query::new(
        &ts_lang,
        r"
        (const_item
          name: (identifier) @name
          value: [
            (string_literal (string_content) @value)
            (raw_string_literal (string_content) @value)
            (identifier) @reference
            (scoped_identifier name: (identifier) @reference)
          ]
        )
        (static_item
          name: (identifier) @name
          value: [
            (string_literal (string_content) @value)
            (raw_string_literal (string_content) @value)
            (identifier) @reference
            (scoped_identifier name: (identifier) @reference)
          ]
        )
        ",
    )
    .expect("Failed to parse Rust constants query")
});

static JS_CONSTANTS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let ts_lang = tree_sitter_javascript::LANGUAGE.into();
    Query::new(
        &ts_lang,
        r"
        (variable_declarator
          name: (identifier) @name
          value: (string (string_fragment) @value)
        )
        (variable_declarator
          name: (identifier) @object_name
          value: (object
            (pair
              key: (property_identifier) @prop_name
              value: (string (string_fragment) @value)
            )
          )
        )
        ",
    )
    .expect("Failed to parse JS constants query")
});

static TS_CONSTANTS_QUERY: LazyLock<Query> = LazyLock::new(|| {
    let ts_lang = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
    Query::new(
        &ts_lang,
        r"
        (variable_declarator
          name: (identifier) @name
          value: [
            (string (string_fragment) @value)
            (as_expression (string (string_fragment) @value))
            (type_assertion (string (string_fragment) @value))
          ]
        )
        (variable_declarator
          name: (identifier) @object_name
          value: (object
            (pair
              key: (property_identifier) @prop_name
              value: [
                (string (string_fragment) @value)
                (as_expression (string (string_fragment) @value))
                (type_assertion (string (string_fragment) @value))
              ]
            )
          )
        )
        ",
    )
    .expect("Failed to parse TS constants query")
});

/// Find a tree-sitter capture by its `Option<u32>` index within a match.
///
/// Returns `None` if `idx` is `None` or the capture is not found.
#[must_use]
pub fn find_capture<'a>(
    m: &'a tree_sitter::QueryMatch<'a, '_>,
    idx: Option<u32>,
) -> Option<&'a tree_sitter::QueryCapture<'a>> {
    idx.and_then(|i| m.captures.iter().find(|c| c.index == i))
}

/// How many `const A: &str = B;` hops are followed before a chain is given up on.
/// Real chains are one or two hops; the limit only stops a cycle.
const MAX_CONSTANT_REFERENCE_DEPTH: usize = 8;

/// String constants declared in one Rust file.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RustConstants {
    /// `const NAME: &str = "value"` (or `r"value"`): name to value.
    pub values: std::collections::HashMap<String, String>,
    /// `const NAME: &str = OTHER` (or `path::OTHER`): name to the name it points at.
    /// Only the last path segment is kept, the same way call sites are looked up.
    pub references: std::collections::HashMap<String, String>,
}

impl RustConstants {
    /// The values, plus every reference that resolves through them or `fallback`.
    #[must_use]
    pub fn resolved(
        &self,
        fallback: &std::collections::HashMap<String, String>,
    ) -> std::collections::HashMap<String, String> {
        let mut values = self.values.clone();
        resolve_constant_references(&mut values, &self.references, fallback);

        values
    }
}

/// Add to `values` every reference whose chain ends in a known value.
///
/// A name that already has a value of its own keeps it. A chain that ends in an
/// unknown name, or loops, adds nothing: the call site stays unresolved rather than
/// pointing at a guess.
pub fn resolve_constant_references<S: std::hash::BuildHasher>(
    values: &mut std::collections::HashMap<String, String, S>,
    references: &std::collections::HashMap<String, String, S>,
    fallback: &std::collections::HashMap<String, String, S>,
) {
    let resolved: Vec<(String, String)> = references
        .iter()
        .filter(|(name, _)| !values.contains_key(*name))
        .filter_map(|(name, target)| {
            let mut current = target;

            for _ in 0..MAX_CONSTANT_REFERENCE_DEPTH {
                if let Some(value) = values.get(current).or_else(|| fallback.get(current)) {
                    return Some((name.clone(), value.clone()));
                }

                current = references.get(current)?;
            }

            None
        })
        .collect();

    values.extend(resolved);
}

/// Extract string constants from a Rust AST root node, references resolved within
/// the file only.
#[must_use]
pub fn extract_rust_constants(
    root: tree_sitter::Node<'_>,
    content: &str,
) -> std::collections::HashMap<String, String> {
    extract_rust_constant_decls(root, content).resolved(&std::collections::HashMap::new())
}

/// Extract string constants and references to other constants from a Rust AST root
/// node, unresolved, for a workspace-wide map to resolve.
#[must_use]
pub fn extract_rust_constant_decls(root: tree_sitter::Node<'_>, content: &str) -> RustConstants {
    let mut constants = RustConstants::default();
    let mut cursor = QueryCursor::new();
    let bytes = content.as_bytes();
    let mut matches = cursor.matches(&RUST_CONSTANTS_QUERY, root, bytes);
    let name_idx = RUST_CONSTANTS_QUERY.capture_index_for_name("name");
    let value_idx = RUST_CONSTANTS_QUERY.capture_index_for_name("value");
    let reference_idx = RUST_CONSTANTS_QUERY.capture_index_for_name("reference");

    while let Some(m) = matches.next() {
        let Some(name_cap) = find_capture(m, name_idx) else {
            continue;
        };

        let name = name_cap
            .node
            .utf8_text(bytes)
            .unwrap_or_default()
            .to_string();

        if let Some(value_cap) = find_capture(m, value_idx) {
            let value = value_cap.node.utf8_text(bytes).unwrap_or_default();
            constants.values.insert(name, value.to_string());
        } else if let Some(reference_cap) = find_capture(m, reference_idx) {
            let target = reference_cap.node.utf8_text(bytes).unwrap_or_default();
            constants.references.insert(name, target.to_string());
        }
    }

    collect_macro_constants(root, bytes, &mut constants);

    constants
}

/// Collect `const NAME: &str = ...` written inside a macro invocation.
///
/// Tree-sitter does not parse a macro body as items: `ts_values! { pub const A: &str
/// = "a" => B; }` is one flat `token_tree`, so the query above never sees the const.
/// The tokens are still all there, so the declaration is matched by its shape:
/// `const`/`static`, a name, then what follows `=` — a string or raw string literal,
/// or a path to another constant that ends the declaration. Anything else
/// (`concat!`, a call) is skipped rather than guessed.
fn collect_macro_constants(
    root: tree_sitter::Node<'_>,
    bytes: &[u8],
    constants: &mut RustConstants,
) {
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        let mut cursor = node.walk();
        let children: Vec<tree_sitter::Node<'_>> = node.children(&mut cursor).collect();

        if node.kind() == "token_tree" {
            collect_constants_from_tokens(&children, bytes, constants);
        }

        stack.extend(children);
    }
}

/// Tokens that may make up a path to a constant: `crate::events::NAME`.
const PATH_TOKEN_KINDS: &[&str] = &["identifier", "crate", "self", "super", "::"];

/// Tokens that end a constant's value inside a macro body.
const DECLARATION_END_TOKENS: &[&str] = &[";", "=>", ","];

fn collect_constants_from_tokens(
    tokens: &[tree_sitter::Node<'_>],
    bytes: &[u8],
    constants: &mut RustConstants,
) {
    let text = |node: &tree_sitter::Node<'_>| node.utf8_text(bytes).unwrap_or_default();

    for (position, token) in tokens.iter().enumerate() {
        if !matches!(text(token), "const" | "static") {
            continue;
        }

        let Some(name) = tokens
            .get(position + 1)
            .filter(|n| n.kind() == "identifier")
        else {
            continue;
        };

        let name = text(name).to_string();

        if constants.values.contains_key(&name) || constants.references.contains_key(&name) {
            continue;
        }

        let rest = &tokens[position + 2..];
        let Some(equals) = rest
            .iter()
            .take_while(|n| text(n) != ";")
            .position(|n| text(n) == "=")
        else {
            continue;
        };

        let value_tokens = &rest[equals + 1..];
        let Some(first) = value_tokens.first() else {
            continue;
        };

        if matches!(first.kind(), "string_literal" | "raw_string_literal") {
            let value = first
                .named_child(0)
                .filter(|n| n.kind() == "string_content")
                .map_or("", |n| text(&n));

            constants.values.insert(name, value.to_string());

            continue;
        }

        let path_length = value_tokens
            .iter()
            .take_while(|n| PATH_TOKEN_KINDS.contains(&n.kind()))
            .count();

        let ends_declaration = value_tokens
            .get(path_length)
            .is_none_or(|n| DECLARATION_END_TOKENS.contains(&text(n)));

        let last_segment = path_length
            .checked_sub(1)
            .and_then(|last| value_tokens.get(last))
            .filter(|n| n.kind() == "identifier");

        if let (true, Some(target)) = (ends_declaration, last_segment) {
            constants.references.insert(name, text(target).to_string());
        }
    }
}

/// Collect every identifier the file binds itself: function parameters, catch
/// clauses and variable declarators.
///
/// A name bound here shadows any same-named constant found elsewhere in the
/// workspace, so cross-file constant resolution must not be applied to it. Scopes
/// are deliberately not modelled — the whole file is treated as one scope, which
/// can only make resolution give up, never invent a wrong name.
#[must_use]
pub fn collect_bound_names(root: tree_sitter::Node<'_>, content: &str) -> HashSet<String> {
    let mut bound = HashSet::new();
    let bytes = content.as_bytes();
    let mut cursor = root.walk();
    let mut stack = vec![root];

    while let Some(node) = stack.pop() {
        match node.kind() {
            "required_parameter" | "optional_parameter" => {
                collect_pattern_names(node.child_by_field_name("pattern"), bytes, &mut bound);
            }
            "arrow_function" => {
                collect_pattern_names(node.child_by_field_name("parameter"), bytes, &mut bound);
            }
            "variable_declarator" | "catch_clause" => {
                collect_pattern_names(node.child_by_field_name("name"), bytes, &mut bound);
                collect_pattern_names(node.child_by_field_name("parameter"), bytes, &mut bound);
            }
            "formal_parameters" => {
                for child in node.named_children(&mut node.walk()) {
                    if child.kind() == "identifier" {
                        collect_pattern_names(Some(child), bytes, &mut bound);
                    }
                }
            }
            _ => {}
        }

        for child in node.named_children(&mut cursor) {
            stack.push(child);
        }
    }

    bound
}

/// Add every identifier a single parameter binds to `bound`, unwrapping type
/// annotations, defaults and destructuring patterns.
#[allow(clippy::implicit_hasher)] // reason: callers use the default hasher throughout
pub fn collect_parameter_names(
    param: tree_sitter::Node<'_>,
    bytes: &[u8],
    bound: &mut HashSet<String>,
) {
    match param.kind() {
        "required_parameter" | "optional_parameter" => {
            collect_pattern_names(param.child_by_field_name("pattern"), bytes, bound);
        }
        _ => collect_pattern_names(Some(param), bytes, bound),
    }
}

/// Add the identifiers introduced by a binding pattern (plain, object or array
/// destructuring) to `bound`.
fn collect_pattern_names(
    node: Option<tree_sitter::Node<'_>>,
    bytes: &[u8],
    bound: &mut HashSet<String>,
) {
    let Some(node) = node else {
        return;
    };

    if node.kind() == "identifier" || node.kind() == "shorthand_property_identifier_pattern" {
        bound.insert(node.utf8_text(bytes).unwrap_or_default().to_string());

        return;
    }

    let mut cursor = node.walk();

    for child in node.named_children(&mut cursor) {
        collect_pattern_names(Some(child), bytes, bound);
    }
}

/// Extract constant definitions from a JavaScript/TypeScript AST root node
#[must_use]
pub fn extract_js_constants(
    root: tree_sitter::Node<'_>,
    content: &str,
    is_javascript: bool,
) -> std::collections::HashMap<String, String> {
    let mut constants = std::collections::HashMap::new();
    let query = if is_javascript {
        &*JS_CONSTANTS_QUERY
    } else {
        &*TS_CONSTANTS_QUERY
    };
    let mut cursor = QueryCursor::new();
    let bytes = content.as_bytes();
    let mut matches = cursor.matches(query, root, bytes);
    let name_idx = query.capture_index_for_name("name");
    let object_name_idx = query.capture_index_for_name("object_name");
    let prop_name_idx = query.capture_index_for_name("prop_name");
    let value_idx = query.capture_index_for_name("value");

    while let Some(m) = matches.next() {
        if let Some(val_cap) = find_capture(m, value_idx) {
            let value = val_cap
                .node
                .utf8_text(bytes)
                .unwrap_or_default()
                .to_string();

            if let Some(name_cap) = find_capture(m, name_idx) {
                let name = name_cap
                    .node
                    .utf8_text(bytes)
                    .unwrap_or_default()
                    .to_string();
                constants.insert(name, value);
            } else if let (Some(obj_cap), Some(prop_cap)) = (
                find_capture(m, object_name_idx),
                find_capture(m, prop_name_idx),
            ) {
                let obj_name = obj_cap.node.utf8_text(bytes).unwrap_or_default();
                let prop_name = prop_cap.node.utf8_text(bytes).unwrap_or_default();
                let full_key = format!("{obj_name}.{prop_name}");
                constants.insert(full_key, value.clone());
                constants.insert(prop_name.to_string(), value);
            }
        }
    }
    constants
}

/// Extract constants and unresolved references from Rust source content (parses
/// content internally)
#[must_use]
pub fn extract_rust_constant_decls_from_content(content: &str) -> RustConstants {
    let ts_lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&ts_lang).is_err() {
        return RustConstants::default();
    }
    let Some(tree) = parser.parse(content, None) else {
        return RustConstants::default();
    };
    extract_rust_constant_decls(tree.root_node(), content)
}

/// Extract constant definitions from JS/TS source content (parses content internally)
#[must_use]
pub fn extract_js_constants_from_content(
    content: &str,
    is_javascript: bool,
) -> std::collections::HashMap<String, String> {
    let ts_lang: tree_sitter::Language = if is_javascript {
        tree_sitter_javascript::LANGUAGE.into()
    } else {
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&ts_lang).is_err() {
        return std::collections::HashMap::new();
    }
    let Some(tree) = parser.parse(content, None) else {
        return std::collections::HashMap::new();
    };
    extract_js_constants(tree.root_node(), content, is_javascript)
}

/// Extract UTF-8 text from a tree-sitter capture by index.
///
/// Returns `""` if the capture is missing or text extraction fails.
#[must_use]
pub fn capture_text<'a>(
    m: &tree_sitter::QueryMatch<'_, '_>,
    idx: Option<u32>,
    content: &'a [u8],
) -> &'a str {
    idx.and_then(|i| m.captures.iter().find(|c| c.index == i))
        .and_then(|cap| cap.node.utf8_text(content).ok())
        .unwrap_or("")
}

/// Convert a camelCase or `PascalCase` identifier to `snake_case`
///
/// Examples:
/// - `getUserProfile` → `get_user_profile`
/// - `createUser` → `create_user`
/// - `ping` → `ping`
#[must_use]
pub fn camel_to_snake(s: &str) -> String {
    camel_to_separated(s, '_')
}

/// Convert tree-sitter Point to LSP Position
#[allow(clippy::cast_possible_truncation)]
#[must_use]
pub fn point_to_position(point: tree_sitter::Point) -> Position {
    Position {
        line: point.row as u32,
        character: point.column as u32,
    }
}

/// Check if a cursor position is inside an LSP range (inclusive start, exclusive end)
#[must_use]
pub fn is_position_in_range(pos: Position, range: Range) -> bool {
    if pos.line < range.start.line || pos.line > range.end.line {
        return false;
    }

    // Single line range
    if range.start.line == range.end.line {
        return pos.character >= range.start.character && pos.character < range.end.character;
    }

    // Multi-line range
    if pos.line == range.start.line {
        return pos.character >= range.start.character;
    }

    if pos.line == range.end.line {
        return pos.character < range.end.character;
    }

    true
}

/// Convert a camelCase or `PascalCase` identifier to `kebab-case`
///
/// Examples:
/// - `globalEvent` → `global-event`
/// - `myCustomEvent` → `my-custom-event`
/// - `ping` → `ping`
#[must_use]
pub fn camel_to_kebab(s: &str) -> String {
    camel_to_separated(s, '-')
}

/// Convert a camelCase or `PascalCase` identifier to a separated format
/// using the given separator character.
fn camel_to_separated(s: &str, separator: char) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            // Insert separator before uppercase if:
            // - not at position 0
            // - previous char is lowercase OR (next char is lowercase AND previous is uppercase)
            let prev_is_lower = i > 0 && chars[i - 1].is_ascii_lowercase();
            let next_is_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            let prev_is_upper = i > 0 && chars[i - 1].is_ascii_uppercase();

            if prev_is_lower || (next_is_lower && prev_is_upper) {
                result.push(separator);
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }

    result
}

/// Convert LSP character offset (UTF-16 code units) to byte index in a string
#[must_use]
pub fn lsp_character_to_byte_index(line: &str, character: usize) -> usize {
    let mut byte_index = 0;
    let mut char_count = 0;

    for (i, c) in line.char_indices() {
        if char_count == character {
            return i;
        }
        char_count += c.len_utf16();
        byte_index = i + c.len_utf8();
    }

    if char_count <= character {
        return byte_index;
    }

    line.len()
}
