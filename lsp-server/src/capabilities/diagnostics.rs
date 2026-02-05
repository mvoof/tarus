//! Diagnostics capability
//!
//! Computes diagnostics (warnings) for Tauri commands and events

use crate::indexer::{DiagnosticInfo, IndexKey, ProjectIndex};
use crate::syntax::{
    map_rust_type_to_ts, should_rename_to_camel, snake_to_camel, Behavior, EntityType,
};
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Diagnostic, DiagnosticSeverity};

#[allow(clippy::too_many_lines)]
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

    for key in &keys {
        let info: DiagnosticInfo = project_index.get_diagnostic_info(key);
        let locations = project_index.get_locations(key.entity, &key.name);

        // Filter locations to only those in current file
        let local_locations: Vec<_> = locations.iter().filter(|l| l.path == *path).collect();

        // Find first occurrence of each behavior type
        let first_call = local_locations
            .iter()
            .find(|l| matches!(l.behavior, Behavior::Call))
            .map(|l| l.range);
        let first_emit = local_locations
            .iter()
            .find(|l| matches!(l.behavior, Behavior::Emit))
            .map(|l| l.range);

        for loc in local_locations {
            // Determine if we should show diagnostic for this location
            let msg = match loc.behavior {
                // Show on Definition if command never called
                Behavior::Definition if key.entity == EntityType::Command && !info.has_calls => {
                    Some((
                        DiagnosticSeverity::WARNING,
                        format!(
                            "Command '{}' is defined but never invoked in frontend",
                            key.name
                        ),
                    ))
                }
                // Show on FIRST Call only if command not defined
                Behavior::Call if !info.has_definition => {
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

            // Additional type checking for Call (frontend invoke)
            if loc.behavior == Behavior::Call && info.has_definition {
                let definition = locations
                    .iter()
                    .find(|l| l.behavior == Behavior::Definition);

                if let Some(def) = definition {
                    // Check parameters
                    if let (Some(ts_params), Some(rust_params)) = (&loc.parameters, &def.parameters)
                    {
                        // Filter rust params that are likely backend-only
                        let filtered_rust_params: Vec<_> = rust_params
                            .iter()
                            .filter(|p| {
                                !["State", "AppHandle", "Window"]
                                    .iter()
                                    .any(|&s| p.type_name.contains(s))
                            })
                            .collect();

                        // Check for unexpected TS parameters
                        for ts_p in ts_params {
                            // Tauri automatically converts camelCase keys to snake_case for command arguments
                            let found = filtered_rust_params.iter().find(|rp| {
                                snake_to_camel(&rp.name) == ts_p.name || rp.name == ts_p.name
                            });

                            if let Some(rp) = found {
                                // Only check primitive types - skip custom types
                                let expected_ts_type = map_rust_type_to_ts(&rp.type_name);
                                // Skip if:
                                // - TS type is "any" (unknown/variable reference)
                                // - Rust type maps to "any" (custom type)
                                if ts_p.type_name != "any"
                                    && expected_ts_type != "any"
                                    && ts_p.type_name != expected_ts_type
                                {
                                    diagnostics.push(Diagnostic {
                                        range: loc.range,
                                        severity: Some(DiagnosticSeverity::WARNING),
                                        source: Some("tarus".to_string()),
                                        message: format!(
                                            "Type mismatch for argument '{}': expected {}, got {}",
                                            ts_p.name, expected_ts_type, ts_p.type_name
                                        ),
                                        ..Default::default()
                                    });
                                }
                            } else {
                                diagnostics.push(Diagnostic {
                                    range: loc.range,
                                    severity: Some(DiagnosticSeverity::WARNING),
                                    source: Some("tarus".to_string()),
                                    message: format!(
                                        "Command '{}' does not expect argument '{}'",
                                        key.name, ts_p.name
                                    ),
                                    ..Default::default()
                                });
                            }
                        }

                        // Check for missing required parameters in TS
                        for rp in &filtered_rust_params {
                            let camel_name = snake_to_camel(&rp.name);

                            if !ts_params
                                .iter()
                                .any(|p| p.name == camel_name || p.name == rp.name)
                            {
                                // Check if the Rust type is Option (optional parameter)
                                if !rp.type_name.contains("Option") {
                                    diagnostics.push(Diagnostic {
                                        range: loc.range,
                                        severity: Some(DiagnosticSeverity::WARNING),
                                        source: Some("tarus".to_string()),
                                        message: format!(
                                            "Missing required argument '{}' for command '{}'",
                                            camel_name, key.name
                                        ),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }

                    // Check return type if generic is provided in TS
                    if let (Some(ts_ret), Some(rust_ret)) = (&loc.return_type, &def.return_type) {
                        let ts_type = ts_ret.trim_start_matches('<').trim_end_matches('>').trim();
                        let expected_ts_type = map_rust_type_to_ts(rust_ret);

                        // Skip validation for:
                        // - "any" in TS (user opts out)
                        if ts_type == "any" {
                            continue;
                        }

                        // Check if it's a custom type (struct)
                        let struct_locs = project_index.get_locations(EntityType::Struct, rust_ret);
                        if !struct_locs.is_empty() {
                            // Try to find the interface and struct definitions
                            let iface_locs =
                                project_index.get_locations(EntityType::Interface, ts_type);

                            let iface_def = iface_locs
                                .iter()
                                .find(|l| l.behavior == Behavior::Definition);
                            let struct_def = struct_locs
                                .iter()
                                .find(|l| l.behavior == Behavior::Definition);

                            if let (Some(id), Some(sd)) = (iface_def, struct_def) {
                                if let (Some(iface_fields), Some(struct_fields)) =
                                    (&id.fields, &sd.fields)
                                {
                                    let rename_to_camel =
                                        should_rename_to_camel(sd.attributes.as_ref());

                                    for if_f in iface_fields {
                                        // Find corresponding field in struct
                                        let st_f = struct_fields.iter().find(|f| {
                                            if rename_to_camel {
                                                snake_to_camel(&f.name) == if_f.name
                                            } else {
                                                f.name == if_f.name
                                            }
                                        });

                                        if let Some(st_f) = st_f {
                                            let expected_f_type =
                                                map_rust_type_to_ts(&st_f.type_name);

                                            if if_f.type_name != "any"
                                                && expected_f_type != "any"
                                                && if_f.type_name != expected_f_type
                                            {
                                                diagnostics.push(Diagnostic {
                                                    range: loc.range,
                                                    severity: Some(DiagnosticSeverity::WARNING),
                                                    source: Some("tarus".to_string()),
                                                    message: format!(
                                                        "Type mismatch for field '{}' in return type '{}': expected {}, got {}",
                                                        if_f.name, ts_type, expected_f_type, if_f.type_name
                                                    ),
                                                    ..Default::default()
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                            continue;
                        }

                        // Only warn for primitive type mismatches
                        if ts_type != expected_ts_type {
                            diagnostics.push(Diagnostic {
                                range: loc.range,
                                severity: Some(DiagnosticSeverity::WARNING),
                                source: Some("tarus".to_string()),
                                message: format!(
                                    "Return type mismatch for command '{}': expected {}, got {}",
                                    key.name, expected_ts_type, ts_type
                                ),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
        }
    }

    diagnostics
}
