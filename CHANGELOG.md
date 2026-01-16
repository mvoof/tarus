## Change Log

### [0.3.0]

- **Document Symbols:** View all commands and events in the current file via `Ctrl+Shift+O`.
- **Workspace Symbols:** Search for commands and events across the entire project via `Ctrl+T`.
- **Completion:** Autocomplete for command and event names inside `invoke`, `emit`, and `listen` calls.
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
