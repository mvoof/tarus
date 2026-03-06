//! Integration tests for full workflow

mod common_fixtures;
mod common_paths;

use common_fixtures::load_fixture;
use common_paths::test_path;
use lsp_server::indexer::ProjectIndex;
use lsp_server::tree_parser;

#[test]
fn test_full_workflow_rust_to_ts() {
    // Create project index
    let index = ProjectIndex::new();

    // Parse Rust backend with command
    let rust_content = load_fixture("rust/simple_command.rs");
    let rust_path = test_path("backend.rs");
    let rust_result = tree_parser::parse(&rust_path, &rust_content);
    assert!(rust_result.is_ok());
    index.add_file(rust_result.unwrap());

    // Parse TypeScript frontend calling command
    let ts_content = load_fixture("typescript/invoke.ts");
    let ts_path = test_path("frontend.ts");
    let ts_result = tree_parser::parse(&ts_path, &ts_content);
    assert!(ts_result.is_ok());
    index.add_file(ts_result.unwrap());

    // Verify command "greet" is linked
    let locations = index.get_locations(lsp_server::syntax::EntityType::Command, "greet");
    assert!(
        locations.len() >= 2,
        "Should have at least definition and call"
    );

    // Verify we can find both backend and frontend locations
    let has_rust_file = locations.iter().any(|l| l.path == rust_path);
    let has_ts_file = locations.iter().any(|l| l.path == ts_path);
    assert!(has_rust_file, "Should include Rust file");
    assert!(has_ts_file, "Should include TypeScript file");
}

#[test]
fn test_full_workflow_events() {
    let index = ProjectIndex::new();

    // Parse Rust backend emitting events
    let rust_content = load_fixture("rust/events.rs");
    let rust_path = test_path("backend.rs");
    let rust_result = tree_parser::parse(&rust_path, &rust_content);
    assert!(rust_result.is_ok());
    index.add_file(rust_result.unwrap());

    // Parse TypeScript frontend listening to events
    let ts_content = load_fixture("typescript/emit.ts");
    let ts_path = test_path("frontend.ts");
    let ts_result = tree_parser::parse(&ts_path, &ts_content);
    assert!(ts_result.is_ok());
    index.add_file(ts_result.unwrap());

    // Verify events are linked
    let all_event_names: Vec<String> = index
        .map
        .iter()
        .filter_map(|entry| {
            if entry.key().entity == lsp_server::syntax::EntityType::Event {
                Some(entry.key().name.clone())
            } else {
                None
            }
        })
        .collect();

    assert!(!all_event_names.is_empty(), "Should have events indexed");
}

#[test]
fn test_multi_language_project() {
    let index = ProjectIndex::new();

    // Add Rust command
    let rust_content = load_fixture("rust/simple_command.rs");
    let rust_result = tree_parser::parse(&test_path("backend.rs"), &rust_content);
    assert!(rust_result.is_ok());
    index.add_file(rust_result.unwrap());

    // Add TypeScript call
    let ts_content = load_fixture("typescript/invoke.ts");
    let ts_result = tree_parser::parse(&test_path("app.ts"), &ts_content);
    assert!(ts_result.is_ok());
    index.add_file(ts_result.unwrap());

    // Add Vue call
    let vue_content = load_fixture("vue/single_script.vue");
    let vue_result = tree_parser::parse(&test_path("Component.vue"), &vue_content);
    assert!(vue_result.is_ok());
    index.add_file(vue_result.unwrap());

    // Add Angular call
    let angular_content = load_fixture("angular/component.component.ts");
    let angular_result = tree_parser::parse(&test_path("user.component.ts"), &angular_content);
    assert!(angular_result.is_ok());
    index.add_file(angular_result.unwrap());

    // Verify "greet" command is found across all languages
    let locations = index.get_locations(lsp_server::syntax::EntityType::Command, "greet");
    assert!(
        locations.len() >= 2,
        "Should find command across multiple languages"
    );
}

#[test]
fn test_project_update_cycle() {
    let index = ProjectIndex::new();
    let path = test_path("backend.rs");

    // Initial parse - simple command
    let content1 = load_fixture("rust/simple_command.rs");
    let result1 = tree_parser::parse(&path, &content1);
    assert!(result1.is_ok());
    index.add_file(result1.unwrap());

    let locations1 = index.get_locations(lsp_server::syntax::EntityType::Command, "greet");
    assert_eq!(locations1.len(), 1);

    // Update with multiple commands
    let content2 = load_fixture("rust/multiple_commands.rs");
    let result2 = tree_parser::parse(&path, &content2);
    assert!(result2.is_ok());
    index.add_file(result2.unwrap());

    // Verify old command is gone
    let locations_greet = index.get_locations(lsp_server::syntax::EntityType::Command, "greet");
    assert_eq!(locations_greet.len(), 0, "Old command should be removed");

    // Verify new commands are present
    let locations_user = index.get_locations(lsp_server::syntax::EntityType::Command, "get_user");
    assert_eq!(locations_user.len(), 1, "New command should be added");
}
