<div align="center">
   <img src="https://raw.githubusercontent.com/mvoof/tarus/main/extension/images/icon.png" alt="TARUS Logo" width="120"/>
   <h1>TARUS</h1>

   <p>
    A <a href=
    "https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension"> VS Code extension </a>for cross-language navigation for TAURI® apps.<br>
    Seamlessly jump between commands/events in frontend and backend code.
   </p>

[![Installs](https://img.shields.io/vscode-marketplace/i/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![Version](https://img.shields.io/vscode-marketplace/v/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![License](https://img.shields.io/github/license/mvoof/tarus)](LICENSE)

</div>

---

<div align="center">
   <i>This extension is not officially supported by the Tauri team and is provided as-is. It is maintained by a third party and may not receive updates or bug fixes in a timely manner. Use at your own risk.</i>
</div>

## Features

### 1. Go to Definition (F12)

Instantly jump from a frontend `invoke` or `emit` call directly to the corresponding Rust command handler or event listener.

- **Frontend → Backend:** Ctrl+Click on `invoke('my_command')` opens the Rust file at `fn my_command`.
- **Backend → Frontend:** Ctrl+Click on `app.emit("my-event")` shows where this event is listened to in React/Vue/Svelte.

### 2. Find References (Shift+F12)

See all places where a specific command or event is used across both TypeScript and Rust files.

### 3. Smart CodeLens

Contextual buttons appear above your commands and events to show usage stats or provide quick navigation.

| Context           | CodeLens Preview | Action                                       |
| :---------------- | :--------------- | :------------------------------------------- |
| **Rust Command**  | `Go to Frontend` | Jumps to the TS file calling this command.   |
| **Frontend Call** | `Go to Rust`     | Jumps to the Rust implementation.            |
| **Multiple Uses** | `3 References`   | Opens a peek view to choose the destination. |

![Demo](https://raw.githubusercontent.com/mvoof/tarus/main/assets/demo.gif)

## License

[MIT](./LICENSE) © 2025 mvoof

_TAURI is trademark of [The Tauri Programme within the Commons Conservancy]_
