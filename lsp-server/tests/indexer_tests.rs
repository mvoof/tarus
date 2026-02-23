//! Indexer functionality tests

mod common_paths;

use common_paths::test_path;
use lsp_server::indexer::{
    CommandSchema, FileIndex, Finding, GeneratorKind, IndexKey, ParamSchema, ProjectIndex,
};
use lsp_server::syntax::{Behavior, EntityType};
use tower_lsp_server::lsp_types::{Position, Range};

fn create_test_finding(key: &str, entity: EntityType, behavior: Behavior) -> Finding {
    Finding {
        key: key.to_string(),
        entity,
        behavior,
        range: Range {
            start: Position { line: 0, character: 0 },
            end: Position { line: 0, character: key.len() as u32 },
        },
        call_arg_count: None,
        call_param_keys: None,
    }
}

#[test]
fn test_add_file_to_index() {
    let index = ProjectIndex::new();
    let path = test_path("test.rs");
    
    let file_index = FileIndex {
        path: path.clone(),
        findings: vec![
            create_test_finding("greet", EntityType::Command, Behavior::Definition),
        ],
    };
    
    index.add_file(file_index);
    
    let locations = index.get_locations(EntityType::Command, "greet");
    assert_eq!(locations.len(), 1);
    assert_eq!(locations[0].path, path);
}

#[test]
fn test_remove_file_from_index() {
    let index = ProjectIndex::new();
    let path = test_path("test.rs");
    
    let file_index = FileIndex {
        path: path.clone(),
        findings: vec![
            create_test_finding("greet", EntityType::Command, Behavior::Definition),
        ],
    };
    
    index.add_file(file_index);
    
    // Verify it's in the index
    let locations_before = index.get_locations(EntityType::Command, "greet");
    assert_eq!(locations_before.len(), 1);
    
    // Remove the file
    index.remove_file(&path);
    
    // Verify it's removed
    let locations_after = index.get_locations(EntityType::Command, "greet");
    assert_eq!(locations_after.len(), 0);
}

#[test]
fn test_get_locations() {
    let index = ProjectIndex::new();
    let path1 = test_path("backend.rs");
    let path2 = test_path("frontend.ts");
    
    // Add backend command definition
    let backend_file = FileIndex {
        path: path1.clone(),
        findings: vec![
            create_test_finding("greet", EntityType::Command, Behavior::Definition),
        ],
    };
    
    // Add frontend command call
    let frontend_file = FileIndex {
        path: path2.clone(),
        findings: vec![
            create_test_finding("greet", EntityType::Command, Behavior::Call),
        ],
    };
    
    index.add_file(backend_file);
    index.add_file(frontend_file);
    
    let locations = index.get_locations(EntityType::Command, "greet");
    assert_eq!(locations.len(), 2);
    
    // Check we have both definition and call
    let has_definition = locations.iter().any(|l| l.behavior == Behavior::Definition);
    let has_call = locations.iter().any(|l| l.behavior == Behavior::Call);
    assert!(has_definition);
    assert!(has_call);
}

#[test]
fn test_get_key_at_position() {
    let index = ProjectIndex::new();
    let path = test_path("test.ts");
    
    let finding = Finding {
        key: "greet".to_string(),
        entity: EntityType::Command,
        behavior: Behavior::Call,
        range: Range {
            start: Position { line: 5, character: 10 },
            end: Position { line: 5, character: 15 },
        },
        call_arg_count: None,
        call_param_keys: None,
    };
    
    let file_index = FileIndex {
        path: path.clone(),
        findings: vec![finding],
    };
    
    index.add_file(file_index);
    
    // Position inside the range
    let result = index.get_key_at_position(&path, Position { line: 5, character: 12 });
    assert!(result.is_some());
    
    let (key, _loc) = result.unwrap();
    assert_eq!(key.name, "greet");
    assert_eq!(key.entity, EntityType::Command);
}

#[test]
fn test_get_diagnostic_info() {
    let index = ProjectIndex::new();
    
    // Add a command definition
    let backend_file = FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            create_test_finding("greet", EntityType::Command, Behavior::Definition),
        ],
    };
    
    // Add a command call
    let frontend_file = FileIndex {
        path: test_path("frontend.ts"),
        findings: vec![
            create_test_finding("greet", EntityType::Command, Behavior::Call),
        ],
    };
    
    index.add_file(backend_file);
    index.add_file(frontend_file);
    
    let key = IndexKey {
        name: "greet".to_string(),
        entity: EntityType::Command,
    };
    
    let info = index.get_diagnostic_info(&key);
    assert!(info.has_definition, "Should have definition");
    assert!(info.has_calls, "Should have calls");
}

