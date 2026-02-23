//! Tests for the bindings reader module

mod common_fixtures;
mod common_paths;

use common_fixtures::load_fixture;
use common_paths::test_path;
use lsp_server::bindings_reader::{parse_specta_bindings, parse_ts_rs_types, parse_typegen_types};
use lsp_server::file_processor::process_file_content;
use lsp_server::indexer::{GeneratorKind, ProjectIndex};
use lsp_server::scanner::detect_generator_kind;
use lsp_server::utils::camel_to_snake;

// ============================================================
// camel_to_snake unit tests
// ============================================================

#[test]
fn test_camel_to_snake() {
    assert_eq!(camel_to_snake("getUserProfile"), "get_user_profile");
    assert_eq!(camel_to_snake("createUser"), "create_user");
    assert_eq!(camel_to_snake("ping"), "ping");
    assert_eq!(camel_to_snake("getHTTPSResponse"), "get_https_response");
}

// ============================================================
// Specta bindings parsing
// ============================================================

#[test]
fn test_parse_specta_extracts_schemas() {
    let content = load_fixture("bindings/specta_bindings.ts");
    let schemas = parse_specta_bindings(&content, test_path("specta_bindings.ts"));

    assert_eq!(schemas.len(), 3, "Should parse 3 command schemas");

    let get_user = schemas.iter().find(|s| s.command_name == "get_user_profile");
    assert!(get_user.is_some(), "Should find get_user_profile");
    let s = get_user.unwrap();
    assert_eq!(s.params.len(), 1);
    assert_eq!(s.params[0].name, "id");
    assert_eq!(s.params[0].ts_type, "number");
    assert_eq!(s.generator, GeneratorKind::Specta);
}

#[test]
fn test_parse_specta_camel_to_snake() {
    let content = load_fixture("bindings/specta_bindings.ts");
    let schemas = parse_specta_bindings(&content, test_path("specta_bindings.ts"));

    // getUserProfile → get_user_profile
    assert!(schemas.iter().any(|s| s.command_name == "get_user_profile"));
    // createUser → create_user
    assert!(schemas.iter().any(|s| s.command_name == "create_user"));
}

#[test]
fn test_parse_specta_result_return_unwrapped() {
    let content = load_fixture("bindings/specta_bindings.ts");
    let schemas = parse_specta_bindings(&content, test_path("specta_bindings.ts"));

    // getUserProfile returns Promise<Result<UserProfile, string>> → "UserProfile"
    let s = schemas.iter().find(|s| s.command_name == "get_user_profile").unwrap();
    assert_eq!(s.return_type, "UserProfile");
}

#[test]
fn test_parse_specta_no_params() {
    let content = load_fixture("bindings/specta_bindings.ts");
    let schemas = parse_specta_bindings(&content, test_path("specta_bindings.ts"));

    let ping = schemas.iter().find(|s| s.command_name == "ping").unwrap();
    assert!(ping.params.is_empty(), "ping should have no params");
    assert_eq!(ping.return_type, "void");
}

#[test]
fn test_parse_specta_multi_params() {
    let content = load_fixture("bindings/specta_bindings.ts");
    let schemas = parse_specta_bindings(&content, test_path("specta_bindings.ts"));

    // createUser(name: string, age: number)
    let s = schemas.iter().find(|s| s.command_name == "create_user").unwrap();
    assert_eq!(s.params.len(), 2);
    assert_eq!(s.params[0].name, "name");
    assert_eq!(s.params[0].ts_type, "string");
    assert_eq!(s.params[1].name, "age");
    assert_eq!(s.params[1].ts_type, "number");
}

// ============================================================
// ts-rs type alias parsing
// ============================================================

#[test]
fn test_parse_ts_rs_type_aliases() {
    let content = load_fixture("bindings/ts_rs_types.ts");
    let aliases = parse_ts_rs_types(&content);

    assert!(aliases.contains_key("UserProfile"), "Should find UserProfile");
    assert!(aliases.contains_key("TaskState"), "Should find TaskState");

    let user_profile = &aliases["UserProfile"];
    assert!(user_profile.contains("id"));
    assert!(user_profile.contains("name"));
}

// ============================================================
// typegen type alias parsing
// ============================================================

#[test]
fn test_parse_typegen_export_type_lines() {
    let content = load_fixture("bindings/typegen_bindings.ts");
    let aliases = parse_typegen_types(&content);

    assert!(aliases.contains_key("SimpleUser"), "Should find SimpleUser");
    assert!(aliases.contains_key("ApiResponse"), "Should find ApiResponse");
    assert!(aliases.contains_key("TaskState"), "Should find TaskState");
}

#[test]
fn test_parse_typegen_export_interface_blocks() {
    let content = load_fixture("bindings/typegen_bindings.ts");
    let aliases = parse_typegen_types(&content);

    // export interface UserProfile { id: number; name: string; ... }
    assert!(aliases.contains_key("UserProfile"), "Should find UserProfile interface");
    let profile = &aliases["UserProfile"];
    assert!(profile.contains("id"), "UserProfile should contain 'id' field");
    assert!(profile.contains("name"), "UserProfile should contain 'name' field");
    assert!(profile.contains("email"), "UserProfile should contain 'email' field");
}

