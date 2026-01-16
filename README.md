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

## License

[MIT](./LICENSE) © 2026 mvoof

_TAURI is trademark of [The Tauri Programme within the Commons Conservancy]_
