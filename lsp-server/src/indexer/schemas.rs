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

    /// Remove all schemas associated with a specific file.
    ///
    /// A name another file has since claimed is left alone: when `new.rs` takes over
    /// `greet` before `old.rs` is deleted, deleting `old.rs` must not drop it.
    pub fn remove_schemas_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.generated_file_paths.remove(path) {
            for name in names {
                self.command_schemas
                    .remove_if(&name, |_, schema| schema.source_path == path);
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
        self.forget_event_diagnostic_info(&name);
        self.generated_event_paths
            .entry(path)
            .or_default()
            .push(name);
    }

    /// Remove all event schemas associated with a specific file, leaving a name
    /// another file has since claimed, as [`Self::remove_schemas_for_file`] does.
    pub fn remove_event_schemas_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.generated_event_paths.remove(path) {
            for name in names {
                if self
                    .event_schemas
                    .remove_if(&name, |_, schema| schema.source_path == path)
                    .is_some()
                {
                    self.forget_event_diagnostic_info(&name);
                }
            }
        }
    }

    /// An event schema counts as an emitter and a listener in the cached
    /// [`super::DiagnosticInfo`], so the event's entry is stale once its schema changes.
    fn forget_event_diagnostic_info(&self, name: &str) {
        self.diagnostic_info_cache.remove(&super::IndexKey {
            entity: crate::syntax::EntityType::Event,
            name: name.to_string(),
        });
    }

    /// Retrieve an event schema by event name
    pub fn get_event_schema(&self, name: &str) -> Option<EventSchema> {
        self.event_schemas.get(name).map(|v| v.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_type_aliases_add_and_remove() {
        let index = ProjectIndex::new();
        let path = PathBuf::from("types.ts");

        index.add_type_alias(
            "UserProfile".to_string(),
            "{ id: number; name: string }".to_string(),
            path.clone(),
        );

        assert_eq!(
            index.type_aliases.get("UserProfile").map(|v| v.clone()),
            Some("{ id: number; name: string }".to_string())
        );

        index.remove_type_aliases_for_file(&path);

        assert!(
            !index.type_aliases.contains_key("UserProfile"),
            "Alias should be removed"
        );
    }

    #[test]
    fn test_event_schema_changes_reach_cached_diagnostic_info() {
        use crate::indexer::{DiagnosticInfo, GeneratorKind, IndexKey};
        use crate::syntax::EntityType;

        let index = ProjectIndex::new();
        let bindings = PathBuf::from("bindings.ts");
        let key = IndexKey {
            entity: EntityType::Event,
            name: "progress".to_string(),
        };
        let has_emitters = |index: &ProjectIndex| {
            matches!(
                index.get_diagnostic_info(&key),
                DiagnosticInfo::Event {
                    has_emitters: true,
                    ..
                }
            )
        };

        assert!(!has_emitters(&index));

        index.add_event_schema(EventSchema {
            event_name: "progress".to_string(),
            payload_type: "number".to_string(),
            source_path: bindings.clone(),
            generator: GeneratorKind::Specta,
        });
        assert!(has_emitters(&index), "a schema added after a cached miss");

        index.remove_event_schemas_for_file(&bindings);
        assert!(!has_emitters(&index), "a schema removed after a cached hit");
    }

    #[test]
    fn test_removing_a_file_keeps_an_event_schema_another_file_took_over() {
        use crate::indexer::GeneratorKind;

        let index = ProjectIndex::new();
        let schema = |source: &str| EventSchema {
            event_name: "progress".to_string(),
            payload_type: "number".to_string(),
            source_path: PathBuf::from(source),
            generator: GeneratorKind::RustSource,
        };

        index.add_event_schema(schema("old.rs"));
        index.add_event_schema(schema("new.rs"));
        index.remove_event_schemas_for_file(Path::new("old.rs"));

        assert_eq!(
            index.get_event_schema("progress").map(|s| s.source_path),
            Some(PathBuf::from("new.rs"))
        );
    }
}
