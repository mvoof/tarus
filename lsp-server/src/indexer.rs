use crate::syntax::{Behavior, EntityType};
use dashmap::DashMap;
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::RwLock;
use tower_lsp_server::lsp_types::{Location, Position, Range, SymbolInformation, SymbolKind, Uri};
use tower_lsp_server::UriExt;

/// Which tool generated the binding file (or the source itself)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorKind {
    TsRs,
    Specta,
    Typegen,
    RustSource,
}

/// A type generator discovered from project configuration files
#[derive(Debug, Clone)]
pub struct DiscoveredGenerator {
    pub kind: GeneratorKind,
    /// Absolute, normalized output path (file or directory)
    pub output_path: PathBuf,
    /// `true` for directory-match generators (ts-rs, typegen); `false` for exact-file generators (specta)
    pub is_directory: bool,
}

/// A single parameter in a command schema
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSchema {
    pub name: String,
    pub ts_type: String,
}

/// Type signature of a Tauri event payload, extracted from bindings or Rust source
#[derive(Debug, Clone)]
pub struct EventSchema {
    pub event_name: String,
    pub payload_type: String,
    pub source_path: PathBuf,
    pub generator: GeneratorKind,
}

/// Type signature of a Tauri command, extracted from bindings or Rust source
#[derive(Debug, Clone)]
pub struct CommandSchema {
    pub command_name: String,
    pub params: Vec<ParamSchema>,
    pub return_type: String,
    pub source_path: PathBuf,
    pub generator: GeneratorKind,
}

/// A single occurrence in a file (parser result)
#[derive(Debug, Clone)]
pub struct Finding {
    pub key: String,                           // Name ("save_file")
    pub entity: EntityType,                    // Command or Event
    pub behavior: Behavior,                    // Call, Emit, Listen
    pub range: Range,                          // Coordinates
    pub call_arg_count: Option<u32>,           // For SpectaCall: positional arg count
    pub call_param_keys: Option<Vec<String>>,  // For Call: object literal keys in second arg
    pub return_type: Option<String>,           // For Call with generics: invoke<T>() type argument
    pub call_name_end: Option<Position>,       // End of "invoke" identifier (for inserting <T>)
    pub type_arg_range: Option<Range>,         // Range of <T> in invoke<T>() (for replacing)
    pub codegen_origin: Option<GeneratorKind>, // Set when call site is from typed codegen (e.g. specta events API)
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
    pub call_arg_count: Option<u32>,
    pub call_param_keys: Option<Vec<String>>,
    pub return_type: Option<String>,
    pub call_name_end: Option<Position>,
    pub type_arg_range: Option<Range>,
    pub codegen_origin: Option<GeneratorKind>,
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
        position: Position,
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
        // If the file has already been indexed...
        if let Some((_, keys)) = self.file_map.remove(&path.to_path_buf()) {
            for key in keys {
                self.map.entry(key.clone()).and_modify(|locs| {
                    locs.retain(|loc| loc.path != path);
                });

                // If the list becomes empty, you can remove the key from the map,
                // to avoid storing garbage
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

    /// Store a command schema (replaces any existing schema for the same command name)
    pub fn add_schema(&self, schema: CommandSchema) {
        let path = schema.source_path.clone();
        let name = schema.command_name.clone();
        self.command_schemas.insert(name.clone(), schema);
        self.generated_file_paths
            .entry(path)
            .or_default()
            .push(name);
    }

    /// Remove all schemas associated with a specific file
    pub fn remove_schemas_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.generated_file_paths.remove(&path.to_path_buf()) {
            for name in names {
                self.command_schemas.remove(&name);
            }
        }
    }

    /// Retrieve a command schema by command name
    pub fn get_schema(&self, name: &str) -> Option<CommandSchema> {
        self.command_schemas.get(name).map(|v| v.clone())
    }

    /// Returns true if at least one binding file (Specta / ts-rs / typegen) has been indexed.
    ///
    /// Used to gate type-level diagnostics: type checking is only meaningful when
    /// a generated bindings file is present in the workspace.
    pub fn has_bindings_files(&self) -> bool {
        self.command_schemas.iter().any(|e| {
            matches!(
                e.value().generator,
                GeneratorKind::Specta | GeneratorKind::TsRs | GeneratorKind::Typegen
            )
        }) || !self.type_aliases.is_empty()
            || self.event_schemas.iter().any(|e| {
                matches!(
                    e.value().generator,
                    GeneratorKind::Specta | GeneratorKind::TsRs | GeneratorKind::Typegen
                )
            })
    }

    /// Replace the list of config-discovered generators.
    ///
    /// # Panics
    ///
    /// Panics if the lock is poisoned (only if another thread panicked while holding it).
    pub fn set_generator_bindings(&self, bindings: Vec<DiscoveredGenerator>) {
        *self.generator_bindings.write().unwrap() = bindings;
    }

    /// Return the `GeneratorKind` for a given file path based on config-discovered generators.
    ///
    /// For directory-match generators the path must be inside the output directory.
    /// For exact-file generators the path must equal the output file.
    ///
    /// # Panics
    ///
    /// Panics if the lock is poisoned (only if another thread panicked while holding it).
    pub fn get_generator_for_file(&self, path: &Path) -> Option<GeneratorKind> {
        let bindings = self.generator_bindings.read().unwrap();
        for b in bindings.iter() {
            if b.is_directory {
                if path.starts_with(&b.output_path) {
                    return Some(b.kind);
                }
            } else if path == b.output_path {
                return Some(b.kind);
            }
        }
        None
    }

    /// Store a type alias (name -> definition string)
    pub fn add_type_alias(&self, name: String, def: String, path: PathBuf) {
        self.type_aliases.insert(name.clone(), def);
        self.generated_alias_paths
            .entry(path)
            .or_default()
            .push(name);
    }

    /// Remove all type aliases associated with a specific file
    pub fn remove_type_aliases_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.generated_alias_paths.remove(&path.to_path_buf()) {
            for name in names {
                self.type_aliases.remove(&name);
            }
        }
    }

