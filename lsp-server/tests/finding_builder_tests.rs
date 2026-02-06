use lsp_server::indexer::Parameter;
use lsp_server::syntax::{Behavior, EntityType};
use lsp_server::tree_parser::FindingBuilder;
use tower_lsp_server::lsp_types::{Position, Range};

fn create_test_range() -> Range {
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 10,
        },
    }
}

#[test]
fn test_finding_builder_basic() {
    let finding = FindingBuilder::new(
        "TestCommand".to_string(),
        EntityType::Command,
        Behavior::Definition,
        create_test_range(),
    )
    .build();

    assert_eq!(finding.key, "TestCommand");
    assert_eq!(finding.entity, EntityType::Command);
    assert_eq!(finding.behavior, Behavior::Definition);
    assert!(finding.parameters.is_none());
    assert!(finding.return_type.is_none());
    assert!(finding.fields.is_none());
    assert!(finding.attributes.is_none());
}

#[test]
fn test_finding_builder_with_parameters() {
    let params = vec![
        Parameter {
            name: "arg1".to_string(),
            type_name: "String".to_string(),
        },
        Parameter {
            name: "arg2".to_string(),
            type_name: "i32".to_string(),
        },
    ];

    let finding = FindingBuilder::new(
        "TestCommand".to_string(),
        EntityType::Command,
        Behavior::Definition,
        create_test_range(),
    )
    .with_parameters_opt(Some(params.clone()))
    .build();

    assert!(finding.parameters.is_some());
    assert_eq!(finding.parameters.unwrap().len(), 2);
}

#[test]
fn test_finding_builder_empty_vec_becomes_none() {
    let finding = FindingBuilder::new(
        "TestStruct".to_string(),
        EntityType::Struct,
        Behavior::Definition,
        create_test_range(),
    )
    .with_fields(Vec::new())
    .with_attributes(Vec::new())
    .build();

    assert!(finding.fields.is_none());
    assert!(finding.attributes.is_none());
}

#[test]
fn test_finding_builder_with_fields() {
    let fields = vec![Parameter {
        name: "field1".to_string(),
        type_name: "String".to_string(),
    }];

    let finding = FindingBuilder::new(
        "TestStruct".to_string(),
        EntityType::Struct,
        Behavior::Definition,
        create_test_range(),
    )
    .with_fields(fields)
    .build();

    assert!(finding.fields.is_some());
    assert_eq!(finding.fields.unwrap().len(), 1);
}

#[test]
fn test_finding_builder_with_return_type() {
    let finding = FindingBuilder::new(
        "TestCommand".to_string(),
        EntityType::Command,
        Behavior::Definition,
        create_test_range(),
    )
    .with_return_type_opt(Some("Result<(), Error>".to_string()))
    .build();

    assert!(finding.return_type.is_some());
    assert_eq!(finding.return_type.unwrap(), "Result<(), Error>");
}

#[test]
fn test_finding_builder_chaining() {
    let params = vec![Parameter {
        name: "arg".to_string(),
        type_name: "String".to_string(),
    }];

    let attrs = vec!["#[tauri::command]".to_string()];

    let finding = FindingBuilder::new(
        "TestCommand".to_string(),
        EntityType::Command,
        Behavior::Definition,
        create_test_range(),
    )
    .with_parameters_opt(Some(params))
    .with_return_type_opt(Some("String".to_string()))
    .with_attributes(attrs)
    .build();

    assert!(finding.parameters.is_some());
    assert!(finding.return_type.is_some());
    assert!(finding.attributes.is_some());
}
