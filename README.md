<div align="center">
   <img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/icon.png" alt="TARUS Logo" width="120"/>
   <h1>TARUS</h1>

   <p>
    A <a href="https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension">VS Code extension</a> — full-featured development toolkit for TAURI® apps.<br>
    Navigation, autocomplete, diagnostics, and symbols for commands and events.
   </p>

[![Installs](https://img.shields.io/visual-studio-marketplace/i/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![Version](https://img.shields.io/visual-studio-marketplace/v/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![License](https://img.shields.io/github/license/mvoof/tarus)](LICENSE)

</div>

---

<div align="center">
   <i>This extension is not officially supported by the Tauri team and is provided as-is. It is maintained by a third party and may not receive updates or bug fixes in a timely manner. Use at your own risk.</i>
</div>

## Features

### Navigation

#### Go to Definition (F12)

Instantly jump from a frontend `invoke` or `emit` call directly to the corresponding Rust command handler or event listener.

- **Frontend → Backend:** Ctrl+Click on `invoke('my_command')` opens the Rust file at `fn my_command`.
- **Backend → Frontend:** Ctrl+Click on `app.emit("my-event")` shows where this event is listened to in React/Vue/Svelte.

#### Find References (Shift+F12)

See all places where a specific command or event is used across both TypeScript and Rust files.

#### Smart CodeLens

Contextual buttons appear above your commands and events to show usage stats or provide quick navigation.

| Context           | CodeLens Preview | Action                                       |
| :---------------- | :--------------- | :------------------------------------------- |
| **Rust Command**  | `Go to Frontend` | Jumps to the TS file calling this command.   |
| **Frontend Call** | `Go to Rust`     | Jumps to the Rust implementation.            |
| **Multiple Uses** | `3 References`   | Opens a peek view to choose the destination. |

### Autocomplete

Start typing inside `invoke("`, `emit("`, or `listen("` and get suggestions for all known commands and events in your project.

### Diagnostics

Real-time warnings for mismatched commands and events:

- **Warning:** Command invoked but not defined in Rust backend
- **Warning:** Event listened for but never emitted
- **Hint:** Command defined but never invoked
- **Hint:** Event emitted but never listened to

### Symbols

#### Document Symbols (Ctrl+Shift+O)

Quick outline of all commands and events in the current file.

#### Workspace Symbols (Ctrl+T)

Search for any command or event across your entire project.

![Demo](https://raw.githubusercontent.com/mvoof/tarus/main/assets/demo.gif)

## Supported Languages

TARUS supports the following languages and frameworks:

| Language   | Extensions               | Features                                      |
| :--------- | :----------------------- | :-------------------------------------------- |
| **Rust**   | `.rs`                    | Command definitions (`#[tauri::command]`), event emit/listen |
| **TypeScript** | `.ts`, `.tsx`        | `invoke()`, `emit()`, `listen()`, generic calls (`invoke<T>()`) |
| **JavaScript** | `.js`, `.jsx`        | Same as TypeScript |
| **Vue**    | `.vue`                   | Script sections with TypeScript/JavaScript |
| **Svelte** | `.svelte`                | Script sections with TypeScript/JavaScript |
| **Angular** | `.component.ts`         | TypeScript in Angular components |

### Import Aliases

TARUS fully supports import aliases, a common JavaScript/TypeScript pattern:

```typescript
import { invoke as myInvoke, emit as sendEvent } from '@tauri-apps/api/core';

// These will be correctly recognized
await myInvoke("my_command");
sendEvent("my_event");
```

### Generic Type Parameters

Generic type calls are fully supported:

```typescript
// All these patterns work with navigation, autocomplete, and diagnostics
await invoke<number>("get_count");
await invoke<Session>("get_session", { id: 1 });
emit<void>("event_name");
```

## Advanced: Tree-sitter Queries

TARUS uses [Tree-sitter](https://tree-sitter.github.io/tree-sitter/) for parsing source code. The parsing patterns are defined in `.scm` query files located in `lsp-server/src/queries/`:

| File              | Description                                      |
| :---------------- | :----------------------------------------------- |
| `rust.scm`        | Patterns for Rust commands and events            |
| `typescript.scm`  | Patterns for TypeScript/JavaScript calls         |
| `javascript.scm`  | Patterns for JavaScript (same as TypeScript)     |

### Query File Structure

Query files use S-expression syntax to match AST patterns. Example from `rust.scm`:

```scheme
; Match #[tauri::command] fn name()
(
  (attribute_item
    (attribute
      (scoped_identifier
        path: (identifier) @_attr_path
        name: (identifier) @_attr_name)))
  .
  (function_item
    name: (identifier) @command_name)
  (#eq? @_attr_path "tauri")
  (#eq? @_attr_name "command")
)
```

### Customizing Queries

If you need to support custom patterns (e.g., wrappers around Tauri APIs), you can modify the query files:

1. Clone the repository
2. Edit the relevant `.scm` file in `lsp-server/src/queries/`
3. Rebuild the LSP server: `cargo build --release --manifest-path lsp-server/Cargo.toml`
4. Replace the binary in `extension/bin/`

**Note:** Query files are embedded at compile time, so changes require rebuilding the server.

## License

[MIT](./LICENSE) © 2026 mvoof

_TAURI is trademark of [The Tauri Programme within the Commons Conservancy]_