    /// Store an event schema (replaces any existing schema for the same event name)
    pub fn add_event_schema(&self, schema: EventSchema) {
        let path = schema.source_path.clone();
        let name = schema.event_name.clone();
        self.event_schemas.insert(name.clone(), schema);
        self.generated_event_paths
            .entry(path)
            .or_default()
            .push(name);
    }

    /// Remove all event schemas associated with a specific file
    pub fn remove_event_schemas_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.generated_event_paths.remove(&path.to_path_buf()) {
            for name in names {
                self.event_schemas.remove(&name);
            }
        }
    }

    /// Retrieve an event schema by event name
    pub fn get_event_schema(&self, name: &str) -> Option<EventSchema> {
        self.event_schemas.get(name).map(|v| v.clone())
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

        // Collect keys
        let Some(keys) = self.file_map.get(&path.to_path_buf()) else {
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
                all_locations.iter().filter(|l| l.path == path).collect();

            // Define "Targets" - where the lens should point
            // These are all the locations MINUS the current file (so as not to reference itself)
            let targets: Vec<LocationInfo> = all_locations
                .iter()
                .filter(|l| l.path != path) // Exclude the current file
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

                let limit = self.reference_limit.load(Ordering::Relaxed);

                if is_current_rust {
                    // Rust Logic: Show frontend files separately
                    Self::push_file_lenses(
                        &mut result,
                        my_loc.range,
                        frontend_targets,
                        limit,
                        "references",
                    );
                } else {
                    // Frontend Logic: "Go to Rust" + "Go to others"
                    // 1. Link to Rust (if exists)
                    Self::push_file_lenses(
                        &mut result,
                        my_loc.range,
                        rust_targets,
                        limit,
                        "rust refs",
                    );
                    // 2. Links to other frontend files
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

        // Group by file path
        let mut files_map: HashMap<PathBuf, Vec<LocationInfo>> = HashMap::new();
        for t in &targets {
            files_map.entry(t.path.clone()).or_default().push(t.clone());
        }

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

                result.push((range, format!("Go to {fname}"), locs));
            }
        } else {
            // If > limit files, show summary
            result.push((
                range,
                format!("{} {}", targets.len(), summary_label),
                targets,
            ));
        }
    }

    /// Generates a report only for a specific file (delta update)
    pub fn file_report(&self, path: &Path) -> String {
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let mut report_message = String::new();

        let _ = writeln!(report_message, "\n📝 === UPDATE REPORT: {filename} ===");

        let Some(keys) = self.file_map.get(&path.to_path_buf()) else {
            return format!("📝 File update: {filename:?} (No Tarus keys found)");
        };

        let keys_clone: Vec<IndexKey> = keys.value().clone();

        report_message.push_str("\n🔑 === 1. MAIN KEY INDEX (Delta Subset) ===\n");

        let mut delta_map: HashMap<IndexKey, Vec<LocationInfo>> = HashMap::new();

        for key in &keys_clone {
            if let Some(locs) = self.map.get(key) {
                let file_locs: Vec<LocationInfo> =
                    locs.iter().filter(|l| l.path == path).cloned().collect();

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
        delta_file_map.insert(path.to_path_buf(), keys_clone);

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
    pub fn get_document_symbols(&self, path: &Path) -> Vec<SymbolInformation> {
        let mut symbols = Vec::new();

        let Some(keys) = self.file_map.get(&path.to_path_buf()) else {
            return symbols;
        };

        let Some(uri) = Uri::from_file_path(path) else {
            return symbols;
        };

        for key in keys.value() {
            if let Some(locations) = self.map.get(key) {
                for loc in locations.iter().filter(|l| l.path == path) {
                    let kind = match key.entity {
                        EntityType::Command => SymbolKind::FUNCTION,
                        EntityType::Event => SymbolKind::EVENT,
                    };

                    // Use behavior terms
                    let behavior_label = match loc.behavior {
                        Behavior::Definition => "command",
                        Behavior::Call => "invoke",
                        Behavior::SpectaCall => "commands",
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
                    Behavior::SpectaCall => "commands",
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
        // Events with an EventSchema from a binding generator are known to exist
        // on the Rust side (e.g. specta typed events use `StructName(...).emit_to()`
        // which isn't captured as a string-based emit Finding).
        let has_event_schema =
            key.entity == EntityType::Event && self.event_schemas.get(&key.name).is_some();
        let info = DiagnosticInfo {
            has_definition: locations.iter().any(|l| l.behavior == Behavior::Definition),
            has_calls: locations
                .iter()
                .any(|l| matches!(l.behavior, Behavior::Call | Behavior::SpectaCall)),
            has_emitters: locations.iter().any(|l| l.behavior == Behavior::Emit)
                || has_event_schema,
            has_listeners: locations.iter().any(|l| l.behavior == Behavior::Listen)
                || has_event_schema,
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
