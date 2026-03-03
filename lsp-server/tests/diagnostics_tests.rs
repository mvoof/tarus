//! Diagnostics tests

mod common_paths;

use common_paths::test_path;
use lsp_server::capabilities::diagnostics::compute_file_diagnostics;
use lsp_server::indexer::{
    CommandSchema, EventSchema, FileIndex, Finding, GeneratorKind, ParamSchema, ProjectIndex,
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
        return_type: None,
        call_name_end: None,
        type_arg_range: None,
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
        return_type: None,
        call_name_end: None,
        type_arg_range: None,
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

// ─── Return type diagnostic tests ────────────────────────────────────────────

fn make_call_with_return_type(command: &str, line: u32, return_type: &str) -> Finding {
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
        call_param_keys: None,
        return_type: Some(return_type.to_string()),
        call_name_end: None,
        type_arg_range: None,
    }
}

fn make_schema_with_return(
    command: &str,
    return_type: &str,
    generator: GeneratorKind,
) -> CommandSchema {
    CommandSchema {
        command_name: command.to_string(),
        params: vec![],
        return_type: return_type.to_string(),
        source_path: test_path("bindings.ts"),
        generator,
    }
}

/// Return type mismatch triggers a WARNING.
#[test]
fn test_return_type_mismatch_warning() {
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
        findings: vec![make_call_with_return_type("get_user", 5, "string")],
    });

    index.add_schema(make_schema_with_return(
        "get_user",
        "User",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let rt_diag = diags
        .iter()
        .find(|d| d.message.contains("return type mismatch"));
    assert!(
        rt_diag.is_some(),
        "Expected return type mismatch diagnostic, got: {diags:?}"
    );
    let msg = &rt_diag.unwrap().message;
    assert!(msg.contains("string"), "Should mention actual type");
    assert!(msg.contains("User"), "Should mention expected type");
}

/// Correct return type → no diagnostic.
#[test]
fn test_return_type_match_no_warning() {
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
        findings: vec![make_call_with_return_type("get_user", 5, "User")],
    });

    index.add_schema(make_schema_with_return(
        "get_user",
        "User",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags
            .iter()
            .all(|d| !d.message.contains("return type mismatch")),
        "Matching return type should not produce diagnostic, got: {diags:?}"
    );
}

/// void return type on invoke → skip diagnostic.
#[test]
fn test_return_type_void_skipped() {
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
        findings: vec![make_call_with_return_type("get_user", 5, "void")],
    });

    index.add_schema(make_schema_with_return(
        "get_user",
        "User",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags
            .iter()
            .all(|d| !d.message.contains("return type mismatch")),
        "void return type should be skipped, got: {diags:?}"
    );
}

/// any return type on invoke → skip diagnostic.
#[test]
fn test_return_type_any_skipped() {
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
        findings: vec![make_call_with_return_type("get_user", 5, "any")],
    });

    index.add_schema(make_schema_with_return(
        "get_user",
        "User",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags
            .iter()
            .all(|d| !d.message.contains("return type mismatch")),
        "any return type should be skipped, got: {diags:?}"
    );
}

/// RustSource schema is skipped when return type has NO binding alias.
#[test]
fn test_return_type_rust_source_skipped_without_alias() {
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
        findings: vec![make_call_with_return_type("get_user", 5, "string")],
    });

    // Add an unrelated type alias so has_bindings_files() is true
    index.add_type_alias(
        "OtherType".to_string(),
        "{ x: number }".to_string(),
        test_path("types.ts"),
    );

    // RustSource schema returns "User" but "User" is NOT in type_aliases
    index.add_schema(make_schema_with_return(
        "get_user",
        "User",
        GeneratorKind::RustSource,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags
            .iter()
            .all(|d| !d.message.contains("return type mismatch")),
        "RustSource without matching alias should not trigger diagnostic, got: {diags:?}"
    );
}

