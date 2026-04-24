//! Command schema, type alias, and event schema CRUD operations

use super::types::{CommandSchema, EventSchema};
use super::ProjectIndex;
use std::path::{Path, PathBuf};

impl ProjectIndex {
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
}
