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

/// Tauri call sites in a file whose command/event name could not be resolved to a
/// string literal — e.g. `emit(eventName, payload)` where `eventName` is a parameter.
///
/// Such a call still uses *some* command or event, we just cannot tell which. Any
/// diagnostic that reports an *absence* ("never emitted", "never invoked") assumes a
/// closed world, so a single unresolved call site of the matching kind invalidates it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DynamicUsages {
    pub invokes: bool,
    pub emitters: bool,
    pub listeners: bool,
}

impl DynamicUsages {
    /// Record a dynamic usage for the given behavior. Definitions always carry a
    /// literal name, so they can never be dynamic.
    pub fn record(&mut self, behavior: Behavior) {
        match behavior {
            Behavior::Call | Behavior::SpectaCall => self.invokes = true,
            Behavior::Emit => self.emitters = true,
            Behavior::Listen => self.listeners = true,
            Behavior::Definition => {}
        }
    }

    #[must_use]
    pub fn is_empty(self) -> bool {
        !self.invokes && !self.emitters && !self.listeners
    }

    /// Fold another file's usages into this one.
    pub fn merge(&mut self, other: Self) {
        self.invokes |= other.invokes;
        self.emitters |= other.emitters;
        self.listeners |= other.listeners;
    }
}

/// A helper that passes one of its own parameters straight into a Tauri call, e.g.
///
/// ```ts
/// const emitToOverlays = async (event: string, payload: unknown) =>
///   emitTo("overlay", event, payload);
/// ```
///
/// The event name never appears at the `emitTo`; it appears at each call of the
/// helper. Recording the forwarder lets those call sites be indexed as real emit
/// sites, so navigation, `CodeLens` and references reach the other side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forwarder {
    /// Name the helper is called by.
    pub function_name: String,
    /// Which argument of the helper carries the command/event name.
    pub param_index: usize,
    pub entity: EntityType,
    pub behavior: Behavior,
}

#[derive(Debug, Default)]
pub struct FileIndex {
    pub path: PathBuf,
    pub findings: Vec<Finding>,
    pub dynamic_usages: DynamicUsages,
    pub forwarders: Vec<Forwarder>,
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
