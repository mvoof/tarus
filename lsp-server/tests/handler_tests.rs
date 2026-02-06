use lsp_server::syntax::{Behavior, EntityType};
use lsp_server::tree_parser::frontend_parser::process_interface_match;
use lsp_server::tree_parser::rust_parser::{
    process_command_matches, process_enum_match, process_struct_match,
};
use lsp_server::tree_parser::{CaptureIndices, LangType};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

// Helper to setup parser and query
fn setup_parser(lang: LangType, query_source: &str) -> (Parser, Query) {
    let mut parser = Parser::new();
    let language = match lang {
        LangType::Rust => tree_sitter_rust::LANGUAGE.into(),
        LangType::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        _ => panic!("Unsupported language for test setup"),
    };
    parser.set_language(&language).unwrap();
    let query = Query::new(&language, query_source).unwrap();
    (parser, query)
}

#[test]
fn test_process_struct_match() {
    let content = "struct MyStruct { field: i32 }";
    let query_source = "(struct_item name: (type_identifier) @struct_name) @struct_def";
    let (mut parser, query) = setup_parser(LangType::Rust, query_source);
    let tree = parser.parse(content, None).unwrap();

    let indices = CaptureIndices::from_query(&query, &["struct_name", "struct_def"]);
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

    let m = matches.next().expect("No match found");
    let finding = process_struct_match(&m, &indices, content).expect("Handler returned None");

    assert_eq!(finding.key, "MyStruct");
    assert_eq!(finding.entity, EntityType::Struct);
    assert_eq!(finding.behavior, Behavior::Definition);
}

#[test]
fn test_process_enum_match() {
    let content = "enum MyEnum { Variant }";
    let query_source = "(enum_item name: (type_identifier) @enum_name) @enum_def";
    let (mut parser, query) = setup_parser(LangType::Rust, query_source);
    let tree = parser.parse(content, None).unwrap();

    let indices = CaptureIndices::from_query(&query, &["enum_name", "enum_def"]);
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

    let m = matches.next().expect("No match found");
    let finding = process_enum_match(&m, &indices, content).expect("Handler returned None");

    assert_eq!(finding.key, "MyEnum");
    assert_eq!(finding.entity, EntityType::Enum);
}

#[test]
fn test_process_command_match() {
    let content = "#[tauri::command]\nfn my_command() -> () {}";
    // Actual query from rust.scm
    let query_source = r#"
        (
          (attribute_item
            (attribute
              (scoped_identifier
                path: (identifier) @_attr_path
                name: (identifier) @_attr_name)))
          .
          (function_item
            name: (identifier) @command_name
            parameters: (parameters) @command_params
            return_type: (_) @command_return_type
          )
          (#eq? @_attr_path "tauri")
          (#eq? @_attr_name "command")
        )
    "#;

    let (mut parser, query) = setup_parser(LangType::Rust, query_source);
    let tree = parser.parse(content, None).unwrap();

    let indices = CaptureIndices::from_query(
        &query,
        &["command_name", "command_params", "command_return_type"],
    );
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

    let m = matches.next().expect("No match found");
    let findings = process_command_matches(&m, &indices, content);

    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].key, "my_command");
    assert_eq!(findings[0].entity, EntityType::Command);
}

#[test]
fn test_process_interface_match() {
    let content = "interface User { name: string; }";
    let query_source =
        "(interface_declaration name: (type_identifier) @interface_name) @interface_def";
    let (mut parser, query) = setup_parser(LangType::TypeScript, query_source);
    let tree = parser.parse(content, None).unwrap();

    let indices = CaptureIndices::from_query(&query, &["interface_name", "interface_def"]);
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), content.as_bytes());

    let m = matches.next().expect("No match found");
    let finding = process_interface_match(&m, &indices, content, 0).expect("Handler returned None");

    assert_eq!(finding.key, "User");
    assert_eq!(finding.entity, EntityType::Interface);

    let fields = finding.fields.expect("No fields found");
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].name, "name");
}

// Function call test usually requires more setup (aliases, patterns), so we'll do a basic one
