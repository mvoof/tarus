//! High-performance project-wide indexer for Tauri commands, events, and types

mod cache;
mod position;
mod symbols;

pub use cache::DiagnosticInfo;

use crate::syntax::{Behavior, EntityType};
use cache::CacheManager;
use dashmap::DashMap;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower_lsp_server::lsp_types::{Position, Range, SymbolInformation};

/// A single occurrence in a file (parser result)
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub key: String,        // Name ("save_file")
    pub entity: EntityType, // Command or Event
    pub behavior: Behavior, // Call, Emit, Listen
    pub range: Range,       // Coordinates
    pub parameters: Option<Vec<Parameter>>,
    pub return_type: Option<String>,
    pub fields: Option<Vec<Parameter>>,
    pub attributes: Option<Vec<String>>,
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
    pub parameters: Option<Vec<Parameter>>,
    pub return_type: Option<String>,
    pub fields: Option<Vec<Parameter>>,
    pub attributes: Option<Vec<String>>,
}

#[derive(Debug)]
pub struct ProjectIndex {
    pub map: DashMap<IndexKey, Vec<LocationInfo>>,
    pub file_map: DashMap<PathBuf, Vec<IndexKey>>,
    cache: CacheManager,
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
            cache: CacheManager::new(),
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

    /// Iterate over all entries in the index
    pub fn iter_all(
        &self,
    ) -> dashmap::iter::Iter<'_, IndexKey, Vec<LocationInfo>, std::hash::RandomState> {
        self.map.iter()
    }

    /// Search for a key by cursor position (Reverse Lookup)
    pub fn get_key_at_position(
        &self,
        path: &PathBuf,
        pos: Position,
    ) -> Option<(IndexKey, LocationInfo)> {
        // Check if we have a cached position index for this file
        if let Some(index) = self.cache.position_index_cache.get(path) {
            // Use binary search on cached sorted index
            return position::binary_search_position(&index, pos, &self.map, path);
        }

        // Build position index for this file
        let position_index = position::build_position_index(path, &self.file_map, &self.map);

        // Cache the index for future lookups
        let result = position::binary_search_position(&position_index, pos, &self.map, path);
        self.cache
            .position_index_cache
            .insert(path.clone(), position_index);
        result
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
                parameters: finding.parameters,
                return_type: finding.return_type,
                fields: finding.fields,
                attributes: finding.attributes,
            };

            self.map.entry(key.clone()).or_default().push(info);

            keys_in_this_file.push(key);
        }

        // Invalidate caches (before moving path_ref)
        self.cache.invalidate_file(&path_ref);

        self.file_map.insert(path_ref, keys_in_this_file);
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
            self.cache.invalidate_file(path);
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

    /// Helper function to group targets by file and format lens data
    fn group_and_format_targets(
        targets: &[LocationInfo],
        my_range: Range,
        limit: usize,
    ) -> Vec<(Range, String, Vec<LocationInfo>)> {
        if targets.is_empty() {
            return Vec::new();
        }

        // Group by file path
        let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();
        for t in targets {
            files_map.entry(t.path.clone()).or_default().push(t.clone());
        }

        if files_map.len() <= limit {
            // Show individual link for each file
            let mut sorted_files: Vec<_> = files_map.into_iter().collect();
            sorted_files.sort_by(|a, b| a.0.cmp(&b.0));

            sorted_files
                .into_iter()
                .map(|(fpath, locs)| {
                    let fname = fpath
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("unknown");
                    (my_range, format!("Go to {fname}"), locs)
                })
                .collect()
        } else {
            // Show summary when too many files
            vec![(
                my_range,
                format!("{} references", targets.len()),
                targets.to_vec(),
            )]
        }
    }

    #[allow(clippy::too_many_lines)]
    /// Preparing data for `CodeLens`
    pub fn get_lens_data(&self, path: &PathBuf) -> Vec<(Range, String, Vec<LocationInfo>)> {
        // Check cache first
        if let Some(cached) = self.cache.lens_data_cache.get(path) {
            return cached.clone();
        }

        let mut result = Vec::new();

        // Collect keys
        let Some(keys) = self.file_map.get(path) else {
            return result;
        };

        let mut processed_keys: std::collections::HashSet<&IndexKey> =
            std::collections::HashSet::new(); //  tracking already processed keys

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
                    let limit = self.reference_limit.load(Ordering::Relaxed);
                    result.extend(Self::group_and_format_targets(
                        &frontend_targets,
                        my_loc.range,
                        limit,
                    ));
                } else {
                    // Frontend Logic: "Go to Rust" + "Go to others"

                    let limit = self.reference_limit.load(Ordering::Relaxed);

                    // 1. Link to Rust (if exists)
                    if !rust_targets.is_empty() {
                        result.extend(Self::group_and_format_targets(
                            &rust_targets,
                            my_loc.range,
                            limit,
                        ));
                    }

                    // 2. Links to other frontend files
                    if !frontend_targets.is_empty() {
                        result.extend(Self::group_and_format_targets(
                            &frontend_targets,
                            my_loc.range,
                            limit,
                        ));
                    }
                }
            }
        }

        // Store in cache before returning
        self.cache
            .lens_data_cache
            .insert(path.clone(), result.clone());

        result
    }

    /// Generates a report only for a specific file (delta update)
    pub fn file_report(&self, path: &PathBuf) -> String {
        symbols::file_report(path, &self.file_map, &self.map)
    }

    /// Creates a readable report of the index contents
    pub fn technical_report(&self) -> String {
        symbols::technical_report(&self.map, &self.file_map)
    }

    /// Get document symbols for outline view
    pub fn get_document_symbols(&self, path: &PathBuf) -> Vec<SymbolInformation> {
        symbols::get_document_symbols(path, &self.file_map, &self.map)
    }

    /// Search workspace symbols by query (Ctrl+T)
    pub fn search_workspace_symbols(&self, query: &str) -> Vec<SymbolInformation> {
        symbols::search_workspace_symbols(query, &self.map)
    }

    /// Get all known names for a specific entity type (for completion)
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn get_all_names(&self, entity: EntityType) -> Vec<(String, Option<LocationInfo>)> {
        // Select appropriate cache
        let cache = match entity {
            EntityType::Command => &self.cache.command_names_cache,
            EntityType::Event => &self.cache.event_names_cache,
            _ => return Vec::new(),
        };

        // Try to read from cache
        {
            let cache_read = cache.read().expect("Cache RwLock poisoned during read");
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
        *cache.write().expect("Cache RwLock poisoned during write") = Some(result.clone());

        result
    }

    /// Get diagnostic information for a key (for diagnostics)
    pub fn get_diagnostic_info(&self, key: &IndexKey) -> DiagnosticInfo {
        // Check cache first
        if let Some(cached) = self.cache.diagnostic_info_cache.get(key) {
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
        self.cache
            .diagnostic_info_cache
            .insert(key.clone(), info.clone());

        info
    }
}
