//! Parser for generated bindings files (ts-rs, tauri-specta, tauri-plugin-typegen).
//!
//! These files are produced by type-generation tools and contain authoritative TypeScript
//! definitions that TARUS uses to power diagnostics and completions.

use crate::indexer::{GeneratorKind, ProjectIndex};
use crate::syntax::{camel_to_snake, Parameter};
use std::path::Path;

// ─── Public entry point ───────────────────────────────────────────────────────

/// Parse a generated TypeScript file and populate the project index.
///
/// Clears any previous data for this path, then re-populates `ts_type_aliases`
/// and/or `command_schemas` depending on the generator kind.
pub fn process_generated_file(
    path: &Path,
    content: &str,
    kind: GeneratorKind,
    project_index: &ProjectIndex,
) {
    let path_buf = path.to_path_buf();

    // Clear old data for this file before re-indexing
    project_index.remove_generated_data(&path_buf);

    // Register the file as generated so the normal TS parser skips it
    project_index
        .generated_file_paths
        .insert(path_buf.clone(), kind);

    match kind {
        GeneratorKind::TsRs => {
            let aliases = parse_ts_rs(content);
            project_index.add_generated_aliases(path_buf, aliases);
        }
        GeneratorKind::Specta | GeneratorKind::Typegen => {
            let (aliases, schemas) = parse_specta(content);
            project_index.add_generated_aliases(path_buf.clone(), aliases);
            project_index.add_generated_schemas(path_buf, schemas);
        }
    }
}

// ─── ts-rs parser ─────────────────────────────────────────────────────────────

/// Parse a ts-rs generated file and return `(type_name, ts_definition)` pairs.
///
/// Handles both single-line and multi-line `export type Name = Def;` declarations.
fn parse_ts_rs(content: &str) -> Vec<(String, String)> {
    let mut result = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_def = String::new();
    let mut depth: i32 = 0;

    for line in content.lines() {
        let trimmed = line.trim();

        if current_name.is_none() {
            // Skip comments and lines that don't start a type declaration
            if trimmed.starts_with("//") || !trimmed.starts_with("export type ") {
                continue;
            }

            let after = &trimmed["export type ".len()..];

            let Some(eq_pos) = after.find(" = ") else {
                continue;
            };

            let raw_name = after[..eq_pos].trim();
            // Strip generic type params: `ApiResponse<T>` → `ApiResponse`
            let base_name = raw_name
                .split('<')
                .next()
                .unwrap_or(raw_name)
                .to_string();

            let def_start = after[eq_pos + 3..].trim();
            depth = brace_depth(def_start);
            let def = def_start.trim_end_matches(';').trim().to_string();

            if depth <= 0 {
                // Single-line type definition
                result.push((base_name, def));
            } else {
                // Multi-line type: start accumulating
                current_name = Some(base_name);
                current_def = def;
            }
        } else {
            // Accumulating a multi-line type definition
            current_def.push(' ');
            current_def.push_str(trimmed);
            depth += brace_depth(trimmed);

            if depth <= 0 {
                let def = current_def.trim().trim_end_matches(';').trim().to_string();
                result.push((current_name.take().unwrap(), def));
                current_def.clear();
                depth = 0;
            }
        }
    }

    result
}

// ─── specta / typegen parser ──────────────────────────────────────────────────

/// Alias type for the return value of `parse_specta`.
type SpectaResult = (Vec<(String, String)>, Vec<(String, Vec<Parameter>)>);

/// Parse a specta or typegen generated file.
///
/// Returns `(type_aliases, command_schemas)` where:
/// - `type_aliases` are `export type Name = Def;` lines (re-exports from ts-rs)
/// - `command_schemas` are `export [async] function name(params): Promise<ret>` signatures
fn parse_specta(content: &str) -> SpectaResult {
    let mut aliases = Vec::new();
    let mut schemas = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if let Some(after) = trimmed.strip_prefix("export type ") {
            if let Some(eq_pos) = after.find(" = ") {
                let raw_name = after[..eq_pos].trim();
                let base_name = raw_name
                    .split('<')
                    .next()
                    .unwrap_or(raw_name)
                    .to_string();
                let def = after[eq_pos + 3..].trim().trim_end_matches(';').to_string();
                aliases.push((base_name, def));
            }
            continue;
        }

        // Try `export [async] function name(params)` style (typegen / older specta)
        if let Some(schema_entry) = parse_function_signature(trimmed) {
            schemas.push(schema_entry);
            continue;
        }

        // Try specta's method-in-object style: `async methodName(params) : Promise<ret> {`
        if let Some(schema_entry) = parse_specta_method_line(trimmed) {
            schemas.push(schema_entry);
        }
    }

    (aliases, schemas)
}

