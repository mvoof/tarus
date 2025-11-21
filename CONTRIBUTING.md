# Contributing

Contributions are welcome!

1.Fork the repository.
2.Create a feature branch (git checkout -b feature/amazing-feature).
3.Commit changes (git commit -m 'Add amazing feature').
4.Push to the branch (git push origin feature/amazing-feature).
5.Open a Pull Request.

## Development

This extension consists of a Node.js Client (VS Code Extension) and a Rust Language Server.

### Setup & Build

1.  **Clone the repository:**

    ```bash
    git clone https://github.com/mvoof/tarus
    cd tarus
    npm install
    ```

2.  **Build everything (Client + Server): This command compiles the TypeScript client, builds the Rust binary (release mode), and copies it to the correct bin folder:**

    ```bash
    npm run vscode:prepublish
    ```

    then run the extension from VSIX file:

    ```bash
    cd extension
    vsce package
    ```

3.  **Run in Debug Mode:**

- Open the project in VS Code.
- Press F5.
  This will launch a new "Extension Development Host" window with Tarus active.

### Formatting & Linting

```bash
 npm run format      # Format TS/JSON files
 npm run lint:fix    # Fix ESLint errors
```
