//! Type generation tests

use lsp_server::indexer::{FileIndex, Finding, Parameter, ProjectIndex};
use lsp_server::syntax::{extract_result_ok_type, Behavior, EntityType};
use lsp_server::typegen::{
    generate_invoke_types, generate_invoke_types_with_config, TypegenConfig,
};
use std::path::Path;

#[test]
fn test_extract_result_ok_type() {
    assert_eq!(extract_result_ok_type("Result<String, Error>"), "String");
    assert_eq!(
        extract_result_ok_type("Result<Vec<User>, String>"),
        "Vec<User>"
    );
    assert_eq!(extract_result_ok_type("String"), "String");
    assert_eq!(extract_result_ok_type("Greet"), "Greet");
}

#[test]
fn test_generate_invoke_types_with_structs() {
    let index = ProjectIndex::new();

    // Add a struct
    index.add_file(FileIndex {
        path: Path::new("backend.rs").to_path_buf(),
        findings: vec![
            Finding {
                key: "User".to_string(),
                entity: EntityType::Struct,
                behavior: Behavior::Definition,
                range: tower_lsp_server::ls_types::Range::default(),
                parameters: None,
                return_type: None,
                fields: Some(vec![
                    Parameter {
                        name: "id".to_string(),
                        type_name: "u32".to_string(),
                    },
                    Parameter {
                        name: "name".to_string(),
                        type_name: "String".to_string(),
                    },
                ]),
                attributes: Some(vec!["#[serde(rename_all = \"camelCase\")]".to_string()]),
            },
            Finding {
                key: "get_user".to_string(),
                entity: EntityType::Command,
                behavior: Behavior::Definition,
                range: tower_lsp_server::ls_types::Range::default(),
                parameters: None,
                return_type: Some("User".to_string()),
                fields: None,
                attributes: None,
            },
        ],
    });

    let output = generate_invoke_types(&index);

    assert!(output.contains("export interface User {"));
    assert!(output.contains("id: number;"));
    assert!(output.contains("name: string;"));
    // Now uses generic return type format
    assert!(output.contains("function invoke<R = User>(cmd: 'get_user'): Promise<R>;"));
}

#[test]
fn test_generic_return_types() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: Path::new("lib.rs").to_path_buf(),
        findings: vec![Finding {
            key: "greet".to_string(),
            entity: EntityType::Command,
            behavior: Behavior::Definition,
            range: tower_lsp_server::ls_types::Range::default(),
            parameters: Some(vec![Parameter {
                name: "name".to_string(),
                type_name: "String".to_string(),
            }]),
            return_type: Some("Result<String, String>".to_string()),
            fields: None,
            attributes: None,
        }],
    });

    let output = generate_invoke_types(&index);

    // Should generate invoke with generic default type
    assert!(
        output.contains("function invoke<R = string>(cmd: 'greet', args: GreetArgs): Promise<R>;"),
        "Expected generic invoke overload, got:\n{output}"
    );
}

#[test]
fn test_strict_mode_no_fallback() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: Path::new("lib.rs").to_path_buf(),
        findings: vec![Finding {
            key: "ping".to_string(),
            entity: EntityType::Command,
            behavior: Behavior::Definition,
            range: tower_lsp_server::ls_types::Range::default(),
            parameters: None,
            return_type: Some("String".to_string()),
            fields: None,
            attributes: None,
        }],
    });

    // Default mode: should have fallback
    let output_default = generate_invoke_types(&index);
    assert!(
        output_default.contains("Fallback for untyped commands"),
        "Default mode should include fallback overload"
    );

    // Strict mode: no fallback
    let config = TypegenConfig {
        dts_output_path: None,
        strict_type_safety: true,
    };
    let output_strict = generate_invoke_types_with_config(&index, &config);
    assert!(
        !output_strict.contains("Fallback for untyped commands"),
        "Strict mode should NOT include fallback overload"
    );
}

