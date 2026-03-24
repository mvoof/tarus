//! Diagnostics capability
//!
//! Computes diagnostics (warnings) for Tauri commands and events.
//!
//! Two layers of diagnostics are provided:
//! 1. **Structural diagnostics** — always active: undefined commands/events, unused definitions.
//! 2. **Type diagnostics** — active ONLY when at least one binding file (ts-rs / tauri-specta /
//!    tauri-typegen) has been indexed. Uses `CommandSchema` sourced from those generators;
//!    `GeneratorKind::RustSource` schemas are intentionally excluded from type checking.

use crate::indexer::{DiagnosticInfo, GeneratorKind, IndexKey, LocationInfo, ProjectIndex};
use crate::syntax::Behavior;
use serde_json::json;
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Range};

/// Create a diagnostic with `tarus` source and optional code/data.
fn tarus_diagnostic(
    range: Range,
    severity: DiagnosticSeverity,
    message: String,
    code: Option<&str>,
    data: Option<serde_json::Value>,
) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(severity),
        source: Some("tarus".to_string()),
        code: code.map(|c| NumberOrString::String(c.to_string())),
        message,
        data,
        ..Default::default()
    }
}

/// Build JSON data payload for type-annotation code actions (shared by return-type and event-payload).
fn make_type_action_data(loc: &LocationInfo, expected: &str) -> serde_json::Value {
    let mut data = json!({ "expected": expected });
    if let Some(pos) = &loc.call_name_end {
        data["callNameEnd"] = json!({ "line": pos.line, "character": pos.character });
    }
    if let Some(r) = &loc.type_arg_range {
        data["typeArgRange"] = json!({
            "start": { "line": r.start.line, "character": r.start.character },
            "end": { "line": r.end.line, "character": r.end.character },
        });
    }
    data
}

/// Generic type-annotation check: handles the `None`/`Some(ts_type)` match on `loc.return_type`.
///
/// Returns a HINT when the generic is missing, or a WARNING when the type doesn't match.
fn check_type_annotation(
    loc: &LocationInfo,
    expected: &str,
    missing_code: &str,
    mismatch_code: &str,
    missing_msg: String,
    mismatch_msg: impl FnOnce(&str) -> String,
    project_index: &ProjectIndex,
) -> Option<Diagnostic> {
    let data = make_type_action_data(loc, expected);

    match &loc.return_type {
        None => Some(tarus_diagnostic(
            loc.range,
            DiagnosticSeverity::HINT,
            missing_msg,
            Some(missing_code),
            Some(data),
        )),
        Some(ts_type) => {
            if ts_type == "void" || ts_type == "any" {
                return None;
            }
            if types_match(ts_type, expected, project_index) {
                return None;
            }
            Some(tarus_diagnostic(
                loc.range,
                DiagnosticSeverity::WARNING,
                mismatch_msg(ts_type),
                Some(mismatch_code),
                Some(data),
            ))
        }
    }
}

