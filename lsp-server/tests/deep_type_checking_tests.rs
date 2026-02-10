#[cfg(test)]
mod tests {
    use lsp_server::capabilities::diagnostics::compute_file_diagnostics;
    use lsp_server::indexer::{IndexKey, LocationInfo, Parameter, ProjectIndex};
    use lsp_server::syntax::{Behavior, EntityType}; // Correctly imported from syntax
    use std::path::PathBuf;

    fn create_mock_project_index() -> ProjectIndex {
        ProjectIndex::default()
    }

    #[test]
    fn test_recursive_struct_validation() {
        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // 1. Define Struct "User" { name: String, age: i32 }
        index.map.insert(
            IndexKey {
                entity: EntityType::Struct,
                name: "User".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/models.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: None,
                fields: Some(vec![
                    Parameter {
                        name: "name".to_string(),
                        type_name: "String".to_string(),
                    },
                    Parameter {
                        name: "age".to_string(),
                        type_name: "i32".to_string(),
                    },
                ]),
                return_type: None,
                attributes: None,
            }],
        );

        // 2. Define Command "create_user" (user: User)
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "create_user".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/main.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: Some(vec![Parameter {
                    name: "user".to_string(),
                    type_name: "User".to_string(),
                }]),
                return_type: None,
                attributes: None,
                fields: None,
            }],
        );

        // 3. Register file usage in file_map
        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "create_user".to_string(),
            }],
        );

        // 4. Add usage location in map
        // Case A: Valid Usage
        let valid_usage_loc = LocationInfo {
            path: file_path.clone(),
            range: tower_lsp_server::ls_types::Range::default(), // Line 0
            behavior: Behavior::Call,
            parameters: Some(vec![Parameter {
                name: "user".to_string(),
                // Simulating: create_user({ name: "Alice", age: 30 })
                type_name: "{ name: string, age: number }".to_string(),
            }]),
            fields: None,
            return_type: None,
            attributes: None,
        };

        // Case B: Invalid Usage (age is string)
        let invalid_usage_loc = LocationInfo {
            path: file_path.clone(),
            range: tower_lsp_server::ls_types::Range {
                start: tower_lsp_server::ls_types::Position {
                    line: 10,
                    character: 0,
                },
                end: tower_lsp_server::ls_types::Position {
                    line: 10,
                    character: 10,
                },
            },
            behavior: Behavior::Call,
            parameters: Some(vec![Parameter {
                name: "user".to_string(),
                // Simulating: create_user({ name: "Bob", age: "thirty" })
                type_name: "{ name: string, age: string }".to_string(),
            }]),
            fields: None,
            return_type: None,
            attributes: None,
        };

        index
            .map
            .get_mut(&IndexKey {
                entity: EntityType::Command,
                name: "create_user".to_string(),
            })
            .unwrap()
            .push(valid_usage_loc);

        index
            .map
            .get_mut(&IndexKey {
                entity: EntityType::Command,
                name: "create_user".to_string(),
            })
            .unwrap()
            .push(invalid_usage_loc);

        // Run diagnostics
        let diags = compute_file_diagnostics(&file_path, &index);

        // Assertions

        // Filter diagnostics to "Type mismatch"
        let mismatch_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("Type mismatch"))
            .collect();

        assert_eq!(
            mismatch_diags.len(),
            1,
            "Expected 1 type mismatch diagnostic, got {:?}",
            diags
        );

        let diag = mismatch_diags[0];
        assert!(
            diag.message.contains("field 'age'"),
            "Message: {}",
            diag.message
        );
        assert!(
            diag.message.contains("expected number"),
            "Message: {}",
            diag.message
        );
    }
}
