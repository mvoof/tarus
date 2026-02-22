//! Diagnostics capability
//!
//! Computes diagnostics (warnings) for Tauri commands and events

use crate::indexer::{DiagnosticInfo, IndexKey, ProjectIndex};
use crate::syntax::{
    apply_rename_all, camel_to_snake, compare_types, get_base_rust_type, is_primitive_rust_type,
    parse_ts_object_string, Behavior, EntityType, RustTypeInfo, RustTypeKind, TypeMatch,
};
use std::path::PathBuf;
use tower_lsp_server::ls_types::{Diagnostic, DiagnosticSeverity};

const TS_PRIMITIVE_TYPES: &[&str] = &[
    "string", "number", "boolean", "void", "null", "undefined", "never",
];

fn is_ts_primitive(ts_type: &str) -> bool {
    TS_PRIMITIVE_TYPES.contains(&ts_type)
}

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

/// Create a hint diagnostic — naming suggestion, not a structural mismatch
fn type_name_hint(
    range: tower_lsp_server::ls_types::Range,
    ts_type: &str,
    rust_type: &str,
) -> Diagnostic {
    use tower_lsp_server::ls_types::NumberOrString;
    // Build the replacement: keep array suffix if present (e.g. "SimpleUser[]" → "SimpleUser1[]")
    let ts_base = ts_type.trim_end_matches("[]");
    let suffix = if ts_type.ends_with("[]") { "[]" } else { "" };
    let replacement = format!("{rust_type}{suffix}");
    Diagnostic {
        range,
        severity: Some(DiagnosticSeverity::HINT),
        source: Some("tarus".to_string()),
        message: format!(
            "TypeScript type '{ts_type}' differs from Rust type '{rust_type}'. Consider renaming to match."
        ),
        code: Some(NumberOrString::String("tarus/return-type-name".to_string())),
        data: Some(serde_json::json!({
            "rustType": rust_type,
            "tsType": ts_base,
            "replacement": replacement
        })),
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

    // Deduplicate: file_map stores one entry per call site, but diagnostics
    // should process each unique key only once to avoid N×N duplicates.
    let keys: Vec<IndexKey> = {
        let raw = match project_index.file_map.get(path) {
            Some(k) => k.value().clone(),
            None => return diagnostics,
        };
        let mut seen = std::collections::HashSet::new();
        raw.into_iter().filter(|k| seen.insert(k.clone())).collect()
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
    _key: &IndexKey,
    loc: &crate::indexer::LocationInfo,
    def: &crate::indexer::LocationInfo,
    project_index: &crate::indexer::ProjectIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // Use native Rust parameter types from the definition
    if let (Some(ts_params), Some(rust_params)) = (&loc.parameters, &def.parameters) {
        for ts_p in ts_params {
            // Map camelCase TS argument name to snake_case Rust parameter name
            let rust_name = camel_to_snake(&ts_p.name);
            let found = rust_params
                .iter()
                .find(|rp| rp.name == rust_name || rp.name == ts_p.name);

            if let Some(rust_p) = found {
                // Skip Tauri-injected parameters
                if ["State", "AppHandle", "Window"]
                    .iter()
                    .any(|&s| rust_p.type_name.contains(s))
                {
                    continue;
                }

                if ts_p.type_name != "any"
                    && is_safe_to_compare(
                        &rust_p.type_name,
                        &ts_p.type_name,
                        project_index,
                    )
                {
                    let result =
                        recursive_type_check(project_index, &rust_p.type_name, &ts_p.type_name);
                    if let TypeMatch::Mismatch(detail) = result {
                        diagnostics.push(warning(
                            loc.range,
                            format!("Type mismatch for argument '{}': {detail}", ts_p.name),
                        ));
                    }
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

        // Skip: "any" / "void" — user is opting out of return type checking
        if ts_type == "any" || ts_type == "void" {
            return diagnostics;
        }

        if is_safe_to_compare(rust_ret, ts_type, project_index) {
            let result = compare_types(rust_ret, ts_type);
            if let TypeMatch::Mismatch(detail) = result {
                // Check whether both sides are named types that differ only in name
                let rust_base = get_base_rust_type(
                    crate::syntax::extract_result_ok_type(rust_ret)
                ).clone();
                let ts_base = ts_type.trim_end_matches("[]");
                let rust_is_known = project_index.rust_types.contains_key(&rust_base);
                let ts_is_named = !is_ts_primitive(ts_base) && !ts_base.starts_with('{');

                if rust_is_known && ts_is_named {
                    // Name-only mismatch: emit HINT with rename suggestion
                    diagnostics.push(type_name_hint(loc.range, ts_type, &rust_base));
                } else {
                    // Structural mismatch: emit WARNING
                    diagnostics.push(warning(
                        loc.range,
                        format!("Return type mismatch for command '{}': {detail}", key.name),
                    ));
                }
            }
        }
    }

    diagnostics
}

/// Compare Rust struct fields against a TypeScript object literal `{ key: type, ... }`
fn compare_struct_fields_to_ts_object(
    type_info: &RustTypeInfo,
    ts_type_repr: &str,
    project_index: &ProjectIndex,
) -> TypeMatch {
    if !ts_type_repr.starts_with('{') || !ts_type_repr.ends_with('}') {
        // Not an object literal; name-based matching already handled upstream
        return TypeMatch::Compatible;
    }

    let ts_fields = parse_ts_object_string(ts_type_repr);
    let rename_strategy = type_info.serde.rename_all.as_deref();

    for field in &type_info.fields {
        // Determine the serialized field name (serde rename overrides rename_all)
        let serialized_name = if let Some(strategy) = rename_strategy {
            apply_rename_all(&field.name, strategy)
        } else {
            field.name.clone()
        };

        let is_optional = field.type_name.starts_with("Option<");

        match ts_fields.get(&serialized_name) {
            Some(ts_field_type) => {
                if ts_field_type != "any" {
                    let result =
                        recursive_type_check(project_index, &field.type_name, ts_field_type);
                    if let TypeMatch::Mismatch(msg) = result {
                        return TypeMatch::Mismatch(format!("field '{serialized_name}': {msg}"));
                    }
                }
            }
            None => {
                if !is_optional {
                    return TypeMatch::Mismatch(format!(
                        "missing required field '{serialized_name}'"
                    ));
                }
            }
        }
    }

    TypeMatch::Exact
}

/// Check if a TypeScript type matches an enum based on its serde representation
fn check_enum_matches_ts(
    type_info: &RustTypeInfo,
    ts_type: &str,
    _project_index: &ProjectIndex,
) -> TypeMatch {
    // Untagged enums are too complex to verify without full type inference
    if type_info.serde.untagged {
        return TypeMatch::Compatible;
    }

    let has_tag = type_info.serde.tag.is_some();
    let has_content = type_info.serde.content.is_some();

    if has_tag && has_content {
        // Adjacent representation: { tag: "Variant", content: ... }
        // Too complex to verify without knowing all variant payloads
        return TypeMatch::Compatible;
    }

    if has_tag {
        // Internal representation: { tag: "Variant", ...fields }
        // Too complex to verify without full payload analysis
        return TypeMatch::Compatible;
    }

    // External representation (default)
    // Unit variants serialize as "VariantName" strings
    // Check if ts_type is a string literal matching a variant
    let ts = ts_type.trim();
    if ts.starts_with('"') && ts.ends_with('"') && ts.len() >= 2 {
        let variant_name = &ts[1..ts.len() - 1];
        let rename_all = type_info.serde.rename_all.as_deref();

        for variant in &type_info.variants {
            if variant.serde_skip {
                continue;
            }
            let serialized = variant.serde_rename.as_deref().unwrap_or_else(|| {
                // We can't return an owned String here, so just compare name directly
                // The rename_all case is handled separately below
                &variant.name
            });

            if serialized == variant_name {
                return TypeMatch::Compatible;
            }

            // Also check with rename_all applied
            if let Some(strategy) = rename_all {
                let renamed = apply_rename_all(&variant.name, strategy);
                if renamed == variant_name {
                    return TypeMatch::Compatible;
                }
            }
        }

        return TypeMatch::Mismatch(format!(
            "'{variant_name}' is not a variant of this enum"
        ));
    }

    // For object literals or type names, treat as compatible
    TypeMatch::Compatible
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

    // 2. Native rust_types registry — check struct/enum fields
    {
        let rust_base_name = get_base_rust_type(rust_type);
        if let Some(type_info) = project_index.rust_types.get(&rust_base_name) {
            return match &type_info.kind {
                RustTypeKind::Struct => {
                    compare_struct_fields_to_ts_object(&type_info, ts_type_repr, project_index)
                }
                RustTypeKind::Enum => {
                    check_enum_matches_ts(&type_info, ts_type_repr, project_index)
                }
            };
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
    let rust_base = crate::syntax::extract_result_ok_type(rust_type);
    let rust_base_clean = crate::syntax::get_base_rust_type(rust_base);

    let is_rust_primitive = is_primitive_rust_type(rust_base);
    let is_ts_primitive = is_ts_primitive(ts_type);

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

    // Case 3: Native rust_types registry has full type info
    if let Some(type_info) = project_index.rust_types.get(&rust_base_clean) {
        return matches!(
            type_info.kind,
            RustTypeKind::Struct | RustTypeKind::Enum
        ); // Always safe: we have full type info
    }

    // Default: Unsafe to compare (might be Enum, Alias, or unknown)
    false
}
