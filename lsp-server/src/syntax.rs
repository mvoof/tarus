//! Core types for Tauri command/event entities and behaviors.
//!
//! These types are used throughout the LSP server to identify
//! and categorize findings from parsed source files.

use serde::Deserialize;

/// Type of entity - either a Command or an Event
#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "camelCase")]
pub enum EntityType {
    /// Tauri command (invoke/definition)
    Command,
    /// Tauri event (emit/listen)
    Event,
    /// Rust Struct
    Struct,
    /// Rust Enum
    Enum,
    /// TS Interface
    Interface,
}

/// Behavior of the entity - how it's used in code
#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Behavior {
    /// Command definition (Rust: #[`tauri::command`] fn `name()`)
    Definition,
    /// Command call (Frontend: invoke("name"))
    Call,
    /// Event emit (emit("event"))
    Emit,
    /// Event listen (listen("event"))
    Listen,
}

/// Parse error types
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// File contains syntax errors
    SyntaxError(String),
    /// Query execution failed
    QueryError(String),
    /// Language configuration error
    LanguageError(String),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::SyntaxError(msg) => write!(f, "Syntax error: {msg}"),
            ParseError::QueryError(msg) => write!(f, "Query error: {msg}"),
            ParseError::LanguageError(msg) => write!(f, "Language error: {msg}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Result type for parsing operations
pub type ParseResult<T> = Result<T, ParseError>;

/// Check if attributes contain serde(rename_all = "camelCase")
#[must_use]
pub fn should_rename_to_camel(attributes: Option<&Vec<String>>) -> bool {
    if let Some(attrs) = attributes {
        for attr in attrs {
            if attr.contains("serde") && attr.contains("rename_all") && attr.contains("camelCase") {
                return true;
            }
        }
    }
    false
}

/// Convert `snake_case` to camelCase
#[must_use]
pub fn snake_to_camel(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize = false;

    for c in s.chars() {
        if c == '_' {
            capitalize = true;
        } else if capitalize {
            result.push(c.to_ascii_uppercase());
            capitalize = false;
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert camelCase to snake_case
#[must_use]
pub fn camel_to_snake(s: &str) -> String {
    let mut result = String::new();

    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }

    result
}

/// Extract inner type from Result<T, E> - returns the success type
#[must_use]
pub fn extract_result_ok_type(rust_type: &str) -> &str {
    let rt = rust_type.trim();

    if rt.starts_with("Result<") {
        let inner = &rt[7..rt.len() - 1];
        // Find the first comma that's not inside nested <>
        let mut depth = 0;

        for (i, c) in inner.char_indices() {
            match c {
                '<' => depth += 1,
                '>' => depth -= 1,
                ',' if depth == 0 => return inner[..i].trim(),
                _ => {}
            }
        }

        inner.trim()
    } else {
        rt
    }
}

/// Map Rust types to TypeScript types
#[must_use]
pub fn map_rust_type_to_ts(rust_type: &str) -> String {
    let rt = extract_result_ok_type(rust_type);
    let rt = rt.trim();

    // Remove references and mut
    let rt = rt.trim_start_matches('&').trim_start_matches("mut ").trim();

    if rt == "String" || rt == "str" || rt == "&str" {
        return "string".to_string();
    }

    if [
        "u8", "i8", "u16", "i16", "u32", "i32", "u64", "i64", "f32", "f64", "usize", "isize",
    ]
    .contains(&rt)
    {
        return "number".to_string();
    }

    if rt == "bool" {
        return "boolean".to_string();
    }

    if rt.starts_with("Vec<") {
        let inner = &rt[4..rt.len() - 1];

        return format!("{}[]", map_rust_type_to_ts(inner));
    }

    if rt.starts_with("Option<") {
        let inner = &rt[7..rt.len() - 1];
        let ts_inner = map_rust_type_to_ts(inner);

        return format!("{ts_inner} | null");
    }

    if rt.starts_with("HashMap<") {
        return "Record<string, any>".to_string();
    }

    if rt == "Value" || rt == "serde_json::Value" {
        return "any".to_string();
    }

    rt.to_string()
}

/// Map TypeScript types to Rust types
#[must_use]
pub fn map_ts_type_to_rust(ts_type: &str) -> String {
    match ts_type.trim() {
        "string" => "String".to_string(),
        "number" => "i64".to_string(),
        "boolean" => "bool".to_string(),
        "any" => "serde_json::Value".to_string(),
        "null" | "undefined" => "()".to_string(),
        t if t.ends_with("[]") => {
            format!("Vec<{}>", map_ts_type_to_rust(&t[..t.len() - 2]))
        }

        t if t.starts_with("Array<") && t.ends_with('>') => {
            format!("Vec<{}>", map_ts_type_to_rust(&t[6..t.len() - 1]))
        }

        t if t.contains('|') => "serde_json::Value".to_string(), // Union types
        t => t.to_string(),                                      // Keep custom types as-is
    }
}

/// Check if a Rust type is a primitive that maps to a standard JS type
#[must_use]
pub fn is_primitive_rust_type(rust_type: &str) -> bool {
    let rt = rust_type
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim();

    if [
        "String",
        "str",
        "u8",
        "i8",
        "u16",
        "i16",
        "u32",
        "i32",
        "u64",
        "i64",
        "f32",
        "f64",
        "usize",
        "isize",
        "bool",
        "Value",
        "serde_json::Value",
    ]
    .contains(&rt)
    {
        return true;
    }

    if rt.starts_with("Vec<") || rt.starts_with("Option<") || rt.starts_with("HashMap<") {
        return true;
    }

    false
}

/// Extract the base type name, removing containers like Vec<>, Option<>, etc.
#[must_use]
pub fn get_base_rust_type(rust_type: &str) -> String {
    let mut rt = rust_type
        .trim()
        .trim_start_matches('&')
        .trim_start_matches("mut ")
        .trim();

    loop {
        if rt.starts_with("Vec<") {
            rt = &rt[4..rt.len() - 1].trim();
        } else if rt.starts_with("Option<") {
            rt = &rt[7..rt.len() - 1].trim();
        } else if rt.starts_with("Result<") {
            // Result is a bit more complex, handle the first part
            rt = &rt[7..];

            if let Some(comma_pos) = rt.find(',') {
                rt = &rt[..comma_pos].trim();
            } else if let Some(end_pos) = rt.rfind('>') {
                rt = &rt[..end_pos].trim();
            }
        } else {
            break;
        }
    }

    rt.to_string()
}