/// Compute diagnostics for a specific file
pub fn compute_file_diagnostics(path: &PathBuf, project_index: &ProjectIndex) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // If file has parse errors, skip diagnostic generation
    // (errors are logged in developer mode only, not shown to user)
    // TS/Rust analyzer already shows syntax errors
    if project_index.get_parse_error(path).is_some() {
        return diagnostics;
    }

    let keys: Vec<IndexKey> = match project_index.file_map.get(path) {
        Some(k) => k.value().clone(),
        None => return diagnostics,
    };

    let has_bindings = project_index.has_bindings_files();

    for key in &keys {
        let info: DiagnosticInfo = project_index.get_diagnostic_info(key);
        let locations = project_index.get_locations(key.entity, &key.name);

        // Filter locations to only those in current file
        let local_locations: Vec<_> = locations.iter().filter(|l| l.path == *path).collect();

        // Find first occurrence of each behavior type
        let first_call = local_locations
            .iter()
            .find(|l| matches!(l.behavior, Behavior::Call | Behavior::SpectaCall))
            .map(|l| l.range);
        let first_emit = local_locations
            .iter()
            .find(|l| matches!(l.behavior, Behavior::Emit))
            .map(|l| l.range);

        for loc in &local_locations {
            // --- Layer 1: Structural diagnostics (always active) ---
            let msg = match loc.behavior {
                // Show on Definition if command/event never used
                Behavior::Definition => {
                    let (entity_label, usage_label, is_unused) = match key.entity {
                        crate::syntax::EntityType::Command => {
                            ("Command", "invoked in frontend", !info.has_calls())
                        }
                        crate::syntax::EntityType::Event => (
                            "Event",
                            "emitted or listened for",
                            !info.has_emitters() && !info.has_listeners(),
                        ),
                    };

                    if is_unused {
                        Some((
                            DiagnosticSeverity::WARNING,
                            format!(
                                "{entity_label} '{}' is defined but never {usage_label}",
                                key.name
                            ),
                        ))
                    } else {
                        None
                    }
                }
                // Show on FIRST Call/SpectaCall only if command not defined
                Behavior::Call | Behavior::SpectaCall if !info.has_definition() => {
                    if first_call == Some(loc.range) {
                        Some((
                            DiagnosticSeverity::WARNING,
                            format!("Command '{}' is not defined in Rust backend", key.name),
                        ))
                    } else {
                        None // Skip subsequent calls
                    }
                }
                // Show on Listen if event never emitted
                Behavior::Listen if !info.has_emitters() => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Event '{}' is listened for but never emitted", key.name),
                )),
                // Show on FIRST Emit only if event never listened
                Behavior::Emit if !info.has_listeners() => {
                    if first_emit == Some(loc.range) {
                        Some((
                            DiagnosticSeverity::WARNING,
                            format!("Event '{}' is emitted but no listeners found", key.name),
                        ))
                    } else {
                        None // Skip subsequent emits
                    }
                }
                _ => None,
            };

            if let Some((severity, message)) = msg {
                diagnostics.push(tarus_diagnostic(loc.range, severity, message, None, None));
            }

            // --- Layer 2: Type diagnostics (only when binding files are present) ---
            if has_bindings {
                if let Some(d) = check_param_keys(loc, &key.name, project_index) {
                    diagnostics.push(d);
                }
                if let Some(d) = check_arg_count(loc, &key.name, project_index) {
                    diagnostics.push(d);
                }
                if let Some(d) = check_return_type(loc, &key.name, project_index) {
                    diagnostics.push(d);
                }
                if let Some(d) = check_event_payload_type(loc, &key.name, project_index) {
                    diagnostics.push(d);
                }
            }
        }
    }

    diagnostics
}

/// Validate the argument keys passed to an `invoke()` call against the expected
/// parameters in the `CommandSchema` from a binding generator.
///
/// Only activates when:
/// - The location is a `Call` (i.e. `invoke("name", { key: val, ... })`)
/// - The call has `call_param_keys` recorded by the parser
/// - A `CommandSchema` exists for this command sourced from a binding generator
///   (`Specta`, `TsRs`, or `Typegen`) — **not** `RustSource`
///
/// Reports:
/// - `WARNING` for missing required parameters
/// - `WARNING` for unexpected (extra) parameters
fn check_param_keys(
    loc: &LocationInfo,
    command_name: &str,
    project_index: &ProjectIndex,
) -> Option<Diagnostic> {
    // Only validate regular invoke() calls that have recorded param keys
    if !matches!(loc.behavior, Behavior::Call) {
        return None;
    }

    let call_keys = loc.call_param_keys.as_ref()?;

    // Get schema — must come from a bindings generator, not from Rust source analysis
    let schema = project_index.get_schema(command_name)?;
    if matches!(schema.generator, GeneratorKind::RustSource) {
        return None;
    }

    // Build sets for comparison
    let expected: Vec<&str> = schema.params.iter().map(|p| p.name.as_str()).collect();
    let actual: std::collections::HashSet<&str> = call_keys.iter().map(String::as_str).collect();
    let expected_set: std::collections::HashSet<&str> = expected.iter().copied().collect();

    // Missing required params (present in schema, absent in call)
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|e| !actual.contains(e))
        .collect();

    if !missing.is_empty() {
        return Some(tarus_diagnostic(
            loc.range,
            DiagnosticSeverity::WARNING,
            format!(
                "invoke('{}') is missing required argument{}: {}",
                command_name,
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", ")
            ),
            None,
            None,
        ));
    }

    // Unexpected extra params (present in call, absent from schema)
    let extra: Vec<&str> = actual
        .iter()
        .copied()
        .filter(|a| !expected_set.contains(a))
        .collect();

    if !extra.is_empty() {
        let mut sorted_extra = extra;
        sorted_extra.sort_unstable();
        return Some(tarus_diagnostic(
            loc.range,
            DiagnosticSeverity::WARNING,
            format!(
                "invoke('{}') has unexpected argument{}: {}",
                command_name,
                if sorted_extra.len() == 1 { "" } else { "s" },
                sorted_extra.join(", ")
            ),
            None,
            None,
        ));
    }

    None
}

