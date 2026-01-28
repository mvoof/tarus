# Contributing

**Contributions are welcome!**

Please follow the steps below to keep the history clean and make reviews easier:

1.  **Fork the repository**
    Create a fork of the main repository in your own GitHub account.

2.  **Clone your fork:**

    ```bash
    git clone https://github.com/mvoof/tarus.git
    cd tarus
    ```

3.  **Add the upstream repository:**

    ```bash
    git remote add upstream https://github.com/mvoof/tarus.git
    ```

    **Verify:**

    ```bash
    git remote -v
    ```

4.  **Create a feature branch:**

    ```bash
    git fetch upstream
    git checkout -b feature/amazing-feature upstream/main
    ```

5.  **Make changes and commit:**

    ```bash
    git commit -m "Add amazing feature"
    ```

6.  **Keep your branch up to date (rebase):**
    Before pushing or opening a Pull Request, rebase your branch onto the latest main.

    ```bash
    git fetch upstream
    git checkout feature/amazing-feature
    git rebase upstream/main
    ```

    **If conflicts occur:**

    ```bash
    git status
    ## resolve conflicts
    git add <files>
    git rebase --continue
    ```

    **To abort the rebase if needed:**

    ```bash
    git rebase --abort
    ```

7.  **Push your branch to your fork**

    **First push:**

    ```bash
    git push origin feature/amazing-feature
    ```

    **If you already pushed before and rebased:**

    ```bash
    git push --force-with-lease origin feature/amazing-feature
    ```

8.  **Open a Pull Request:**
    - Base repository: [mvoof/tarus](https://github.com/mvoof/tarus.git)
    - Base branch: **main**
    - Compare branch: your-username:feature/amazing-feature

    Make sure:
    - the branch is rebased onto the latest main
    - CI checks pass
    - there are no unresolved conflicts

## Important rules

- rebase only your own feature branches
- never rebase main
- do not push directly to main
- one feature branch per Pull Request

## Commit messages

Commit messages should follow the [Conventional Commits](https://conventionalcommits.org) specification:

```
<type>[optional scope]: <description>
```

### Allowed `<type>`

- `chore`: any repository maintainance changes
- `feat`: code change that adds a new feature
- `fix`: bug fix
- `perf`: code change that improves performance
- `refactor`: code change that is neither a feature addition nor a bug fix nor a performance improvement
- `docs`: documentation only changes
- `ci`: a change made to CI configurations and scripts
- `style`: cosmetic code change
- `test`: change that only adds or corrects tests
- `revert`: change that reverts previous commits

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
