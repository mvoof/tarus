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
