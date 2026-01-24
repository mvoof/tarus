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
