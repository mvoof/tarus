## Change Log

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
