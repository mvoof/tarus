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

#[test]
fn test_map_rust_type_to_ts_extended() {
    // HashMap with typed value
    assert_eq!(
        map_rust_type_to_ts("HashMap<String, u32>"),
        "Record<string, number>"
    );
    assert_eq!(
        map_rust_type_to_ts("HashMap<String, Vec<String>>"),
        "Record<string, string[]>"
    );

    // HashSet
    assert_eq!(map_rust_type_to_ts("HashSet<String>"), "Set<string>");
    assert_eq!(map_rust_type_to_ts("HashSet<u32>"), "Set<number>");

    // Tuples
    assert_eq!(map_rust_type_to_ts("(String, i32)"), "[string, number]");
    assert_eq!(
        map_rust_type_to_ts("(bool, String, u64)"),
        "[boolean, string, number]"
    );
}

#[test]
fn test_compare_types_primitives() {
    assert_eq!(compare_types("String", "string"), TypeMatch::Exact);
    assert_eq!(compare_types("u32", "number"), TypeMatch::Exact);
    assert_eq!(compare_types("bool", "boolean"), TypeMatch::Exact);
    assert_eq!(compare_types("String", "any"), TypeMatch::Compatible);
}

#[test]
fn test_compare_types_containers() {
    assert_eq!(compare_types("Vec<String>", "string[]"), TypeMatch::Exact);
    assert_eq!(compare_types("Vec<u32>", "Array<number>"), TypeMatch::Exact);
    assert_eq!(
        compare_types("Option<String>", "string | null"),
        TypeMatch::Exact
    );
    // Option<T> also accepts T without null (lenient)
    assert_eq!(compare_types("Option<u32>", "number"), TypeMatch::Exact);
}

#[test]
fn test_compare_types_hashmap() {
    assert_eq!(
        compare_types("HashMap<String, u32>", "Record<string, number>"),
        TypeMatch::Exact
    );
}

#[test]
fn test_compare_types_hashset() {
    assert_eq!(
        compare_types("HashSet<String>", "Set<string>"),
        TypeMatch::Exact
    );
}

#[test]
fn test_compare_types_tuples() {
    assert_eq!(
        compare_types("(String, i32)", "[string, number]"),
        TypeMatch::Exact
    );
    // length mismatch
    assert!(matches!(
        compare_types("(String, i32)", "[string]"),
        TypeMatch::Mismatch(_)
    ));
}

#[test]
fn test_compare_types_mismatch() {
    assert!(matches!(
        compare_types("String", "number"),
        TypeMatch::Mismatch(_)
    ));
    assert!(matches!(
        compare_types("u32", "string"),
        TypeMatch::Mismatch(_)
    ));
}

#[test]
fn test_compare_types_custom_names() {
    // Same custom type name = exact (map_rust_type_to_ts passes through)
    assert_eq!(compare_types("User", "User"), TypeMatch::Exact);
    // Result<T, E> extracts Ok type
    assert_eq!(
        compare_types("Result<String, Error>", "string"),
        TypeMatch::Exact
    );
}
