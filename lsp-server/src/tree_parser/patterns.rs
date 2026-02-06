//! Pattern definitions for Rust and Frontend parsers
//!
//! This module centralizes the configuration of what constitutes a "command", "event emit",
//! or "event listen" in both Rust and TypeScript/JavaScript code.

use crate::syntax::{Behavior, EntityType};
use std::collections::HashMap;

/// Argument position for function call matching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgPosition {
    First,
    Second,
}

/// Function pattern configuration
#[derive(Debug, Clone)]
pub struct FunctionPatternWithPos {
    pub name: &'static str,
    pub entity: EntityType,
    pub behavior: Behavior,
    pub arg_position: ArgPosition,
}

/// Get method patterns for Rust backend (method_name -> (entity, behavior))
pub fn get_rust_event_patterns() -> HashMap<&'static str, (EntityType, Behavior)> {
    let mut patterns = HashMap::new();
    // Emit methods
    patterns.insert("emit", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_to", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_str", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_str_to", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_filter", (EntityType::Event, Behavior::Emit));
    patterns.insert("emit_str_filter", (EntityType::Event, Behavior::Emit));
    // Listen methods
    patterns.insert("listen", (EntityType::Event, Behavior::Listen));
    patterns.insert("listen_any", (EntityType::Event, Behavior::Listen));
    patterns.insert("once", (EntityType::Event, Behavior::Listen));
    patterns.insert("once_any", (EntityType::Event, Behavior::Listen));
    patterns
}

/// Get all frontend patterns including those with second argument
pub fn get_all_frontend_patterns() -> Vec<FunctionPatternWithPos> {
    vec![
        // First argument patterns - Commands
        FunctionPatternWithPos {
            name: "invoke",
            entity: EntityType::Command,
            behavior: Behavior::Call,
            arg_position: ArgPosition::First,
        },
        // First argument patterns - Events (emit)
        FunctionPatternWithPos {
            name: "emit",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::First,
        },
        // First argument patterns - Events (listen/subscribe)
        FunctionPatternWithPos {
            name: "listen",
            entity: EntityType::Event,
            behavior: Behavior::Listen,
            arg_position: ArgPosition::First,
        },
        FunctionPatternWithPos {
            name: "once",
            entity: EntityType::Event,
            behavior: Behavior::Listen,
            arg_position: ArgPosition::First,
        },
        // Second argument patterns
        FunctionPatternWithPos {
            name: "emitTo",
            entity: EntityType::Event,
            behavior: Behavior::Emit,
            arg_position: ArgPosition::Second,
        },
    ]
}
