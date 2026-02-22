#[cfg(test)]
mod tests {
    use lsp_server::capabilities::diagnostics::compute_file_diagnostics;
    use lsp_server::indexer::{IndexKey, LocationInfo, Parameter, ProjectIndex};
    use lsp_server::syntax::{Behavior, EntityType};
    use std::path::PathBuf;

    fn create_mock_project_index() -> ProjectIndex {
        ProjectIndex::default()
    }

    /// Without external bindings (specta/typegen/ts-rs), parameter type mismatches
    /// should NOT produce diagnostics — we can't reliably judge custom types.
    #[test]
    fn test_no_parameter_diagnostics_without_bindings() {
        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // Define Struct "User" { name: String, age: i32 }
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

        // Define Command "create_user" (user: User)
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

        // Register file usage in file_map
        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "create_user".to_string(),
            }],
        );

        // Invalid Usage (age is string instead of number) — but no bindings available
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
            .push(invalid_usage_loc);

        // Run diagnostics
        let diags = compute_file_diagnostics(&file_path, &index);

        // Without external bindings, parameter type mismatches should NOT be reported
        let mismatch_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("Type mismatch"))
            .collect();

        assert_eq!(
            mismatch_diags.len(),
            0,
            "Expected no type mismatch diagnostics without bindings, got {:?}",
            mismatch_diags
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    /// Without external bindings, custom return types with matching names should NOT
    /// produce diagnostics. Only primitive type mismatches should be reported.
    #[test]
    fn test_no_return_type_diagnostics_for_custom_types_without_bindings() {
        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // Define Command "get_user" -> Result<UserProfile, String>
        let def_loc = LocationInfo {
            path: PathBuf::from("src-tauri/src/main.rs"),
            range: Default::default(),
            behavior: Behavior::Definition,
            parameters: None,
            return_type: Some("Result<UserProfile, String>".to_string()),
            attributes: None,
            fields: None,
        };

        // TS call: invoke<UserProfile>("get_user") — same name, should be OK
        let call_same_name = LocationInfo {
            path: file_path.clone(),
            range: tower_lsp_server::ls_types::Range {
                start: tower_lsp_server::ls_types::Position {
                    line: 5,
                    character: 0,
                },
                end: tower_lsp_server::ls_types::Position {
                    line: 5,
                    character: 10,
                },
            },
            behavior: Behavior::Call,
            parameters: None,
            return_type: Some("UserProfile".to_string()),
            attributes: None,
            fields: None,
        };

        // TS call: invoke<OtherType>("get_user") — different custom name, should NOT diagnose
        let call_diff_name = LocationInfo {
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
            parameters: None,
            return_type: Some("OtherType".to_string()),
            attributes: None,
            fields: None,
        };

        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "get_user".to_string(),
            },
            vec![def_loc, call_same_name, call_diff_name],
        );

        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "get_user".to_string(),
            }],
        );

        let diags = compute_file_diagnostics(&file_path, &index);

        let mismatch_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("mismatch"))
            .collect();

        assert_eq!(
            mismatch_diags.len(),
            0,
            "Expected no return type mismatch for custom types without bindings, got {:?}",
            mismatch_diags
                .iter()
                .map(|d| &d.message)
                .collect::<Vec<_>>()
        );
    }

    /// Primitive type mismatches should still be reported even without bindings.
    #[test]
    fn test_primitive_return_type_mismatch_without_bindings() {
        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // Define Command "get_count" -> i32
        let def_loc = LocationInfo {
            path: PathBuf::from("src-tauri/src/main.rs"),
            range: Default::default(),
            behavior: Behavior::Definition,
            parameters: None,
            return_type: Some("i32".to_string()),
            attributes: None,
            fields: None,
        };

        // TS call: invoke<string>("get_count") — primitive mismatch
        let call_loc = LocationInfo {
            path: file_path.clone(),
            range: tower_lsp_server::ls_types::Range {
                start: tower_lsp_server::ls_types::Position {
                    line: 5,
                    character: 0,
                },
                end: tower_lsp_server::ls_types::Position {
                    line: 5,
                    character: 10,
                },
            },
            behavior: Behavior::Call,
            parameters: None,
            return_type: Some("string".to_string()),
            attributes: None,
            fields: None,
        };

        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "get_count".to_string(),
            },
            vec![def_loc, call_loc],
        );

        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "get_count".to_string(),
            }],
        );

        let diags = compute_file_diagnostics(&file_path, &index);

        let mismatch_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("mismatch"))
            .collect();

        assert_eq!(
            mismatch_diags.len(),
            1,
            "Expected 1 primitive return type mismatch, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// When rust_types registry has full struct info, field-level type mismatches
    /// in invoke arguments should produce diagnostics.
    #[test]
    fn test_struct_field_type_mismatch_with_native_types() {
        use lsp_server::syntax::{RustTypeInfo, RustTypeKind, SerdeAttributes};

        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // Register SimpleUser1 in rust_types with fields: name: String, age: u8
        index.rust_types.insert(
            "SimpleUser1".to_string(),
            RustTypeInfo {
                kind: RustTypeKind::Struct,
                fields: vec![
                    Parameter {
                        name: "name".to_string(),
                        type_name: "String".to_string(),
                    },
                    Parameter {
                        name: "age".to_string(),
                        type_name: "u8".to_string(),
                    },
                ],
                variants: vec![],
                serde: SerdeAttributes::default(),
                generic_params: vec![],
            },
        );

        // Command definition: update_user(user: SimpleUser1) -> SimpleUser1
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "update_user".to_string(),
            },
            vec![
                LocationInfo {
                    path: PathBuf::from("src-tauri/src/lib.rs"),
                    range: Default::default(),
                    behavior: Behavior::Definition,
                    parameters: Some(vec![Parameter {
                        name: "user".to_string(),
                        type_name: "SimpleUser1".to_string(),
                    }]),
                    return_type: Some("SimpleUser1".to_string()),
                    fields: None,
                    attributes: None,
                },
                // Call site: invoke("update_user", { user: { name: "Alice", age: "twenty" } })
                // age is string but should be number
                LocationInfo {
                    path: file_path.clone(),
                    range: tower_lsp_server::ls_types::Range {
                        start: tower_lsp_server::ls_types::Position {
                            line: 10,
                            character: 0,
                        },
                        end: tower_lsp_server::ls_types::Position {
                            line: 10,
                            character: 13,
                        },
                    },
                    behavior: Behavior::Call,
                    parameters: Some(vec![Parameter {
                        name: "user".to_string(),
                        type_name: "{ name: string, age: string }".to_string(),
                    }]),
                    return_type: None,
                    fields: None,
                    attributes: None,
                },
            ],
        );

        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "update_user".to_string(),
            }],
        );

        let diags = compute_file_diagnostics(&file_path, &index);

        let mismatch_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("Type mismatch"))
            .collect();

        assert_eq!(
            mismatch_diags.len(),
            1,
            "Expected 1 field type mismatch (age: string vs u8), got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        assert!(
            mismatch_diags[0].message.contains("age"),
            "Mismatch should mention 'age' field, got: {}",
            mismatch_diags[0].message
        );
    }

    // ─── Bindings-reader integration tests ─────────────────────────────────

    /// When a specta/typegen schema is present, parameter type mismatches should
    /// be reported using the schema types (not the native Rust types).
    #[test]
    fn test_schema_param_type_mismatch() {
        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // Populate command_schemas as if specta generated it
        index.command_schemas.insert(
            "remove_localization".to_string(),
            vec![
                Parameter {
                    name: "baseFolderPath".to_string(),
                    type_name: "string".to_string(),
                },
                Parameter {
                    name: "selectedLanguageCode".to_string(),
                    type_name: "string".to_string(),
                },
            ],
        );

        // Command definition (native Rust — present but should be superseded by schema)
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "remove_localization".to_string(),
            },
            vec![
                LocationInfo {
                    path: PathBuf::from("src-tauri/src/lib.rs"),
                    range: Default::default(),
                    behavior: Behavior::Definition,
                    parameters: Some(vec![
                        Parameter {
                            name: "base_folder_path".to_string(),
                            type_name: "String".to_string(),
                        },
                        Parameter {
                            name: "selected_language_code".to_string(),
                            type_name: "String".to_string(),
                        },
                    ]),
                    return_type: None,
                    fields: None,
                    attributes: None,
                },
                // Call site: baseFolderPath is number (wrong — schema expects string)
                LocationInfo {
                    path: file_path.clone(),
                    range: tower_lsp_server::ls_types::Range {
                        start: tower_lsp_server::ls_types::Position {
                            line: 5,
                            character: 0,
                        },
                        end: tower_lsp_server::ls_types::Position {
                            line: 5,
                            character: 20,
                        },
                    },
                    behavior: Behavior::Call,
                    parameters: Some(vec![
                        Parameter {
                            name: "baseFolderPath".to_string(),
                            type_name: "number".to_string(), // wrong! schema says string
                        },
                        Parameter {
                            name: "selectedLanguageCode".to_string(),
                            type_name: "string".to_string(), // correct
                        },
                    ]),
                    return_type: None,
                    fields: None,
                    attributes: None,
                },
            ],
        );

        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "remove_localization".to_string(),
            }],
        );

        let diags = compute_file_diagnostics(&file_path, &index);

        let param_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("bindings expect"))
            .collect();

        assert_eq!(
            param_diags.len(),
            1,
            "Expected 1 param mismatch from schema, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            param_diags[0].message.contains("baseFolderPath"),
            "Diagnostic should mention 'baseFolderPath', got: {}",
            param_diags[0].message
        );
    }

    /// Missing parameters that appear in the schema but not in the call site
    /// should trigger a warning.
    #[test]
    fn test_schema_missing_required_param() {
        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        index.command_schemas.insert(
            "save_data".to_string(),
            vec![
                Parameter {
                    name: "key".to_string(),
                    type_name: "string".to_string(),
                },
                Parameter {
                    name: "value".to_string(),
                    type_name: "string".to_string(),
                },
            ],
        );

        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "save_data".to_string(),
            },
            vec![
                LocationInfo {
                    path: PathBuf::from("src-tauri/src/lib.rs"),
                    range: Default::default(),
                    behavior: Behavior::Definition,
                    parameters: None,
                    return_type: None,
                    fields: None,
                    attributes: None,
                },
                // Call site: only 'key' is provided, 'value' is missing
                LocationInfo {
                    path: file_path.clone(),
                    range: tower_lsp_server::ls_types::Range {
                        start: tower_lsp_server::ls_types::Position {
                            line: 3,
                            character: 0,
                        },
                        end: tower_lsp_server::ls_types::Position {
                            line: 3,
                            character: 10,
                        },
                    },
                    behavior: Behavior::Call,
                    parameters: Some(vec![Parameter {
                        name: "key".to_string(),
                        type_name: "string".to_string(),
                    }]),
                    return_type: None,
                    fields: None,
                    attributes: None,
                },
            ],
        );

        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "save_data".to_string(),
            }],
        );

        let diags = compute_file_diagnostics(&file_path, &index);

        let missing_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("Missing required parameter"))
            .collect();

        assert_eq!(
            missing_diags.len(),
            1,
            "Expected 1 missing-param diagnostic, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
        assert!(
            missing_diags[0].message.contains("value"),
            "Missing param diagnostic should mention 'value', got: {}",
            missing_diags[0].message
        );
    }

    /// `ts_type_aliases` (Case 5 in `is_safe_to_compare`) should allow comparison
    /// of known TypeScript types even when they aren't in `rust_types`.
    #[test]
    fn test_ts_type_aliases_enable_return_type_comparison() {
        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // Simulate ts-rs having generated UserProfile.ts
        index
            .ts_type_aliases
            .insert("UserProfile".to_string(), "{ id: number, username: string }".to_string());

        // Command definition: getUser() -> UserProfile (but NOT in rust_types)
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "get_user".to_string(),
            },
            vec![
                LocationInfo {
                    path: PathBuf::from("src-tauri/src/lib.rs"),
                    range: Default::default(),
                    behavior: Behavior::Definition,
                    parameters: None,
                    return_type: Some("UserProfile".to_string()),
                    fields: None,
                    attributes: None,
                },
                // invoke<WrongType>("get_user") — ts type not matching
                LocationInfo {
                    path: file_path.clone(),
                    range: tower_lsp_server::ls_types::Range {
                        start: tower_lsp_server::ls_types::Position {
                            line: 1,
                            character: 0,
                        },
                        end: tower_lsp_server::ls_types::Position {
                            line: 1,
                            character: 5,
                        },
                    },
                    behavior: Behavior::Call,
                    parameters: None,
                    return_type: Some("UserProfile".to_string()), // same name → should be OK
                    fields: None,
                    attributes: None,
                },
            ],
        );

        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "get_user".to_string(),
            }],
        );

        let diags = compute_file_diagnostics(&file_path, &index);

        // invoke<UserProfile> on a UserProfile-returning command → no mismatch
        let mismatch_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.message.contains("mismatch"))
            .collect();

        assert_eq!(
            mismatch_diags.len(),
            0,
            "Expected no mismatch for matching type names, got {:?}",
            mismatch_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }

    /// With ts_type_aliases, a known TS type name that differs from the Rust type
    /// should produce a HINT diagnostic (name-only mismatch).
    #[test]
    fn test_ts_type_aliases_name_mismatch_emits_hint() {
        use lsp_server::syntax::{RustTypeInfo, RustTypeKind, SerdeAttributes};

        let index = create_mock_project_index();
        let file_path = PathBuf::from("src/test.ts");

        // Rust has UserProfile in rust_types
        index.rust_types.insert(
            "UserProfile".to_string(),
            RustTypeInfo {
                kind: RustTypeKind::Struct,
                fields: vec![Parameter {
                    name: "id".to_string(),
                    type_name: "u32".to_string(),
                }],
                variants: vec![],
                serde: SerdeAttributes::default(),
                generic_params: vec![],
            },
        );

        // ts-rs has generated MyUser.ts → ts_type_aliases knows "MyUser"
        index.ts_type_aliases.insert(
            "MyUser".to_string(),
            "{ id: number }".to_string(),
        );

        // Command definition: getUser() -> UserProfile
        index.map.insert(
            IndexKey {
                entity: EntityType::Command,
                name: "get_user".to_string(),
            },
            vec![
                LocationInfo {
                    path: PathBuf::from("src-tauri/src/lib.rs"),
                    range: Default::default(),
                    behavior: Behavior::Definition,
                    parameters: None,
                    return_type: Some("UserProfile".to_string()),
                    fields: None,
                    attributes: None,
                },
                // invoke<MyUser>("get_user") — name mismatch (MyUser vs UserProfile)
                LocationInfo {
                    path: file_path.clone(),
                    range: tower_lsp_server::ls_types::Range {
                        start: tower_lsp_server::ls_types::Position {
                            line: 2,
                            character: 0,
                        },
                        end: tower_lsp_server::ls_types::Position {
                            line: 2,
                            character: 10,
                        },
                    },
                    behavior: Behavior::Call,
                    parameters: None,
                    return_type: Some("MyUser".to_string()),
                    fields: None,
                    attributes: None,
                },
            ],
        );

        index.file_map.insert(
            file_path.clone(),
            vec![IndexKey {
                entity: EntityType::Command,
                name: "get_user".to_string(),
            }],
        );

        let diags = compute_file_diagnostics(&file_path, &index);

        // Should emit a HINT suggesting rename (because rust type is known AND ts type is known
        // from ts_type_aliases — now both sides are "named types")
        use tower_lsp_server::ls_types::DiagnosticSeverity;
        let hint_diags: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Some(DiagnosticSeverity::HINT))
            .collect();

        assert_eq!(
            hint_diags.len(),
            1,
            "Expected 1 HINT for name-only return type mismatch, got {:?}",
            diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );
    }
}
