//! Utilities for inspecting Rust tree-sitter attribute nodes.
//!
//! Detects `#[tauri::command]` / `#[command]` on functions and
//! `#[derive(...Event...)]` on structs.

/// Check if a function node has a `#[tauri::command]` or `#[command]` attribute
/// among its immediately-preceding siblings, skipping other attribute items and comments.
#[must_use]
pub fn has_tauri_command_attr(fn_node: tree_sitter::Node<'_>, content: &str) -> bool {
    has_preceding_attr(fn_node, content, |text| {
        text == "#[tauri::command]" || text == "#[command]"
    })
}

/// Check if a struct has a derive attribute containing `Event` (covers
/// `tauri_specta::Event`, its common alias `SpectaEvent`, and bare `Event`).
///
/// Parses the derive argument list to check each trait individually,
/// avoiding false positives from unrelated derives whose names happen
/// to contain "Event" (e.g. `EventEmitter`).
#[must_use]
pub fn has_specta_event_derive(struct_node: tree_sitter::Node<'_>, content: &str) -> bool {
    has_preceding_attr(struct_node, content, |text| {
        is_derive_with_event_trait(text)
    })
}

/// Return true if `attr_text` is a `#[derive(...)]` attribute where one of
/// the comma-separated arguments is exactly `Event`, `SpectaEvent`, or
/// a path ending in `::Event` (e.g. `tauri_specta::Event`).
fn is_derive_with_event_trait(attr_text: &str) -> bool {
    // attr_text looks like "#[derive(Clone, tauri_specta::Event)]"
    let Some(inner) = attr_text.strip_prefix("#[derive(") else {
        return false;
    };

    let Some(inner) = inner.strip_suffix(")]") else {
        return false;
    };

    inner.split(',').any(|arg| {
        let arg = arg.trim();

        arg == "Event" || arg == "SpectaEvent" || arg.ends_with("::Event")
    })
}

/// Walk backwards from `node` through preceding siblings, checking each
/// `attribute_item` with `predicate`. Skips comments; stops at any other node kind.
fn has_preceding_attr(
    node: tree_sitter::Node<'_>,
    content: &str,
    predicate: impl Fn(&str) -> bool,
) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };

    let mut cursor = parent.walk();
    let children: Vec<_> = parent.children(&mut cursor).collect();

    let Some(idx) = children.iter().position(|n| n.id() == node.id()) else {
        return false;
    };

    for sibling in children[..idx].iter().rev() {
        let kind = sibling.kind();

        if kind == "attribute_item" {
            let text = sibling.utf8_text(content.as_bytes()).unwrap_or("");

            if predicate(text) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::{Language, Parser};

    fn parse_rust(content: &str) -> tree_sitter::Tree {
        let lang: Language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = Parser::new();

        parser.set_language(&lang).unwrap();
        parser.parse(content, None).unwrap()
    }

    /// Find the first node of given kind in the tree (DFS).
    fn find_node<'a>(node: tree_sitter::Node<'a>, kind: &str) -> Option<tree_sitter::Node<'a>> {
        if node.kind() == kind {
            return Some(node);
        }

        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            if let Some(found) = find_node(child, kind) {
                return Some(found);
            }
        }
        None
    }

    // ── has_tauri_command_attr ───────────────────────────────────────────

    #[test]
    fn detects_full_tauri_command_attr() {
        let src = r"
#[tauri::command]
fn greet(name: String) -> String { name }
";
        let tree = parse_rust(src);
        let fn_node = find_node(tree.root_node(), "function_item").unwrap();

        assert!(has_tauri_command_attr(fn_node, src));
    }

    #[test]
    fn detects_short_command_attr() {
        let src = r"
#[command]
fn greet(name: String) -> String { name }
";
        let tree = parse_rust(src);
        let fn_node = find_node(tree.root_node(), "function_item").unwrap();

        assert!(has_tauri_command_attr(fn_node, src));
    }

    #[test]
    fn skips_comments_between_attr_and_fn() {
        let src = r"
#[tauri::command]
// this is a comment
fn greet() {}
";
        let tree = parse_rust(src);
        let fn_node = find_node(tree.root_node(), "function_item").unwrap();

        assert!(has_tauri_command_attr(fn_node, src));
    }

    #[test]
    fn no_attr_returns_false() {
        let src = "fn greet() {}";
        let tree = parse_rust(src);
        let fn_node = find_node(tree.root_node(), "function_item").unwrap();

        assert!(!has_tauri_command_attr(fn_node, src));
    }

    #[test]
    fn unrelated_attr_returns_false() {
        let src = r"
#[derive(Debug)]
fn greet() {}
";
        let tree = parse_rust(src);
        let fn_node = find_node(tree.root_node(), "function_item").unwrap();

        assert!(!has_tauri_command_attr(fn_node, src));
    }

    #[test]
    fn no_false_positive_on_partial_match() {
        let src = r"
#[my_command_wrapper]
fn greet() {}
";
        let tree = parse_rust(src);
        let fn_node = find_node(tree.root_node(), "function_item").unwrap();

        assert!(!has_tauri_command_attr(fn_node, src));
    }

    #[test]
    fn multiple_attrs_with_command() {
        let src = r"
#[allow(unused)]
#[tauri::command]
fn greet() {}
";
        let tree = parse_rust(src);
        let fn_node = find_node(tree.root_node(), "function_item").unwrap();

        assert!(has_tauri_command_attr(fn_node, src));
    }

    // ── has_specta_event_derive ─────────────────────────────────────────

    #[test]
    fn detects_specta_event_derive() {
        let src = r"
#[derive(Clone, serde::Serialize, tauri_specta::Event)]
struct MyEvent {
    message: String,
}
";
        let tree = parse_rust(src);
        let struct_node = find_node(tree.root_node(), "struct_item").unwrap();

        assert!(has_specta_event_derive(struct_node, src));
    }

    #[test]
    fn detects_event_alias_derive() {
        let src = r"
#[derive(Clone, Serialize, SpectaEvent)]
struct Payload { data: u32 }
";
        let tree = parse_rust(src);
        let struct_node = find_node(tree.root_node(), "struct_item").unwrap();

        assert!(has_specta_event_derive(struct_node, src));
    }

    #[test]
    fn no_event_derive_returns_false() {
        let src = r"
#[derive(Clone, Debug, Serialize)]
struct Payload { data: u32 }
";
        let tree = parse_rust(src);
        let struct_node = find_node(tree.root_node(), "struct_item").unwrap();

        assert!(!has_specta_event_derive(struct_node, src));
    }

    #[test]
    fn no_false_positive_on_event_substring() {
        // "EventEmitter" contains "Event" but is not an Event derive
        let src = r#"
#[derive(Clone, EventEmitter)]
struct Payload { data: u32 }
"#;
        let tree = parse_rust(src);
        let struct_node = find_node(tree.root_node(), "struct_item").unwrap();

        assert!(!has_specta_event_derive(struct_node, src));
    }

    #[test]
    fn detects_full_path_event_derive() {
        let src = r#"
#[derive(Clone, some_crate::Event)]
struct Payload { data: u32 }
"#;
        let tree = parse_rust(src);
        let struct_node = find_node(tree.root_node(), "struct_item").unwrap();

        assert!(has_specta_event_derive(struct_node, src));
    }

    #[test]
    fn no_derive_at_all_returns_false() {
        let src = "struct Plain { x: i32 }";
        let tree = parse_rust(src);
        let struct_node = find_node(tree.root_node(), "struct_item").unwrap();

        assert!(!has_specta_event_derive(struct_node, src));
    }
}
