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

## Support and Integration

### Language Support

- **Backend**: Rust (`#[tauri::command]`, `AppHandle::emit`, `Window::listen`)
- **Frontend**: TypeScript, JavaScript, JSX, TSX
- **Frameworks**: React, Vue 3 (SFC), Svelte, Angular

### Type Generator Support

TARUS reads generated bindings to provide structural type checking for:

- **tauri-specta** (both `.export()` and standalone `.export_to()`)
- **specta** / **specta-typescript** (standalone `.export_to()`)
- **ts-rs**
- **tauri-typegen**

#### Supported Features

- **Hybrid Type Checking**: Tarus provides protection even without generated bindings:
  - **Zero-Config (RustSource)**: For basic types (`string`, `number`, `boolean`, `any`, `null`, `undefined`), arrays, and unions (like `Option<T>` → `T | null`), Tarus extracts information directly from Rust source code to provide instant validation.

  - **Full Structural Safety**: For custom structs and complex interfaces, Tarus integrates with binding generators (Specta, ts-rs, Typegen) to ensure your TypeScript objects match your Rust data models exactly.

- **TypeScript Definitions**: Full support for both `export type` aliases and `export interface` blocks.
- **Collection Types**: Recursive support for array notation (e.g., `User[]`, `Array<User>`, `Vec<T>` → `T[]`).
- **Rust Type Detection**: For event payloads (`app.emit(...)`), Tarus identifies types through:
  - **Explicit type annotations**: `let data: MyType = ...;`
  - **Direct struct instantiation**: `let data = MyType { ... };`
  - **Expression Resolution**: Correctly handles references (`&data`) and parenthesized expressions.

> [!IMPORTANT]
> **Type Inference Limitations:** Tarus uses **Tree-sitter** for surgical AST analysis rather than being a full language compiler. It does not perform deep semantic analysis or cross-function type inference. To enable diagnostics for event payloads, the type must be explicitly declared at the emission site (e.g., via type annotation or direct struct instantiation).

---

<div align="center">
   <i>Source code available on <a href="https://github.com/mvoof/tarus">GitHub</a></i>
</div>

TAURI is a trademark of The Tauri Programme within the Commons Conservancy.
