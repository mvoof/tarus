#[cfg(test)]
mod tests {
    use lsp_server::indexer::{IndexKey, LocationInfo, Parameter, ProjectIndex};
    use lsp_server::syntax::{Behavior, EntityType};
    use lsp_server::typegen::{self, TypegenConfig};
    use std::path::PathBuf;

    fn create_mock_project_index() -> ProjectIndex {
        let index = ProjectIndex::default();

        // Add a simple command: fn simple_cmd(name: String) -> String
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "simple_cmd".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/main.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: Some(vec![Parameter {
                    name: "name".to_string(),
                    type_name: "String".to_string(),
                }]),
                return_type: Some("String".to_string()),
                ..Default::default()
            }],
        );

        // Add a command with Result: fn result_cmd() -> Result<MyStruct, String>
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "result_cmd".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/main.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: Some(vec![]),
                return_type: Some("Result<MyStruct, String>".to_string()),
                ..Default::default()
            }],
        );

        // Add the struct definition: struct MyStruct { id: i32 }
        index.map.insert(
            IndexKey {
                entity: EntityType::Struct,
                name: "MyStruct".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/main.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: None,
                fields: Some(vec![Parameter {
                    name: "id".to_string(),
                    type_name: "i32".to_string(),
                }]),
                return_type: None,
                ..Default::default()
            }],
        );

        index
    }

    #[test]
    fn test_generate_simple_types() {
        let index = create_mock_project_index();
        let output = typegen::generate_invoke_types_with_config(&index, &TypegenConfig::default());

        assert!(output
            .contains("invoke<R = string>(cmd: 'simple_cmd', args: SimpleCmdArgs): Promise<R>"));
        assert!(output.contains("export interface SimpleCmdArgs {"));
        assert!(output.contains("name: string;"));
        // Current implementation might map Result<T, E> to Promise<T>
        // Check for MyStruct interface usage
        assert!(output.contains("interface MyStruct {"));
        assert!(output.contains("id: number;"));
    }

    #[test]
    fn test_strict_mode() {
        let index = create_mock_project_index();
        let config = TypegenConfig {
            strict_type_safety: true,
            dts_output_path: None,
        };
        let output = typegen::generate_invoke_types_with_config(&index, &config);

        // Strict mode should NOT contain the catch-all fallback
        assert!(!output.contains("invoke<R = unknown>(cmd: string"));

        // In strict mode, maybe we don't generate the fallback overload?
        // Or we use 'unknown' instead of 'any'?
        // This test documents current behavior or fails if feature not implemented
    }

    #[test]
    fn test_tagged_union() {
        let index = create_mock_project_index();

        // Add tagged union: enum TaggedEnum { #[serde(tag = "type")] VariantA, VariantB { x: i32 } }
        index.map.insert(
            IndexKey {
                entity: EntityType::Enum,
                name: "TaggedEnum".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/main.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: None,
                fields: Some(vec![
                    Parameter {
                        name: "VariantA".to_string(),
                        type_name: "enum_variant".to_string(),
                    },
                    Parameter {
                        name: "VariantB".to_string(),
                        type_name: "StructVariant".to_string(),
                    },
                ]),
                attributes: Some(vec!["#[serde(tag = \"kind\")]".to_string()]),
                return_type: None,
            }],
        );

        // Add StructVariant definition
        index.map.insert(
            IndexKey {
                entity: EntityType::Struct,
                name: "StructVariant".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/main.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: None,
                fields: Some(vec![Parameter {
                    name: "x".to_string(),
                    type_name: "i32".to_string(),
                }]),
                return_type: None,
                ..Default::default()
            }],
        );

        // Add command using TaggedEnum
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "use_tagged".to_string(),
            },
            vec![LocationInfo {
                path: PathBuf::from("src-tauri/src/main.rs"),
                range: Default::default(),
                behavior: Behavior::Definition,
                parameters: Some(vec![]),
                return_type: Some("TaggedEnum".to_string()),
                ..Default::default()
            }],
        );

        let output = typegen::generate_invoke_types_with_config(&index, &TypegenConfig::default());

        // Check for TaggedEnum with internal tag 'kind'
        // VariantA: { kind: 'VariantA' }
        assert!(output.contains("{ kind: 'VariantA' }"));
        // VariantB: ({ kind: 'VariantB' } & StructVariant)
        // or similar format depending on implementation
        assert!(output.contains("{ kind: 'VariantB' }"));
    }
}
