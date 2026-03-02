//! Parsers for generated TypeScript binding files
//!
//! Supports three generators:
//! - tauri-specta: `export const commands = { async methodName(...): Promise<...> { ... } }`
//! - ts-rs: `export type Name = { ... };`
//! - tauri-typegen: `export type Name = { ... };`

use crate::indexer::{CommandSchema, GeneratorKind, ParamSchema};
use crate::utils::camel_to_snake;
use std::collections::HashMap;
use std::path::Path;

/// Parse a Specta-generated bindings file and return a list of `CommandSchema`.
///
/// Expects lines like:
/// ```text
/// async getUserProfile(id: number): Promise<Result<UserProfile, string>> {
/// ```
/// inside an `export const commands = { ... }` block.
#[must_use]
pub fn parse_specta_bindings(content: &str, source_path: &Path) -> Vec<CommandSchema> {
    let mut schemas = Vec::new();
    let mut in_commands_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Detect start of commands object
        if trimmed.contains("export const commands") && trimmed.contains('{') {
            in_commands_block = true;
            continue;
        }

        // Detect end of commands object
        if in_commands_block && trimmed == "};" {
            break;
        }

        if !in_commands_block {
            continue;
        }

        // Try to parse a method line
        if let Some((camel_name, params, return_type)) = parse_method_line(trimmed) {
            let command_name = camel_to_snake(&camel_name);
            schemas.push(CommandSchema {
                command_name,
                params,
                return_type,
                source_path: source_path.to_path_buf(),
                generator: GeneratorKind::Specta,
            });
        }
    }

    schemas
}

/// Parse a single method line inside a specta commands block.
///
/// Input examples:
/// - `async getUserProfile(id: number): Promise<Result<UserProfile, string>> {`
/// - `async ping(): Promise<void> {`
fn parse_method_line(line: &str) -> Option<(String, Vec<ParamSchema>, String)> {
    // Must start with "async "
    let line = line.strip_prefix("async ")?;

    // Find the function name (up to the opening paren)
    let paren_pos = line.find('(')?;
    let method_name = line[..paren_pos].trim().to_string();

    if method_name.is_empty() {
        return None;
    }

    // Find matching closing paren for params
    let after_open = &line[paren_pos + 1..];
    let close_paren = find_matching_paren(after_open)?;
    let params_str = &after_open[..close_paren];

    // Parse params
    let params = parse_param_list(params_str);

    // Find return type: ): Promise<...>
    let after_close = &after_open[close_paren + 1..];
    let promise_start = after_close.find("Promise<")?;
    let after_promise = &after_close[promise_start + "Promise<".len()..];

    // Find the matching '>' for Promise<...>
    let promise_inner_end = find_matching_angle(after_promise)?;
    let promise_inner = &after_promise[..promise_inner_end];

    // Unwrap Result<T, E> to T
    let return_type = extract_ok_type(promise_inner.trim()).to_string();

    Some((method_name, params, return_type))
}

/// Find the position of the closing `)` matching the first character of `s`.
/// Handles nested angle brackets in generic types.
fn find_matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut angle_depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' => angle_depth += 1,
            '>' if angle_depth > 0 => angle_depth -= 1,
            '(' => depth += 1,
            ')' if angle_depth == 0 => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Find the position of the closing `>` for a `Type<...>` inner string.
