//! Central project index — stores all parsed findings and provides lookup/query methods
//!
//! ## Submodules
//! - `types` — core data types (`Finding`, `IndexKey`, `LocationInfo`, schemas, `DiagnosticInfo`)
//! - `generators` — generator discovery and bindings detection
//! - `schemas` — command/event schema and type alias CRUD
//! - `symbols` — document and workspace symbol search
//! - `reports` — debug reports and introspection
//! - `cache` — name and diagnostic info caching

mod cache;
mod generators;
mod reports;
mod schemas;
mod symbols;
pub mod types;

pub use types::*;

use crate::syntax::EntityType;
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use tower_lsp_server::lsp_types::Range;

#[derive(Debug)]
pub struct ProjectIndex {
    pub map: DashMap<IndexKey, Vec<LocationInfo>>,
    pub file_map: DashMap<PathBuf, Vec<IndexKey>>,
    // Caches for get_all_names() results
    pub(crate) command_names_cache: RwLock<NameCache>,
    pub(crate) event_names_cache: RwLock<NameCache>,
    // Cache for diagnostic info (avoids re-iterating locations)
    pub(crate) diagnostic_info_cache: DashMap<IndexKey, DiagnosticInfo>,
    // Parse errors by file path
    pub parse_errors: DashMap<PathBuf, String>,
    // Configuration: Max number of individual file links to show in CodeLens before summarizing
    pub reference_limit: AtomicUsize,
    // Schema storage: command_name -> CommandSchema
    pub command_schemas: DashMap<String, CommandSchema>,
    // Reverse index: source_path -> list of command names (for stale removal)
    pub generated_file_paths: DashMap<PathBuf, Vec<String>>,
    // Type alias storage: alias_name -> type definition string
    pub type_aliases: DashMap<String, String>,
    // Reverse index: source_path -> list of alias names (for stale removal)
    pub generated_alias_paths: DashMap<PathBuf, Vec<String>>,
    // Event schema storage: event_name -> EventSchema
    pub event_schemas: DashMap<String, EventSchema>,
    // Reverse index: source_path -> list of event names (for stale removal)
    pub generated_event_paths: DashMap<PathBuf, Vec<String>>,
    // Generators discovered from project configuration files
    pub generator_bindings: RwLock<Vec<DiscoveredGenerator>>,
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
            command_schemas: DashMap::new(),
            generated_file_paths: DashMap::new(),
            type_aliases: DashMap::new(),
            generated_alias_paths: DashMap::new(),
            event_schemas: DashMap::new(),
            generated_event_paths: DashMap::new(),
            generator_bindings: RwLock::new(Vec::new()),
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
        path: &Path,
        position: tower_lsp_server::lsp_types::Position,
    ) -> Option<(IndexKey, LocationInfo)> {
        let keys_in_file = self.file_map.get(&path.to_path_buf())?;

        for key in keys_in_file.value() {
            if let Some(locations) = self.map.get(key) {
                for loc in locations.value() {
                    if loc.path == path && crate::utils::is_position_in_range(position, loc.range) {
                        return Some((key.clone(), loc.clone()));
                    }
                }
            }
        }

        None
    }

    /// Appends (or overwrites) the parsing results of a single file
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn add_file(&self, file_index: FileIndex) {
        // Clear old data about this file so that there are no duplicates
        self.remove_file(&file_index.path);

        let mut keys_in_this_file = std::collections::HashSet::new();
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
                call_arg_count: finding.call_arg_count,
                call_param_keys: finding.call_param_keys,
                return_type: finding.return_type,
                call_name_end: finding.call_name_end,
                type_arg_range: finding.type_arg_range,
                codegen_origin: finding.codegen_origin,
            };

            self.map.entry(key.clone()).or_default().push(info);

            keys_in_this_file.insert(key);
        }

        let keys_vec: Vec<_> = keys_in_this_file.iter().cloned().collect();
        self.file_map.insert(path_ref, keys_vec);

        // Invalidate caches
        *self.command_names_cache.write().unwrap() = None;
        *self.event_names_cache.write().unwrap() = None;
        for key in &keys_in_this_file {
            self.diagnostic_info_cache.remove(key);
        }
    }

    /// Deletes all entries associated with a specific file.
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn remove_file(&self, path: &Path) {
        if let Some((_, keys)) = self.file_map.remove(&path.to_path_buf()) {
            for key in keys {
                self.map.entry(key.clone()).and_modify(|locs| {
                    locs.retain(|loc| loc.path != path);
                });

                if self.map.get(&key).is_some_and(|locs| locs.is_empty()) {
                    self.map.remove(&key);
                }

                self.diagnostic_info_cache.remove(&key);
            }

            // Invalidate caches
            *self.command_names_cache.write().unwrap() = None;
            *self.event_names_cache.write().unwrap() = None;
        }

        // Also remove parse errors for this file
        self.parse_errors.remove(&path.to_path_buf());
    }

    /// Store a parse error for a file
    pub fn set_parse_error(&self, path: PathBuf, error: String) {
        self.parse_errors.insert(path, error);
    }

    /// Get parse error for a file (if any)
    pub fn get_parse_error(&self, path: &Path) -> Option<String> {
        self.parse_errors
            .get(&path.to_path_buf())
            .map(|e| e.value().clone())
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
    pub fn get_lens_data(&self, path: &Path) -> Vec<(Range, String, Vec<LocationInfo>)> {
        let mut result = Vec::new();

        let Some(keys) = self.file_map.get(&path.to_path_buf()) else {
            return result;
        };

        let mut processed_keys: HashSet<&IndexKey> = HashSet::new();

        for key in keys.value() {
            if !processed_keys.insert(key) {
                continue;
            }

            let Some(all_locations) = self.map.get(key) else {
                continue;
            };

            let current_file_locations: Vec<&LocationInfo> =
                all_locations.iter().filter(|l| l.path == path).collect();

            let targets: Vec<LocationInfo> = all_locations
                .iter()
                .filter(|l| l.path != path)
                .cloned()
                .collect();

            if targets.is_empty() {
                continue;
            }

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

                let limit = self.reference_limit.load(Ordering::Relaxed);

                if is_current_rust {
                    Self::push_file_lenses(
                        &mut result,
                        my_loc.range,
                        frontend_targets,
                        limit,
                        "references",
                    );
                } else {
                    Self::push_file_lenses(
                        &mut result,
                        my_loc.range,
                        rust_targets,
                        limit,
                        "rust refs",
                    );
                    Self::push_file_lenses(
                        &mut result,
                        my_loc.range,
                        frontend_targets,
                        limit,
                        "references",
                    );
                }
            }
        }
        result
    }

    fn push_file_lenses(
        result: &mut Vec<(Range, String, Vec<LocationInfo>)>,
        range: Range,
        targets: Vec<LocationInfo>,
        limit: usize,
        summary_label: &str,
    ) {
        if targets.is_empty() {
            return;
        }

        let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();
        for t in &targets {
            files_map.entry(t.path.clone()).or_default().push(t.clone());
        }

        if files_map.len() <= limit {
            let mut sorted_files: Vec<_> = files_map.into_iter().collect();
            sorted_files.sort_by(|a, b| a.0.cmp(&b.0));

            for (fpath, locs) in sorted_files {
                let fname = fpath
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                result.push((range, format!("Go to {fname}"), locs));
            }
        } else {
            result.push((
                range,
                format!("{} {}", targets.len(), summary_label),
                targets,
            ));
        }
    }
}
