# Contributing

> [!NOTE]
> Please prefer English language for all communication.

## Creating an issue

Before creating an issue please ensure that the problem is not [already reported](https://github.com/mvoof/tarus/issues).

## How to Contribute

1. **Fork and Clone the Repository**

   First, create your own copy of the repository by clicking the "Fork" button on GitHub. Then, clone your fork to your local machine:

   ```sh
   git clone https://github.com/your-username/tarus.git
   cd tarus
   git remote add upstream https://github.com/mvoof/tarus.git
   ```

2. **Create a New Branch**

   ```sh
   git checkout -b feature/short-description
   ```

3. **Make Changes**

   Implement your feature or fix the bug. Be sure to follow the project's coding style and add tests if necessary.

4. **Commit Changes**

   Before committing, ensure your code is clean and functional:
   - Run linting and formatting: `npm run lint` and `npm run format`
   - Build the extension: `npm run vscode:prepublish`
   - Run tests: `cargo test --manifest-path lsp-server/Cargo.toml`
   - Check for warnings: `cargo clippy --manifest-path lsp-server/Cargo.toml`

   Once verified, commit your changes:

   ```sh
   git add .
   git commit -m "feat: add new super feature"
   ```

5. **Keep Your Branch Up to Date**

   Before pushing, make sure your branch is rebased on top of the latest `main` to avoid merge conflicts and keep the history clean:

   ```sh
   git fetch upstream
   git rebase upstream/main
   ```

   If conflicts arise, resolve them, then continue:

   ```sh
   git rebase --continue
   ```

6. **Push Changes**

   ```sh
   git push -u origin feature/short-description
   ```

   If you had to rebase after already pushing, use `--force-with-lease`:

   ```sh
   git push --force-with-lease
   ```

7. **Create a Pull Request**

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

If you have any questions or need help, feel free to open an issue or ask in the discussions section. We appreciate your contributions!