#[test]
fn test_multiple_files_same_command() {
    let index = ProjectIndex::new();

    // Add command definition
    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            create_test_finding("get_user", EntityType::Command, Behavior::Definition),
        ],
    });

    // Add multiple calls from different files
    index.add_file(FileIndex {
        path: test_path("app.ts"),
        findings: vec![
            create_test_finding("get_user", EntityType::Command, Behavior::Call),
        ],
    });

    index.add_file(FileIndex {
        path: test_path("profile.tsx"),
        findings: vec![
            create_test_finding("get_user", EntityType::Command, Behavior::Call),
        ],
    });

    let locations = index.get_locations(EntityType::Command, "get_user");
    assert_eq!(locations.len(), 3, "Should find definition + 2 calls");
}

fn make_schema(command_name: &str, path: &str, generator: GeneratorKind) -> CommandSchema {
    CommandSchema {
        command_name: command_name.to_string(),
        params: vec![ParamSchema {
            name: "id".to_string(),
            ts_type: "number".to_string(),
        }],
        return_type: "string".to_string(),
        source_path: test_path(path),
        generator,
    }
}

#[test]
fn test_add_schema_to_index() {
    let index = ProjectIndex::new();
    let schema = make_schema("get_user", "bindings.ts", GeneratorKind::Specta);

    index.add_schema(schema);

    let result = index.get_schema("get_user");
    assert!(result.is_some());
    let s = result.unwrap();
    assert_eq!(s.command_name, "get_user");
    assert_eq!(s.return_type, "string");
    assert_eq!(s.params.len(), 1);
}

#[test]
fn test_remove_schemas_for_file() {
    let index = ProjectIndex::new();
    let path = "bindings.ts";

    index.add_schema(make_schema("get_user", path, GeneratorKind::Specta));
    index.add_schema(make_schema("create_user", path, GeneratorKind::Specta));

    assert!(index.get_schema("get_user").is_some());
    assert!(index.get_schema("create_user").is_some());

    index.remove_schemas_for_file(&test_path(path));

    assert!(index.get_schema("get_user").is_none(), "get_user schema should be removed");
    assert!(index.get_schema("create_user").is_none(), "create_user schema should be removed");
}

#[test]
fn test_schema_survives_file_map_remove() {
    let index = ProjectIndex::new();

    // Add a Rust finding for the command
    index.add_file(FileIndex {
        path: test_path("lib.rs"),
        findings: vec![
            create_test_finding("get_user", EntityType::Command, Behavior::Definition),
        ],
    });

    // Add a schema from bindings
    index.add_schema(make_schema("get_user", "bindings.ts", GeneratorKind::Specta));

    // Remove the Rust file from the main index
    index.remove_file(&test_path("lib.rs"));

    // Schema should still be there (it's in a separate map)
    let result = index.get_schema("get_user");
    assert!(result.is_some(), "Schema should survive remove_file on Rust file");
}

#[test]
fn test_overwrite_schema() {
    let index = ProjectIndex::new();

    // Add initial schema
    let schema1 = make_schema("get_user", "bindings.ts", GeneratorKind::Specta);
    index.add_schema(schema1);

    // Overwrite with a new schema that has different params
    let schema2 = CommandSchema {
        command_name: "get_user".to_string(),
        params: vec![
            ParamSchema { name: "id".to_string(), ts_type: "number".to_string() },
            ParamSchema { name: "token".to_string(), ts_type: "string".to_string() },
        ],
        return_type: "User".to_string(),
        source_path: test_path("bindings.ts"),
        generator: GeneratorKind::Specta,
    };
    index.add_schema(schema2);

    let result = index.get_schema("get_user").unwrap();
    assert_eq!(result.params.len(), 2, "Second add should overwrite first");
    assert_eq!(result.return_type, "User");
}

#[test]
fn test_type_aliases() {
    let index = ProjectIndex::new();
    let path = test_path("types.ts");

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

    assert!(index.type_aliases.get("UserProfile").is_none(), "Alias should be removed");
}
