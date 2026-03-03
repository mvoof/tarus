//! Shared utility functions

use tower_lsp_server::lsp_types::{Position, Range};

/// Convert a camelCase or `PascalCase` identifier to `snake_case`
///
/// Examples:
/// - `getUserProfile` → `get_user_profile`
/// - `createUser` → `create_user`
/// - `ping` → `ping`
#[must_use]
pub fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            // Insert underscore before uppercase if:
            // - not at position 0
            // - previous char is lowercase OR (next char is lowercase AND previous is uppercase)
            let prev_is_lower = i > 0 && chars[i - 1].is_ascii_lowercase();
            let next_is_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            let prev_is_upper = i > 0 && chars[i - 1].is_ascii_uppercase();

            if prev_is_lower || (next_is_lower && prev_is_upper) {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }

    result
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

/// Find the index of the closing bracket that matches an implicit opening at position 0.
///
/// The string `s` should start just after the opening bracket.
/// Tracks nesting of the same bracket pair only.
///
/// Examples:
/// - `find_matching_bracket("a + b}", '{', '}')` → `Some(5)`
/// - `find_matching_bracket("inner>outer>", '<', '>')` → `Some(5)`
#[must_use]
pub fn find_matching_bracket(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

/// Find the closing `)` matching an implicit `(` at position 0, respecting nested `<>`.
///
/// Unlike `find_matching_bracket`, this variant also tracks angle bracket depth
/// so that `)` inside generic type arguments (e.g. `Promise<Result<T, E>>`) is
/// not treated as the closing paren.
#[must_use]
pub fn find_matching_paren_with_generics(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut angle_depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '(' => depth += 1,
            ')' if angle_depth == 0 => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_to_snake_basic() {
        assert_eq!(camel_to_snake("getUserProfile"), "get_user_profile");
        assert_eq!(camel_to_snake("createUser"), "create_user");
        assert_eq!(camel_to_snake("ping"), "ping");
    }

    #[test]
    fn test_camel_to_snake_already_snake() {
        assert_eq!(camel_to_snake("get_user"), "get_user");
    }

    #[test]
    fn test_camel_to_snake_single_word() {
        assert_eq!(camel_to_snake("ping"), "ping");
        assert_eq!(camel_to_snake("Ping"), "ping");
    }

    #[test]
    fn test_camel_to_snake_acronym() {
        // "getHTTPSResponse" -> "get_https_response"
        assert_eq!(camel_to_snake("getHTTPSResponse"), "get_https_response");
    }

    #[test]
    fn test_find_matching_bracket_angle() {
        assert_eq!(find_matching_bracket("User>", '<', '>'), Some(4));
        assert_eq!(find_matching_bracket("Vec<string>>", '<', '>'), Some(11));
        assert_eq!(find_matching_bracket("", '<', '>'), None);
    }

    #[test]
    fn test_find_matching_bracket_brace() {
        assert_eq!(find_matching_bracket("a: 1}", '{', '}'), Some(4));
        assert_eq!(find_matching_bracket("{inner}}", '{', '}'), Some(7));
    }

    #[test]
    fn test_find_matching_bracket_paren() {
        assert_eq!(find_matching_bracket("x, y)", '(', ')'), Some(4));
        assert_eq!(find_matching_bracket("(a))", '(', ')'), Some(3));
    }

    #[test]
    fn test_find_matching_paren_with_generics() {
        // Simple case
        assert_eq!(find_matching_paren_with_generics("x: number)"), Some(9));
        // Paren inside angle brackets should be ignored
        assert_eq!(
            find_matching_paren_with_generics("x: Promise<Result<T, E>>)"),
            Some(24)
        );
        // No closing paren
        assert_eq!(find_matching_paren_with_generics("x: number"), None);
    }
}
