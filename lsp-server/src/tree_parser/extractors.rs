//! Type argument extraction helpers for generic call expressions

use crate::utils::{find_capture, point_to_position};
use tower_lsp_server::lsp_types::Range;

/// Result of extracting type argument info from a generic call expression.
pub(super) struct TypeArgInfo {
    /// The type text (e.g. "User" from `invoke<User>`)
    pub type_text: String,
    /// The range of the full `<User>` including angle brackets
    pub type_arg_range: Range,
}

/// Extract the type argument text and range from a generic call expression.
///
/// For `invoke<User>("cmd")`, the `call_expression` node has a `type_arguments` child
/// containing `<User>`. We strip the angle brackets to return `"User"` and also
/// return the range of `<User>` for code action replacement.
pub(super) fn extract_type_argument_info(
    m: &tree_sitter::QueryMatch<'_, '_>,
    call_generic_idx: Option<u32>,
    call_await_generic_idx: Option<u32>,
    content: &str,
) -> Option<TypeArgInfo> {
    // Find the call_expression node from the generic pattern captures
    let call_node =
        find_capture(m, call_generic_idx).or_else(|| find_capture(m, call_await_generic_idx))?;

    // Walk children to find type_arguments
    let node = call_node.node;
    let mut tree_cursor = node.walk();
    for child in node.children(&mut tree_cursor) {
        if child.kind() == "type_arguments" {
            let text = child.utf8_text(content.as_bytes()).unwrap_or_default();
            let type_arg_range = Range {
                start: point_to_position(child.start_position()),
                end: point_to_position(child.end_position()),
            };
            // Strip angle brackets: "<User>" → "User"
            let trimmed = text.strip_prefix('<').unwrap_or(text);
            let trimmed = trimmed.strip_suffix('>').unwrap_or(trimmed);
            let trimmed = trimmed.trim();
            if !trimmed.is_empty() {
                return Some(TypeArgInfo {
                    type_text: trimmed.to_string(),
                    type_arg_range,
                });
            }
        }
    }

    None
}

/// Count the positional arguments in a `SpectaCall` expression.
pub(super) fn count_specta_call_args(
    m: &tree_sitter::QueryMatch<'_, '_>,
    specta_call_idx: Option<u32>,
    content: &str,
) -> u32 {
    let Some(call_idx) = specta_call_idx else {
        return 0;
    };
    let Some(call_cap) = find_capture(m, Some(call_idx)) else {
        return 0;
    };

    // Find the arguments node among children of the call_expression
    let call_node = call_cap.node;
    let mut tree_cursor = call_node.walk();
    let children: Vec<_> = call_node.children(&mut tree_cursor).collect();

    for child in &children {
        if child.kind() == "arguments" {
            // Count non-punctuation children of arguments
            let mut arg_cursor = child.walk();
            let count = child
                .children(&mut arg_cursor)
                .filter(|n| n.kind() != "," && n.kind() != "(" && n.kind() != ")")
                .count();

            return u32::try_from(count).unwrap_or(0);
        }
    }

    // Fallback: look for arguments via text
    let _ = content;
    0
}
