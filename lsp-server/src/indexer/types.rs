//! Core type definitions for the project index

use crate::syntax::{Behavior, EntityType};
use std::path::PathBuf;
use tower_lsp_server::lsp_types::{Position, Range};

/// Which tool generated the binding file (or the source itself)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratorKind {
    TsRs,
    Specta,
    Typegen,
    RustSource,
}

/// A type generator discovered from project configuration files
#[derive(Debug, Clone)]
pub struct DiscoveredGenerator {
    pub kind: GeneratorKind,
    /// Absolute, normalized output path (file or directory)
    pub output_path: PathBuf,
    /// `true` for directory-match generators (ts-rs, typegen); `false` for exact-file generators (specta)
    pub is_directory: bool,
}

/// A single parameter in a command schema
#[derive(Debug, Clone, PartialEq)]
pub struct ParamSchema {
    pub name: String,
    pub ts_type: String,
}

/// Type signature of a Tauri event payload, extracted from bindings or Rust source
#[derive(Debug, Clone)]
pub struct EventSchema {
    pub event_name: String,
    pub payload_type: String,
    pub source_path: PathBuf,
    pub generator: GeneratorKind,
}

/// Type signature of a Tauri command, extracted from bindings or Rust source
#[derive(Debug, Clone)]
pub struct CommandSchema {
    pub command_name: String,
    pub params: Vec<ParamSchema>,
    pub return_type: String,
    pub source_path: PathBuf,
    pub generator: GeneratorKind,
}

/// A single occurrence in a file (parser result)
#[derive(Debug, Clone)]
pub struct Finding {
    pub key: String,                           // Name ("save_file")
    pub entity: EntityType,                    // Command or Event
    pub behavior: Behavior,                    // Call, Emit, Listen
    pub range: Range,                          // Coordinates
    pub call_arg_count: Option<u32>,           // For SpectaCall: positional arg count
    pub call_param_keys: Option<Vec<String>>,  // For Call: object literal keys in second arg
    pub return_type: Option<String>,           // For Call with generics: invoke<T>() type argument
    pub call_name_end: Option<Position>,       // End of "invoke" identifier (for inserting <T>)
    pub type_arg_range: Option<Range>,         // Range of <T> in invoke<T>() (for replacing)
    pub codegen_origin: Option<GeneratorKind>, // Set when call site is from typed codegen (e.g. specta events API)
}

impl Finding {
    /// Create a new Finding with only required fields; optional fields default to `None`.
    #[must_use]
    pub fn new(key: String, entity: EntityType, behavior: Behavior, range: Range) -> Self {
        Self {
            key,
            entity,
            behavior,
            range,
            call_arg_count: None,
            call_param_keys: None,
            return_type: None,
            call_name_end: None,
            type_arg_range: None,
            codegen_origin: None,
        }
    }
}

impl From<(&PathBuf, Finding)> for LocationInfo {
    fn from((path, f): (&PathBuf, Finding)) -> Self {
        Self {
            path: path.clone(),
            range: f.range,
            behavior: f.behavior,
            call_arg_count: f.call_arg_count,
            call_param_keys: f.call_param_keys,
            return_type: f.return_type,
            call_name_end: f.call_name_end,
            type_arg_range: f.type_arg_range,
            codegen_origin: f.codegen_origin,
        }
    }
}

#[derive(Debug)]
pub struct FileIndex {
    pub path: PathBuf,
    pub findings: Vec<Finding>,
}

/// Search Key (Hashmap Key)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct IndexKey {
    pub entity: EntityType,
    pub name: String,
}

/// Location Information (Value)
#[derive(Debug, Clone)]
pub struct LocationInfo {
    pub path: PathBuf,
    pub range: Range,
    pub behavior: Behavior,
    pub call_arg_count: Option<u32>,
    pub call_param_keys: Option<Vec<String>>,
    pub return_type: Option<String>,
    pub call_name_end: Option<Position>,
    pub type_arg_range: Option<Range>,
    pub codegen_origin: Option<GeneratorKind>,
}

/// A name paired with optional location information
pub type NameLocation = (String, Option<LocationInfo>);

/// Cache for command and event names
pub(super) type NameCache = Option<std::sync::Arc<Vec<NameLocation>>>;

/// Diagnostic information for a command/event
#[derive(Clone, Debug)]
pub enum DiagnosticInfo {
    Command {
        has_definition: bool,
        has_calls: bool,
    },
    Event {
        has_definition: bool,
        has_emitters: bool,
        has_listeners: bool,
    },
}

impl DiagnosticInfo {
    #[must_use]
    pub fn has_definition(&self) -> bool {
        match self {
            DiagnosticInfo::Command { has_definition, .. }
            | DiagnosticInfo::Event { has_definition, .. } => *has_definition,
        }
    }

    #[must_use]
    pub fn has_calls(&self) -> bool {
        match self {
            DiagnosticInfo::Command { has_calls, .. } => *has_calls,
            DiagnosticInfo::Event { .. } => false,
        }
    }

    #[must_use]
    pub fn has_emitters(&self) -> bool {
        match self {
            DiagnosticInfo::Event { has_emitters, .. } => *has_emitters,
            DiagnosticInfo::Command { .. } => false,
        }
    }

    #[must_use]
    pub fn has_listeners(&self) -> bool {
        match self {
            DiagnosticInfo::Event { has_listeners, .. } => *has_listeners,
            DiagnosticInfo::Command { .. } => false,
        }
    }
}
