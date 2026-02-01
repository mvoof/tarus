use crate::syntax::{Behavior, EntityType};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
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

#[derive(Debug)]
pub struct ProjectIndex {
    pub map: DashMap<IndexKey, Vec<LocationInfo>>,
    pub file_map: DashMap<PathBuf, Vec<IndexKey>>,
    // Caches for get_all_names() results
    #[allow(clippy::type_complexity)]
    command_names_cache: RwLock<Option<Vec<(String, Option<LocationInfo>)>>>,
    #[allow(clippy::type_complexity)]
    event_names_cache: RwLock<Option<Vec<(String, Option<LocationInfo>)>>>,
    // Cache for diagnostic info (avoids re-iterating locations)
    diagnostic_info_cache: DashMap<IndexKey, DiagnosticInfo>,
    // Parse errors by file path
    pub parse_errors: DashMap<PathBuf, String>,
    // Configuration: Max number of individual file links to show in CodeLens before summarizing
    pub reference_limit: AtomicUsize,
}

impl Default for ProjectIndex {
    fn default() -> Self {
        Self {
            map: DashMap::new(),
            file_map: DashMap::new(),
            command_names_cache: RwLock::new(None),
            event_names_cache: RwLock::new(None),
            diagnostic_info_cache: DashMap::new(),
            parse_errors: DashMap::new(),
            reference_limit: AtomicUsize::new(3),
        }
    }
}

