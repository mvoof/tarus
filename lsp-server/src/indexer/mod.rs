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

/// The constants and forwarding helpers one file declares.
#[derive(Debug, Clone, Default)]
pub struct FileNames {
    rust: crate::utils::RustConstants,
    js: std::collections::HashMap<String, String>,
    forwarders: Vec<Forwarder>,
}

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
    // Constant storage: constant_name -> every (file, string_value) declaring it.
    // Each entry keeps its file so that re-indexing one file drops only its own
    // declarations, never a same-named one from another file.
    pub(crate) rust_constants: DashMap<String, Vec<(PathBuf, String)>>,
    // `const A: T = B;` in Rust: constant_name -> every (file, reference)
    pub(crate) rust_constant_references:
        DashMap<String, Vec<(PathBuf, crate::utils::ConstantReference)>>,
    // `type A = &str;` in Rust: alias_name -> every file declaring it
    pub(crate) rust_str_type_aliases: DashMap<String, Vec<(PathBuf, ())>>,
    pub(crate) js_constants: DashMap<String, Vec<(PathBuf, String)>>,
    // Reverse index for stale removal: file_path -> list of constant names
    pub(crate) rust_constants_paths: DashMap<PathBuf, Vec<String>>,
    pub(crate) rust_constant_reference_paths: DashMap<PathBuf, Vec<String>>,
    pub(crate) rust_str_type_alias_paths: DashMap<PathBuf, Vec<String>>,
    pub(crate) js_constants_paths: DashMap<PathBuf, Vec<String>>,
    // Files holding Tauri calls whose name could not be resolved to a literal
    pub(crate) dynamic_usages: DashMap<PathBuf, DynamicUsages>,
    // The same calls with where they are written, for the per-file hint
    pub(crate) unresolved_calls: DashMap<PathBuf, Vec<UnresolvedCall>>,
    // Helpers that forward a parameter into a Tauri call: function_name -> every
    // declaration seen under that name. Kept as a list so that two files declaring
    // the same helper name are detected as ambiguous instead of overwriting.
    pub(crate) forwarders: DashMap<String, Vec<(PathBuf, Forwarder)>>,
    // Reverse index for stale removal: file_path -> declared forwarder names
    pub(crate) forwarder_paths: DashMap<PathBuf, Vec<String>>,
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
            rust_constants: DashMap::new(),
            rust_constant_references: DashMap::new(),
            rust_str_type_aliases: DashMap::new(),
            js_constants: DashMap::new(),
            rust_constants_paths: DashMap::new(),
            rust_constant_reference_paths: DashMap::new(),
            rust_str_type_alias_paths: DashMap::new(),
            js_constants_paths: DashMap::new(),
            dynamic_usages: DashMap::new(),
            unresolved_calls: DashMap::new(),
            forwarders: DashMap::new(),
            forwarder_paths: DashMap::new(),
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
        let keys_in_file = self.file_map.get(path)?;

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

        if file_index.dynamic_usages.is_empty() {
            self.dynamic_usages.remove(&path_ref);
        } else {
            self.dynamic_usages
                .insert(path_ref.clone(), file_index.dynamic_usages);
        }

        if !file_index.unresolved_calls.is_empty() {
            self.unresolved_calls
                .insert(path_ref.clone(), file_index.unresolved_calls);
        }

        self.add_forwarders(path_ref.clone(), file_index.forwarders);

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
        if let Some((_, keys)) = self.file_map.remove(path) {
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

        // Remove parse errors for this file.
        // NOTE: Do NOT remove constants here — `process_file_content` calls
        // `add_rust/js_constants` (which handles cleanup) before calling `add_file`,
        // so removing constants in `remove_file` would discard the freshly re-extracted
        // constants and break cross-file constant resolution.
        self.parse_errors.remove(path);
        self.dynamic_usages.remove(path);
        self.unresolved_calls.remove(path);
        self.remove_forwarders_for_file(path);
    }

    /// Tauri calls in `path` whose name could not be resolved to a string.
    #[must_use]
    pub fn unresolved_calls_in(&self, path: &Path) -> Vec<UnresolvedCall> {
        self.unresolved_calls
            .get(path)
            .map(|calls| calls.value().clone())
            .unwrap_or_default()
    }

    /// Unresolved Tauri call sites across the whole workspace, folded into one value.
    #[must_use]
    pub fn dynamic_usages(&self) -> DynamicUsages {
        let mut merged = DynamicUsages::default();

        for entry in &self.dynamic_usages {
            merged.merge(*entry.value());
        }

        merged
    }

    /// Unresolved Tauri call sites in `path` alone.
    #[must_use]
    pub fn dynamic_usages_of(&self, path: &Path) -> DynamicUsages {
        self.dynamic_usages
            .get(path)
            .map(|entry| *entry.value())
            .unwrap_or_default()
    }

    /// [`Self::dynamic_usages`] of every file but `path`.
    #[must_use]
    pub fn dynamic_usages_except(&self, path: &Path) -> DynamicUsages {
        let mut merged = DynamicUsages::default();

        for entry in self
            .dynamic_usages
            .iter()
            .filter(|entry| entry.key() != path)
        {
            merged.merge(*entry.value());
        }

        merged
    }

    /// The constants and forwarding helpers `path` declares: what it adds to the
    /// names other files resolve through.
    #[must_use]
    pub fn file_names(&self, path: &Path) -> FileNames {
        let rust = crate::utils::RustConstants {
            values: owned_by(&self.rust_constants, &self.rust_constants_paths, path),
            references: owned_by(
                &self.rust_constant_references,
                &self.rust_constant_reference_paths,
                path,
            ),
            str_type_aliases: owned_by(
                &self.rust_str_type_aliases,
                &self.rust_str_type_alias_paths,
                path,
            )
            .into_keys()
            .collect(),
        };

        FileNames {
            rust,
            js: owned_by(&self.js_constants, &self.js_constants_paths, path),
            forwarders: owned_by(&self.forwarders, &self.forwarder_paths, path)
                .into_values()
                .collect(),
        }
    }

    /// Put back what [`Self::file_names`] returned, replacing what `path` declares now.
    pub fn restore_file_names(&self, path: &Path, names: FileNames) {
        self.add_rust_constants(path.to_path_buf(), names.rust);
        self.add_js_constants(path.to_path_buf(), names.js);
        self.add_forwarders(path.to_path_buf(), names.forwarders);
    }

    /// Store a parse error for a file
    pub fn set_parse_error(&self, path: PathBuf, error: String) {
        self.parse_errors.insert(path, error);
    }

    /// Get parse error for a file (if any)
    pub fn get_parse_error(&self, path: &Path) -> Option<String> {
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

    /// Set the reference limit for `CodeLens` display
    pub fn set_reference_limit(&self, limit: usize) {
        self.reference_limit.store(limit, Ordering::Relaxed);
    }

    /// Get keys associated with a file path
    pub fn get_file_keys(&self, path: &Path) -> Vec<IndexKey> {
        self.file_map
            .get(path)
            .map(|keys| keys.value().clone())
            .unwrap_or_default()
    }

    /// Whether `path` is in the index.
    #[must_use]
    pub fn is_indexed(&self, path: &Path) -> bool {
        self.file_map.contains_key(path)
    }

    /// Get all indexed file paths (for iterating over the entire index)
    pub fn get_indexed_paths(&self) -> Vec<PathBuf> {
        self.file_map.iter().map(|e| e.key().clone()).collect()
    }

    /// Get all locations for a given index key
    pub fn get_locations_for_key(&self, key: &IndexKey) -> Vec<LocationInfo> {
        self.map.get(key).map(|v| v.clone()).unwrap_or_default()
    }

    /// Store Rust constants extracted from a file, replacing any previous constants for that file
    pub fn add_rust_constants(&self, path: PathBuf, constants: crate::utils::RustConstants) {
        self.remove_rust_constants_for_file(&path);
        let value_names: Vec<String> = constants.values.keys().cloned().collect();
        let reference_names: Vec<String> = constants.references.keys().cloned().collect();
        let alias_names: Vec<String> = constants.str_type_aliases.iter().cloned().collect();
        for (name, value) in constants.values {
            insert_owned(&self.rust_constants, &path, name, value);
        }
        for (name, reference) in constants.references {
            insert_owned(&self.rust_constant_references, &path, name, reference);
        }
        for alias in constants.str_type_aliases {
            insert_owned(&self.rust_str_type_aliases, &path, alias, ());
        }
        self.rust_str_type_alias_paths
            .insert(path.clone(), alias_names);
        self.rust_constants_paths.insert(path.clone(), value_names);
        self.rust_constant_reference_paths
            .insert(path, reference_names);
    }

    /// Store JS/TS constants extracted from a file, replacing any previous constants for that file
    pub fn add_js_constants(
        &self,
        path: PathBuf,
        constants: std::collections::HashMap<String, String>,
    ) {
        self.remove_js_constants_for_file(&path);
        let names: Vec<String> = constants.keys().cloned().collect();
        for (name, value) in constants {
            insert_owned(&self.js_constants, &path, name, value);
        }
        self.js_constants_paths.insert(path, names);
    }

    /// Store the forwarding helpers declared in a file, replacing any previous set.
    pub fn add_forwarders(&self, path: PathBuf, forwarders: Vec<Forwarder>) {
        self.remove_forwarders_for_file(&path);
        let names: Vec<String> = forwarders.iter().map(|f| f.function_name.clone()).collect();

        for forwarder in forwarders {
            self.forwarders
                .entry(forwarder.function_name.clone())
                .or_default()
                .push((path.clone(), forwarder));
        }

        self.forwarder_paths.insert(path, names);
    }

    /// Remove all forwarding helpers associated with a file
    pub fn remove_forwarders_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.forwarder_paths.remove(path) {
            remove_owned(&self.forwarders, path, names);
        }
    }

    /// Forwarding helpers that can be resolved unambiguously, keyed by function name.
    ///
    /// A name declared by two files with *different* shapes is dropped: without
    /// following imports there is no way to tell which declaration a call site
    /// means, and guessing would attribute the event to the wrong helper.
    #[must_use]
    pub fn get_all_forwarders(&self) -> std::collections::HashMap<String, Forwarder> {
        self.forwarders
            .iter()
            .filter_map(|entry| {
                let (first, rest) = entry.value().split_first()?;

                rest.iter()
                    .all(|(_, other)| other == &first.1)
                    .then(|| (entry.key().clone(), first.1.clone()))
            })
            .collect()
    }

    /// Remove all Rust constants associated with a file
    pub fn remove_rust_constants_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.rust_constants_paths.remove(path) {
            remove_owned(&self.rust_constants, path, names);
        }
        if let Some((_, names)) = self.rust_constant_reference_paths.remove(path) {
            remove_owned(&self.rust_constant_references, path, names);
        }
        if let Some((_, names)) = self.rust_str_type_alias_paths.remove(path) {
            remove_owned(&self.rust_str_type_aliases, path, names);
        }
    }

    /// Remove all JS/TS constants associated with a file
    pub fn remove_js_constants_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.js_constants_paths.remove(path) {
            remove_owned(&self.js_constants, path, names);
        }
    }

    /// Collect all global Rust constants as a `HashMap` (for passing to parsers),
    /// with `const A: &str = B;` resolved across files
    pub fn get_all_rust_constants(&self) -> std::collections::HashMap<String, String> {
        let mut values = latest_owned(&self.rust_constants);
        let str_type_aliases: std::collections::HashSet<String> = self
            .rust_str_type_aliases
            .iter()
            .map(|entry| entry.key().clone())
            .collect();
        let references = crate::utils::str_typed_references(
            latest_owned(&self.rust_constant_references),
            &str_type_aliases,
        );

        crate::utils::resolve_constant_references(
            &mut values,
            &references,
            &std::collections::HashMap::new(),
        );

        values
    }

    /// Collect all global JS/TS constants as a `HashMap` (for passing to parsers)
    pub fn get_all_js_constants(&self) -> std::collections::HashMap<String, String> {
        latest_owned(&self.js_constants)
    }
}

