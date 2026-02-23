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
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Diagnostic, DiagnosticSeverity};

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
                // Show on Definition if command never called
                Behavior::Definition if !info.has_calls => Some((
                    DiagnosticSeverity::WARNING,
                    format!(
                        "Command '{}' is defined but never invoked in frontend",
                        key.name
                    ),
                )),
                // Show on FIRST Call/SpectaCall only if command not defined
                Behavior::Call | Behavior::SpectaCall if !info.has_definition => {
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
                Behavior::Listen if !info.has_emitters => Some((
                    DiagnosticSeverity::WARNING,
                    format!("Event '{}' is listened for but never emitted", key.name),
                )),
                // Show on FIRST Emit only if event never listened
                Behavior::Emit if !info.has_listeners => {
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
                diagnostics.push(Diagnostic {
                    range: loc.range,
                    severity: Some(severity),
                    source: Some("tarus".to_string()),
                    message,
                    ..Default::default()
                });
            }

            // --- Layer 2: Type diagnostics (only when binding files are present) ---
            if has_bindings {
                if let Some(d) = check_param_keys(loc, &key.name, project_index) {
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
    let actual: std::collections::HashSet<&str> =
        call_keys.iter().map(String::as_str).collect();
    let expected_set: std::collections::HashSet<&str> = expected.iter().copied().collect();

    // Missing required params (present in schema, absent in call)
    let missing: Vec<&str> = expected
        .iter()
        .copied()
        .filter(|e| !actual.contains(e))
        .collect();

    if !missing.is_empty() {
        return Some(Diagnostic {
            range: loc.range,
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("tarus".to_string()),
            message: format!(
                "invoke('{}') is missing required argument{}: {}",
                command_name,
                if missing.len() == 1 { "" } else { "s" },
                missing.join(", ")
            ),
            ..Default::default()
        });
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
        return Some(Diagnostic {
            range: loc.range,
            severity: Some(DiagnosticSeverity::WARNING),
            source: Some("tarus".to_string()),
            message: format!(
                "invoke('{}') has unexpected argument{}: {}",
                command_name,
                if sorted_extra.len() == 1 { "" } else { "s" },
                sorted_extra.join(", ")
            ),
            ..Default::default()
        });
    }

    None
}