impl ProjectIndex {
    #[must_use]
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
                    if loc.path == *path && Self::is_position_in_range(position, loc.range) {
                        return Some((key.clone(), loc.clone()));
                    }
                }
            }
        }

        None
    }

    /// Helper for checking if the cursor is inside a range
    fn is_position_in_range(pos: Position, range: Range) -> bool {
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
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
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

        // Invalidate caches
        *self.command_names_cache.write().unwrap() = None;
        *self.event_names_cache.write().unwrap() = None;
        self.diagnostic_info_cache.clear();
    }

    /// Deletes all entries associated with a specific file.
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn remove_file(&self, path: &PathBuf) {
        // If the file has already been indexed...
        if let Some((_, keys)) = self.file_map.remove(path) {
            for key in keys {
                self.map.entry(key.clone()).and_modify(|locs| {
                    locs.retain(|loc| loc.path != *path);
                });

                // If the list becomes empty, you can remove the key from the map,
                // to avoid storing garbage
                if self.map.get(&key).is_some_and(|locs| locs.is_empty()) {
                    self.map.remove(&key);
                }
            }

            // Invalidate caches
            *self.command_names_cache.write().unwrap() = None;
            *self.event_names_cache.write().unwrap() = None;
            self.diagnostic_info_cache.clear();
        }

        // Also remove parse errors for this file
        self.parse_errors.remove(path);
    }

    /// Store a parse error for a file
    pub fn set_parse_error(&self, path: PathBuf, error: String) {
        self.parse_errors.insert(path, error);
    }

    /// Get parse error for a file (if any)
    pub fn get_parse_error(&self, path: &PathBuf) -> Option<String> {
        self.parse_errors.get(path).map(|e| e.value().clone())
    }

    /// Retrieves all locations associated with a specific entity
    pub fn get_locations(&self, entity: EntityType, name: &str) -> Vec<LocationInfo> {
        let key = IndexKey {
            entity,
            name: name.to_string(),
        };

        self.map.get(&key).map(|v| v.clone()).unwrap_or_default()
    }

    /// Preparing data for `CodeLens`
    pub fn get_lens_data(&self, path: &PathBuf) -> Vec<(Range, String, Vec<LocationInfo>)> {
        let mut result = Vec::new();

        // Collect keys
        let Some(keys) = self.file_map.get(path) else {
            return result;
        };

        let mut processed_keys: HashSet<&IndexKey> = HashSet::new(); //  tracking already processed keys

        for key in keys.value() {
            if !processed_keys.insert(key) {
                continue;
            }

            // Get ALL locations for key
            let Some(all_locations) = self.map.get(key) else {
                continue;
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
                let is_current_rust = path.extension().and_then(|s| s.to_str()) == Some("rs");

                let mut rust_targets = Vec::new();
                let mut frontend_targets = Vec::new();

                for t in &targets {
                    if t.path.extension().and_then(|s| s.to_str()) == Some("rs") {
                        rust_targets.push(t.clone());
                    } else {
                        frontend_targets.push(t.clone());
                    }
                }

                if is_current_rust {
                    // Rust Logic: Show frontend files separately

                    // Group by file path
                    let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();
                    for t in frontend_targets.iter() {
                        files_map.entry(t.path.clone()).or_default().push(t.clone());
                    }

                    let limit = self.reference_limit.load(Ordering::Relaxed);

                    if files_map.len() <= limit {
                        // If <= limit files, show distinct link for EACH file
                        let mut sorted_files: Vec<_> = files_map.into_iter().collect();
                        // Sort for consistency
                        sorted_files.sort_by(|a, b| a.0.cmp(&b.0));

                        for (fpath, locs) in sorted_files {
                            let fname = fpath
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string();

                            result.push((my_loc.range, format!("Go to {}", fname), locs));
                        }
                    } else {
                        // If > limit files, show summary
                        result.push((
                            my_loc.range,
                            format!("{} references", frontend_targets.len()),
                            frontend_targets.clone(),
                        ));
                    }
                } else {
                    // Frontend Logic: "Go to Rust" + "Go to others"

                    let limit = self.reference_limit.load(Ordering::Relaxed);

                    // 1. Link to Rust (if exists)

                    if !rust_targets.is_empty() {
                        // Group by file path
                        let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();
                        for t in rust_targets.iter() {
                            files_map.entry(t.path.clone()).or_default().push(t.clone());
                        }

                        if files_map.len() <= limit {
                            // If <= limit files, show distinct link for EACH file
                            let mut sorted_files: Vec<_> = files_map.into_iter().collect();
                            sorted_files.sort_by(|a, b| a.0.cmp(&b.0));

                            for (fpath, locs) in sorted_files {
                                let fname = fpath
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown")
                                    .to_string();

                                result.push((my_loc.range, format!("Go to {}", fname), locs));
                            }
                        } else {
                            // If > limit files, show summary
                            result.push((
                                my_loc.range,
                                format!("{} rust refs", rust_targets.len()),
                                rust_targets,
                            ));
                        }
                    }

                    // 2. Links to other frontend files
                    // Group by file path
                    let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();

                    for t in frontend_targets.iter() {
                        files_map.entry(t.path.clone()).or_default().push(t.clone());
                    }

                    if files_map.len() <= limit {
                        // If <= limit files, show distinct link for EACH file
                        let mut sorted_files: Vec<_> = files_map.into_iter().collect();
                        sorted_files.sort_by(|a, b| a.0.cmp(&b.0));

                        for (fpath, locs) in sorted_files {
                            let fname = fpath
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or("unknown")
                                .to_string();

                            result.push((my_loc.range, format!("Go to {}", fname), locs));
                        }
                    } else {
                        // If > limit files, show summary
                        result.push((
                            my_loc.range,
                            format!("{} references", frontend_targets.len()),
                            frontend_targets,
                        ));
                    }
                }
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

        let _ = writeln!(report_message, "\n📝 === UPDATE REPORT: {filename} ===");

        let Some(keys) = self.file_map.get(path) else {
            return format!("📝 File update: {filename:?} (No Tarus keys found)");
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
            let _ = writeln!(report_message, "{delta_map:#?}");
        }

        report_message.push_str("\n📄 === 2. REVERSE FILE MAP (Delta Subset) ===\n");

        let mut delta_file_map: HashMap<PathBuf, Vec<IndexKey>> = HashMap::new();
        delta_file_map.insert(path.clone(), keys_clone);

        let _ = writeln!(report_message, "{delta_file_map:#?}");

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

        let _ = writeln!(report_message, "{:#?}", self.map);

        report_message.push_str("\n\n📄 === 2. REVERSE FILE MAP (file_map) ===\n");
        report_message.push_str("   [FilePath -> List of ALL Keys in that file]\n");

        if self.file_map.is_empty() {
            report_message.push_str("   (File Map is Empty)\n");
        } else {
            let _ = writeln!(report_message, "{:#?}", self.file_map);
        }

        report_message
    }

    /// Get document symbols for outline view
    pub fn get_document_symbols(&self, path: &PathBuf) -> Vec<SymbolInformation> {
        let mut symbols = Vec::new();

        let Some(keys) = self.file_map.get(path) else {
            return symbols;
        };

        let Some(uri) = Uri::from_file_path(path) else {
            return symbols;
        };

        for key in keys.value() {
            if let Some(locations) = self.map.get(key) {
                for loc in locations.iter().filter(|l| l.path == *path) {
                    let kind = match key.entity {
                        EntityType::Command => SymbolKind::FUNCTION,
                        EntityType::Event => SymbolKind::EVENT,
                    };

                    // Use behavior terms
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

        for entry in &self.map {
            let key = entry.key();

            // Filter by query (substring match)
            if !query.is_empty() && !key.name.to_lowercase().contains(&query_lower) {
                continue;
            }

            for loc in entry.value() {
                let Some(uri) = Uri::from_file_path(&loc.path) else {
                    continue;
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

    /// Get all known names for a specific entity type (for completion)
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn get_all_names(&self, entity: EntityType) -> Vec<(String, Option<LocationInfo>)> {
        // Select appropriate cache
        let cache = match entity {
            EntityType::Command => &self.command_names_cache,
            EntityType::Event => &self.event_names_cache,
        };

        // Try to read from cache
        {
            let cache_read = cache.read().unwrap();
            if let Some(cached) = cache_read.as_ref() {
                return cached.clone();
            }
        }

        // Cache miss - compute result
        let result: Vec<(String, Option<LocationInfo>)> = self
            .map
            .iter()
            .filter(|e| e.key().entity == entity)
            .map(|e| {
                let definition = e
                    .value()
                    .iter()
                    .find(|l| l.behavior == Behavior::Definition)
                    .cloned();
                (e.key().name.clone(), definition)
            })
            .collect();

        // Store in cache
        *cache.write().unwrap() = Some(result.clone());

        result
    }

    /// Get diagnostic information for a key (for diagnostics)
    pub fn get_diagnostic_info(&self, key: &IndexKey) -> DiagnosticInfo {
        // Check cache first
        if let Some(cached) = self.diagnostic_info_cache.get(key) {
            return cached.clone();
        }

        // Cache miss - compute
        let locations = self.map.get(key).map(|v| v.clone()).unwrap_or_default();
        let info = DiagnosticInfo {
            has_definition: locations.iter().any(|l| l.behavior == Behavior::Definition),
            has_calls: locations.iter().any(|l| l.behavior == Behavior::Call),
            has_emitters: locations.iter().any(|l| l.behavior == Behavior::Emit),
            has_listeners: locations.iter().any(|l| l.behavior == Behavior::Listen),
        };

        // Store in cache
        self.diagnostic_info_cache.insert(key.clone(), info.clone());

        info
    }
}

/// Diagnostic information for a command/event
#[derive(Clone, Debug)]
#[allow(clippy::struct_excessive_bools)]
pub struct DiagnosticInfo {
    pub has_definition: bool,
    pub has_calls: bool,
    pub has_emitters: bool,
    pub has_listeners: bool,
}
