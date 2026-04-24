<div align="center">
   <img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/icon.png" alt="TARUS Logo" width="120"/>
   <h1>TARUS</h1>

   <p>
    <b>The Missing Link for TAURI® Development</b><br>
    Bridge the gap between Rust and TypeScript with zero configuration.
   </p>

[![Marketplace](https://vsmarketplacebadges.dev/version-short/mvoof.tarus-vscode-extension.svg?style=flat-square&label=Marketplace)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![Installs](https://vsmarketplacebadges.dev/installs-short/mvoof.tarus-vscode-extension.svg?style=flat-square)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![TAURI v2.0](https://img.shields.io/badge/TAURI-v2.0-blue?style=flat-square)](https://tauri.app) [![License](https://img.shields.io/github/license/mvoof/tarus?style=flat-square)](LICENSE) [![GitHub Stars](https://img.shields.io/github/stars/mvoof/tarus?style=flat-square)](https://github.com/mvoof/tarus/stargazers)

</div>

<div align="center">
   <i>This extension is not officially supported by the TAURI team and is provided as-is. It is maintained by a third party and may not receive updates or bug fixes in a timely manner. Use at your own risk.</i>
</div>

---

## Introduction

In TAURI® projects, it’s often difficult to quickly navigate between the frontend and the Rust backend and understand where a command is defined or called.
This extension is designed to make that navigation fast and effortless, allowing you to jump between the frontend and backend parts of the project in both directions.
As secondary features, the extension can also detect unused commands and help autocomplete command names while typing.

---

## Features

<table width="100%">
<tr>
<td width="50%">

### Navigation

Jump from frontend `invoke` or `emit` calls directly to their Rust implementations using **F12 (Go to Definition)**. Use **Shift+F12** to find all frontend call sites from a Rust function.

- Frontend to Backend navigation
- Backend to Frontend references
- Support for complex project structures

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/Navigation.gif" width="100%"/>

</td>
</tr>

<tr>
<td>

### Autocomplete

Intelligent suggestions for command and event names. Suggestions appear as soon as you open a quote in `invoke`, `emit`, or `listen` calls, eliminating manual searching.

- Context-aware suggestions
- Support for command and event names
- Real-time indexing of new definitions

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/Autocomplete.gif" width="100%"/>

</td>
</tr>

<tr>
<td>

### Diagnostics

Real-time analysis to detect errors before they reach the browser:

- **Undefined Commands**: Warns if you invoke a command missing from Rust.
- **Payload Validation**: Detects missing or extra keys in `invoke` arguments.
- **Event Desync**: Identifies unlistened or unhandled events.

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/Diagnostics.gif" width="100%"/>

</td>
</tr>

<tr>
<td>

### Code Actions

Quick fixes for development efficiency:

- **Add Missing Types**: Automatically insert generic type parameters.
- **Generate Commands**: Create Rust command templates directly from frontend call sites.

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/CodeActions.gif" width="100%"/>

</td>
</tr>
</table>

---

## Support and Integration

### Language Support

- **Backend**: Rust (`#[tauri::command]`, `AppHandle::emit`, `Window::listen`)
- **Frontend**: TypeScript, JavaScript, JSX, TSX
- **Frameworks**: React, Vue 3 (SFC), Svelte, Angular

### Type Generator Support

TARUS reads generated bindings to provide structural type checking for:

- **tauri-specta**
- **ts-rs**
- **tauri-typegen**

---

## Documentation

- **[Changelog](./CHANGELOG.md)** — Version history and release notes.
- **[Architecture Guide](./docs/ARCHITECTURE.md)** — Deep dive into Tree-sitter and LSP internals.
- **[Contributing](./CONTRIBUTING.md)** — Guidelines for building and testing.

## License

[MIT](./LICENSE) © 2026 mvoof

TAURI is a trademark of The Tauri Programme within the Commons Conservancy.
