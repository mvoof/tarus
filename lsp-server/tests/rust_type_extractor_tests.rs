//! Tests for the rust_type_extractor module

mod common_fixtures;
mod common_paths;

use common_fixtures::load_fixture;
use common_paths::test_path;
use lsp_server::indexer::types::{CommandSchema, EventSchema};
use lsp_server::rust_type_extractor::{
    extract_command_schemas_from_tree, extract_event_schemas_from_tree, rust_type_to_ts,
};
use std::path::Path;
use tree_sitter::Parser;

fn parse_rust(content: &str) -> tree_sitter::Tree {
    let lang: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let mut parser = Parser::new();
    parser.set_language(&lang).unwrap();
    parser.parse(content, None).unwrap()
}

fn extract_command_schemas(content: &str, source_path: &Path) -> Vec<CommandSchema> {
    let tree = parse_rust(content);
    extract_command_schemas_from_tree(tree.root_node(), content, source_path)
}

fn extract_event_schemas(content: &str, source_path: &Path) -> Vec<EventSchema> {
    let tree = parse_rust(content);
    extract_event_schemas_from_tree(tree.root_node(), content, &tree, source_path)
}

// ============================================================
// rust_type_to_ts primitive mappings
// ============================================================

#[test]
fn test_rust_type_to_ts_primitives() {
    assert_eq!(rust_type_to_ts("u8"), "number");
    assert_eq!(rust_type_to_ts("u16"), "number");
    assert_eq!(rust_type_to_ts("u32"), "number");
    assert_eq!(rust_type_to_ts("u64"), "number");
    assert_eq!(rust_type_to_ts("i32"), "number");
    assert_eq!(rust_type_to_ts("f64"), "number");
    assert_eq!(rust_type_to_ts("usize"), "number");
    assert_eq!(rust_type_to_ts("String"), "string");
    assert_eq!(rust_type_to_ts("&str"), "string");
    assert_eq!(rust_type_to_ts("bool"), "boolean");
    assert_eq!(rust_type_to_ts("()"), "void");
}

#[test]
fn test_rust_type_to_ts_result() {
    assert_eq!(rust_type_to_ts("Result<String, String>"), "string");
    assert_eq!(rust_type_to_ts("Result<u32, String>"), "number");
    assert_eq!(
        rust_type_to_ts("Result<UserProfile, String>"),
        "UserProfile"
    );
}

#[test]
fn test_rust_type_to_ts_option() {
    assert_eq!(rust_type_to_ts("Option<String>"), "string | null");
    assert_eq!(rust_type_to_ts("Option<u32>"), "number | null");
}

#[test]
fn test_rust_type_to_ts_vec() {
    assert_eq!(rust_type_to_ts("Vec<String>"), "string[]");
    assert_eq!(rust_type_to_ts("Vec<u32>"), "number[]");
}

#[test]
fn test_rust_type_to_ts_unknown_passthrough() {
    assert_eq!(rust_type_to_ts("UserProfile"), "UserProfile");
    assert_eq!(rust_type_to_ts("MyStruct"), "MyStruct");
}

// ============================================================
// extract_command_schemas from fixture
// ============================================================

#[test]
fn test_extract_single_param() {
    let content = load_fixture("rust/typed_commands.rs");
    let schemas = extract_command_schemas(&content, &test_path("lib.rs"));

    let get_user = schemas.iter().find(|s| s.command_name == "get_user");
    assert!(get_user.is_some(), "Should find get_user");
    let s = get_user.unwrap();
    assert_eq!(s.params.len(), 1, "get_user should have 1 param");
    assert_eq!(s.params[0].name, "id");
    assert_eq!(s.params[0].ts_type, "number");
    // Result<String, String> → string
    assert_eq!(s.return_type, "string");
}

