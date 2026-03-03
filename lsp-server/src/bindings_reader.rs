//! Parsers for generated TypeScript binding files
//!
//! Supports three generators:
//! - tauri-specta: `export const commands = { async methodName(...): Promise<...> { ... } }`
//! - ts-rs: `export type Name = { ... };`
//! - tauri-typegen: `export type Name = { ... };`

use crate::indexer::{CommandSchema, EventSchema, GeneratorKind, ParamSchema};
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

// ─── Event schema parsers ────────────────────────────────────────────────────

/// Parse event schemas from a Specta-generated bindings file.
///
/// Looks for the `__makeEvents__` block:
/// ```text
/// export const events = __makeEvents__<{
///     DemoEvent: string,
///     UserUpdated: UserProfile,
/// }>({
///     DemoEvent: "demo-event",
///     UserUpdated: "user-updated",
/// })
/// ```
///
/// The type parameter maps TS names to payload types.
/// The value object maps TS names to actual event name strings.
#[must_use]
pub fn parse_specta_events(content: &str, source_path: &Path) -> Vec<EventSchema> {
    let mut schemas = Vec::new();

    // Find __makeEvents__<{ ... }>({ ... })
    let Some(make_pos) = content.find("__makeEvents__<") else {
        return schemas;
    };

    let after_make = &content[make_pos + "__makeEvents__<".len()..];

    // Parse type parameter block: { TypeName: PayloadType, ... }
    let Some(type_block_start) = after_make.find('{') else {
        return schemas;
    };
    let type_block_content = &after_make[type_block_start + 1..];
    let Some(type_block_end) = find_matching_brace(type_block_content) else {
        return schemas;
    };
    let type_block = &type_block_content[..type_block_end];

    // Parse type pairs: TypeName: PayloadType
    let mut type_map: HashMap<String, String> = HashMap::new();
    for line in type_block.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        if let Some(colon_pos) = trimmed.find(':') {
            let ts_name = trimmed[..colon_pos].trim().to_string();
            let payload_type = trimmed[colon_pos + 1..].trim().to_string();
            if !ts_name.is_empty() && !payload_type.is_empty() {
                type_map.insert(ts_name, payload_type);
            }
        }
    }

    // Find value object block: { TypeName: "event-name", ... }
    let after_type_block = &type_block_content[type_block_end + 1..];
    // Skip `>({` between the two blocks
    let Some(value_block_start) = after_type_block.find('{') else {
        return schemas;
    };
    let value_block_content = &after_type_block[value_block_start + 1..];
    let Some(value_block_end) = find_matching_brace(value_block_content) else {
        return schemas;
    };
    let value_block = &value_block_content[..value_block_end];

    // Parse value pairs: TypeName: "actual-event-name"
    for line in value_block.lines() {
        let trimmed = line.trim().trim_end_matches(',');
        if let Some(colon_pos) = trimmed.find(':') {
            let ts_name = trimmed[..colon_pos].trim();
            let event_str = trimmed[colon_pos + 1..].trim();
            // Extract string literal value
            let event_name = event_str.trim_matches('"').trim_matches('\'').to_string();
            if !event_name.is_empty() {
                if let Some(payload_type) = type_map.get(ts_name) {
                    schemas.push(EventSchema {
                        event_name,
                        payload_type: payload_type.clone(),
                        source_path: source_path.to_path_buf(),
                        generator: GeneratorKind::Specta,
                    });
                }
            }
        }
    }

    schemas
}

/// Parse event schemas from a typegen-generated events file.
///
/// Looks for `listen<T>('event-name', ...)` patterns:
/// ```text
/// export async function onNotificationSent(
///   handler: (payload: types.message) => void
/// ): Promise<UnlistenFn> {
///   return listen<types.message>('notification-sent', (event) => {
///     handler(event.payload);
///   });
/// }
/// ```
#[must_use]
pub fn parse_typegen_events(content: &str, source_path: &Path) -> Vec<EventSchema> {
    let mut schemas = Vec::new();

    // Find all listen<T>('event-name' patterns
    let listen_prefix = "listen<";
    let mut search_from = 0;

    while let Some(pos) = content[search_from..].find(listen_prefix) {
        let abs_pos = search_from + pos;
        let after_listen = &content[abs_pos + listen_prefix.len()..];

        // Extract T from listen<T>
        if let Some(angle_end) = find_matching_angle(after_listen) {
            let payload_type = after_listen[..angle_end].trim();
            let after_angle = &after_listen[angle_end + 1..];

            // Expect >('event-name'
            if let Some(quote_start) = after_angle.find(['\'', '"']) {
                let quote_char = after_angle.as_bytes()[quote_start];
                let after_quote = &after_angle[quote_start + 1..];
                if let Some(quote_end) = after_quote.find(quote_char as char) {
                    let event_name = &after_quote[..quote_end];

                    // Strip "types." prefix if present
                    let (clean_type, was_prefixed) =
                        if let Some(stripped) = payload_type.strip_prefix("types.") {
                            (stripped.to_string(), true)
                        } else {
                            (payload_type.to_string(), false)
                        };

                    // Skip lowercase names from types.X — these are variable names, not types
                    if !event_name.is_empty()
                        && !clean_type.is_empty()
                        && (!was_prefixed
                            || clean_type.starts_with(char::is_uppercase))
                    {
                        schemas.push(EventSchema {
                            event_name: event_name.to_string(),
                            payload_type: clean_type,
                            source_path: source_path.to_path_buf(),
                            generator: GeneratorKind::Typegen,
                        });
                    }
                }
            }
        }

        search_from = abs_pos + listen_prefix.len();
    }

    schemas
}

/// Find position of closing `}` matching the first character of `s`.
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
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
