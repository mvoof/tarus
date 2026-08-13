## Change Log

### [Unreleased]

### [0.12.1] - 2026-08-13

- **Bug fixes**
  - Fixed wrapper functions not being resolved when the call is both awaited and given an explicit type argument (`await listenTo<Payload>("event", ...)`). Such a call was skipped, so every event the helper subscribed to was reported as having no listeners

### [0.12.0] - 2026-07-29

- **Bug fixes**
  - Fixed a false positive where a command or event name passed as a function parameter was matched against an unrelated constant from another file. Names are now resolved from the nearest local variable first
  - Fixed false "no listeners" / "command not found" warnings appearing when a name could not be determined at all (for example, a name built at runtime). Such calls are now skipped instead of reported
  - Fixed spurious warnings while typing: a file with a syntax error is no longer partially indexed, so half-written code no longer breaks diagnostics for the whole event. The last valid version is kept until the file parses cleanly again
  - Fixed a bug where names written in template literals (`` invoke(`my_command`) ``) were not recognized
  - Fixed CodeLens ignoring the reference limit — a line with both Rust and frontend references could show twice as many links as configured and never collapse into a summary
  - Fixed the extension starting on non-Tauri projects. Activation now follows the officially documented Tauri config file names, and startup errors are no longer silently swallowed

- **Added**
  - Names passed through your own wrapper functions are now resolved. If a helper forwards its argument into `invoke`/`emit`/`listen`, the literal at the call site becomes a real reference with navigation, CodeLens and Find References
  - CodeLens now also shows references inside the same file, so an `emit` and its `listen` a hundred lines apart are linked

### [0.11.0] - 2026-07-20

- **Added**
  - SolidJS support — `TypeScriptJsx` and `JavaScriptJsx` file variants are now parsed, enabling navigation, diagnostics, and completions in `.tsx`/`.jsx` SolidJS projects
  - External file edit detection — the LSP now registers a `didChangeWatchedFiles` watcher so changes made outside the editor (e.g. regenerated binding files) trigger re-indexing automatically

### [0.10.0] - 2026-06-15

- **Added**
  - Support for constants as command and event names — `invoke`, `emit`, `listen`, `emitTo` and other calls now resolve constants defined in both Rust and TypeScript, including across files
  - **TARUS: Restart TARUS Server** command to restart the language server without reloading VS Code

### [0.9.1] - 2026-06-01

- **Bug fixes:** Fixed an issue with false positives for functions whose names match standard Tauri v2 functions (invoke, emit, listen, once, emitTo)

### [0.9.0] - 2026-05-14

- **Enhanced Type Support**
  - **Complex Types:** Now supports union types (e.g., `string | null`). This allows for accurate checking of Rust's `Option<T>` and optional TypeScript fields, including `undefined`.
  - **Full Array Support:** Improved validation for arrays (both `T[]` and `Array<T>`). You'll now get precise diagnostics and Quick Fixes when working with lists of data.
  - **Deep Nesting:** Better handling of complex nested types like `Array<string | null>`, ensuring your data is validated correctly at any depth.

- **Generator Improvements**
  - **Unified Parsing:** We've merged the type processing for `ts-rs`, `Specta`, and `tauri-typegen`. Support for `interface` is now more consistent across all tools.
  - **Specta Fixes:** Resolved an issue where some Specta-generated files weren't recognized. Your type aliases are now indexed reliably.

- **Rust Enhancements**
  - **Pass-by-Reference:** Improved type detection for Rust `emit` calls when data is passed by reference (e.g., `&payload`).

- **Bug Fixes & Stability**
  - Fixed a bug where type checking would stop working for arrays of custom interfaces.
  - Hover tooltips now accurately display return types discovered from your Rust source code.
  - Internal tests now better reflect real-world projects, ensuring a more stable experience.

### [0.8.0] - 2026-05-05

- **Code quality & maintainability**
  - Removed ~200 lines of duplicated parsing logic across Rust and TypeScript parsers
  - Split several large functions (50–100+ lines each) into smaller, focused helpers
  - Extracted shared utilities to eliminate copy-paste in query setup, schema building,  
    and symbol construction
  - Moved CodeLens logic into its own file to reduce the size of the central index module
  - Replaced magic strings scattered across the codebase with named constants

- **Bug fixes**
  - `string[]` and `Array<string>` are now treated as the same type in diagnostics
  - Fixed a dead code path in language detection that could never be reached
  - Removed an unsafe `unwrap()` call during file parsing

- **Performance**
  - Name completion no longer copies the full list on every keystroke — uses shared  
    reference instead
  - Hover info for commands/events no longer re-allocates data on each request
  - Regex/pattern tables for Rust and TypeScript parsers are now built once at startup  
    instead of on every file parse

- **Reliability**
  - Code actions no longer freeze the editor while scanning Rust source files
  - Test cleanup now runs correctly even when a test crashes midway

### [0.7.0] - 2026-04-24

- **Enhanced Type Support:** Full bidirectional payload type checking between Rust and Frontend.
  - **Rust-to-TS Mapping:** Accurate conversion for primitives, `Result`, `Option`, `Vec`, and complex nested types using tree-sitter.
  - **TAURI® v2 API:** Support for `emit`, `listen`, `emitTo`, and `win.emit` with full type validation.
