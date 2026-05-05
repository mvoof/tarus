//! Central project index — stores all parsed findings and provides lookup/query methods
//!
//! ## Submodules
//! - `types` — core data types (`Finding`, `IndexKey`, `LocationInfo`, schemas, `DiagnosticInfo`)
//! - `generators` — generator discovery and bindings detection
//! - `schemas` — command/event schema and type alias CRUD
//! - `symbols` — document and workspace symbol search
//! - `lens` — `CodeLens` data preparation
//! - `reports` — debug reports and introspection
//! - `cache` — name and diagnostic info caching

mod cache;
mod generators;
mod lens;
mod reports;
mod schemas;
mod symbols;
pub mod types;

pub use types::*;

use crate::syntax::EntityType;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Debug)]
pub struct ProjectIndex {
    pub(crate) map: DashMap<IndexKey, Vec<LocationInfo>>,
    pub(crate) file_map: DashMap<PathBuf, Vec<IndexKey>>,
    // Caches for get_all_names() results
    pub(crate) command_names_cache: RwLock<NameCache>,
    pub(crate) event_names_cache: RwLock<NameCache>,
    // Cache for diagnostic info (avoids re-iterating locations)
    pub(crate) diagnostic_info_cache: DashMap<IndexKey, DiagnosticInfo>,
    // Parse errors by file path
    pub(crate) parse_errors: DashMap<PathBuf, String>,
    // Configuration: Max number of individual file links to show in CodeLens before summarizing
    pub(crate) reference_limit: AtomicUsize,
    // Schema storage: command_name -> CommandSchema
    pub(crate) command_schemas: DashMap<String, CommandSchema>,
    // Reverse index: source_path -> list of command names (for stale removal)
    pub(crate) generated_file_paths: DashMap<PathBuf, Vec<String>>,
    // Type alias storage: alias_name -> type definition string
    pub(crate) type_aliases: DashMap<String, String>,
    // Reverse index: source_path -> list of alias names (for stale removal)
    pub(crate) generated_alias_paths: DashMap<PathBuf, Vec<String>>,
    // Event schema storage: event_name -> EventSchema
    pub(crate) event_schemas: DashMap<String, EventSchema>,
    // Reverse index: source_path -> list of event names (for stale removal)
    pub(crate) generated_event_paths: DashMap<PathBuf, Vec<String>>,
    // Generators discovered from project configuration files
    pub(crate) generator_bindings: RwLock<Vec<DiscoveredGenerator>>,
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
            reference_limit: AtomicUsize::new(crate::constants::DEFAULT_REFERENCE_LIMIT),
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

    /// Invalidate all name caches (should be called after any mutation to the index)
    fn invalidate_caches(&self) {
        *self.command_names_cache.write() = None;
        *self.event_names_cache.write() = None;
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
                name: finding.key.clone(),
            };

            let info = LocationInfo::from((&path_ref, finding));

            self.map.entry(key.clone()).or_default().push(info);

            keys_in_this_file.insert(key);
        }

        let keys_vec: Vec<_> = keys_in_this_file.iter().cloned().collect();
        self.file_map.insert(path_ref, keys_vec);

        self.invalidate_caches();

        for key in &keys_in_this_file {
            self.diagnostic_info_cache.remove(key);
        }
    }

    /// Deletes all entries associated with a specific file.
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

            self.invalidate_caches();
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

    /// Set the reference limit for `CodeLens` display
    pub fn set_reference_limit(&self, limit: usize) {
        self.reference_limit.store(limit, Ordering::Relaxed);
    }

    /// Get keys associated with a file path
    pub fn get_file_keys(&self, path: &Path) -> Vec<IndexKey> {
        self.file_map
            .get(&path.to_path_buf())
            .map(|keys| keys.value().clone())
            .unwrap_or_default()
    }

    /// Get all indexed file paths (for iterating over the entire index)
    pub fn get_indexed_paths(&self) -> Vec<PathBuf> {
        self.file_map.iter().map(|e| e.key().clone()).collect()
    }

    /// Get all locations for a given index key
    pub fn get_locations_for_key(&self, key: &IndexKey) -> Vec<LocationInfo> {
        self.map.get(key).map(|v| v.clone()).unwrap_or_default()
    }

}