/// Validate the argument count of a `commands.methodName(...)` (`SpectaCall`) against the
/// expected parameter count in the `CommandSchema`.
///
/// Only activates for `SpectaCall` behavior with a binding-sourced schema.
///
/// Reports:
/// - `WARNING` for too few arguments
/// - `WARNING` for too many arguments
fn check_arg_count(
    loc: &LocationInfo,
    command_name: &str,
    project_index: &ProjectIndex,
) -> Option<Diagnostic> {
    if !matches!(loc.behavior, Behavior::SpectaCall) {
        return None;
    }

    let actual_count = loc.call_arg_count?;

    let schema = project_index.get_schema(command_name)?;
    if matches!(schema.generator, GeneratorKind::RustSource) {
        return None;
    }

    let expected_count = schema.params.len();

    if actual_count as usize == expected_count {
        return None;
    }

    let message = format!(
        "commands.{}() expected {} argument{} but got {}",
        command_name,
        expected_count,
        if expected_count == 1 { "" } else { "s" },
        actual_count
    );

    Some(tarus_diagnostic(
        loc.range,
        DiagnosticSeverity::WARNING,
        message,
        Some("tarus/arg-count-mismatch"),
        None,
    ))
}

/// Validate the return type of an `invoke()` call against the `CommandSchema`.
///
/// Two cases:
/// - **Missing generic**: `invoke("cmd")` when command returns non-void → HINT
/// - **Wrong generic**: `invoke<Wrong>("cmd")` when type doesn't match → WARNING
///
/// Only activates for `Call` behavior with a binding-sourced schema (not `RustSource`).
fn check_return_type(
    loc: &LocationInfo,
    command_name: &str,
    project_index: &ProjectIndex,
) -> Option<Diagnostic> {
    if !matches!(loc.behavior, Behavior::Call) {
        return None;
    }

    let schema = project_index.get_schema(command_name)?;

    // RustSource schemas are allowed for return type checks only when the return type
    // has a known binding (type alias from ts-rs/specta/typegen), giving us confidence.
    if matches!(schema.generator, GeneratorKind::RustSource)
        && !project_index.type_aliases.contains_key(&schema.return_type)
    {
        return None;
    }

    let expected = &schema.return_type;
    if expected == "void" {
        return None;
    }

    check_type_annotation(
        loc,
        expected,
        "tarus/return-type-missing",
        "tarus/return-type-mismatch",
        format!("invoke('{command_name}') is missing return type, expected '{expected}'"),
        |ts_type| {
            format!(
                "invoke<{ts_type}>('{command_name}') return type mismatch: expected '{expected}'"
            )
        },
        project_index,
    )
}

/// Validate the payload type of an `emit()` / `listen()` / `once()` call against the `EventSchema`.
///
/// Two cases:
/// - **Missing generic**: `listen("event")` when payload is non-void → HINT
/// - **Wrong generic**: `listen<Wrong>("event")` when type doesn't match → WARNING
fn check_event_payload_type(
    loc: &LocationInfo,
    event_name: &str,
    project_index: &ProjectIndex,
) -> Option<Diagnostic> {
    if !matches!(loc.behavior, Behavior::Emit | Behavior::Listen) {
        return None;
    }

    // Rust files don't use generic type parameters on emit/listen/once — skip
    if loc.path.extension().is_some_and(|ext| ext == "rs") {
        return None;
    }

    // Typed codegen APIs (e.g. specta events.X.listen) already provide type safety — skip
    if loc.codegen_origin.is_some() {
        return None;
    }

    let schema = project_index.get_event_schema(event_name)?;

    // RustSource schemas are allowed only when the payload type has a known binding
    if matches!(schema.generator, GeneratorKind::RustSource)
        && !project_index
            .type_aliases
            .contains_key(&schema.payload_type)
    {
        return None;
    }

    let expected = &schema.payload_type;
    if expected == "void" || expected == "null" {
        return None;
    }

    let behavior_label = match loc.behavior {
        Behavior::Emit => "emit",
        Behavior::Listen => "listen",
        _ => return None,
    };

    check_type_annotation(
        loc,
        expected,
        "tarus/event-payload-missing",
        "tarus/event-payload-mismatch",
        format!("{behavior_label}('{event_name}') is missing payload type, expected '{expected}'"),
        |ts_type| {
            format!(
                "{behavior_label}<{ts_type}>('{event_name}') payload type mismatch: expected '{expected}'"
            )
        },
        project_index,
    )
}

/// Check if two type strings match, considering type aliases.
pub fn types_match(ts_type: &str, expected: &str, project_index: &ProjectIndex) -> bool {
    if ts_type == expected {
        return true;
    }

    // Check if both resolve to the same type alias definition
    if let Some(expected_def) = project_index.type_aliases.get(expected) {
        if let Some(ts_def) = project_index.type_aliases.get(ts_type) {
            return expected_def.value() == ts_def.value();
        }
    }

    false
}
