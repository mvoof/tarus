<div align="center">
   <img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/icon.png" alt="TARUS Logo" width="120"/>
   <h1>TARUS</h1>
   <p><b>The Missing Link for TAURI® Development</b></p>
   <p>Bridge the gap between Rust and TypeScript with zero configuration.</p>

[![Marketplace](https://img.shields.io/visual-studio-marketplace/v/mvoof.tarus-vscode-extension?style=flat-square&label=Marketplace)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![Installs](https://img.shields.io/visual-studio-marketplace/i/mvoof.tarus-vscode-extension?style=flat-square)](https://marketplace.visualstudio.com/items?itemName=mvoof.tarus-vscode-extension) [![TAURI v2.0](https://img.shields.io/badge/TAURI-v2.0-blue?style=flat-square)](https://tauri.app) [![License](https://img.shields.io/github/license/mvoof/tarus?style=flat-square)](LICENSE)

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

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/Navigation.gif" width="100%"/>

</td>
</tr>

<tr>
<td>

### Autocomplete

Intelligent suggestions for command and event names. suggestions appear as soon as you open a quote in `invoke`, `emit`, or `listen` calls.

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/Autocomplete.gif" width="100%"/>

</td>
</tr>

<tr>
<td>

### Diagnostics

Real-time analysis to detect errors before runtime:

- **Undefined Commands**: Warns if a command is missing from Rust.
- **Payload Validation**: Detects missing or extra keys in arguments.
- **Event Desync**: Identifies unhandled events.

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/Diagnostics.gif" width="100%"/>

</td>
</tr>

<tr>
<td>

### Code Actions

Efficiency tools for common tasks:

- **Add Missing Types**: Insert generic type parameters automatically.
- **Generate Commands**: Create Rust templates from call sites.

</td>
<td align="center">

<img src="https://raw.githubusercontent.com/mvoof/tarus/main/assets/CodeActions.gif" width="100%"/>

</td>
</tr>
</table>

---

## Integration

### Language Support

- **Backend**: Rust
- **Frontend**: TypeScript, JavaScript
- **Frameworks**: React, Vue 3, Svelte, Angular

### Type Generators

TARUS supports automatic discovery of generated bindings from:

- **tauri-specta**
- **ts-rs**
- **tauri-typegen**

---

<div align="center">
   <i>Source code available on <a href="https://github.com/mvoof/tarus">GitHub</a></i>
</div>

TAURI is a trademark of The Tauri Programme within the Commons Conservancy.
