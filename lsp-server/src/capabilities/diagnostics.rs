//! Diagnostics capability
//!
//! Computes diagnostics (warnings) for Tauri commands and events

use crate::indexer::{DiagnosticInfo, IndexKey, ProjectIndex};
use crate::syntax::{
    compare_types, is_primitive_rust_type, parse_ts_object_string, should_rename_to_camel,
    snake_to_camel, Behavior, EntityType, TypeMatch,
};
use std::path::PathBuf;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity};

/// Create a warning diagnostic with "tarus" source
fn warning(range: tower_lsp_server::ls_types::Range, message: String) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::WARNING),
        source: Some("tarus".to_string()),
        message,
        ..Default::default()
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

            // 4. Type checking for Listen (frontend event listener)
            if loc.behavior == Behavior::Listen && info.has_emitters() {
                let emitters = locations
                    .iter()
                    .filter(|l| l.behavior == Behavior::Emit)
                    .collect::<Vec<_>>();

                for em in emitters {
                    diagnostics.extend(check_event_payload_diagnostics(
                        key,
                        loc,
                        em,
                        project_index,
                    ));
                }
            }

            // 5. Type checking for Emit (frontend event emitter)
            if loc.behavior == Behavior::Emit && info.has_listeners() {
                let listeners = locations
                    .iter()
                    .filter(|l| l.behavior == Behavior::Listen)
                    .collect::<Vec<_>>();

                for li in listeners {
                    // Reuse event payload check (order: listener, emitter)
                    diagnostics.extend(check_event_payload_diagnostics(
                        key,
                        li,
                        loc,
                        project_index,
                    ));
                }
            }

            // 3. Duplicate Type Detection for Interfaces
            if loc.behavior == Behavior::Definition && key.entity == EntityType::Interface {
                if let Some(diag) = check_duplicate_types(key, loc, project_index) {
                    diagnostics.push(diag);
                }
            }
        }
    }

    diagnostics
}

fn check_duplicate_types(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    project_index: &ProjectIndex,
) -> Option<Diagnostic> {
    let locations = project_index.get_locations(EntityType::Interface, &key.name);
    // If only one definition (this one), no conflict
    if locations.len() <= 1 {
        return None;
    }

    // Heuristic: generated files often located in bindings/ or named bindings.ts
    let current_path_str = loc.path.to_string_lossy();
    let current_is_generated = crate::indexer::is_generated_bindings_path(&current_path_str);

    // If current file IS generated, we don't warn here (we warn on the manual one)
    if current_is_generated {
        return None;
    }

    let conflict = locations.iter().find(|l| {
        l.path != loc.path && crate::indexer::is_generated_bindings_path(&l.path.to_string_lossy())
    });

    if let Some(conflict_loc) = conflict {
        let file_name = conflict_loc
            .path
            .file_name()
            .map_or_else(|| "generated file".into(), |n| n.to_string_lossy());

        return Some(warning(
            loc.range,
            format!(
                "Type '{}' is also defined in generated file '{}'. This may cause 'Duplicate identifier' errors.",
                key.name, file_name
            ),
        ));
    }
    None
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
        Behavior::Definition if key.entity == EntityType::Command && !info.has_calls() => {
            Some(format!(
                "Command '{}' is defined but never invoked in frontend",
                key.name
            ))
        }
        // Show on FIRST Call only if command not defined
        Behavior::Call if !info.has_definition() => {
            if first_call == Some(loc.range) {
                Some(format!(
                    "Command '{}' is not defined in Rust backend",
                    key.name
                ))
            } else {
                None
            }
        }
        // Show on Listen if event never emitted
        Behavior::Listen if !info.has_emitters() => Some(format!(
            "Event '{}' is listened for but never emitted",
            key.name
        )),
        // Show on FIRST Emit only if event never listened
        Behavior::Emit if !info.has_listeners() => {
            if first_emit == Some(loc.range) {
                Some(format!(
                    "Event '{}' is emitted but no listeners found",
                    key.name
                ))
            } else {
                None
            }
        }
        _ => None,
    };

    msg.map(|message| warning(loc.range, message))
}

