//! Diagnostics tests

mod common_paths;

use common_paths::test_path;
use lsp_server::indexer::{FileIndex, Finding, ProjectIndex};
use lsp_server::syntax::{Behavior, EntityType};
use tower_lsp_server::ls_types::{Position, Range};

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
        parameters: None,
        return_type: None,
        fields: None,
        attributes: None,
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
    assert!(!info.has_definition(), "Command should not have definition");
    assert!(info.has_calls(), "Command should have calls");
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
    assert!(info.has_definition(), "Command should have definition");
    assert!(!info.has_calls(), "Command should not have calls");
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
    assert!(info.has_listeners(), "Event should have listeners");
    assert!(!info.has_emitters(), "Event should not have emitters");
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
    assert!(info.has_emitters(), "Event should have emitters");
    assert!(!info.has_listeners(), "Event should not have listeners");
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
    assert!(info.has_definition(), "Should have definition");
    assert!(info.has_calls(), "Should have calls");
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
    assert!(info.has_emitters(), "Should have emitters");
    assert!(info.has_listeners(), "Should have listeners");
}

#[test]
fn test_event_payload_type_mismatch() {
    let index = ProjectIndex::new();

    // Rust emitter with CalculationStatus payload type
    let mut emit_finding = create_finding("status-update", EntityType::Event, Behavior::Emit, 5);
    emit_finding.return_type = Some("CalculationStatus".to_string());

    // TypeScript listener with string payload type
    let mut listen_finding =
        create_finding("status-update", EntityType::Event, Behavior::Listen, 10);
    listen_finding.return_type = Some("string".to_string());

    let backend_path = test_path("backend.rs");
    let frontend_path = test_path("frontend.ts");

    index.add_file(FileIndex {
        path: backend_path,
        findings: vec![emit_finding],
    });

    index.add_file(FileIndex {
        path: frontend_path.clone(),
        findings: vec![listen_finding],
    });

    // Compute diagnostics for the frontend file (where the listener is)
    let diagnostics =
        lsp_server::capabilities::diagnostics::compute_file_diagnostics(&frontend_path, &index);

    // Without external bindings, CalculationStatus (custom type) vs string should NOT
    // produce diagnostics — we can't know how CalculationStatus serializes
    let mismatch_diags: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("Payload type mismatch"))
        .collect();

    assert!(
        mismatch_diags.is_empty(),
        "Should NOT have payload type mismatch for custom type vs primitive without bindings. Got: {:?}",
        mismatch_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
    );
}