/// RustSource schema IS used when return type HAS a binding alias.
#[test]
fn test_return_type_rust_source_used_with_alias() {
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
        findings: vec![make_call_with_return_type("get_user", 5, "string")],
    });

    // "User" IS in type_aliases (from ts-rs)
    index.add_type_alias(
        "User".to_string(),
        "{ id: number }".to_string(),
        test_path("types.ts"),
    );

    index.add_schema(make_schema_with_return(
        "get_user",
        "User",
        GeneratorKind::RustSource,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let rt_diag = diags
        .iter()
        .find(|d| d.message.contains("return type mismatch"));
    assert!(
        rt_diag.is_some(),
        "RustSource with matching alias should trigger diagnostic, got: {diags:?}"
    );
}

// ─── Missing generic diagnostic tests ────────────────────────────────────────

/// invoke("cmd") without <T> when command returns non-void → HINT
#[test]
fn test_missing_return_type_hint() {
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
    // Call without return_type (no generic)
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![create_finding(
            "get_user",
            EntityType::Command,
            Behavior::Call,
            5,
        )],
    });

    index.add_schema(make_schema_with_return(
        "get_user",
        "UserProfile",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let missing_diag = diags
        .iter()
        .find(|d| d.message.contains("missing return type"));
    assert!(
        missing_diag.is_some(),
        "Expected missing return type diagnostic, got: {diags:?}"
    );
    let d = missing_diag.unwrap();
    assert!(d.message.contains("UserProfile"));
    assert_eq!(
        d.severity,
        Some(tower_lsp_server::lsp_types::DiagnosticSeverity::HINT)
    );
}

/// invoke("cmd") without <T> when command returns void → no diagnostic
#[test]
fn test_missing_return_type_void_no_hint() {
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
        findings: vec![create_finding(
            "ping",
            EntityType::Command,
            Behavior::Call,
            5,
        )],
    });

    index.add_schema(make_schema_with_return(
        "ping",
        "void",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags
            .iter()
            .all(|d| !d.message.contains("missing return type")),
        "void return should not trigger missing return type hint, got: {diags:?}"
    );
}

// ─── Event payload type diagnostic tests ──────────────────────────────────────

fn make_event_finding(event: &str, behavior: Behavior, line: u32) -> Finding {
    Finding {
        key: event.to_string(),
        entity: EntityType::Event,
        behavior,
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: event.len() as u32,
            },
        },
        call_arg_count: None,
        call_param_keys: None,
        return_type: None,
        call_name_end: None,
        type_arg_range: None,
    }
}

fn make_event_finding_with_type(
    event: &str,
    behavior: Behavior,
    line: u32,
    return_type: &str,
) -> Finding {
    Finding {
        key: event.to_string(),
        entity: EntityType::Event,
        behavior,
        range: Range {
            start: Position { line, character: 0 },
            end: Position {
                line,
                character: event.len() as u32,
            },
        },
        call_arg_count: None,
        call_param_keys: None,
        return_type: Some(return_type.to_string()),
        call_name_end: None,
        type_arg_range: None,
    }
}

fn make_event_schema(event: &str, payload: &str, generator: GeneratorKind) -> EventSchema {
    EventSchema {
        event_name: event.to_string(),
        payload_type: payload.to_string(),
        source_path: test_path("bindings.ts"),
        generator,
    }
}

/// No event type diagnostic without bindings files.
#[test]
fn test_event_no_type_diagnostic_without_bindings() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![create_finding(
            "my-event",
            EntityType::Event,
            Behavior::Emit,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![make_event_finding("my-event", Behavior::Listen, 5)],
    });

    assert!(!index.has_bindings_files());

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags.iter().all(|d| !d.message.contains("payload type")),
        "Should not produce event type diagnostics without bindings, got: {diags:?}"
    );
}

