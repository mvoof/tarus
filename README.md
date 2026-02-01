<div align="center">
   <img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/icon.png" alt="TARUS Logo" width="120"/>
   <h1>TARUS</h1>

   <p>
    A <a href="https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension">VS Code extension</a> — coding assistant for TAURI® apps.<br>
    Navigation, autocomplete, diagnostics, and symbols for commands and events.
   </p>

[![Installs](https://img.shields.io/visual-studio-marketplace/i/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![Version](https://img.shields.io/visual-studio-marketplace/v/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![Tauri v2.0](https://img.shields.io/badge/Tauri-v2.0-blue)](https://tauri.app) [![License](https://img.shields.io/github/license/mvoof/tarus)](LICENSE)

</div>

---

<div align="center">
   <i>This extension is not officially supported by the Tauri team and is provided as-is. It is maintained by a third party and may not receive updates or bug fixes in a timely manner. Use at your own risk.</i>
</div>

## Features

<table>
<tr>
<td width="50%">

**Navigation**

**Go to Definition (F12)** — Jump from frontend `invoke`/`emit` to Rust handlers.

- Frontend → Backend: Ctrl+Click on `invoke('cmd')` opens Rust `fn cmd`
- Backend → Frontend: Ctrl+Click on `app.emit("event")` shows listeners

**Find References (Shift+F12)** — See all usages across TS and Rust files.

**Smart CodeLens** — Contextual buttons above commands/events for quick navigation.

</td>
<td align="center">

https://github.com/user-attachments/assets/433d32bc-5e61-474c-bd24-929dc3930f9a

</td>
</tr>
<tr>
<td>

**Autocomplete**

Start typing inside `invoke("`, `emit("`, or `listen("` to get suggestions for all known commands and events in your project.

</td>
<td align="center">

https://github.com/user-attachments/assets/3d441c79-6222-4433-8cd4-1aeebc2e1419

</td>
</tr>
<tr>
<td>

**Diagnostics**

Real-time warnings for mismatched commands and events:

- Command invoked but not defined in Rust
- Event listened for but never emitted
- Command defined but never invoked
- Event emitted but never listened to

</td>
<td align="center">

https://github.com/user-attachments/assets/cce70622-e214-4fd9-a796-b2dac428003e

</td>
</tr>
<tr>
<td>

**Code Actions**

Quick fixes for undefined commands:

- Press `Ctrl+.` on an undefined command to see code actions
- Choose which file to create the Rust command in: `lib.rs`, `main.rs`, or other modules
- Command template is automatically inserted at the optimal location

**Note:** Code actions are only available for commands

</td>

<td align="center">

https://github.com/user-attachments/assets/5b94de95-6341-4e97-bb73-5cdb1393a43e

</td>
</tr>
</table>

### Symbols

Workspace Symbols (Ctrl+T) — Search commands/events across entire project.

### Import Aliases

TARUS fully supports import aliases, a common JavaScript/TypeScript pattern:

```typescript
import { invoke as myInvoke, emit as sendEvent } from "@tauri-apps/api/core";

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

### Extension Settings

This extension contributes the following settings:

*   `tarus.developerMode` (boolean): Enables detailed logging and internal diagnostics (for extension developers). Requires VS Code restart. Default: `false`.
*   `tarus.referenceLimit` (number): The maximum number of individual file links to show in CodeLens before summarizing them (e.g., '5 references'). Minimum value is 0. Default: `3`.

## Supported Languages

TARUS supports the following languages and frameworks:

| Language       | Extensions      | Features                                                        |
| :------------- | :-------------- | :-------------------------------------------------------------- |
| **Rust**       | `.rs`           | Command definitions (`#[tauri::command]`), event emit/listen    |
| **TypeScript** | `.ts`, `.tsx`   | `invoke()`, `emit()`, `listen()`, generic calls (`invoke<T>()`) |
| **JavaScript** | `.js`, `.jsx`   | Same as TypeScript                                              |
| **Vue**        | `.vue`          | Script sections with TypeScript/JavaScript                      |
| **Svelte**     | `.svelte`       | Script sections with TypeScript/JavaScript                      |
| **Angular**    | `.component.ts` | TypeScript in Angular components                                |

## Advanced: Tree-sitter Queries

TARUS uses [Tree-sitter](https://tree-sitter.github.io/tree-sitter/) for parsing source code. The parsing patterns are defined in `.scm` query files located in `lsp-server/src/queries/`:

| File             | Description                 |
| :--------------- | :-------------------------- |
| `rust.scm`       | Rust commands and events    |
| `typescript.scm` | TypeScript/JavaScript calls |
| `javascript.scm` | JavaScript calls            |

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