- **Performance Improvements:**
  - **Concurrent Indexing:** Switched to `parking_lot::RwLock` for faster, panic-free state management.
  - **Background Tasks:** Generator discovery and heavy indexing now run on dedicated threads to keep the LSP responsive.
- **Stability & Fixes:**
  - **Smarter Resolution:** Fixed type resolution for shadowed local variables and asymmetric type aliases.
  - **Discovery:** Added TOML support for `tauri-typegen` and improved detection of TAURI config files.
  - **Precision:** Eliminated false positives in `Event` derive detection and redundant diagnostic messages.
- **Internal Refactoring:** Major cleanup of `tree_parser` and `indexer` modules into focused submodules for better maintainability.

### [0.6.2]

- **Diagnostics Fix:** Resolved a false-positive warning where unused TAURI Events were incorrectly reported as unused Commands (`Command '...' is defined but never invoked in frontend`). The internal `DiagnosticInfo` was refactored into a strict enum to cleanly separate `Command` and `Event` evaluation logic.

### [0.6.1]

- **Specta Event Support:** Navigation, CodeLens, hover, references, and diagnostics for tauri-specta's typed event API (`events.X.listen/emit/once`).
- **Standalone specta-typescript:** Added discovery for `specta-typescript` crate (`Typescript::default().export_to(...)`) alongside existing `tauri-specta` support.
- **Removed header-based detection:** Binding files are now discovered exclusively via project config parsing — more reliable, no false positives.

### [0.6.0]

- **Cross-Type Diagnostics:** Real-time type checking for commands and events using generated binding files from [tauri-specta](https://github.com/specta-rs/tauri-specta), [ts-rs](https://github.com/Aleph-Alpha/ts-rs), and [tauri-typegen](https://github.com/thwbh/tauri-typegen).
  - **Param-key validation:** Warns when `invoke()` call passes missing or extra parameter keys compared to the command schema.
  - **Return type diagnostics:** Hints when `invoke()` is missing a generic type parameter, warns when it mismatches the expected return type.
  - **Event payload diagnostics:** Hints when `emit()`/`listen()` is missing a payload type, warns on payload type mismatches.
- **Code Actions for Types:** Quick fixes to add or correct generic type parameters on `invoke()`, `emit()`, and `listen()` calls.
- **Binding File Auto-Detection:** Automatically discovers generated type files by reading project configuration files (`.cargo/config.toml`, `tauri.conf.json`, Rust source) — no extra configuration required.
- **Refactoring:** Improved support for `#[tauri::command]` functions defined outside `#[cfg_attr]` instrument blocks.

### [0.5.0]

- **Improved CodeLens Navigation:** Replaced generic "Go to Rust" labels with specific filenames (e.g., `Go to lib.rs`). Multiple references now appear as distinct, clickable links.
- **Smart Summarization:** References are automatically summarized (e.g., `5 references`) when exceeding the configured limit to keep the UI clean.
- **Settings Validation:** Enforced a minimum value of `0` for `tarus.referenceLimit` to prevent invalid inputs.
- **Stability:** Fixed a panic issue when processing source files containing non-ASCII characters (e.g., Cyrillic).

### [0.4.0]

- **Migration to `Tree-sitter`:** Replaced previous parsers with a unified tree-sitter-based approach for improved accuracy, error handling, and multi-language support (Angular, Vue 3, Svelte, Rust).
- **Performance Improvements:** Introduced debouncing and a dual-layer caching system for a faster and more responsive experience.
- **UX Enhancements:** Added smart diagnostics to reduce noise, enhanced hover information with more context, and implemented a multi-file quick fix for creating Rust commands.
- **Developer Experience:** Added support for import aliases and generic type parameters in `invoke`.
- **Code Quality:** Major refactoring to a modular architecture, improving maintainability and enabling comprehensive testing.

### [0.3.1]

- **Completion:** Autocomplete for command and event names inside TAURI API calls. Triggers only in context (uses `command_syntax.json`).
- **Diagnostics:** For all command/event mismatch diagnostic messages, use the WARNING status.

### [0.3.0]

- **Document Symbols:** View all commands and events in the current file via `Ctrl+Shift+O`.
- **Workspace Symbols:** Search for commands and events across the entire project via `Ctrl+T`.
- **Diagnostics:** Real-time warnings for undefined commands, unlistened events, and unused definitions.

### [0.2.2]

- **Refactor:** use tower-lsp-server instead tower-lsp.

### [0.2.1]

- **Silent by Default:** Removed reference counting logs. The extension now runs silently in the background without spamming "Updated index" messages.
- **Smart Activation:** Strictly validates TAURI projects on startup. Disables LSP features for non-TAURI workspaces.
- **Developer Experience:** Added incremental debug reporting. When `tarus.developerMode` is enabled, saving a file logs a detailed structure report for that file.
- **Performance:** Optimized the indexing loop by removing unnecessary count aggregations.

### [0.2.0]

- **Architecture:** Replaced the Regex-based parser with a dedicated Rust Language Server (LSP).
- **Parsing:** Now utilizes `oxc` (frontend) and `syn` (backend) for accurate AST analysis instead of regex.

### [0.1.0]

- Initial release.
- Regex-based parser.
