//! Diagnostics capability
//!
//! Computes diagnostics (warnings) for Tauri commands and events

use crate::indexer::{DiagnosticInfo, IndexKey, ProjectIndex};
use crate::syntax::{
    apply_serde_rename, compare_types, map_rust_type_to_ts, parse_serde_attributes,
    parse_ts_enum_or_union, parse_ts_object_string, should_rename_to_camel, snake_to_camel,
    Behavior, EntityType, TsEnumRepresentation, TypeMatch,
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

    // Heuristic: generated files often named tauri-commands.d.ts or located in bindings/
    let current_path_str = loc.path.to_string_lossy();
    let current_is_generated = current_path_str.contains("tauri-commands.d.ts")
        || current_path_str.contains("bindings.d.ts");

    // If current file IS generated, we don't warn here (we warn on the manual one)
    if current_is_generated {
        return None;
    }

    let conflict = locations.iter().find(|l| {
        l.path != loc.path
            && (l.path.to_string_lossy().contains("tauri-commands.d.ts")
                || l.path.to_string_lossy().contains("bindings.d.ts"))
    });

    if let Some(conflict_loc) = conflict {
        let file_name = conflict_loc
            .path
            .file_name()
            .map_or_else(|| "generated file".into(), |n| n.to_string_lossy());

        return Some(Diagnostic {
             range: loc.range,
             severity: Some(DiagnosticSeverity::WARNING),
             source: Some("tarus".to_string()),
             message: format!(
                 "Type '{}' is also defined in generated file '{}'. This may cause 'Duplicate identifier' errors.",
                 key.name, file_name
             ),
             ..Default::default()
         });
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

#[allow(clippy::too_many_lines)]
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
                && !rp.type_name.contains("Option")
            {
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
        let _expected_ts_type = map_rust_type_to_ts(rust_ret);

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
                                diagnostics.push(Diagnostic {
                                    range: loc.range,
                                    severity: Some(DiagnosticSeverity::WARNING),
                                    source: Some("tarus".to_string()),
                                    message: format!(
                                        "Type mismatch for field '{}' in return type '{}' (from {}): {detail}",
                                        ext_f.name,
                                        ts_type,
                                        format!("{:?}", ext_type.source).to_lowercase()
                                    ),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }
            return diagnostics;
        }

        // Check if it's an enum type - use enhanced enum serialization validation
        if let Some(diag) = check_enum_serialization_match(
            rust_ret,
            ts_type,
            loc.range,
            project_index,
            Some(&format!("Return type mismatch for command '{}'", key.name)),
        ) {
            diagnostics.push(diag);
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
                                return TypeMatch::Mismatch(format!("field '{field_name}': {msg}"));
                            }
                        }
                        None => {
                            if !field.type_name.starts_with("Option<") {
                                return TypeMatch::Mismatch(format!(
                                    "missing required field '{field_name}'"
                                ));
                            }
                        }
                    }
                }

                return TypeMatch::Exact;
            }
        }

        // If struct not found in index, check types_cache for external type definitions
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

        // Try enum validation first
        if let Some(enum_diag) = check_enum_serialization_match(
            rust_payload,
            ts_type,
            loc.range,
            project_index,
            Some(&format!("Payload type mismatch for event '{}'", key.name)),
        ) {
            diagnostics.push(enum_diag);
            return diagnostics;
        }

        // Use recursive type checking for structs and deep comparison for other types
        let result = recursive_type_check(project_index, rust_payload, ts_type);
        if let TypeMatch::Mismatch(detail) = result {
            diagnostics.push(Diagnostic {
                range: loc.range,
                severity: Some(DiagnosticSeverity::WARNING),
                source: Some("tarus".to_string()),
                message: format!("Payload type mismatch for event '{}': {detail}", key.name),
                ..Default::default()
            });
        }
    }

    diagnostics
}

