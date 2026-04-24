//! Name and diagnostic info caching

use crate::syntax::{Behavior, EntityType};

use super::types::{DiagnosticInfo, IndexKey, LocationInfo, NameLocation};
use super::ProjectIndex;

impl ProjectIndex {
    /// Get all known names for a specific entity type (for completion)
    pub fn get_all_names(&self, entity: EntityType) -> Vec<NameLocation> {
        // Select appropriate cache
        let cache = match entity {
            EntityType::Command => &self.command_names_cache,
            EntityType::Event => &self.event_names_cache,
        };

        // Try to read from cache
        {
            let cache_read = cache.read();

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
        *cache.write() = Some(result.clone());

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
        let has_definition = locations.iter().any(|l| l.behavior == Behavior::Definition);

        let info = match key.entity {
            EntityType::Command => DiagnosticInfo::Command {
                has_definition,
                has_calls: locations
                    .iter()
                    .any(|l| matches!(l.behavior, Behavior::Call | Behavior::SpectaCall)),
            },
            EntityType::Event => {
                // Events with an EventSchema from a binding generator are known to exist
                // on the Rust side (e.g. specta typed events use `StructName(...).emit_to()`
                // which isn't captured as a string-based emit Finding).
                let has_event_schema = self.event_schemas.get(&key.name).is_some();

                DiagnosticInfo::Event {
                    has_definition,
                    has_emitters: locations.iter().any(|l| l.behavior == Behavior::Emit)
                        || has_event_schema,
                    has_listeners: locations.iter().any(|l| l.behavior == Behavior::Listen)
                        || has_event_schema,
                }
            }
        };

        // Store in cache
        self.diagnostic_info_cache.insert(key.clone(), info.clone());

        info
    }
}