/// Comprehensive reproducer test simulating the handling-events project
/// This test mirrors the exact setup to identify any false diagnostics.
#[test]
fn test_handling_events_project_diagnostics() {
    use lsp_server::indexer::Parameter;

    let index = ProjectIndex::new();
    let rs_path = test_path("src-tauri/src/lib.rs");
    let ts_path = test_path("src/main.ts");
    let types_path = test_path("src/types.ts");

    // --- Rust findings ---
    let mut rust_findings: Vec<Finding> = Vec::new();

    // Struct: UserProfile { id: u32, username: String, roles: Vec<Role>, is_active: bool }
    let mut user_profile_struct =
        create_finding("UserProfile", EntityType::Struct, Behavior::Definition, 10);
    user_profile_struct.fields = Some(vec![
        Parameter {
            name: "id".into(),
            type_name: "u32".into(),
        },
        Parameter {
            name: "username".into(),
            type_name: "String".into(),
        },
        Parameter {
            name: "roles".into(),
            type_name: "Vec<Role>".into(),
        },
        Parameter {
            name: "is_active".into(),
            type_name: "bool".into(),
        },
    ]);
    rust_findings.push(user_profile_struct);

    // Enum: Role with #[serde(rename_all = "lowercase")]
    let mut role_enum = create_finding("Role", EntityType::Enum, Behavior::Definition, 18);
    role_enum.fields = Some(vec![
        Parameter {
            name: "Admin".into(),
            type_name: "unit".into(),
        },
        Parameter {
            name: "Editor".into(),
            type_name: "unit".into(),
        },
        Parameter {
            name: "Viewer".into(),
            type_name: "unit".into(),
        },
    ]);
    role_enum.attributes = Some(vec!["#[serde(rename_all = \"lowercase\")]".into()]);
    rust_findings.push(role_enum);

    // Struct: CalculationRequest
    let mut calc_request_struct = create_finding(
        "CalculationRequest",
        EntityType::Struct,
        Behavior::Definition,
        26,
    );
    calc_request_struct.fields = Some(vec![
        Parameter {
            name: "operation".into(),
            type_name: "Operation".into(),
        },
        Parameter {
            name: "operand_a".into(),
            type_name: "f64".into(),
        },
        Parameter {
            name: "operand_b".into(),
            type_name: "f64".into(),
        },
    ]);
    rust_findings.push(calc_request_struct);

    // Enum: Operation with #[serde(rename_all = "lowercase")]
    let mut operation_enum =
        create_finding("Operation", EntityType::Enum, Behavior::Definition, 33);
    operation_enum.fields = Some(vec![
        Parameter {
            name: "Add".into(),
            type_name: "unit".into(),
        },
        Parameter {
            name: "Subtract".into(),
            type_name: "unit".into(),
        },
        Parameter {
            name: "Multiply".into(),
            type_name: "unit".into(),
        },
        Parameter {
            name: "Divide".into(),
            type_name: "unit".into(),
        },
    ]);
    operation_enum.attributes = Some(vec!["#[serde(rename_all = \"lowercase\")]".into()]);
    rust_findings.push(operation_enum);

    // Struct: CalculationResponse
    let mut calc_response_struct = create_finding(
        "CalculationResponse",
        EntityType::Struct,
        Behavior::Definition,
        42,
    );
    calc_response_struct.fields = Some(vec![
        Parameter {
            name: "result".into(),
            type_name: "f64".into(),
        },
        Parameter {
            name: "message".into(),
            type_name: "String".into(),
        },
        Parameter {
            name: "status".into(),
            type_name: "CalculationStatus".into(),
        },
    ]);
    rust_findings.push(calc_response_struct);

    // Enum: CalculationStatus with #[serde(tag = "type", content = "data")]
    let mut calc_status_enum = create_finding(
        "CalculationStatus",
        EntityType::Enum,
        Behavior::Definition,
        49,
    );
    calc_status_enum.fields = Some(vec![
        Parameter {
            name: "Success".into(),
            type_name: "unit".into(),
        },
        Parameter {
            name: "Partial".into(),
            type_name: "struct".into(),
        },
        Parameter {
            name: "CriticalFailure".into(),
            type_name: "struct".into(),
        },
    ]);
    calc_status_enum.attributes = Some(vec!["#[serde(tag = \"type\", content = \"data\")]".into()]);
    rust_findings.push(calc_status_enum);

    // Command: get_user_profile(user_id: u32) -> Result<UserProfile, String>
    let mut get_user_cmd = create_finding(
        "get_user_profile",
        EntityType::Command,
        Behavior::Definition,
        59,
    );
    get_user_cmd.parameters = Some(vec![Parameter {
        name: "user_id".into(),
        type_name: "u32".into(),
    }]);
    get_user_cmd.return_type = Some("Result<UserProfile, String>".into());
    rust_findings.push(get_user_cmd);

    // Command: perform_calculation(request: CalculationRequest) -> Result<CalculationResponse, String>
    let mut perform_calc_cmd = create_finding(
        "perform_calculation",
        EntityType::Command,
        Behavior::Definition,
        72,
    );
    perform_calc_cmd.parameters = Some(vec![Parameter {
        name: "request".into(),
        type_name: "CalculationRequest".into(),
    }]);
    perform_calc_cmd.return_type = Some("Result<CalculationResponse, String>".into());
    rust_findings.push(perform_calc_cmd);

    // Command: start_periodic_events(app: AppHandle, interval_ms: u64) - no return type
    let mut start_periodic_cmd = create_finding(
        "start_periodic_events",
        EntityType::Command,
        Behavior::Definition,
        100,
    );
    start_periodic_cmd.parameters = Some(vec![
        Parameter {
            name: "app".into(),
            type_name: "AppHandle".into(),
        },
        Parameter {
            name: "interval_ms".into(),
            type_name: "u64".into(),
        },
    ]);
    rust_findings.push(start_periodic_cmd);

    // Command: trigger_backend_listener(app: AppHandle, message: String)
    let mut trigger_cmd = create_finding(
        "trigger_backend_listener",
        EntityType::Command,
        Behavior::Definition,
        119,
    );
    trigger_cmd.parameters = Some(vec![
        Parameter {
            name: "app".into(),
            type_name: "AppHandle".into(),
        },
        Parameter {
            name: "message".into(),
            type_name: "String".into(),
        },
    ]);
    rust_findings.push(trigger_cmd);

    // Event: status-update (Rust emitter with CalculationStatus payload)
    let mut status_update_emit =
        create_finding("status-update", EntityType::Event, Behavior::Emit, 114);
    status_update_emit.return_type = Some("CalculationStatus".into());
    rust_findings.push(status_update_emit);

    // Event: backend-pong (String payload)
    let mut backend_pong_emit =
        create_finding("backend-pong", EntityType::Event, Behavior::Emit, 134);
    backend_pong_emit.return_type = Some("String".into());
    rust_findings.push(backend_pong_emit);

    // Event: backend-ack (String payload)
    let mut backend_ack_emit =
        create_finding("backend-ack", EntityType::Event, Behavior::Emit, 140);
    backend_ack_emit.return_type = Some("String".into());
    rust_findings.push(backend_ack_emit);

    // Rust listeners
    rust_findings.push(create_finding(
        "frontend-ping",
        EntityType::Event,
        Behavior::Listen,
        132,
    ));
    rust_findings.push(create_finding(
        "setup-complete",
        EntityType::Event,
        Behavior::Listen,
        138,
    ));
    rust_findings.push(create_finding(
        "internal-event",
        EntityType::Event,
        Behavior::Listen,
        143,
    ));
    let mut internal_event_emit =
        create_finding("internal-event", EntityType::Event, Behavior::Emit, 121);
    internal_event_emit.return_type = Some("String".into());
    rust_findings.push(internal_event_emit);

    index.add_file(FileIndex {
        path: rs_path,
        findings: rust_findings,
    });

    // --- TypeScript findings (main.ts) ---
    let mut ts_findings: Vec<Finding> = Vec::new();

    // listen<number>("status-update") — WRONG TYPE (should be CalculationStatus)
    let mut status_listen =
        create_finding("status-update", EntityType::Event, Behavior::Listen, 31);
    status_listen.return_type = Some("number".into());
    ts_findings.push(status_listen);

    // listen<number>("periodic-tick") — no emitter
    let mut periodic_listen =
        create_finding("periodic-tick", EntityType::Event, Behavior::Listen, 35);
    periodic_listen.return_type = Some("number".into());
    ts_findings.push(periodic_listen);

    // listen<string>("backend-pong") — correct type
    let mut pong_listen = create_finding("backend-pong", EntityType::Event, Behavior::Listen, 39);
    pong_listen.return_type = Some("string".into());
    ts_findings.push(pong_listen);

    // listen<string>("backend-ack") — correct type
    let mut ack_listen = create_finding("backend-ack", EntityType::Event, Behavior::Listen, 44);
    ack_listen.return_type = Some("string".into());
    ts_findings.push(ack_listen);

    // invoke<UserProfile>("get_user_profile", { userId: id })
    let mut get_user_call =
        create_finding("get_user_profile", EntityType::Command, Behavior::Call, 57);
    get_user_call.return_type = Some("UserProfile".into());
    get_user_call.parameters = Some(vec![Parameter {
        name: "userId".into(),
        type_name: "any".into(),
    }]);
    ts_findings.push(get_user_call);

    // invoke<CalculationResponse>("perform_calculation", { request })
    let mut perform_calc_call = create_finding(
        "perform_calculation",
        EntityType::Command,
        Behavior::Call,
        83,
    );
    perform_calc_call.return_type = Some("CalculationResponse".into());
    perform_calc_call.parameters = Some(vec![Parameter {
        name: "request".into(),
        type_name: "any".into(),
    }]);
    ts_findings.push(perform_calc_call);

    // invoke("start_periodic_events", { intervalMs: 1500 })
    let mut start_periodic_call = create_finding(
        "start_periodic_events",
        EntityType::Command,
        Behavior::Call,
        98,
    );
    start_periodic_call.parameters = Some(vec![Parameter {
        name: "intervalMs".into(),
        type_name: "number".into(),
    }]);
    ts_findings.push(start_periodic_call);

    // invoke("trigger_backend_listener", { message: "Internal Trigger" })
    let mut trigger_call = create_finding(
        "trigger_backend_listener",
        EntityType::Command,
        Behavior::Call,
        113,
    );
    trigger_call.parameters = Some(vec![Parameter {
        name: "message".into(),
        type_name: "string".into(),
    }]);
    ts_findings.push(trigger_call);

    // emit("frontend-ping", "Hello from TS!")
    let mut ping_emit = create_finding("frontend-ping", EntityType::Event, Behavior::Emit, 103);
    ping_emit.return_type = Some("String".into());
    ts_findings.push(ping_emit);

    // emit("setup-complete", null)
    let setup_emit = create_finding("setup-complete", EntityType::Event, Behavior::Emit, 108);
    ts_findings.push(setup_emit);

    index.add_file(FileIndex {
        path: ts_path.clone(),
        findings: ts_findings,
    });

    // --- TypeScript findings (types.ts) - Interface definitions ---
    let mut types_findings: Vec<Finding> = Vec::new();

    let mut user_profile_iface = create_finding(
        "UserProfile",
        EntityType::Interface,
        Behavior::Definition,
        27,
    );
    user_profile_iface.fields = Some(vec![
        Parameter {
            name: "id".into(),
            type_name: "number".into(),
        },
        Parameter {
            name: "username".into(),
            type_name: "string".into(),
        },
        Parameter {
            name: "roles".into(),
            type_name: "Role[]".into(),
        },
        Parameter {
            name: "is_active".into(),
            type_name: "boolean".into(),
        },
    ]);
    types_findings.push(user_profile_iface);

    let mut calc_req_iface = create_finding(
        "CalculationRequest",
        EntityType::Interface,
        Behavior::Definition,
        34,
    );
    calc_req_iface.fields = Some(vec![
        Parameter {
            name: "operation".into(),
            type_name: "Operation".into(),
        },
        Parameter {
            name: "operand_a".into(),
            type_name: "number".into(),
        },
        Parameter {
            name: "operand_b".into(),
            type_name: "number".into(),
        },
    ]);
    types_findings.push(calc_req_iface);

    let mut calc_resp_iface = create_finding(
        "CalculationResponse",
        EntityType::Interface,
        Behavior::Definition,
        40,
    );
    calc_resp_iface.fields = Some(vec![
        Parameter {
            name: "result".into(),
            type_name: "number".into(),
        },
        Parameter {
            name: "message".into(),
            type_name: "string".into(),
        },
        Parameter {
            name: "status".into(),
            type_name: "CalculationStatus".into(),
        },
    ]);
    types_findings.push(calc_resp_iface);

    index.add_file(FileIndex {
        path: types_path,
        findings: types_findings,
    });

    // --- Compute diagnostics for the frontend file ---
    let diagnostics =
        lsp_server::capabilities::diagnostics::compute_file_diagnostics(&ts_path, &index);

    // Print all diagnostics for debugging
    eprintln!("\n=== ALL DIAGNOSTICS FOR main.ts ===");
    for (i, d) in diagnostics.iter().enumerate() {
        eprintln!("  [{}] line {}: {}", i, d.range.start.line, d.message);
    }
    eprintln!("=== TOTAL: {} diagnostics ===\n", diagnostics.len());

    // --- Verify expected diagnostics ---

    // 1. status-update: listen<number> but Rust emits CalculationStatus
    //    Without bindings, custom type vs primitive should NOT produce mismatch
    let status_update_mismatches: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("status-update") && d.message.contains("mismatch"))
        .collect();
    assert!(
        status_update_mismatches.is_empty(),
        "Should NOT have payload mismatch for 'status-update' without bindings (CalculationStatus is custom), got: {:?}",
        status_update_mismatches.iter().map(|d| &d.message).collect::<Vec<_>>()
    );

    // 2. periodic-tick: listened but never emitted → structural warning
    let periodic_tick_warnings: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("periodic-tick"))
        .collect();
    assert!(
        !periodic_tick_warnings.is_empty(),
        "Expected 'listened for but never emitted' warning for 'periodic-tick'"
    );

    // 3. backend-pong with listen<string> vs Rust String → should NOT produce mismatch
    let pong_mismatches: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("backend-pong") && d.message.contains("mismatch"))
        .collect();
    assert!(
        pong_mismatches.is_empty(),
        "Should NOT have payload mismatch for 'backend-pong' (string vs String), got: {:?}",
        pong_mismatches
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // 4. get_user_profile return type → should NOT produce mismatch
    let user_profile_mismatches: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("get_user_profile") && d.message.contains("mismatch"))
        .collect();
    assert!(
        user_profile_mismatches.is_empty(),
        "Should NOT have return type mismatch for 'get_user_profile', got: {:?}",
        user_profile_mismatches
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // 5. perform_calculation return type → should NOT produce mismatch
    let calc_resp_mismatches: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("perform_calculation") && d.message.contains("mismatch"))
        .collect();
    assert!(
        calc_resp_mismatches.is_empty(),
        "Should NOT have return type mismatch for 'perform_calculation', got: {:?}",
        calc_resp_mismatches
            .iter()
            .map(|d| &d.message)
            .collect::<Vec<_>>()
    );

    // 6. Count total false diagnostics (unexpected mismatches)
    let false_mismatches: Vec<_> = diagnostics
        .iter()
        .filter(|d| d.message.contains("mismatch"))
        .collect();

    if !false_mismatches.is_empty() {
        eprintln!("\n=== FALSE POSITIVE DIAGNOSTICS ===");
        for d in &false_mismatches {
            eprintln!("  FALSE: line {}: {}", d.range.start.line, d.message);
        }
        panic!(
            "Found {} false positive mismatch diagnostics!",
            false_mismatches.len()
        );
    }
}
