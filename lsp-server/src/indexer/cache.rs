//! Cache management for the project index to ensure high performance

use super::{IndexKey, LocationInfo};
use dashmap::DashMap;
use std::path::PathBuf;
use std::sync::RwLock;
use tower_lsp_server::lsp_types::Range;

type CommandNamesCache = RwLock<Option<Vec<(String, Option<LocationInfo>)>>>;
type LensDataCache = DashMap<PathBuf, Vec<(Range, String, Vec<LocationInfo>)>>;
type PositionIndexCache = DashMap<PathBuf, Vec<(Range, IndexKey)>>;

/// Cache manager for all index-related caches
#[derive(Debug)]
pub struct CacheManager {
    /// Cache for `get_all_names()` results (command names)
    pub command_names: CommandNamesCache,

    /// Cache for `get_all_names()` results (event names)
    pub event_names: CommandNamesCache,

    /// Cache for diagnostic info (avoids re-iterating locations)
    pub diagnostic_info: DashMap<IndexKey, DiagnosticInfo>,

    /// Cache for `CodeLens` data by file path
    pub lens_data: LensDataCache,

    /// Spatial index for fast position lookups (sorted by start position)
    pub position_index: PositionIndexCache,
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
            command_names: RwLock::new(None),
            event_names: RwLock::new(None),
            diagnostic_info: DashMap::new(),
            lens_data: DashMap::new(),
            position_index: DashMap::new(),
        }
    }

    /// Invalidate all caches
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn invalidate_all(&self) {
        *self
            .command_names
            .write()
            .expect("Command names cache lock poisoned") = None;
        *self
            .event_names
            .write()
            .expect("Event names cache lock poisoned") = None;
        self.diagnostic_info.clear();
    }

    /// Invalidate caches for a specific file
    ///
    /// # Panics
    ///
    /// Panics if the cache lock is poisoned (only occurs if another thread panicked while holding the lock)
    pub fn invalidate_file(&self, path: &PathBuf) {
        self.invalidate_all();
        self.lens_data.remove(path);
        self.position_index.remove(path);
    }
}

/// Status of a command
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommandStatus {
    pub has_definition: bool,
    pub has_calls: bool,
}

/// Status of an event
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventStatus {
    pub has_emitters: bool,
    pub has_listeners: bool,
}

/// Diagnostic information for a command/event
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticInfo {
    pub command: CommandStatus,
    pub event: EventStatus,
}

impl DiagnosticInfo {
    #[must_use]
    pub fn has_definition(&self) -> bool {
        self.command.has_definition
    }

    #[must_use]
    pub fn has_calls(&self) -> bool {
        self.command.has_calls
    }

    #[must_use]
    pub fn has_emitters(&self) -> bool {
        self.event.has_emitters
    }

    #[must_use]
    pub fn has_listeners(&self) -> bool {
        self.event.has_listeners
    }
}