fn find_matching_angle(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '<' => depth += 1,
            '>' => {
                if depth == 0 {
                    return Some(i);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Parse a parameter list string like `id: number, name: string` into `Vec<ParamSchema>`.
fn parse_param_list(s: &str) -> Vec<ParamSchema> {
    if s.trim().is_empty() {
        return Vec::new();
    }

    // Split on commas (but not inside angle brackets)
    let mut params = Vec::new();
    let mut depth = 0i32;
    let mut current = String::new();

    for ch in s.chars() {
        match ch {
            '<' => {
                depth += 1;
                current.push(ch);
            }
            '>' => {
                depth -= 1;
                current.push(ch);
            }
            ',' if depth == 0 => {
                if let Some(param) = parse_single_param(current.trim()) {
                    params.push(param);
                }
                current = String::new();
            }
            _ => current.push(ch),
        }
    }

    if let Some(param) = parse_single_param(current.trim()) {
        params.push(param);
    }

    params
}

/// Parse a single `name: type` string.
fn parse_single_param(s: &str) -> Option<ParamSchema> {
    let colon_pos = s.find(':')?;
    let name = s[..colon_pos].trim().to_string();
    let ts_type = s[colon_pos + 1..].trim().to_string();

    if name.is_empty() || ts_type.is_empty() {
        return None;
    }

    Some(ParamSchema { name, ts_type })
}

/// Extract the Ok type `T` from `Result<T, E>`, or return the type unchanged.
fn extract_ok_type(ty: &str) -> &str {
    if let Some(inner) = ty.strip_prefix("Result<") {
        // Find the comma separating T and E (not inside nested angle brackets)
        let mut depth = 0i32;
        for (i, ch) in inner.char_indices() {
            match ch {
                '<' => depth += 1,
                '>' if depth > 0 => depth -= 1,
                '>' => return inner[..i].trim(),
                ',' if depth == 0 => return inner[..i].trim(),
                _ => {}
            }
        }
    }
    ty
}

/// Parse a ts-rs generated file and return a map of `TypeName -> definition`.
///
/// Looks for lines like:
/// ```text
/// export type UserProfile = { id: number; name: string };
/// ```
#[must_use]
pub fn parse_ts_rs_types(content: &str) -> HashMap<String, String> {
    parse_type_aliases(content)
}

/// Parse a typegen-generated file and return a map of `TypeName -> definition`.
///
/// Handles both:
/// - `export type Name = ...;`  (inline type aliases)
/// - `export interface Name { field: type; ... }` (multi-line interface blocks)
#[must_use]
pub fn parse_typegen_types(content: &str) -> HashMap<String, String> {
    let mut aliases = parse_type_aliases(content);
    aliases.extend(parse_interface_blocks(content));
    aliases
}

/// Shared parser for `export type Name = ...;` lines.
fn parse_type_aliases(content: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();

    for line in content.lines() {
        if let Some((name, def)) = parse_type_alias_line(line.trim()) {
            aliases.insert(name, def);
        }
    }

    aliases
}

/// Parse `export interface Name { field: type; ... }` blocks from typegen output.
///
/// Collects all public fields (ignoring index signatures like `[key: string]: unknown`)
/// and returns them as a compact inline object definition string, e.g.:
/// `"{ id: number; name: string; email: string }"`
fn parse_interface_blocks(content: &str) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();

        // Match `export interface TypeName {`
        if let Some(rest) = line.strip_prefix("export interface ") {
            let brace_pos = rest.find('{');
            if let Some(bpos) = brace_pos {
                let name = rest[..bpos].trim().to_string();
                if !name.is_empty() {
                    // Collect field lines until the closing `}`
                    let mut fields = Vec::new();
                    i += 1;

                    while i < lines.len() {
                        let field_line = lines[i].trim();

                        if field_line == "}" || field_line == "};" {
                            break;
                        }

                        // Skip index signatures: `[key: string]: unknown;`
                        if field_line.starts_with('[') {
                            i += 1;
                            continue;
                        }

                        // Parse `fieldName: type;`
                        if let Some(colon) = field_line.find(':') {
                            let field_name = field_line[..colon].trim();
                            let field_type = field_line[colon + 1..]
                                .trim()
                                .trim_end_matches(';')
                                .trim()
                                .to_string();


                            if !field_name.is_empty() && !field_type.is_empty() {
                                fields.push(format!("{field_name}: {field_type}"));
                            }
                        }

                        i += 1;
                    }

                    if !fields.is_empty() {
                        let def = format!("{{ {} }}", fields.join("; "));
                        aliases.insert(name, def);
                    }
                }
            }
        }

        i += 1;
    }

    aliases
}

/// Parse a single `export type Name = ...;` line.
fn parse_type_alias_line(line: &str) -> Option<(String, String)> {
    let line = line.strip_prefix("export type ")?;
    let eq_pos = line.find('=')?;
    let name = line[..eq_pos].trim().to_string();
    let def = line[eq_pos + 1..].trim_end_matches(';').trim().to_string();

    if name.is_empty() || def.is_empty() {
        return None;
    }

    Some((name, def))
}