#[test]
fn test_parse_typegen_interface_skips_index_signatures() {
    let content = load_fixture("bindings/typegen_bindings.ts");
    let aliases = parse_typegen_types(&content);

    // GreetParams has `[key: string]: unknown` which should be skipped
    assert!(aliases.contains_key("GreetParams"), "Should find GreetParams interface");
    let greet_params = &aliases["GreetParams"];
    assert!(greet_params.contains("name"), "GreetParams should contain 'name' field");
    assert!(
        !greet_params.contains("[key"),
        "GreetParams should NOT contain index signature"
    );
}

#[test]
fn test_parse_typegen_interface_in_full_file() {
    // Test with the actual typegen format header
    let content = r#"/**
 * Auto-generated TypeScript bindings for Tauri commands
 * Generated by tauri-typegen v0.4.1
 */

export interface SimpleModel {
  id: number;
  label: string;
}
"#;

    let aliases = parse_typegen_types(content);
    assert!(aliases.contains_key("SimpleModel"));
    let def = &aliases["SimpleModel"];
    assert!(def.contains("id: number"));
    assert!(def.contains("label: string"));
}

// ============================================================
// Generator detection
// ============================================================

#[test]
fn test_detect_ts_rs_kind() {
    let content = load_fixture("bindings/ts_rs_types.ts");
    let kind = detect_generator_kind(&content);
    assert_eq!(kind, Some(GeneratorKind::TsRs));
}

#[test]
fn test_detect_specta_kind() {
    let content = load_fixture("bindings/specta_bindings.ts");
    let kind = detect_generator_kind(&content);
    assert_eq!(kind, Some(GeneratorKind::Specta));
}

#[test]
fn test_detect_typegen_kind() {
    let content = load_fixture("bindings/typegen_bindings.ts");
    let kind = detect_generator_kind(&content);
    assert_eq!(kind, Some(GeneratorKind::Typegen));
}

#[test]
fn test_detect_none_for_regular() {
    let content = r#"
import { invoke } from '@tauri-apps/api';
const result = await invoke('get_user');
"#;
    let kind = detect_generator_kind(content);
    assert!(kind.is_none(), "Regular TS file should not be detected as a generator");
}

// ============================================================
// Phase 4: File processor routing
// ============================================================

#[test]
fn test_process_specta_file_populates_schema() {
    let index = ProjectIndex::new();
    let content = load_fixture("bindings/specta_bindings.ts");
    let path = test_path("specta_bindings.ts");

    let result = process_file_content(&path, &content, &index);
    assert!(result, "process_file_content should return true");

    let schema = index.get_schema("get_user_profile");
    assert!(schema.is_some(), "get_user_profile schema should be in index");
    assert_eq!(schema.unwrap().generator, GeneratorKind::Specta);
}

#[test]
fn test_process_ts_rs_file_populates_type_aliases() {
    let index = ProjectIndex::new();
    let content = load_fixture("bindings/ts_rs_types.ts");
    let path = test_path("ts_rs_types.ts");

    process_file_content(&path, &content, &index);

    assert!(
        index.type_aliases.contains_key("UserProfile"),
        "UserProfile alias should be in index"
    );
    assert!(
        index.type_aliases.contains_key("TaskState"),
        "TaskState alias should be in index"
    );
}

#[test]
fn test_rust_file_populates_rust_source_schema() {
    let index = ProjectIndex::new();
    let content = load_fixture("rust/typed_commands.rs");
    let path = test_path("lib.rs");

    process_file_content(&path, &content, &index);

    let schema = index.get_schema("get_user");
    assert!(schema.is_some(), "get_user schema should be extracted from Rust");
    assert_eq!(schema.unwrap().generator, GeneratorKind::RustSource);
}

#[test]
fn test_bindings_schema_overrides_rust_source() {
    let index = ProjectIndex::new();

    // First, process the Rust file (produces RustSource schema)
    let rust_content = load_fixture("rust/typed_commands.rs");
    process_file_content(&test_path("lib.rs"), &rust_content, &index);

    assert_eq!(
        index.get_schema("get_user").unwrap().generator,
        GeneratorKind::RustSource
    );

    // Now process a specta bindings file that covers get_user_profile
    // (the specta fixture has "get_user_profile" not "get_user", so use inline content)
    let specta_content = r#"// This file was generated by [tauri-specta]. Do not edit.

export const commands = {
    async getUser(id: number): Promise<Result<string, string>> {
        return await invoke("get_user", { id });
    },
};
"#;

    process_file_content(&test_path("specta.ts"), specta_content, &index);

    let schema = index.get_schema("get_user").unwrap();
    assert_eq!(
        schema.generator,
        GeneratorKind::Specta,
        "Specta schema should replace RustSource for get_user"
    );
}
