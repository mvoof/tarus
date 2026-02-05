//! Syntax utility tests

use lsp_server::syntax::*;

#[test]
fn test_snake_to_camel() {
    assert_eq!(snake_to_camel("get_user"), "getUser");
    assert_eq!(
        snake_to_camel("my_long_variable_name"),
        "myLongVariableName"
    );
}

#[test]
fn test_camel_to_snake() {
    assert_eq!(camel_to_snake("getUser"), "get_user");
    assert_eq!(
        camel_to_snake("myLongVariableName"),
        "my_long_variable_name"
    );
}

#[test]
fn test_map_rust_type_to_ts() {
    assert_eq!(map_rust_type_to_ts("String"), "string");
    assert_eq!(map_rust_type_to_ts("u32"), "number");
    assert_eq!(map_rust_type_to_ts("bool"), "boolean");
    assert_eq!(map_rust_type_to_ts("Vec<String>"), "string[]");
    assert_eq!(map_rust_type_to_ts("Option<u32>"), "number | null");
    assert_eq!(map_rust_type_to_ts("Result<String, Error>"), "string");
}

#[test]
fn test_map_ts_type_to_rust() {
    assert_eq!(map_ts_type_to_rust("string"), "String");
    assert_eq!(map_ts_type_to_rust("number"), "i64");
    assert_eq!(map_ts_type_to_rust("boolean"), "bool");
    assert_eq!(map_ts_type_to_rust("string[]"), "Vec<String>");
    assert_eq!(map_ts_type_to_rust("any"), "serde_json::Value");
}

#[test]
fn test_should_rename_to_camel() {
    let attrs = Some(vec!["#[serde(rename_all = \"camelCase\")]".to_string()]);
    assert!(should_rename_to_camel(attrs.as_ref()));

    let attrs2 = Some(vec!["#[derive(Debug)]".to_string()]);
    assert!(!should_rename_to_camel(attrs2.as_ref()));
}
