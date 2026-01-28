## Change Log

### [0.4.0]

- **Migration to `Tree-sitter`:** Replaced previous parsers with a unified tree-sitter-based approach for improved accuracy, error handling, and multi-language support (Angular, Vue 3, Svelte, Rust).
- **Performance Improvements:** Introduced debouncing and a dual-layer caching system for a faster and more responsive experience.
- **UX Enhancements:** Added smart diagnostics to reduce noise, enhanced hover information with more context, and implemented a multi-file quick fix for creating Rust commands.
- **Developer Experience:** Added support for import aliases and generic type parameters in `invoke`.
- **Code Quality:** Major refactoring to a modular architecture, improving maintainability and enabling comprehensive testing.

### [0.3.1]
- **Completion:** Autocomplete for command and event names inside Tauri API calls. Triggers only in context (uses `command_syntax.json`).
- **Diagnostics:** For all command/event mismatch diagnostic messages, use the WARNING status.

### [0.3.0]

- **Document Symbols:** View all commands and events in the current file via `Ctrl+Shift+O`.
- **Workspace Symbols:** Search for commands and events across the entire project via `Ctrl+T`.
- **Diagnostics:** Real-time warnings for undefined commands, unlistened events, and unused definitions.

### [0.2.2]

- **Refactor:** use tower-lsp-server  instead tower-lsp.

### [0.2.1]

- **Silent by Default:** Removed reference counting logs. The extension now runs silently in the background without spamming "Updated index" messages.
- **Smart Activation:** Strictly validates Tauri projects on startup. Disables LSP features for non-Tauri workspaces.
- **Developer Experience:** Added incremental debug reporting. When `tarus.developerMode` is enabled, saving a file logs a detailed structure report for that file.
- **Performance:** Optimized the indexing loop by removing unnecessary count aggregations.

### [0.2.0]

- **Architecture:** Replaced the Regex-based parser with a dedicated Rust Language Server (LSP).
- **Parsing:** Now utilizes `oxc` (frontend) and `syn` (backend) for accurate AST analysis instead of regex.

### [0.1.0]

- Initial release.
- Regex-based parser.