/// Check if a Rust enum matches a TypeScript enum/union, considering serde serialization
///
/// Returns Some(Diagnostic) if there's a mismatch, None if they match or if enum not found
#[allow(clippy::too_many_lines)]
fn check_enum_serialization_match(
    rust_type: &str,
    ts_type: &str,
    range: tower_lsp_server::ls_types::Range,
    project_index: &ProjectIndex,
    message_prefix: Option<&str>,
) -> Option<Diagnostic> {
    // Extract base type name (unwrap Vec, Option, Result, etc.)
    let rust_base = crate::syntax::get_base_rust_type(rust_type);

    // Look up Rust enum definition
    let enum_locs = project_index.get_locations(EntityType::Enum, &rust_base);
    let enum_def = enum_locs
        .iter()
        .find(|l| l.behavior == Behavior::Definition)?;

    // Parse serde attributes
    let serde_attrs = parse_serde_attributes(enum_def.attributes.as_ref());

    // Get enum variants (use legacy field for now, as variants field may not be populated yet)
    let variants = enum_def.fields.as_ref()?;

    // Try to parse TypeScript enum/union
    let ts_repr = parse_ts_enum_or_union(ts_type)?;

    let prefix = message_prefix.unwrap_or("Type mismatch");

    match ts_repr {
        TsEnumRepresentation::UnionType(ts_values) => {
            // Simple string literal union: "add" | "subtract"
            // Compare against Rust variant names with serde transformation

            if variants.len() != ts_values.len() {
                return Some(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("tarus".to_string()),
                    message: format!(
                        "{}: enum '{}' has {} variants, but TypeScript union has {} values",
                        prefix,
                        rust_base,
                        variants.len(),
                        ts_values.len()
                    ),
                    ..Default::default()
                });
            }

            // Check each variant
            for variant in variants {
                let expected_ts_name =
                    apply_serde_rename(&variant.name, serde_attrs.rename_all.as_deref());

                if !ts_values.contains(&expected_ts_name) {
                    return Some(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("tarus".to_string()),
                        message: format!(
                            "{}: enum variant '{}' should serialize to '{}', but it's not in TypeScript union",
                            prefix, variant.name, expected_ts_name
                        ),
                        ..Default::default()
                    });
                }
            }

            None // All variants match
        }

        TsEnumRepresentation::DiscriminatedUnion(ts_variants) => {
            // Discriminated union: { type: "Success" } | { type: "Error", data: {...} }
            // This requires serde(tag = "...", content = "...")

            if serde_attrs.tag.is_none() {
                return Some(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("tarus".to_string()),
                    message: format!(
                        "{prefix}: enum '{rust_base}' needs #[serde(tag = \"...\")] to match discriminated union"
                    ),
                    ..Default::default()
                });
            }

            if variants.len() != ts_variants.len() {
                return Some(Diagnostic {
                    range,
                    severity: Some(DiagnosticSeverity::WARNING),
                    source: Some("tarus".to_string()),
                    message: format!(
                        "{}: enum '{}' has {} variants, but TypeScript has {} union variants",
                        prefix,
                        rust_base,
                        variants.len(),
                        ts_variants.len()
                    ),
                    ..Default::default()
                });
            }

            // Check each Rust variant against TypeScript variants
            for variant in variants {
                let expected_tag =
                    apply_serde_rename(&variant.name, serde_attrs.rename_all.as_deref());

                let ts_variant = ts_variants.iter().find(|tsv| tsv.tag_value == expected_tag);

                if ts_variant.is_none() {
                    return Some(Diagnostic {
                        range,
                        severity: Some(DiagnosticSeverity::WARNING),
                        source: Some("tarus".to_string()),
                        message: format!(
                            "{}: enum variant '{}' with tag '{}' not found in TypeScript union",
                            prefix, variant.name, expected_tag
                        ),
                        ..Default::default()
                    });
                }

                // For struct/tuple variants, check if content field is present
                // This is a simplified check - full validation would require EnumVariant with fields
                // For now, just check consistency
            }

            None // All variants present
        }

        TsEnumRepresentation::StringEnum(_) => {
            // TypeScript string enum - less common with Tauri
            // Could add support later if needed
            None
        }
    }
}