#[test]
fn test_enum_unit_variants() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: Path::new("types.rs").to_path_buf(),
        findings: vec![
            Finding {
                key: "Status".to_string(),
                entity: EntityType::Enum,
                behavior: Behavior::Definition,
                range: tower_lsp_server::ls_types::Range::default(),
                parameters: None,
                return_type: None,
                fields: Some(vec![
                    Parameter {
                        name: "Active".to_string(),
                        type_name: "enum_variant".to_string(),
                    },
                    Parameter {
                        name: "Inactive".to_string(),
                        type_name: "enum_variant".to_string(),
                    },
                ]),
                attributes: None,
            },
            Finding {
                key: "get_status".to_string(),
                entity: EntityType::Command,
                behavior: Behavior::Definition,
                range: tower_lsp_server::ls_types::Range::default(),
                parameters: None,
                return_type: Some("Status".to_string()),
                fields: None,
                attributes: None,
            },
        ],
    });

    let output = generate_invoke_types(&index);

    // Unit variant enum should be string literal union
    assert!(
        output.contains("export type Status = 'Active' | 'Inactive';"),
        "Expected string literal union for unit enum, got:\n{output}"
    );
}

#[test]
fn test_enum_tagged_union() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: Path::new("types.rs").to_path_buf(),
        findings: vec![
            Finding {
                key: "Message".to_string(),
                entity: EntityType::Enum,
                behavior: Behavior::Definition,
                range: tower_lsp_server::ls_types::Range::default(),
                parameters: None,
                return_type: None,
                fields: Some(vec![
                    Parameter {
                        name: "Text".to_string(),
                        type_name: "String".to_string(),
                    },
                    Parameter {
                        name: "Number".to_string(),
                        type_name: "i32".to_string(),
                    },
                    Parameter {
                        name: "None".to_string(),
                        type_name: "enum_variant".to_string(),
                    },
                ]),
                attributes: None,
            },
            Finding {
                key: "get_message".to_string(),
                entity: EntityType::Command,
                behavior: Behavior::Definition,
                range: tower_lsp_server::ls_types::Range::default(),
                parameters: None,
                return_type: Some("Message".to_string()),
                fields: None,
                attributes: None,
            },
        ],
    });

    let output = generate_invoke_types(&index);

    // Tagged union: mixed data and unit variants
    assert!(
        output.contains("export type Message ="),
        "Expected tagged union type declaration, got:\n{output}"
    );
    assert!(
        output.contains("{ Text: string }"),
        "Expected Text variant with string data, got:\n{output}"
    );
    assert!(
        output.contains("{ Number: number }"),
        "Expected Number variant with number data, got:\n{output}"
    );
    assert!(
        output.contains("'None'"),
        "Expected unit variant 'None', got:\n{output}"
    );
}

#[test]
fn test_event_payload_types() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: Path::new("events.rs").to_path_buf(),
        findings: vec![Finding {
            key: "download-progress".to_string(),
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            range: tower_lsp_server::ls_types::Range::default(),
            parameters: None,
            return_type: Some("u64".to_string()),
            fields: None,
            attributes: None,
        }],
    });

    let output = generate_invoke_types(&index);

    // Event with known payload type should use it instead of 'any'
    assert!(
        output.contains(
            "function emit(event: 'download-progress', payload?: number): Promise<void>;"
        ),
        "Expected typed emit payload, got:\n{output}"
    );
    assert!(
        output.contains("function listen<T = number>(event: 'download-progress'"),
        "Expected typed listen default, got:\n{output}"
    );
}

#[test]
fn test_event_unknown_payload_defaults_to_unknown() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: Path::new("events.rs").to_path_buf(),
        findings: vec![Finding {
            key: "app-ready".to_string(),
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            range: tower_lsp_server::ls_types::Range::default(),
            parameters: None,
            return_type: None,
            fields: None,
            attributes: None,
        }],
    });

    let output = generate_invoke_types(&index);

    // Event without payload type should default to 'unknown'
    assert!(
        output.contains("function emit(event: 'app-ready', payload?: unknown): Promise<void>;"),
        "Expected unknown payload for untyped event, got:\n{output}"
    );
}

#[test]
fn test_void_return_type() {
    let index = ProjectIndex::new();

    index.add_file(FileIndex {
        path: Path::new("lib.rs").to_path_buf(),
        findings: vec![Finding {
            key: "do_something".to_string(),
            entity: EntityType::Command,
            behavior: Behavior::Definition,
            range: tower_lsp_server::ls_types::Range::default(),
            parameters: None,
            return_type: None,
            fields: None,
            attributes: None,
        }],
    });

    let output = generate_invoke_types(&index);

    assert!(
        output.contains("function invoke<R = void>(cmd: 'do_something'): Promise<R>;"),
        "Expected void default for command without return type, got:\n{output}"
    );
}
