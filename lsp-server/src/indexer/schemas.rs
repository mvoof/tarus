//! Command schema, type alias, and event schema CRUD operations

use super::types::{CommandSchema, EventSchema};
use super::{GeneratorKind, ProjectIndex};
use dashmap::DashMap;
use std::path::{Path, PathBuf};

/// What choosing between two files' schemas for one name needs.
trait Schema: Clone {
    fn source_path(&self) -> &Path;
    fn generator(&self) -> GeneratorKind;
}

impl Schema for CommandSchema {
    fn source_path(&self) -> &Path {
        &self.source_path
    }

    fn generator(&self) -> GeneratorKind {
        self.generator
    }
}

impl Schema for EventSchema {
    fn source_path(&self) -> &Path {
        &self.source_path
    }

    fn generator(&self) -> GeneratorKind {
        self.generator
    }
}

impl ProjectIndex {
    /// Store a command schema (replaces any existing schema for the same command name)
    pub fn add_schema(&self, schema: CommandSchema) {
        self.command_schemas
            .insert(schema.command_name.clone(), schema.clone());
        self.add_schema_candidate(schema);
    }

    /// Record a command schema without making it the one in use: a lower-priority
    /// declaration that takes over once the file whose schema is in use drops it.
    pub fn add_schema_candidate(&self, schema: CommandSchema) {
        let name = schema.command_name.clone();
        record_candidate(&self.command_schema_candidates, &name, schema.clone());
        self.generated_file_paths
            .entry(schema.source_path)
            .or_default()
            .push(name);
    }

    /// Remove all schemas associated with a specific file.
    ///
    /// A name another file still declares falls back to that file's schema: when
    /// `old.rs` and `new.rs` both define `greet`, deleting either keeps `greet`.
    pub fn remove_schemas_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.generated_file_paths.remove(path) {
            drop_file_schemas(
                &self.command_schemas,
                &self.command_schema_candidates,
                path,
                names,
            );
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
        let name = schema.event_name.clone();
        self.event_schemas.insert(name.clone(), schema.clone());
        self.forget_event_diagnostic_info(&name);
        self.add_event_schema_candidate(schema);
    }

    /// Record an event schema without making it the one in use, as
    /// [`Self::add_schema_candidate`] does for commands.
    pub fn add_event_schema_candidate(&self, schema: EventSchema) {
        let name = schema.event_name.clone();
        record_candidate(&self.event_schema_candidates, &name, schema.clone());
        self.generated_event_paths
            .entry(schema.source_path)
            .or_default()
            .push(name);
    }

    /// Remove all event schemas associated with a specific file, falling back to
    /// another file's schema as [`Self::remove_schemas_for_file`] does.
    pub fn remove_event_schemas_for_file(&self, path: &Path) {
        if let Some((_, names)) = self.generated_event_paths.remove(path) {
            for name in drop_file_schemas(
                &self.event_schemas,
                &self.event_schema_candidates,
                path,
                names,
            ) {
                self.forget_event_diagnostic_info(&name);
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

/// Keep `schema` as its file's declaration of `name`, replacing an earlier one.
fn record_candidate<S: Schema>(candidates: &DashMap<String, Vec<S>>, name: &str, schema: S) {
    let mut entry = candidates.entry(name.to_string()).or_default();
    entry.retain(|candidate| candidate.source_path() != schema.source_path());
    entry.push(schema);
}

/// Drop what `path` declared under `names`. A name whose schema in use came from
/// `path` falls back to another file's: a generated binding first, as indexing
/// prefers, then the most recently indexed. Returns the names whose schema in use
/// changed.
fn drop_file_schemas<S: Schema>(
    active: &DashMap<String, S>,
    candidates: &DashMap<String, Vec<S>>,
    path: &Path,
    names: Vec<String>,
) -> Vec<String> {
    let mut changed = Vec::new();

    for name in names {
        let fallback = candidates.get_mut(&name).and_then(|mut entry| {
            entry.retain(|candidate| candidate.source_path() != path);

            entry
                .iter()
                .rev()
                .find(|candidate| candidate.generator() != GeneratorKind::RustSource)
                .or_else(|| entry.last())
                .cloned()
        });

        candidates.remove_if(&name, |_, entry| entry.is_empty());

        if active
            .get(&name)
            .is_none_or(|schema| schema.source_path() != path)
        {
            continue;
        }

        match fallback {
            Some(schema) => {
                active.insert(name.clone(), schema);
            }

            None => {
                active.remove(&name);
            }
        }

        changed.push(name);
    }

    changed
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

    #[test]
    fn test_removing_a_bindings_file_falls_back_to_the_schema_it_shadowed() {
        use crate::indexer::GeneratorKind;

        let index = ProjectIndex::new();
        let schema = |source: &str, generator| CommandSchema {
            command_name: "greet".to_string(),
            params: Vec::new(),
            return_type: "string".to_string(),
            source_path: PathBuf::from(source),
            generator,
        };

        index.add_schema(schema("bindings.ts", GeneratorKind::Specta));
        index.add_schema_candidate(schema("commands.rs", GeneratorKind::RustSource));
        assert_eq!(
            index.get_schema("greet").map(|s| s.generator),
            Some(GeneratorKind::Specta)
        );

        index.remove_schemas_for_file(Path::new("bindings.ts"));

        assert_eq!(
            index.get_schema("greet").map(|s| s.source_path),
            Some(PathBuf::from("commands.rs"))
        );

        index.remove_schemas_for_file(Path::new("commands.rs"));

        assert!(index.get_schema("greet").is_none());
    }
}