#[test]
fn test_extract_multi_param() {
    let content = load_fixture("rust/typed_commands.rs");
    let schemas = extract_command_schemas(&content, &test_path("lib.rs"));

    let create_user = schemas.iter().find(|s| s.command_name == "create_user");
    assert!(create_user.is_some(), "Should find create_user");
    let s = create_user.unwrap();
    assert_eq!(s.params.len(), 2, "create_user should have 2 params");
    assert_eq!(s.params[0].name, "name");
    assert_eq!(s.params[0].ts_type, "string");
    assert_eq!(s.params[1].name, "age");
    assert_eq!(s.params[1].ts_type, "number");
}

#[test]
fn test_extract_no_param() {
    let content = load_fixture("rust/typed_commands.rs");
    let schemas = extract_command_schemas(&content, &test_path("lib.rs"));

    let ping = schemas.iter().find(|s| s.command_name == "ping");
    assert!(ping.is_some(), "Should find ping");
    let s = ping.unwrap();
    assert!(s.params.is_empty(), "ping should have no params");
    assert_eq!(s.return_type, "void");
}

#[test]
fn test_extract_vec_return() {
    let content = load_fixture("rust/typed_commands.rs");
    let schemas = extract_command_schemas(&content, &test_path("lib.rs"));

    let get_items = schemas.iter().find(|s| s.command_name == "get_items");
    assert!(get_items.is_some());
    assert_eq!(get_items.unwrap().return_type, "string[]");
}

#[test]
fn test_extract_option_return() {
    let content = load_fixture("rust/typed_commands.rs");
    let schemas = extract_command_schemas(&content, &test_path("lib.rs"));

    let find_user = schemas.iter().find(|s| s.command_name == "find_user");
    assert!(find_user.is_some());
    assert_eq!(find_user.unwrap().return_type, "string | null");
}

#[test]
fn test_only_tauri_command_extracted() {
    let content = load_fixture("rust/typed_commands.rs");
    let schemas = extract_command_schemas(&content, &test_path("lib.rs"));

    // helper_fn has no #[tauri::command] and should NOT be extracted
    let helper = schemas.iter().find(|s| s.command_name == "helper_fn");
    assert!(
        helper.is_none(),
        "helper_fn should not be extracted (no #[tauri::command])"
    );
}

// ─── Event schema extraction ──────────────────────────────────────────────────

#[test]
fn test_event_schema_local_variable_payload() {
    let rust_code = r#"
use tauri::AppHandle;

struct Payload {
    text: String,
    count: u32,
}

fn setup(app: AppHandle) {
    let payload = Payload { text: "hello".into(), count: 42 };
    app.emit("my-event", payload).unwrap();
}
"#;

    let schemas = extract_event_schemas(rust_code, &test_path("lib.rs"));
    assert_eq!(schemas.len(), 1, "Should find 1 event schema");
    assert_eq!(schemas[0].event_name, "my-event");
    assert_eq!(schemas[0].payload_type, "Payload");
}

#[test]
fn test_event_schema_direct_struct_payload() {
    let rust_code = r#"
use tauri::AppHandle;

struct Info {
    msg: String,
}

fn setup(app: AppHandle) {
    app.emit("info-event", Info { msg: "hi".into() }).unwrap();
}
"#;

    let schemas = extract_event_schemas(rust_code, &test_path("lib.rs"));
    assert_eq!(schemas.len(), 1, "Should find 1 event schema");
    assert_eq!(schemas[0].event_name, "info-event");
    assert_eq!(schemas[0].payload_type, "Info");
}

#[test]
fn test_event_schema_local_variable_with_type_annotation() {
    let rust_code = r#"
use tauri::AppHandle;

fn setup(app: AppHandle) {
    let data: MyData = get_data();
    app.emit("data-event", data).unwrap();
}
"#;

    let schemas = extract_event_schemas(rust_code, &test_path("lib.rs"));
    assert_eq!(schemas.len(), 1, "Should find 1 event schema");
    assert_eq!(schemas[0].event_name, "data-event");
    assert_eq!(schemas[0].payload_type, "MyData");
}