/// listen("event") without <T> when payload is non-void → HINT.
#[test]
fn test_event_missing_payload_type_hint() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![create_finding(
            "user-updated",
            EntityType::Event,
            Behavior::Emit,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![make_event_finding("user-updated", Behavior::Listen, 5)],
    });

    index.add_event_schema(make_event_schema(
        "user-updated",
        "UserProfile",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let missing_diag = diags
        .iter()
        .find(|d| d.message.contains("missing payload type"));
    assert!(
        missing_diag.is_some(),
        "Expected missing payload type diagnostic, got: {diags:?}"
    );
    let d = missing_diag.unwrap();
    assert!(d.message.contains("UserProfile"));
    assert_eq!(
        d.severity,
        Some(tower_lsp_server::lsp_types::DiagnosticSeverity::HINT)
    );
}

/// listen<Wrong>("event") → WARNING.
#[test]
fn test_event_wrong_payload_type_warning() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![create_finding(
            "user-updated",
            EntityType::Event,
            Behavior::Emit,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![make_event_finding_with_type(
            "user-updated",
            Behavior::Listen,
            5,
            "string",
        )],
    });

    index.add_event_schema(make_event_schema(
        "user-updated",
        "UserProfile",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    let mismatch = diags
        .iter()
        .find(|d| d.message.contains("payload type mismatch"));
    assert!(
        mismatch.is_some(),
        "Expected payload type mismatch diagnostic, got: {diags:?}"
    );
    let d = mismatch.unwrap();
    assert!(d.message.contains("string"));
    assert!(d.message.contains("UserProfile"));
    assert_eq!(
        d.severity,
        Some(tower_lsp_server::lsp_types::DiagnosticSeverity::WARNING)
    );
}

/// Correct payload type → no diagnostic.
#[test]
fn test_event_correct_payload_type_no_warning() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![create_finding(
            "user-updated",
            EntityType::Event,
            Behavior::Emit,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![make_event_finding_with_type(
            "user-updated",
            Behavior::Listen,
            5,
            "UserProfile",
        )],
    });

    index.add_event_schema(make_event_schema(
        "user-updated",
        "UserProfile",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags.iter().all(|d| !d.message.contains("payload type")),
        "Correct payload type should not produce diagnostic, got: {diags:?}"
    );
}

/// void payload → no diagnostic.
#[test]
fn test_event_void_payload_no_hint() {
    let index = ProjectIndex::new();
    let path = test_path("app.ts");

    index.add_file(FileIndex {
        path: test_path("backend.rs"),
        findings: vec![create_finding(
            "app-ready",
            EntityType::Event,
            Behavior::Emit,
            0,
        )],
    });
    index.add_file(FileIndex {
        path: path.clone(),
        findings: vec![make_event_finding("app-ready", Behavior::Listen, 5)],
    });

    index.add_event_schema(make_event_schema(
        "app-ready",
        "void",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&path, &index);
    assert!(
        diags.iter().all(|d| !d.message.contains("payload type")),
        "void payload should not trigger diagnostic, got: {diags:?}"
    );
}

/// Rust emit with EventSchema → no diagnostic (Rust has its own type system).
#[test]
fn test_event_payload_rust_file_skipped() {
    let index = ProjectIndex::new();
    let rs_path = test_path("backend.rs");

    index.add_file(FileIndex {
        path: rs_path.clone(),
        findings: vec![make_event_finding("user-updated", Behavior::Emit, 10)],
    });

    index.add_event_schema(make_event_schema(
        "user-updated",
        "UserProfile",
        GeneratorKind::Specta,
    ));

    let diags = compute_file_diagnostics(&rs_path, &index);
    let payload_diags: Vec<_> = diags
        .iter()
        .filter(|d| {
            d.message.contains("payload type")
                || d.message.contains("event-payload")
        })
        .collect();
    assert!(
        payload_diags.is_empty(),
        "Rust files should not get event payload diagnostics, got: {payload_diags:?}"
    );
}

/// Specta event schema parsing test.
#[test]
fn test_specta_event_schema_parsing() {
    let content = r#"
// This file was generated by [tauri-specta]. Do not edit this file manually.
export const events = __makeEvents__<{
    DemoEvent: string,
    UserUpdated: UserProfile,
}>({
    DemoEvent: "demo-event",
    UserUpdated: "user-updated",
})
"#;

    let schemas =
        lsp_server::bindings_reader::parse_specta_events(content, &test_path("bindings.ts"));
    assert_eq!(schemas.len(), 2, "Should parse 2 event schemas");

    let demo = schemas.iter().find(|s| s.event_name == "demo-event");
    assert!(demo.is_some(), "Should find demo-event");
    assert_eq!(demo.unwrap().payload_type, "string");

    let user = schemas.iter().find(|s| s.event_name == "user-updated");
    assert!(user.is_some(), "Should find user-updated");
    assert_eq!(user.unwrap().payload_type, "UserProfile");
}

/// Typegen event schema parsing test.
#[test]
fn test_typegen_event_schema_parsing() {
    let content = r#"
import { listen } from '@tauri-apps/api/event';

export async function onNotificationSent(
  handler: (payload: types.Message) => void
): Promise<UnlistenFn> {
  return listen<types.Message>('notification-sent', (event) => {
    handler(event.payload);
  });
}

export async function onAppReady(
  handler: (payload: string) => void
): Promise<UnlistenFn> {
  return listen<string>('app-ready', (event) => {
    handler(event.payload);
  });
}
"#;

    let schemas =
        lsp_server::bindings_reader::parse_typegen_events(content, &test_path("events.ts"));
    assert_eq!(schemas.len(), 2, "Should parse 2 event schemas");

    let notif = schemas.iter().find(|s| s.event_name == "notification-sent");
    assert!(notif.is_some(), "Should find notification-sent");
    assert_eq!(notif.unwrap().payload_type, "Message");

    let ready = schemas.iter().find(|s| s.event_name == "app-ready");
    assert!(ready.is_some(), "Should find app-ready");
    assert_eq!(ready.unwrap().payload_type, "string");
}
