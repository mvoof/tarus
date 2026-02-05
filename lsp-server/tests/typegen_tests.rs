//! Type generation tests

use lsp_server::indexer::{FileIndex, Finding, Parameter, ProjectIndex};
use lsp_server::syntax::{extract_result_ok_type, Behavior, EntityType};
use lsp_server::typegen::generate_invoke_types;
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
                range: tower_lsp_server::lsp_types::Range::default(),
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
                range: tower_lsp_server::lsp_types::Range::default(),
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
    assert!(output.contains("function invoke(cmd: 'get_user'): Promise<User>;"));
}
