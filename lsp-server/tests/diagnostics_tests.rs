//! Diagnostics tests

mod common_paths;

use common_paths::test_path;
use lsp_server::capabilities::diagnostics::compute_file_diagnostics;
use lsp_server::indexer::{
    CommandSchema, FileIndex, Finding, GeneratorKind, ParamSchema, ProjectIndex,
};
use lsp_server::syntax::{Behavior, EntityType};
use tower_lsp_server::lsp_types::{Position, Range};

fn create_finding(key: &str, entity: EntityType, behavior: Behavior, line: u32) -> Finding {
    Finding {
        key: key.to_string(),
        entity,
        behavior,
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: key.len() as u32,
            },
        },
        call_arg_count: None,
        call_param_keys: None,
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
        findings: vec![create_finding(
            "undefined_cmd",
            EntityType::Command,
            Behavior::Call,
            5,
        )],
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
        findings: vec![create_finding(
            "unused_cmd",
            EntityType::Command,
            Behavior::Definition,
            10,
        )],
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
        findings: vec![create_finding(
            "some-event",
            EntityType::Event,
            Behavior::Listen,
            5,
        )],
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
        findings: vec![create_finding(
            "notification",
            EntityType::Event,
            Behavior::Emit,
            15,
        )],
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
        findings: vec![create_finding(
            "greet",
            EntityType::Command,
            Behavior::Definition,
            5,
        )],
    });

    index.add_file(FileIndex {
        path: test_path("frontend.ts"),
        findings: vec![create_finding(
            "greet",
            EntityType::Command,
            Behavior::Call,
            10,
        )],
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
        findings: vec![create_finding(
            "data-update",
            EntityType::Event,
            Behavior::Emit,
            5,
        )],
    });

    index.add_file(FileIndex {
        path: test_path("frontend.ts"),
        findings: vec![create_finding(
            "data-update",
            EntityType::Event,
            Behavior::Listen,
            10,
        )],
    });

    let key = lsp_server::indexer::IndexKey {
        name: "data-update".to_string(),
        entity: EntityType::Event,
    };

    let info = index.get_diagnostic_info(&key);
    assert!(info.has_emitters, "Should have emitters");
    assert!(info.has_listeners, "Should have listeners");
}

// ─── Type diagnostic tests (Phase 5) ────────────────────────────────────────

fn make_call_finding(command: &str, line: u32, param_keys: Vec<&str>) -> Finding {
    Finding {
        key: command.to_string(),
        entity: EntityType::Command,
        behavior: Behavior::Call,
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: command.len() as u32,
            },
        },
        call_arg_count: None,
        call_param_keys: Some(param_keys.into_iter().map(String::from).collect()),
    }
}

fn make_schema(command: &str, params: &[(&str, &str)], generator: GeneratorKind) -> CommandSchema {
    CommandSchema {
        command_name: command.to_string(),
        params: params
            .iter()
            .map(|(n, t)| ParamSchema {
                name: n.to_string(),
                ts_type: t.to_string(),
            })
            .collect(),
        return_type: "void".to_string(),
        source_path: test_path("bindings.ts"),
        generator,
    }
}

/// No type diagnostic is emitted when no binding file has been indexed.
#[test]
fn test_no_type_diagnostic_without_bindings() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    // Add a definition + call (without bindings)
    index.add_file(FileIndex {
        path: test_path("lib.rs"),
        findings: vec![create_finding(
            "greet",
            EntityType::Command,
            Behavior::Definition,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![make_call_finding("greet", 5, vec!["wrong_key"])],
    });

    // No bindings → has_bindings_files() is false
    assert!(!index.has_bindings_files());

    let diags = compute_file_diagnostics(&path, &index);
    // Only structural diagnostics (none here since definition exists)
    assert!(
        diags
            .iter()
            .all(|d| !d.message.contains("missing") && !d.message.contains("unexpected")),
        "Should not produce type diagnostics without bindings"
    );
}

/// Missing required param key triggers a WARNING.
#[test]
fn test_missing_param_key_warning() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("lib.rs"),
        findings: vec![create_finding(
            "get_user",
            EntityType::Command,
            Behavior::Definition,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        // Call is missing the required "id" param
        findings: vec![make_call_finding("get_user", 5, vec![])],
    });

    index.add_schema(make_schema(
        "get_user",
        &[("id", "number")],
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let type_diag = diags.iter().find(|d| d.message.contains("missing"));
    assert!(
        type_diag.is_some(),
        "Expected missing-param diagnostic, got: {diags:?}"
    );
    assert!(type_diag.unwrap().message.contains("id"));
}

/// Unexpected extra param key triggers a WARNING.
#[test]
fn test_extra_param_key_warning() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("lib.rs"),
        findings: vec![create_finding(
            "ping",
            EntityType::Command,
            Behavior::Definition,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        // Call has "extra" param that schema doesn't expect
        findings: vec![make_call_finding("ping", 5, vec!["extra"])],
    });

    // Schema expects no params
    index.add_schema(make_schema("ping", &[], GeneratorKind::Specta));

    let diags = compute_file_diagnostics(&path, &index);
    let type_diag = diags.iter().find(|d| d.message.contains("unexpected"));
    assert!(
        type_diag.is_some(),
        "Expected unexpected-param diagnostic, got: {diags:?}"
    );
    assert!(type_diag.unwrap().message.contains("extra"));
}

/// Correct param keys → no type diagnostic.
#[test]
fn test_correct_param_keys_no_warning() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("lib.rs"),
        findings: vec![create_finding(
            "create_user",
            EntityType::Command,
            Behavior::Definition,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![make_call_finding("create_user", 5, vec!["name", "email"])],
    });

    index.add_schema(make_schema(
        "create_user",
        &[("name", "string"), ("email", "string")],
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let type_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("missing") || d.message.contains("unexpected"))
        .collect();
    assert!(
        type_diags.is_empty(),
        "Correct params should produce no type diagnostics, got: {type_diags:?}"
    );
}

/// RustSource schemas are ignored for type checking.
#[test]
fn test_rust_source_schema_skipped_for_type_check() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("lib.rs"),
        findings: vec![create_finding(
            "greet",
            EntityType::Command,
            Behavior::Definition,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        // Wrong param key — but schema is RustSource so should be ignored
        findings: vec![make_call_finding("greet", 5, vec!["bad_key"])],
    });

    // Add a Specta type alias so has_bindings_files() is true
    index.add_type_alias(
        "UserProfile".to_string(),
        "{ id: number }".to_string(),
        test_path("types.ts"),
    );

    // Add RustSource schema only (no Specta/TsRs/Typegen schema for "greet")
    index.add_schema(make_schema(
        "greet",
        &[("name", "string")],
        GeneratorKind::RustSource,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let type_diags: Vec<_> = diags
        .iter()
        .filter(|d| d.message.contains("missing") || d.message.contains("unexpected"))
        .collect();
    assert!(
        type_diags.is_empty(),
        "RustSource schema should not trigger type diagnostics, got: {type_diags:?}"
    );
}
