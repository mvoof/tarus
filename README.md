[![Installs](https://img.shields.io/vscode-marketplace/i/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![Version](https://img.shields.io/vscode-marketplace/v/mvoof.tarus-vscode-extension)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![License](https://img.shields.io/github/license/mvoof/tarus)](LICENSE)

<div align="center">
   <img src="https://raw.githubusercontent.com/mvoof/tarus/main/images/icon.png" alt="TARUS Logo" width="100" align="center"/> 
   <h1>TARUS</h1>

   <p>A VS Code extension that provides convenient cross-language navigation between commands/events in frontend and backend code in the IDE (for TAURI® projects).</p>
</div>

---

<div align="center">
   <i>This extension is not officially supported by the Tauri team and is provided as-is. It is maintained by a third party and may not receive updates or bug fixes in a timely manner. Use at your own risk.</i>
</div>

## Features

- **Go to Definition**: Ctrl+Click (or F12) on frontend event/command names to jump to the Rust implementation.
- **Hover Information**: Display command/event details on hover in frontend code.
- **CodeLens Navigation**:

  | Direction           | Action                     |
  | ------------------- | -------------------------- |
  | **Frontend → Rust** | `Go to Rust: my-command`   |
  | **Rust → Frontend** | `Go to Frontend: my-event` |

### Demo

![Demo](https://raw.githubusercontent.com/mvoof/tarus/main/images/demo.gif)

## Installation

1. **From VS Code Marketplace** (Recommended):
   - Open VS Code.
   - Go to **Extensions** view (`Ctrl+Shift+X`).
   - Search for **"Tarus"**.
   - Click **Install**.

2. **From VSIX File**:

   ```bash
   code --install-extension tarus.v0.0.1.vsix
   ```

3. **From Source (Development)**:
   ```bash
   npm install -g @vscode/vsce
   git clone https://github.com/mvoof/tarus
   cd tarus
   vsce package
   ```

## Usage

1.  **Open a Project:**

    ```
    my-tauri-app/
    ├── src/ # Frontend root (default)
    └── src-tauri/src/ # Rust root (default)
    ```

2.  **Navigation Examples:**
    Frontend (TSX):

    ```
    import { invoke } from '@tauri-apps/api/core';
    invoke('my_command');  // Ctrl+Click → Jump to Rust fn my_command()
    ```

    Rust:

    ```
    #[tauri::command]
    fn my*command() { /* ... \_/ }
    app.emit("my-event", &payload); // CodeLens: "Go to Frontend: my-event"
    ```

## Development

1. **Setup:**

   ```bash
   npm install
   ```

2. **Compile & Watch:**

   ```
   npm run compile
   press 'F5' for run extension and test
   ```

3. **Lint:**

   ```bash
   npm run lint
   npm run lint:fix
   ```

4. **Package:**
   > for testing the assembly
   ```bash
   npm run compile
   vsce package
   ```

## Contributing

Contributions are welcome! Please:

- Fork the repository.
- Create a feature branch (git checkout -b feature/amazing-feature).
- Commit changes (git commit -m 'Add amazing feature').
- Push (git push origin feature/amazing-feature).
- Open a Pull Request.

## License

[MIT](./LICENSE) © 2025 mvoof

_TAURI is trademark of [The Tauri Programme within the Commons Conservancy]_
