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
}

/// Behavior of the entity - how it's used in code
#[derive(Debug, Deserialize, Clone, PartialEq, Eq, Hash, Copy)]
#[serde(rename_all = "camelCase")]
pub enum Behavior {
    /// Command definition (Rust: #[tauri::command] fn name())
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
            ParseError::SyntaxError(msg) => write!(f, "Syntax error: {}", msg),
            ParseError::QueryError(msg) => write!(f, "Query error: {}", msg),
            ParseError::LanguageError(msg) => write!(f, "Language error: {}", msg),
        }
    }
}

impl std::error::Error for ParseError {}

/// Result type for parsing operations
pub type ParseResult<T> = Result<T, ParseError>;
