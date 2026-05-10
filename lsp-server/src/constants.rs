//! Shared constants used across the LSP server.

/// File extensions the server can parse and index.
pub const SUPPORTED_EXTENSIONS: &[&str] = &["rs", "ts", "tsx", "js", "jsx", "vue", "svelte"];

// ---------------------------------------------------------------------------
// Diagnostic codes — used in `diagnostics.rs` for publishing and in
// `code_actions.rs` for matching.
// ---------------------------------------------------------------------------

pub const DIAG_ARG_COUNT_MISMATCH: &str = "tarus/arg-count-mismatch";
pub const DIAG_RETURN_TYPE_MISSING: &str = "tarus/return-type-missing";
pub const DIAG_RETURN_TYPE_MISMATCH: &str = "tarus/return-type-mismatch";
pub const DIAG_EVENT_PAYLOAD_MISSING: &str = "tarus/event-payload-missing";
pub const DIAG_EVENT_PAYLOAD_MISMATCH: &str = "tarus/event-payload-mismatch";

// ---------------------------------------------------------------------------
// File priority scores for code-action candidate ranking.
// Higher = more likely to be the right file for a new `#[tauri::command]`.
// ---------------------------------------------------------------------------

/// `lib.rs` — typical Tauri entry point
pub const PRIORITY_LIB_RS: u8 = 100;
/// `main.rs` — alternative entry point
pub const PRIORITY_MAIN_RS: u8 = 95;
/// File that contains `invoke_handler(` — already wires commands
pub const PRIORITY_INVOKE_HANDLER: u8 = 85;
/// File with `#[tauri::command]` — already has commands
pub const PRIORITY_HAS_COMMAND_ATTR: u8 = 80;
/// Well-known command file names
pub const PRIORITY_COMMAND_FILE: u8 = 70;
/// `mod.rs`
pub const PRIORITY_MOD_RS: u8 = 65;
/// Any other Rust file
pub const PRIORITY_DEFAULT: u8 = 50;

// ---------------------------------------------------------------------------
// Timing & limits
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Specta/ts-rs discovery method names
// ---------------------------------------------------------------------------

/// `tauri-specta` export method name
pub const SPECTA_EXPORT_METHOD: &str = "export";
/// `specta_typescript` export method name (standalone specta-typescript crate)
pub const SPECTA_EXPORT_TO_METHOD: &str = "export_to";
/// Specta bindings variable name in generated TS
pub const SPECTA_COMMANDS_VAR: &str = "commands";
/// Specta events builder function in generated TS
pub const SPECTA_MAKE_EVENTS_FN: &str = "__makeEvents__";
/// Typegen listen function name in generated TS
pub const TYPEGEN_LISTEN_FN: &str = "listen";

// ---------------------------------------------------------------------------
// Timing & limits
// ---------------------------------------------------------------------------

/// Debounce delay (ms) before re-processing a file after edits.
pub const DEBOUNCE_MS: u64 = 300;

/// Default maximum number of references shown for a command/event.
pub const DEFAULT_REFERENCE_LIMIT: usize = 3;