/// Parse a specta method-in-object line: `async methodName(params) : Promise<ret> {`
///
/// Specta generates `export const commands = { async method(params) {...}, ... }`.
/// Returns `(snake_case_name, parameters)` or `None` if not a method signature.
fn parse_specta_method_line(line: &str) -> Option<(String, Vec<Parameter>)> {
    let rest = line.strip_prefix("async ")?;

    // Extract name up to '('
    let paren_pos = rest.find('(')?;
    let name = rest[..paren_pos].trim();

    // Must be a valid identifier (non-empty, no whitespace)
    if name.is_empty() || name.chars().any(char::is_whitespace) {
        return None;
    }

    // Strip generic params: `createUser<T>` → `createUser`
    let name = name.split('<').next().unwrap_or(name);
    let snake_name = camel_to_snake(name);

    // Extract the params string (content between '(' and its matching ')')
    let after_paren = &rest[paren_pos + 1..];
    let params_str = find_closing_paren(after_paren)?;
    let params = parse_ts_function_params(params_str);

    Some((snake_name, params))
}

/// Parse a single `export [async] function name(params): Promise<ret>` line.
/// Returns `(snake_case_name, parameters)` or `None` if the line is not a valid signature.
fn parse_function_signature(line: &str) -> Option<(String, Vec<Parameter>)> {
    let rest = line.strip_prefix("export ")?;
    let rest = rest
        .strip_prefix("async function ")
        .or_else(|| rest.strip_prefix("function "))?;

    // Extract function name up to '(' (may include generic params like `<T>`)
    let paren_pos = rest.find('(')?;
    let raw_name = rest[..paren_pos].trim();
    // Strip generic params: `createUser<T>` → `createUser`
    let name = raw_name.split('<').next().unwrap_or(raw_name);
    let snake_name = camel_to_snake(name);

    // Extract the params string (content between the first '(' and its matching ')')
    let after_paren = &rest[paren_pos + 1..];
    let params_str = find_closing_paren(after_paren)?;
    let params = parse_ts_function_params(params_str);

    Some((snake_name, params))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Net brace depth change for a string (positive = more `{`, negative = more `}`).
fn brace_depth(s: &str) -> i32 {
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Return the string slice up to (but not including) the closing `)` at depth 0.
fn find_closing_paren(s: &str) -> Option<&str> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                if depth == 0 {
                    return Some(&s[..i]);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Parse a comma-separated list of TypeScript parameter declarations.
/// Each entry is `name: type` or `name?: type`.
fn parse_ts_function_params(params_str: &str) -> Vec<Parameter> {
    if params_str.trim().is_empty() {
        return Vec::new();
    }

    let parts = split_at_depth_zero(params_str, ',');
    let mut params = Vec::new();

    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(colon_pos) = find_type_colon(part) {
            let name = part[..colon_pos]
                .trim()
                .trim_end_matches('?')
                .to_string();
            let type_name = part[colon_pos + 1..].trim().to_string();
            if !name.is_empty() {
                params.push(Parameter { name, type_name });
            }
        }
    }

    params
}

/// Find the position of `:` that separates a parameter name from its type.
/// Skips `:` that appear inside angle brackets, parentheses, brackets, or braces.
fn find_type_colon(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// Split `s` at `delimiter` characters that occur at depth 0.
fn split_at_depth_zero(s: &str, delimiter: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;

    for (i, c) in s.char_indices() {
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            c if c == delimiter && depth == 0 => {
                parts.push(&s[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&s[start..]);
    parts
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_ts_rs_simple() {
        let content = r#"
// This file was generated by [ts-rs]. Do not edit this file manually.
export type UserProfile = { id: number, username: string, email: string };
export type TaskState = "Active" | "Completed" | "Failed";
"#;
        let aliases = parse_ts_rs(content);
        assert_eq!(aliases.len(), 2);
        assert_eq!(aliases[0].0, "UserProfile");
        assert!(aliases[0].1.contains("id: number"));
        assert_eq!(aliases[1].0, "TaskState");
        assert!(aliases[1].1.contains("Active"));
    }

    #[test]
    fn test_parse_ts_rs_generic_name() {
        let content =
            "export type ApiResponse<T> = { data: T, success: boolean };\n";
        let aliases = parse_ts_rs(content);
        assert_eq!(aliases.len(), 1);
        // Generic param stripped from key
        assert_eq!(aliases[0].0, "ApiResponse");
    }

    #[test]
    fn test_parse_ts_rs_multiline() {
        let content = "export type BigType = {\n  field1: string,\n  field2: number\n};\n";
        let aliases = parse_ts_rs(content);
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].0, "BigType");
        assert!(aliases[0].1.contains("field1"));
    }

    #[test]
    fn test_parse_specta_function() {
        let content = r#"
// This file was generated by tauri-specta
export async function removeLocalization(baseFolderPath: string, selectedLanguageCode: string): Promise<void>
export function ping(): Promise<void>
export async function getUserProfile(id: number): Promise<UserProfile>
"#;
        let (_aliases, schemas) = parse_specta(content);
        assert_eq!(schemas.len(), 3);

        let remove = schemas.iter().find(|(n, _)| n == "remove_localization").unwrap();
        assert_eq!(remove.1.len(), 2);
        assert_eq!(remove.1[0].name, "baseFolderPath");
        assert_eq!(remove.1[0].type_name, "string");

        let ping = schemas.iter().find(|(n, _)| n == "ping").unwrap();
        assert!(ping.1.is_empty());

        let get_user = schemas.iter().find(|(n, _)| n == "get_user_profile").unwrap();
        assert_eq!(get_user.1.len(), 1);
        assert_eq!(get_user.1[0].name, "id");
        assert_eq!(get_user.1[0].type_name, "number");
    }

    #[test]
    fn test_parse_specta_optional_param() {
        let content =
            "export function search(query: string, limit?: number): Promise<string[]>\n";
        let (_aliases, schemas) = parse_specta(content);
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].1[0].name, "query");
        assert_eq!(schemas[0].1[1].name, "limit"); // '?' stripped
        assert_eq!(schemas[0].1[1].type_name, "number");
    }

    #[test]
    fn test_parse_specta_actual_format() {
        // Real tauri-specta output format: methods inside `export const commands = {}`
        let content = r#"
// This file was generated by [tauri-specta](https://github.com/oscartbeaumont/tauri-specta). Do not edit this file manually.

export const commands = {
async greet(name: string) : Promise<number> {
    return await TAURI_INVOKE("greet", { name });
},
async reverseString(text: string) : Promise<string> {
    return await TAURI_INVOKE("reverse_string", { text });
},
async createUser(name: string, email: string, age: number) : Promise<UserProfile> {
    return await TAURI_INVOKE("create_user", { name, email, age });
},
async maybeError(fail: boolean) : Promise<Result<string, string>> {
    try {
    return { status: "ok", data: await TAURI_INVOKE("maybe_error", { fail }) };
} catch (e) {
    if(e instanceof Error) throw e;
    else return { status: "error", error: e as any };
},
}

export type UserProfile = { id: number; name: string; email: string; age: number; active: boolean }
"#;
        let (aliases, schemas) = parse_specta(content);

        // Types are extracted
        assert_eq!(aliases.len(), 1);
        assert_eq!(aliases[0].0, "UserProfile");

        // Commands are extracted
        assert!(schemas.len() >= 3, "expected at least 3 schemas, got {}", schemas.len());

        let greet = schemas.iter().find(|(n, _)| n == "greet").unwrap();
        assert_eq!(greet.1.len(), 1);
        assert_eq!(greet.1[0].name, "name");
        assert_eq!(greet.1[0].type_name, "string");

        let reverse = schemas.iter().find(|(n, _)| n == "reverse_string").unwrap();
        assert_eq!(reverse.1.len(), 1);
        assert_eq!(reverse.1[0].type_name, "string");

        let create = schemas.iter().find(|(n, _)| n == "create_user").unwrap();
        assert_eq!(create.1.len(), 3);
        assert_eq!(create.1[0].name, "name");
        assert_eq!(create.1[1].name, "email");
        assert_eq!(create.1[2].name, "age");
        assert_eq!(create.1[2].type_name, "number");
    }

    #[test]
    fn test_parse_specta_complex_types() {
        let content = "export async function save(data: Record<string, number>, tags: string[]): Promise<void>\n";
        let (_aliases, schemas) = parse_specta(content);
        assert_eq!(schemas.len(), 1);
        let params = &schemas[0].1;
        assert_eq!(params[0].name, "data");
        assert_eq!(params[0].type_name, "Record<string, number>");
        assert_eq!(params[1].name, "tags");
        assert_eq!(params[1].type_name, "string[]");
    }
}
