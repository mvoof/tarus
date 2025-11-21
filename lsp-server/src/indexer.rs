use crate::syntax::{Behavior, EntityType};
use dashmap::DashMap;
use std::path::PathBuf;
use tower_lsp::lsp_types::{Position, Range};

/// A single occurrence in a file (parser result)
#[derive(Debug, Clone)]
pub struct Finding {
    pub key: String,        // Name ("save_file")
    pub entity: EntityType, // Command or Event
    pub behavior: Behavior, // Call, Emit, Listen
    pub range: Range,       // Coordinates
}

#[derive(Debug)]
pub struct FileIndex {
    pub path: PathBuf,
    pub findings: Vec<Finding>,
}

/// Search Key (Hashmap Key)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexKey {
    pub entity: EntityType,
    pub name: String,
}

/// Location Information (Value)
#[derive(Debug, Clone)]
pub struct LocationInfo {
    pub path: PathBuf,
    pub range: Range,
    pub behavior: Behavior,
}

#[derive(Debug, Default)]
pub struct ProjectIndex {
    pub map: DashMap<IndexKey, Vec<LocationInfo>>,
    pub file_map: DashMap<PathBuf, Vec<IndexKey>>,
}

impl ProjectIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Search for a key by cursor position (Reverse Lookup)
    pub fn get_key_at_position(
        &self,
        path: &PathBuf,
        position: Position,
    ) -> Option<(IndexKey, LocationInfo)> {
        let keys_in_file = self.file_map.get(path)?;

        for key in keys_in_file.value() {
            if let Some(locations) = self.map.get(key) {
                for loc in locations.value() {
                    if loc.path == *path {
                        if self.is_position_in_range(position, loc.range) {
                            return Some((key.clone(), loc.clone()));
                        }
                    }
                }
            }
        }

        None
    }

    /// Helper for checking if the cursor is inside a range
    fn is_position_in_range(&self, pos: Position, range: Range) -> bool {
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

    /// Appends (or overwrites) the parsing results of a single file
    pub fn add_file(&self, file_index: FileIndex) {
        // Clear old data about this file so that there are no duplicates
        self.remove_file(&file_index.path);

        let mut keys_in_this_file = Vec::new();
        let path_ref = file_index.path;

        for finding in file_index.findings {
            let key = IndexKey {
                entity: finding.entity,
                name: finding.key,
            };

            let info = LocationInfo {
                path: path_ref.clone(),
                range: finding.range,
                behavior: finding.behavior,
            };

            self.map.entry(key.clone()).or_default().push(info);

            keys_in_this_file.push(key);
        }

        self.file_map.insert(path_ref, keys_in_this_file);
    }

    /// Deletes all entries associated with a specific file.
    pub fn remove_file(&self, path: &PathBuf) {
        // If the file has already been indexed...
        if let Some((_, keys)) = self.file_map.remove(path) {
            for key in keys {
                self.map.entry(key.clone()).and_modify(|locs| {
                    locs.retain(|loc| loc.path != *path);
                });

                // If the list becomes empty, you can remove the key from the map,
                // to avoid storing garbage
                if self.map.get(&key).map_or(false, |locs| locs.is_empty()) {
                    self.map.remove(&key);
                }
            }
        }
    }

    /// Retrieves all locations associated with a specific entity
    pub fn get_locations(&self, entity: EntityType, name: &str) -> Vec<LocationInfo> {
        let key = IndexKey {
            entity,
            name: name.to_string(),
        };

        self.map.get(&key).map(|v| v.clone()).unwrap_or_default()
    }

    /// Preparing data for CodeLens
    pub fn get_lens_data(&self, path: &PathBuf) -> Vec<(Range, String, Vec<LocationInfo>)> {
        let mut result = Vec::new();

        // Collect keys
        let keys = match self.file_map.get(path) {
            Some(k) => k,
            None => return result,
        };

        for key in keys.value() {
            // Get ALL locations for key
            let all_locations = match self.map.get(key) {
                Some(l) => l,
                None => continue,
            };

            // Find where exactly in the CURRENT file this key is located
            let current_file_locations: Vec<&LocationInfo> =
                all_locations.iter().filter(|l| l.path == *path).collect();

            // Define "Targets" - where the lens should point
            // These are all the locations MINUS the current file (so as not to reference itself)
            let targets: Vec<LocationInfo> = all_locations
                .iter()
                .filter(|l| l.path != *path) // Exclude the current file
                .cloned()
                .collect();

            if targets.is_empty() {
                continue;
            }

            // Generate a Lens for each occurrence in the current file
            for my_loc in current_file_locations {
                let title = if targets.len() == 1 {
                    let target = &targets[0];

                    let ext = target
                        .path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");

                    match ext {
                        "rs" => "Go to Rust".to_string(),
                        _ => "Go to Frontend".to_string(),
                    }
                } else {
                    format!("{} References", targets.len())
                };

                result.push((my_loc.range, title.clone(), targets.clone()));
            }
        }
        result
    }

    /// Creates a readable report of the index contents
    pub fn technical_report(&self) -> String {
        let mut report_message = String::from("\n💾 === TECHNICAL INDEX DUMP ===\n");

        if self.map.is_empty() {
            report_message.push_str("   (Storage is Empty)\n");

            return report_message;
        }

        // ===============================================
        // SECTION 1: MAIN INDEX (map)
        // Shows where each KEY is used
        // ===============================================

        report_message.push_str("\n\n🔑 === 1. MAIN KEY INDEX (map) ===\n");
        report_message.push_str("   [Key -> List of ALL Locations]\n");

        report_message.push_str(&format!("{:#?}\n", self.map));

        // ===============================================
        // SECTION 2: REVERSE FILE INDEX (file_map)
        // Shows which KEYS are in each FILE
        // ===============================================

        report_message.push_str("\n\n📄 === 2. REVERSE FILE MAP (file_map) ===\n");
        report_message.push_str("   [FilePath -> List of ALL Keys in that file]\n");

        if self.file_map.is_empty() {
            report_message.push_str("   (File Map is Empty)\n");
        } else {
            report_message.push_str(&format!("{:#?}\n", self.file_map));
        }

        report_message
    }
}
