//! Cache management for the project index to ensure high performance

use super::{IndexKey, LocationInfo};
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tower_lsp_server::lsp_types::Range;

/// Cache manager for all index-related caches
#[derive(Debug)]
pub struct CacheManager {
    /// Cache for get_all_names() results (command names)
    #[allow(clippy::type_complexity)]
    pub command_names_cache: RwLock<Option<Vec<(String, Option<LocationInfo>)>>>,

    /// Cache for get_all_names() results (event names)
    #[allow(clippy::type_complexity)]
    pub event_names_cache: RwLock<Option<Vec<(String, Option<LocationInfo>)>>>,

    /// Cache for diagnostic info (avoids re-iterating locations)
    pub diagnostic_info_cache: DashMap<IndexKey, DiagnosticInfo>,

    /// Cache for CodeLens data by file path
    #[allow(clippy::type_complexity)]
    pub lens_data_cache: DashMap<PathBuf, Vec<(Range, String, Vec<LocationInfo>)>>,

    /// Spatial index for fast position lookups (sorted by start position)
    #[allow(clippy::type_complexity)]
    pub position_index_cache: DashMap<PathBuf, Vec<(Range, IndexKey)>>,
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}

impl CacheManager {
    /// Create a new cache manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            command_names_cache: RwLock::new(None),
            event_names_cache: RwLock::new(None),
            diagnostic_info_cache: DashMap::new(),
            lens_data_cache: DashMap::new(),
            position_index_cache: DashMap::new(),
        }
    }

    /// Invalidate all caches
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn invalidate_all(&self) {
        *self
            .command_names_cache
            .write()
            .expect("Command names cache lock poisoned") = None;
        *self
            .event_names_cache
            .write()
            .expect("Event names cache lock poisoned") = None;
        self.diagnostic_info_cache.clear();
    }

    /// Invalidate caches for a specific file
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn invalidate_file(&self, path: &PathBuf) {
        self.invalidate_all();
        self.lens_data_cache.remove(path);
        self.position_index_cache.remove(path);
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
