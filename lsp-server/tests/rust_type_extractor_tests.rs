//! Tests for the rust_type_extractor module

mod common_fixtures;
mod common_paths;

use common_fixtures::load_fixture;
use common_paths::test_path;
use lsp_server::rust_type_extractor::{extract_command_schemas, rust_type_to_ts};

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