fn check_parameters_diagnostics(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    _def: &crate::indexer::LocationInfo,
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
                            diagnostics.push(warning(
                                loc.range,
                                format!("Type mismatch for argument '{}': {detail}", ts_p.name),
                            ));
                        }
                    }
                } else {
                    diagnostics.push(warning(
                        loc.range,
                        format!(
                            "Command '{}' does not expect argument '{}'",
                            key.name, ts_p.name
                        ),
                    ));
                }
            }

            // Check for missing required parameters
            for bp in &binding.args {
                let is_optional =
                    bp.type_name.contains("undefined") || bp.type_name.contains("null");

                if !is_optional && !ts_params.iter().any(|p| p.name == bp.name) {
                    diagnostics.push(warning(
                        loc.range,
                        format!(
                            "Missing required argument '{}' for command '{}'",
                            bp.name, key.name
                        ),
                    ));
                }
            }
        }
        return diagnostics;
    }

    // Without external bindings, no parameter type diagnostics
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

        // Skip validation for:
        // - "any" in TS (user opts out)
        if ts_type == "any" {
            return diagnostics;
        }

        // Check if we have an external type definition from types_cache
        if let Some(ext_type) = project_index.types_cache.get(ts_type) {
            // If the external type has fields, compare against Rust struct fields
            if let (Some(ext_fields), Some(struct_def)) = (
                &ext_type.fields,
                project_index
                    .get_locations(EntityType::Struct, rust_ret)
                    .iter()
                    .find(|l| l.behavior == Behavior::Definition)
                    .and_then(|l| l.fields.as_ref()),
            ) {
                let rename_to_camel = should_rename_to_camel(
                    project_index
                        .get_locations(EntityType::Struct, rust_ret)
                        .iter()
                        .find(|l| l.behavior == Behavior::Definition)
                        .and_then(|l| l.attributes.as_ref()),
                );

                for ext_f in ext_fields {
                    let st_f = struct_def.iter().find(|f| {
                        if rename_to_camel {
                            snake_to_camel(&f.name) == ext_f.name
                        } else {
                            f.name == ext_f.name
                        }
                    });

                    if let Some(st_f) = st_f {
                        if ext_f.type_name != "any" {
                            let result = compare_types(&st_f.type_name, &ext_f.type_name);
                            if let TypeMatch::Mismatch(detail) = result {
                                diagnostics.push(warning(
                                    loc.range,
                                    format!(
                                        "Type mismatch for field '{}' in return type '{}' (from {}): {detail}",
                                        ext_f.name,
                                        ts_type,
                                        format!("{:?}", ext_type.source).to_lowercase()
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
            return diagnostics;
        }

        // Without external bindings: report mismatches if we can safely compare
        if is_safe_to_compare(rust_ret, ts_type, project_index) {
            let result = compare_types(rust_ret, ts_type);
            if let TypeMatch::Mismatch(detail) = result {
                diagnostics.push(warning(
                    loc.range,
                    format!("Return type mismatch for command '{}': {detail}", key.name),
                ));
            }
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

    // 2. If TS type is an object literal "{ key: type, ... }", check against external type definitions
    if ts_type_repr.starts_with('{') && ts_type_repr.ends_with('}') {
        let rust_base_name = crate::syntax::extract_result_ok_type(rust_type);
        let base = if rust_base_name.starts_with("Option<") {
            &rust_base_name[7..rust_base_name.len() - 1]
        } else {
            rust_base_name
        };

        if let Some(ext_type) = project_index.types_cache.get(base) {
            if let Some(ext_fields) = &ext_type.fields {
                let ts_fields = parse_ts_object_string(ts_type_repr);

                for ext_f in ext_fields {
                    match ts_fields.get(&ext_f.name) {
                        Some(ts_field_type) => {
                            let result = compare_types(&ext_f.type_name, ts_field_type);
                            if let TypeMatch::Mismatch(msg) = result {
                                return TypeMatch::Mismatch(format!(
                                    "field '{}': {msg}",
                                    ext_f.name
                                ));
                            }
                        }
                        None => {
                            if !ext_f.type_name.contains("null")
                                && !ext_f.type_name.contains("undefined")
                            {
                                return TypeMatch::Mismatch(format!(
                                    "missing required field '{}'",
                                    ext_f.name
                                ));
                            }
                        }
                    }
                }

                return TypeMatch::Exact;
            }
        }
    }

    // 3. Check types_cache by name match (when TS side uses type name directly)
    if let Some(ext_type) = project_index.types_cache.get(ts_type_repr) {
        // If the Rust type name matches the TS type name (possibly with rename),
        // consider it compatible
        let rust_base = crate::syntax::get_base_rust_type(rust_type);
        if ext_type.ts_name == rust_base || ext_type.ts_name == ts_type_repr {
            return TypeMatch::Compatible;
        }
    }

    // Default to strict mismatch if structure analysis failed
    basic_match
}

fn check_event_payload_diagnostics(
    key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    em: &crate::indexer::LocationInfo,
    project_index: &ProjectIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if let (Some(ts_payload), Some(rust_payload)) = (&loc.return_type, &em.return_type) {
        let ts_type = ts_payload
            .trim_start_matches('<')
            .trim_end_matches('>')
            .trim();

        // Skip validation for:
        // - "any" in TS (user opts out)
        if ts_type == "any" {
            return diagnostics;
        }

        // Without external bindings: report mismatches if we can safely compare
        if is_safe_to_compare(rust_payload, ts_type, project_index) {
            let result = compare_types(rust_payload, ts_type);
            if let TypeMatch::Mismatch(detail) = result {
                diagnostics.push(warning(
                    loc.range,
                    format!("Payload type mismatch for event '{}': {detail}", key.name),
                ));
            }
        }
    }

    diagnostics
}

/// Check if it is safe to compare types without external bindings.
///
/// We can safely compare if:
/// 1. Both sides are primitives (e.g., u8 vs number).
/// 2. Rust side is a known Struct and TS side is a Primitive (definite mismatch, as Structs are objects).
///
/// We avoid comparing if Rust side is an Enum (unless we have bindings), as Enums could
/// serialize to strings, numbers, or objects.
fn is_safe_to_compare(rust_type: &str, ts_type: &str, project_index: &ProjectIndex) -> bool {
    const TS_PRIMITIVES: &[&str] = &[
        "string",
        "number",
        "boolean",
        "void",
        "null",
        "undefined",
        "never",
    ];

    let rust_base = crate::syntax::extract_result_ok_type(rust_type);
    let rust_base_clean = crate::syntax::get_base_rust_type(rust_base);

    let is_rust_primitive = is_primitive_rust_type(rust_base);
    let is_ts_primitive = TS_PRIMITIVES.contains(&ts_type);

    // Case 1: Both are primitives -> Safe to compare
    if is_rust_primitive && is_ts_primitive {
        return true;
    }

    // Case 2: Rust is a Struct (Object) vs TS Primitive -> Safe mismatch
    // We check if the Rust type is indexed as a Struct
    // Note: get_locations returns a list, check if any definitions exist for Struct
    if !project_index
        .get_locations(EntityType::Struct, &rust_base_clean)
        .is_empty()
        && is_ts_primitive
    {
        return true;
    }

    // Default: Unsafe to compare (might be Enum, Alias, or unknown)
    false
}
