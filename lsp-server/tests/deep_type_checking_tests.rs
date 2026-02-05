//! Deep type checking tests for custom structs/interfaces

mod common_paths;

use common_paths::test_path;
use lsp_server::capabilities::diagnostics::compute_file_diagnostics;
use lsp_server::indexer::{FileIndex, Finding, Parameter, ProjectIndex};
use lsp_server::syntax::{Behavior, EntityType};
use tower_lsp_server::lsp_types::{DiagnosticSeverity, Position, Range};

#[test]
fn test_deep_type_mismatch_return_type() {
    let index = ProjectIndex::new();

    // 1. Define a Rust struct
    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            Finding {
                key: "Greet".to_string(),
                entity: EntityType::Struct,
                behavior: Behavior::Definition,
                range: Range::default(),
                parameters: None,
                return_type: None,
                fields: Some(vec![Parameter {
                    name: "message".to_string(),
                    type_name: "String".to_string(),
                }]),
                attributes: None,
            },
            // Define a command that returns this struct
            Finding {
                key: "greet".to_string(),
                entity: EntityType::Command,
                behavior: Behavior::Definition,
                range: Range::default(),
                parameters: None,
                return_type: Some("Greet".to_string()),
                fields: None,
                attributes: None,
            },
        ],
    });

    // 2. Define a TypeScript interface with a MISMATCHED type (number instead of string)
    let frontend_path = test_path("frontend.ts");
    index.add_file(FileIndex {
        path: frontend_path.clone(),
        findings: vec![
            Finding {
                key: "GreetType1".to_string(),
                entity: EntityType::Interface,
                behavior: Behavior::Definition,
                range: Range::default(),
                parameters: None,
                return_type: None,
                fields: Some(vec![Parameter {
                    name: "message".to_string(),
                    type_name: "number".to_string(),
                }]),
                attributes: None,
            },
            // Call the command with this mismatched interface
            Finding {
                key: "greet".to_string(),
                entity: EntityType::Command,
                behavior: Behavior::Call,
                range: Range {
                    start: Position {
                        line: 10,
                        character: 0,
                    },
                    end: Position {
                        line: 10,
                        character: 20,
                    },
                },
                parameters: None,
                return_type: Some("<GreetType1>".to_string()),
                fields: None,
                attributes: None,
            },
        ],
    });

    // 3. Compute diagnostics
    let diagnostics = compute_file_diagnostics(&frontend_path, &index);

    // 4. Verify diagnostic is present
    assert!(
        !diagnostics.is_empty(),
        "Should have at least one diagnostic"
    );

    let mismatch = diagnostics
        .iter()
        .find(|d| d.message.contains("Type mismatch for field 'message'"));
    assert!(
        mismatch.is_some(),
        "Expected type mismatch diagnostic for field 'message'"
    );
    assert_eq!(
        mismatch.unwrap().severity,
        Some(DiagnosticSeverity::WARNING)
    );
    assert!(mismatch
        .unwrap()
        .message
        .contains("expected string, got number"));
}

#[test]
fn test_deep_type_match_return_type() {
    let index = ProjectIndex::new();

    // 1. Define a Rust struct
    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            Finding {
                key: "Greet".to_string(),
                entity: EntityType::Struct,
                behavior: Behavior::Definition,
                range: Range::default(),
                parameters: None,
                return_type: None,
                fields: Some(vec![Parameter {
                    name: "message".to_string(),
                    type_name: "String".to_string(),
                }]),
                attributes: None,
            },
            Finding {
                key: "greet".to_string(),
                entity: EntityType::Command,
                behavior: Behavior::Definition,
                range: Range::default(),
                parameters: None,
                return_type: Some("Greet".to_string()),
                fields: None,
                attributes: None,
            },
        ],
    });

    // 2. Define a TypeScript interface with MATCHING type
    let frontend_path = test_path("frontend.ts");
    index.add_file(FileIndex {
        path: frontend_path.clone(),
        findings: vec![
            Finding {
                key: "GreetType1".to_string(),
                entity: EntityType::Interface,
                behavior: Behavior::Definition,
                range: Range::default(),
                parameters: None,
                return_type: None,
                fields: Some(vec![Parameter {
                    name: "message".to_string(),
                    type_name: "string".to_string(),
                }]),
                attributes: None,
            },
            Finding {
                key: "greet".to_string(),
                entity: EntityType::Command,
                behavior: Behavior::Call,
                range: Range {
                    start: Position {
                        line: 10,
                        character: 0,
                    },
                    end: Position {
                        line: 10,
                        character: 20,
                    },
                },
                parameters: None,
                return_type: Some("<GreetType1>".to_string()),
                fields: None,
                attributes: None,
            },
        ],
    });

    // 3. Compute diagnostics
    let diagnostics = compute_file_diagnostics(&frontend_path, &index);

    // 4. Verify NO mismatch diagnostic is present
    let mismatch = diagnostics
        .iter()
        .find(|d| d.message.contains("Type mismatch"));
    assert!(
        mismatch.is_none(),
        "Should NOT have type mismatch diagnostic"
    );
}
