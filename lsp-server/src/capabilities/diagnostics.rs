//! Diagnostics capability
//!
//! Computes diagnostics (warnings) for Tauri commands and events

use crate::indexer::{DiagnosticInfo, IndexKey, ProjectIndex};
use crate::syntax::{
    compare_types, map_rust_type_to_ts, should_rename_to_camel, snake_to_camel, Behavior,
    EntityType, TypeMatch,
};
use std::path::PathBuf;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity};

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
            // 1. Structural checks (undefined/unused)
            if let Some(diag) =
                check_structural_diagnostics(key, &info, loc, first_call, first_emit)
            {
                diagnostics.push(diag);
            }

            // 2. Type checking for Call (frontend invoke)
            if loc.behavior == Behavior::Call && info.has_definition() {
                let definition = locations
                    .iter()
                    .find(|l| l.behavior == Behavior::Definition);

                if let Some(def) = definition {
                    diagnostics.extend(check_parameters_diagnostics(key, loc, def, project_index));
                    diagnostics.extend(check_return_type_diagnostics(key, loc, def, project_index));
                }
            }
        }
    }

    diagnostics
}

fn check_structural_diagnostics(
    key: &IndexKey,
    info: &DiagnosticInfo,
    loc: &crate::indexer::LocationInfo,
    first_call: Option<tower_lsp_server::ls_types::Range>,
    first_emit: Option<tower_lsp_server::ls_types::Range>,
) -> Option<Diagnostic> {
    let msg = match loc.behavior {
        // Show on Definition if command never called
        Behavior::Definition if key.entity == EntityType::Command && !info.has_calls() => Some((
            DiagnosticSeverity::WARNING,
            format!(
                "Command '{}' is defined but never invoked in frontend",
                key.name
            ),
        )),
        // Show on FIRST Call only if command not defined
        Behavior::Call if !info.has_definition() => {
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

    msg.map(|(severity, message)| Diagnostic {
        range: loc.range,
        severity: Some(severity),
        source: Some("tarus".to_string()),
        message,
        ..Default::default()
    })
}

fn check_parameters_diagnostics(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    def: &crate::indexer::LocationInfo,
    project_index: &crate::indexer::ProjectIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // 1. Check if we have external bindings for this command
    if let Some(binding) = project_index.bindings_cache.get(&key.name) {
        if let Some(ts_params) = &loc.parameters {
            // Check for unexpected TS parameters
            for ts_p in ts_params {
                let found = binding.args.iter().find(|bp| bp.name == ts_p.name);

                if let Some(bp) = found {
                    if ts_p.type_name != "any" {
                        // Compare TS usage type vs Binding TS type
                        // Bindings already have TS types, so recursive check might need adjustment or just work if types match
                        // But binding.args[].type_name is TS type. recursive_type_check expects Rust type as first arg usually.
                        // However, recursive_type_check calls compare_types(rust, ts).
                        // If binding type is "MyInterface" and usage is "{ x: 1 }", compare_types("MyInterface", "{...}") -> Mismatch.
                        // recursive_type_check then tries to look up "MyInterface" as Rust struct.
                        // But "MyInterface" is TS type.
                        // We need to know the underlying Rust type for the binding if possible.
                        // But binding entry only knows TS type.
                        // So for bindings, we can only do shallow check unless we map TS type back to Rust type.
                        // For now, let's keep basic compare_types for bindings, OR use recursive_type_check assuming direct match.
                        // If binding says "MyStruct", it likely maps to Rust "MyStruct".
                        let result =
                            recursive_type_check(project_index, &bp.type_name, &ts_p.type_name);
                        if let TypeMatch::Mismatch(detail) = result {
                            diagnostics.push(Diagnostic {
                                range: loc.range,
                                severity: Some(DiagnosticSeverity::WARNING),
                                source: Some("tarus".to_string()),
                                message: format!(
                                    "Type mismatch for argument '{}': {detail}",
                                    ts_p.name
                                ),
                                ..Default::default()
                            });
                        }
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

            // Check for missing required parameters
            for bp in &binding.args {
                let is_optional =
                    bp.type_name.contains("undefined") || bp.type_name.contains("null");

                if !is_optional && !ts_params.iter().any(|p| p.name == bp.name) {
                    diagnostics.push(Diagnostic {
                        range: loc.range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("tarus".to_string()),
                        message: format!(
                            "Missing required argument '{}' for command '{}'",
                            bp.name, key.name
                        ),
                        ..Default::default()
                    });
                }
            }
        }
        return diagnostics;
    }

    // 2. Fallback: Use Rust definition
    if let (Some(ts_params), Some(rust_params)) = (&loc.parameters, &def.parameters) {
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
            let found = filtered_rust_params
                .iter()
                .find(|rp| snake_to_camel(&rp.name) == ts_p.name || rp.name == ts_p.name);

            if let Some(rp) = found {
                if ts_p.type_name != "any" {
                    // Use recursive check for deep validation
                    let result =
                        recursive_type_check(project_index, &rp.type_name, &ts_p.type_name);
                    if let TypeMatch::Mismatch(detail) = result {
                        diagnostics.push(Diagnostic {
                            range: loc.range,
                            severity: Some(DiagnosticSeverity::WARNING),
                            source: Some("tarus".to_string()),
                            message: format!(
                                "Type mismatch for argument '{}': {detail}",
                                ts_p.name
                            ),
                            ..Default::default()
                        });
                    }
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

    diagnostics
}

fn check_return_type_diagnostics(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    def: &crate::indexer::LocationInfo,
    project_index: &ProjectIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if let (Some(ts_ret), Some(rust_ret)) = (&loc.return_type, &def.return_type) {
        let ts_type = ts_ret.trim_start_matches('<').trim_end_matches('>').trim();
        let expected_ts_type = map_rust_type_to_ts(rust_ret);

        // Skip validation for:
        // - "any" in TS (user opts out)
        if ts_type == "any" {
            return diagnostics;
        }

        // Check if it's a custom struct type
        let struct_locs = project_index.get_locations(EntityType::Struct, rust_ret);
        if !struct_locs.is_empty() {
            // Try to find the interface and struct definitions
            let iface_locs = project_index.get_locations(EntityType::Interface, ts_type);

            let iface_def = iface_locs
                .iter()
                .find(|l| l.behavior == Behavior::Definition);
            let struct_def = struct_locs
                .iter()
                .find(|l| l.behavior == Behavior::Definition);

            if let (Some(id), Some(sd)) = (iface_def, struct_def) {
                if let (Some(iface_fields), Some(struct_fields)) = (&id.fields, &sd.fields) {
                    let rename_to_camel = should_rename_to_camel(sd.attributes.as_ref());

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
                            // Use deep type comparison for struct fields
                            if if_f.type_name != "any" {
                                let result = compare_types(&st_f.type_name, &if_f.type_name);
                                if let TypeMatch::Mismatch(detail) = result {
                                    diagnostics.push(Diagnostic {
                                        range: loc.range,
                                        severity: Some(DiagnosticSeverity::WARNING),
                                        source: Some("tarus".to_string()),
                                        message: format!(
                                            "Type mismatch for field '{}' in return type '{}': {detail}",
                                            if_f.name, ts_type
                                        ),
                                        ..Default::default()
                                    });
                                }
                            }
                        }
                    }
                }
            }
            return diagnostics;
        }

        // Check if it's an enum type
        let enum_locs = project_index.get_locations(EntityType::Enum, rust_ret);
        if !enum_locs.is_empty() {
            // Enum return type: just check that TS type name matches
            if ts_type != rust_ret && ts_type != expected_ts_type {
                diagnostics.push(Diagnostic {
                    range: loc.range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("tarus".to_string()),
                    message: format!(
                        "Return type mismatch for command '{}': expected {}, got {}",
                        key.name, rust_ret, ts_type
                    ),
                    ..Default::default()
                });
            }
            return diagnostics;
        }

        // Use deep comparison for primitive and container types
        let result = compare_types(rust_ret, ts_type);
        if let TypeMatch::Mismatch(detail) = result {
            diagnostics.push(Diagnostic {
                range: loc.range,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("tarus".to_string()),
                message: format!("Return type mismatch for command '{}': {detail}", key.name),
                ..Default::default()
            });
        }
    }

    diagnostics
}

/// Recursively check if a Rust type matches a TS type representation
fn recursive_type_check(
    project_index: &ProjectIndex,
    rust_type: &str,
    ts_type_repr: &str,
) -> TypeMatch {
    // 1. Basic check first
    let basic_match = compare_types(rust_type, ts_type_repr);
    if matches!(basic_match, TypeMatch::Exact | TypeMatch::Compatible) {
        return basic_match;
    }

    // 2. If TS type is an object literal "{ key: type, ... }", check against Rust struct
    if ts_type_repr.starts_with('{') && ts_type_repr.ends_with('}') {
        // Look up Rust struct definition
        let struct_name = crate::syntax::extract_result_ok_type(rust_type);
        // Handle Option<MyStruct> wrapper
        let rust_base = if struct_name.starts_with("Option<") {
            &struct_name[7..struct_name.len() - 1]
        } else {
            struct_name
        };

        let struct_locs = project_index.get_locations(EntityType::Struct, rust_base);
        if let Some(def) = struct_locs
            .iter()
            .find(|l| l.behavior == Behavior::Definition)
        {
            if let Some(fields) = &def.fields {
                let ts_fields = parse_ts_object_string(ts_type_repr);
                let should_rename = should_rename_to_camel(def.attributes.as_ref());

                for field in fields {
                    let field_name = if should_rename {
                        snake_to_camel(&field.name)
                    } else {
                        field.name.clone()
                    };

                    match ts_fields.get(&field_name) {
                        Some(ts_field_type) => {
                            // Recursively check field type
                            let match_result = recursive_type_check(
                                project_index,
                                &field.type_name,
                                ts_field_type,
                            );
                            if let TypeMatch::Mismatch(msg) = match_result {
                                return TypeMatch::Mismatch(format!(
                                    "field '{}': {}",
                                    field_name, msg
                                ));
                            }
                        }
                        None => {
                            // Field missing in TS object.
                            // However, we only warn if strict checking is enabled or if field is not Option.
                            // Currently we assume required unless explicit Option.
                            if !field.type_name.starts_with("Option<") {
                                return TypeMatch::Mismatch(format!(
                                    "missing required field '{}'",
                                    field_name
                                ));
                            }
                        }
                    }
                }

                return TypeMatch::Exact;
            }
        }
    }

    // Default to strict mismatch if structure analysis failed
    basic_match
}

/// Simple parser for "{ key: value, ... }" string produced by extractors.rs
fn parse_ts_object_string(s: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    let content = s.trim_start_matches('{').trim_end_matches('}').trim();
    if content.is_empty() {
        return map;
    }

    // Split by comma, but be careful about nested braces.
    let mut depth = 0;
    let mut current_field = String::new();

    for c in content.chars() {
        match c {
            '{' => {
                depth += 1;
                current_field.push(c);
            }
            '}' => {
                depth -= 1;
                current_field.push(c);
            }
            ',' if depth == 0 => {
                if !current_field.trim().is_empty() {
                    parse_kv_pair(&current_field, &mut map);
                }
                current_field.clear();
            }
            _ => current_field.push(c),
        }
    }
    if !current_field.trim().is_empty() {
        parse_kv_pair(&current_field, &mut map);
    }

    map
}

fn parse_kv_pair(s: &str, map: &mut std::collections::HashMap<String, String>) {
    if let Some(idx) = s.find(':') {
        let key = s[..idx].trim().to_string();
        let value = s[idx + 1..].trim().to_string();
        map.insert(key, value);
    }
}