/// Record that `path` declares `name`. Files keep their own entries side by side.
fn insert_owned<T>(map: &DashMap<String, Vec<(PathBuf, T)>>, path: &Path, name: String, value: T) {
    map.entry(name)
        .or_default()
        .push((path.to_path_buf(), value));
}

/// Drop the entries `path` declared under `names`, leaving other files' entries.
///
/// The emptiness check and the removal happen under one shard lock (`remove_if`):
/// checked and removed as two steps, a same-named entry another thread adds in
/// between would be removed with it.
fn remove_owned<T>(map: &DashMap<String, Vec<(PathBuf, T)>>, path: &Path, names: Vec<String>) {
    for name in names {
        if let Some(mut entry) = map.get_mut(&name) {
            entry.retain(|(declared_in, _)| declared_in != path);
        }

        map.remove_if(&name, |_, entries| entries.is_empty());
    }
}

/// The entries `path` declared, by name.
fn owned_by<T: Clone>(
    map: &DashMap<String, Vec<(PathBuf, T)>>,
    paths: &DashMap<PathBuf, Vec<String>>,
    path: &Path,
) -> std::collections::HashMap<String, T> {
    let Some(names) = paths.get(path) else {
        return std::collections::HashMap::new();
    };

    names
        .iter()
        .filter_map(|name| {
            let entries = map.get(name)?;
            let (_, value) = entries
                .iter()
                .find(|(declared_in, _)| declared_in == path)?;

            Some((name.clone(), value.clone()))
        })
        .collect()
}

/// One value per name: the most recently indexed file's, as a single map always gave.
fn latest_owned<T: Clone>(
    map: &DashMap<String, Vec<(PathBuf, T)>>,
) -> std::collections::HashMap<String, T> {
    map.iter()
        .filter_map(|entry| {
            let (_, value) = entry.value().last()?;

            Some((entry.key().clone(), value.clone()))
        })
        .collect()
}
