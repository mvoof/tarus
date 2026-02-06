//! Position lookup and spatial indexing for fast O(log n) position-to-entity mapping

use super::{IndexKey, LocationInfo};
use dashmap::DashMap;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Position, Range};

/// Build a sorted position index for a file to enable binary search
pub fn build_position_index(
    path: &PathBuf,
    file_map: &DashMap<PathBuf, Vec<IndexKey>>,
    map: &DashMap<IndexKey, Vec<LocationInfo>>,
) -> Vec<(Range, IndexKey)> {
    let Some(keys_in_file) = file_map.get(path) else {
        return Vec::new();
    };

    let mut position_index: Vec<(Range, IndexKey)> = Vec::new();

    for key in keys_in_file.value() {
        if let Some(locations) = map.get(key) {
            for loc in locations.value() {
                if loc.path == *path {
                    position_index.push((loc.range, key.clone()));
                }
            }
        }
    }

    // Sort by start position (line, then character) for binary search
    position_index.sort_by(|a, b| {
        let cmp_line = a.0.start.line.cmp(&b.0.start.line);
        if cmp_line == std::cmp::Ordering::Equal {
            a.0.start.character.cmp(&b.0.start.character)
        } else {
            cmp_line
        }
    });

    position_index
}

/// Binary search for position in sorted position index
pub fn binary_search_position(
    index: &[(Range, IndexKey)],
    position: Position,
    map: &DashMap<IndexKey, Vec<LocationInfo>>,
    path: &PathBuf,
) -> Option<(IndexKey, LocationInfo)> {
    // Binary search for the range containing the position
    let idx = index.partition_point(|(range, _)| {
        range.start.line < position.line
            || (range.start.line == position.line && range.start.character <= position.character)
    });

    // Check candidates around the found position
    // We need to check a few entries because ranges can overlap
    for i in idx.saturating_sub(2)..std::cmp::min(idx + 2, index.len()) {
        let (range, key) = &index[i];
        if is_position_in_range(position, *range) {
            // Found the range, now get the full location info
            if let Some(locations) = map.get(key) {
                for loc in locations.value() {
                    if loc.path == *path && is_position_in_range(position, loc.range) {
                        return Some((key.clone(), loc.clone()));
                    }
                }
            }
        }
    }

    None
}

/// Helper for checking if the cursor is inside a range
pub fn is_position_in_range(pos: Position, range: Range) -> bool {
    // LSP Range inclusive start, exclusive end
    if pos.line < range.start.line || pos.line > range.end.line {
        return false;
    }

    // If one line
    if range.start.line == range.end.line {
        return pos.character >= range.start.character && pos.character < range.end.character;
    }

    // If multi-line range
    if pos.line == range.start.line {
        return pos.character >= range.start.character;
    }

    if pos.line == range.end.line {
        return pos.character < range.end.character;
    }

    true
}
