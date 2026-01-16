use crate::syntax::{Behavior, EntityType};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Location, Position, Range, SymbolInformation, SymbolKind, Uri};
use tower_lsp_server::UriExt;

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

        let mut processed_keys: HashSet<&IndexKey> = HashSet::new(); //  tracking already processed keys

        for key in keys.value() {
            if !processed_keys.insert(key) {
                continue;
            }

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

    /// Generates a report only for a specific file (delta update)
    pub fn file_report(&self, path: &PathBuf) -> String {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let mut report_message = String::new();

        report_message.push_str(&format!("\n📝 === UPDATE REPORT: {} ===\n", filename));

        let keys = match self.file_map.get(path) {
            Some(k) => k,
            None => {
                return format!("📝 File update: {:?} (No Tarus keys found)", filename);
            }
        };

        let keys_clone: Vec<IndexKey> = keys.value().clone();

        report_message.push_str("\n🔑 === 1. MAIN KEY INDEX (Delta Subset) ===\n");

        let mut delta_map: HashMap<IndexKey, Vec<LocationInfo>> = HashMap::new();

        for key in &keys_clone {
            if let Some(locs) = self.map.get(key) {
                let file_locs: Vec<LocationInfo> =
                    locs.iter().filter(|l| l.path == *path).cloned().collect();

                if !file_locs.is_empty() {
                    delta_map.insert(key.clone(), file_locs);
                }
            }
        }

        if delta_map.is_empty() {
            report_message.push_str("   (Map subset is empty)\n");
        } else {
            report_message.push_str(&format!("{:#?}\n", delta_map));
        }

        report_message.push_str("\n📄 === 2. REVERSE FILE MAP (Delta Subset) ===\n");

        let mut delta_file_map: HashMap<PathBuf, Vec<IndexKey>> = HashMap::new();
        delta_file_map.insert(path.clone(), keys_clone);

        report_message.push_str(&format!("{:#?}\n", delta_file_map));

        report_message
    }

    /// Creates a readable report of the index contents
    pub fn technical_report(&self) -> String {
        let mut report_message = String::from("\n💾 === TECHNICAL INDEX DUMP ===\n");

        if self.map.is_empty() {
            report_message.push_str("   (Storage is Empty)\n");

            return report_message;
        }

        report_message.push_str("\n\n🔑 === 1. MAIN KEY INDEX (map) ===\n");
        report_message.push_str("   [Key -> List of ALL Locations]\n");
        
        report_message.push_str(&format!("{:#?}\n", self.map));

        report_message.push_str("\n\n📄 === 2. REVERSE FILE MAP (file_map) ===\n");
        report_message.push_str("   [FilePath -> List of ALL Keys in that file]\n");

        if self.file_map.is_empty() {
            report_message.push_str("   (File Map is Empty)\n");
        } else {
            report_message.push_str(&format!("{:#?}\n", self.file_map));
        }

        report_message
    }

    /// Get document symbols for outline view (Ctrl+Shift+O)
    pub fn get_document_symbols(&self, path: &PathBuf) -> Vec<SymbolInformation> {
        let mut symbols = Vec::new();

        let keys = match self.file_map.get(path) {
            Some(k) => k,
            None => return symbols,
        };

        let uri = match Uri::from_file_path(path) {
            Some(u) => u,
            None => return symbols,
        };

        for key in keys.value() {
            if let Some(locations) = self.map.get(key) {
                for loc in locations.iter().filter(|l| l.path == *path) {
                    let kind = match key.entity {
                        EntityType::Command => SymbolKind::FUNCTION,
                        EntityType::Event => SymbolKind::EVENT,
                    };

                    // Use terms from command_syntax.json
                    let behavior_label = match loc.behavior {
                        Behavior::Definition => "command",
                        Behavior::Call => "invoke",
                        Behavior::Emit => "emit",
                        Behavior::Listen => "listen",
                    };

                    #[allow(deprecated)]
                    symbols.push(SymbolInformation {
                        name: format!("{} ({})", key.name, behavior_label),
                        kind,
                        tags: None,
                        deprecated: None,
                        location: Location {
                            uri: uri.clone(),
                            range: loc.range,
                        },
                        container_name: Some(format!("{:?}", key.entity)),
                    });
                }
            }
        }

        symbols.sort_by_key(|s| s.location.range.start.line);
        symbols
    }

    /// Search workspace symbols by query (Ctrl+T)
    pub fn search_workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        let mut symbols = Vec::new();
        let query_lower = query.to_lowercase();

        for entry in self.map.iter() {
            let key = entry.key();

            // Filter by query (substring match)
            if !query.is_empty() && !key.name.to_lowercase().contains(&query_lower) {
                continue;
            }

            for loc in entry.value().iter() {
                let uri = match Uri::from_file_path(&loc.path) {
                    Some(u) => u,
                    None => continue,
                };

                let kind = match key.entity {
                    EntityType::Command => SymbolKind::FUNCTION,
                    EntityType::Event => SymbolKind::EVENT,
                };

                let behavior_label = match loc.behavior {
                    Behavior::Definition => "command",
                    Behavior::Call => "invoke",
                    Behavior::Emit => "emit",
                    Behavior::Listen => "listen",
                };

                #[allow(deprecated)]
                symbols.push(SymbolInformation {
                    name: format!("{} ({})", key.name, behavior_label),
                    kind,
                    tags: None,
                    deprecated: None,
                    location: Location {
                        uri,
                        range: loc.range,
                    },
                    container_name: Some(format!("{:?}", key.entity)),
                });
            }
        }

        // Limit results
        symbols.truncate(100);
        symbols
    }
}
