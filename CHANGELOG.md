## Change Log

### [Unreleased]

- **Performance**
  - Fixed the startup scan taking minutes on larger projects, with CodeLens and diagnostics empty until it finished. Each file compiled the language's tree-sitter query again (~75 ms for TypeScript, longer than parsing the file), twice per file at startup and once per edit. The queries are now compiled once: on a project of ~980 files the scan went from ~3 minutes to ~5 seconds

- **Bug fixes**
  - Fixed build output and other git-ignored files being indexed: a Storybook build or any folder outside the built-in list (`node_modules`, `target`, `dist`, ...) filled the constants with minified names and slowed the scan. The scan and the files the editor opens or the file watcher reports now follow the same rules git does: `.gitignore` in every folder, `.ignore`, `.git/info/exclude` and the global excludes file, with the built-in list still applied on top. Files a binding generator writes are indexed even when git ignores them. An edit to an ignore file takes effect after a server restart
  - Fixed generator discovery reading every `.rs` file under `src-tauri`, `target/` included

- **Added**
  - A warning in the output for each indexed file over 512 KB, which is usually build output: it names the file and suggests adding it to `.gitignore`, or opening an issue if Tarus should skip such files by default. The file is still indexed

### [0.13.0] - 2026-10-17

- **Bug fixes**
  - Fixed calls through a constant or a forwarding helper keeping the old name after that constant's value or that helper changed in another file. A call stores the name it resolved to when its file was parsed, so `emit(EVENT_X)` stayed linked to the old event until its own file was edited. Files that use a changed constant or helper are now parsed again, including through a `const A: &str = B;` chain. Each file records the names it resolved while it was parsed, so only those files are read: a file that merely mentions the name in a comment or a string is left alone
  - Fixed a deleted file's constants, command and event signatures and generated types staying in the index, so names kept resolving to deleted values and hover, completion and type checks kept using deleted signatures. A signature another file also declares is kept, from that file. A file deleted on disk while open in the editor also loses its warnings once its tab is closed
  - Fixed a closed file keeping its unsaved editor text in the index: closing discards those edits, so the file is now indexed from disk again
  - Fixed the startup scan overwriting an edit made while it was still running with the older text on disk, and the startup diagnostics overwriting fresher ones published for an edit made meanwhile
  - Fixed a file that does not parse mid-edit (an unterminated string, a half-typed call) dropping or mangling the constants and forwarding helpers it declares, which turned their calls in every other file into unresolved names on each typing pause. A file that does not parse now keeps its last good constants and helpers until it parses again. A Rust file with syntax errors is still indexed from the parts that parsed, as before
  - Fixed diagnostics being published from a half-built index while the startup scan was still running, after opening, closing or editing a file. They are published once the scan is done
  - Fixed a command or event signature disappearing when the file it was taken from stopped declaring it while another file still did: removing a command from one of two files that define it, or regenerating bindings without a command the Rust source still has. Hover, completion and type checks broke for it until the other file was edited; the other file's signature is now used
  - Fixed adding, regenerating or deleting a bindings file not updating "never emitted" / "no listeners" warnings for the events it declares
  - Fixed a member of something that is not a name (`emit(getEvents().STARTED)`, `emit(Events[key].STARTED)`) being resolved through an unrelated constant of the same last name. They now get the `tarus/unresolved-name` hint
  - Fixed a name literal with an escape sequence (`emit("run\u003Aa")`, `listen('a\'b')`) being indexed as its text up to the first escape (`run`). Escapes are now decoded, surrogate pairs (`"\uD83D\uDE00"`) included
  - Fixed diagnostics being computed on the language server's async threads, which stalled hover and completion while many files were republished
  - Fixed regenerating a bindings file not refreshing payload and return-type checks in the files that call those commands and events
  - Fixed event and command names built by an expression in TypeScript/JavaScript (`emit("run://" + id)`, `invoke(nameFor(id))`, `listen(cond ? a : b)`, `emit(await nameFor(id))`) being ignored entirely. Such a call is now treated as an unresolved name: it gets the `tarus/unresolved-name` hint and silences "never emitted" / "no listeners" / "never invoked" like any other unresolved call, instead of letting Tarus report an event as never emitted while code emits it. Parentheses and type-only wrappers are seen through, so `emit(("ready"))`, `emit(EVENT as string)`, `listen(name!, …)`, `listen(<string>name, …)` and `invoke(cmd satisfies Cmd)` resolve like the bare literal or constant, and `emit(Events["STARTED"])` resolves like `emit(Events.STARTED)`, escapes in the key decoded
  - Fixed "never emitted", "no listeners" and "never invoked" warnings in other files not updating when a call's name became resolvable or unresolvable (e.g. replacing a `concat!` name with a string). Such a call shares no name with those files, so they kept their old diagnostics until edited themselves; now every file holding such a warning is refreshed when that happens
  - Fixed saving a file, or changing or deleting it outside the editor, refreshing diagnostics in that file only. Files sharing a command or event with it are now refreshed too, as they already were while typing
  - Fixed event and command names declared as constants inside a macro body (`my_names! { pub const EVENT_X: &str = "x" => X; }`) not being resolved. Tree-sitter reads a macro body as plain tokens, so such a const was never indexed: every `emit(EVENT_X, ..)` became an unknown name, its `listen` lost navigation and CodeLens, and one unknown emit turned off "never emitted" / "no listeners" checks for the whole workspace
  - Fixed editing one file dropping a same-named constant declared in another file, in Rust and in TypeScript/JavaScript. Two files declaring `EVENT_X` shared one entry, so re-indexing either removed it for both, and every call through `EVENT_X` became an unknown name until the other file was indexed again. Each file now keeps and removes only its own declarations

- **Added**
  - Rust constants written as raw strings (`const EVENT_X: &str = r"x";`, `r#"x"#`) are now resolved as command and event names, in an item or a macro body
  - Rust constants that name another constant (`const EVENT_X: &str = OTHER;`, `crate::events::OTHER`) now resolve through it, across files and through chains. Only a string constant is read this way — typed `&str`, `&'static str`, or a type alias of one (`type EventName = &'static str;`, from any file) — so `const L: Level = Level::Info;` is never taken for a name. A chain that loops or ends in an unknown name is left unresolved instead of guessed
  - A hint (`tarus/unresolved-name`) on every `invoke`/`emit`/`listen` whose name Tarus cannot read as a string — `concat!`, a value computed at runtime, a field of a parameter. Such a call already switched "never emitted", "no listeners" or "never invoked" warnings off for the whole workspace, silently; the hint now says so where the call is written

### [0.12.2] - 2026-09-30

- **Bug fixes**
  - Fixed files from excluded folders (`node_modules`, `target`, `dist`, ...) and `.d.ts` files being indexed when opened in the editor. The workspace scan skipped them, but opening one (Go to Definition into a library, a search result) indexed it anyway, so a library's own `invoke("...")` calls showed up as "command is not defined in Rust backend" warnings

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
