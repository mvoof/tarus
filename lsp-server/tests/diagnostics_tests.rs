//! Diagnostics tests

mod common_paths;

use common_paths::test_path;
use lsp_server::indexer::{FileIndex, Finding, ProjectIndex};
use lsp_server::syntax::{Behavior, EntityType};
use tower_lsp_server::lsp_types::{Position, Range};

fn create_finding(key: &str, entity: EntityType, behavior: Behavior, line: u32) -> Finding {
    Finding {
        key: key.to_string(),
        entity,
        behavior,
        range: Range {
            start: Position { line, character: 0 },
            end: Position { line, character: key.len() as u32 },
        },
    }
}

// Note: compute_file_diagnostics will be moved to capabilities/diagnostics.rs during refactoring
// For now, we'll test the diagnostic logic through the indexer

#[test]
fn test_undefined_command_warning() {
    let index = ProjectIndex::new();
    
    // Add only a call, no definition
    let frontend_file = FileIndex {
        path: test_path("app.ts"),
        findings: vec![
            create_finding("undefined_cmd", EntityType::Command, Behavior::Call, 5),
        ],
    };
    
    index.add_file(frontend_file);
    
    // Check diagnostic info
    let key = lsp_server::indexer::IndexKey {
        name: "undefined_cmd".to_string(),
        entity: EntityType::Command,
    };
    
    let info = index.get_diagnostic_info(&key);
    assert!(!info.has_definition, "Command should not have definition");
    assert!(info.has_calls, "Command should have calls");
}

#[test]
fn test_unused_command_warning() {
    let index = ProjectIndex::new();
    
    // Add only a definition, no calls
    let backend_file = FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            create_finding("unused_cmd", EntityType::Command, Behavior::Definition, 10),
        ],
    };
    
    index.add_file(backend_file);
    
    // Check diagnostic info
    let key = lsp_server::indexer::IndexKey {
        name: "unused_cmd".to_string(),
        entity: EntityType::Command,
    };
    
    let info = index.get_diagnostic_info(&key);
    assert!(info.has_definition, "Command should have definition");
    assert!(!info.has_calls, "Command should not have calls");
}

#[test]
fn test_event_no_emitter() {
    let index = ProjectIndex::new();
    
    // Add listener only, no emitter
    let frontend_file = FileIndex {
        path: test_path("app.ts"),
        findings: vec![
            create_finding("some-event", EntityType::Event, Behavior::Listen, 5),
        ],
    };
    
    index.add_file(frontend_file);
    
    let key = lsp_server::indexer::IndexKey {
        name: "some-event".to_string(),
        entity: EntityType::Event,
    };
    
    let info = index.get_diagnostic_info(&key);
    assert!(info.has_listeners, "Event should have listeners");
    assert!(!info.has_emitters, "Event should not have emitters");
}

#[test]
fn test_event_no_listener() {
    let index = ProjectIndex::new();
    
    // Add emitter only, no listener
    let backend_file = FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            create_finding("notification", EntityType::Event, Behavior::Emit, 15),
        ],
    };
    
    index.add_file(backend_file);
    
    let key = lsp_server::indexer::IndexKey {
        name: "notification".to_string(),
        entity: EntityType::Event,
    };
    
    let info = index.get_diagnostic_info(&key);
    assert!(info.has_emitters, "Event should have emitters");
    assert!(!info.has_listeners, "Event should not have listeners");
}

#[test]
fn test_complete_command_no_warnings() {
    let index = ProjectIndex::new();
    
    // Add both definition and call
    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            create_finding("greet", EntityType::Command, Behavior::Definition, 5),
        ],
    });
    
    index.add_file(FileIndex {
        path: test_path("frontend.ts"),
        findings: vec![
            create_finding("greet", EntityType::Command, Behavior::Call, 10),
        ],
    });
    
    let key = lsp_server::indexer::IndexKey {
        name: "greet".to_string(),
        entity: EntityType::Command,
    };
    
    let info = index.get_diagnostic_info(&key);
    assert!(info.has_definition, "Should have definition");
    assert!(info.has_calls, "Should have calls");
}

#[test]
fn test_complete_event_no_warnings() {
    let index = ProjectIndex::new();
    
    // Add both emitter and listener
    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![
            create_finding("data-update", EntityType::Event, Behavior::Emit, 5),
        ],
    });
    
    index.add_file(FileIndex {
        path: test_path("frontend.ts"),
        findings: vec![
            create_finding("data-update", EntityType::Event, Behavior::Listen, 10),
        ],
    });
    
    let key = lsp_server::indexer::IndexKey {
        name: "data-update".to_string(),
        entity: EntityType::Event,
    };
    
    let info = index.get_diagnostic_info(&key);
    assert!(info.has_emitters, "Should have emitters");
    assert!(info.has_listeners, "Should have listeners");
}
