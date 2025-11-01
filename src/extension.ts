import * as vscode from "vscode";
import TauriIndexer from "./symbol-indexer";
import { CodeLensType } from "./types";

const output = vscode.window.createOutputChannel("TARUS");

const FRONTEND_LANGUAGES = [
  "typescript",
  "typescriptreact",
  "javascript",
  "javascriptreact",
  "vue",
] as const;

const RUST_LANGUAGES = ["rust"] as const;

/** Extract event/command name under cursor */
function extractSymbolAtPosition(
  document: vscode.TextDocument,
  position: vscode.Position
): { name: string; range: vscode.Range } | undefined {
  const wordRange = document.getWordRangeAtPosition(position, /["'][^"']+["']/);

  if (!wordRange) {
    return undefined;
  }

  const text = document.getText(wordRange);
  const name = text.slice(1, -1);

  return { name, range: wordRange };
}

export function activate(context: vscode.ExtensionContext): void {
  const config = vscode.workspace.getConfiguration("tarus");

  output.appendLine("Taurus: Extension activated");

  const useReferences =
    config.get<CodeLensType>("codeLensAction", "open") === "references";

  const indexer = new TauriIndexer(config);

  const triggerRescan = () => indexer.trigger();

  context.subscriptions.push(
    vscode.commands.registerCommand("tarus.rescan", triggerRescan),

    vscode.workspace.onDidSaveTextDocument((doc) => {
      if (doc.fileName.includes("src-tauri") || doc.fileName.includes("src/")) {
        triggerRescan();
      }
    }),

    // Definition: Frontend → Rust
    vscode.languages.registerDefinitionProvider(FRONTEND_LANGUAGES, {
      provideDefinition(document, position) {
        const symbol = extractSymbolAtPosition(document, position);

        return symbol ? indexer.getDefinition(symbol.name) : undefined;
      },
    }),

    // Hover: Show symbol info
    vscode.languages.registerHoverProvider(FRONTEND_LANGUAGES, {
      provideHover(document, position) {
        const symbol = extractSymbolAtPosition(document, position);
        if (!symbol) {
          return undefined;
        }

        const entry = [...indexer.getSymbols()].find(
          ([k]) => k === symbol.name
        )?.[1];
        if (!entry) {
          return undefined;
        }

        const relPath = vscode.workspace.asRelativePath(entry.location.uri);
        const kind = entry.type === "command" ? "Command" : "Event";
        const md = new vscode.MarkdownString(
          `**Tauri ${kind}**: \`${symbol.name}\`\n\`${relPath}\``
        );
        md.isTrusted = true;

        return new vscode.Hover(md, symbol.range);
      },
    }),

    // Codelens Provider
    vscode.languages.registerCodeLensProvider(
      [...FRONTEND_LANGUAGES, ...RUST_LANGUAGES],
      {
        provideCodeLenses(document: vscode.TextDocument): vscode.CodeLens[] {
          const lenses: vscode.CodeLens[] = [];
          const text = document.getText();

          const isFrontendSide = FRONTEND_LANGUAGES.includes(
            document.languageId as (typeof FRONTEND_LANGUAGES)[number]
          );

          for (const [name, symbol] of indexer.getSymbols()) {
            // Skip symbols not from the current file (only for Rust → Frontend)
            if (
              !isFrontendSide &&
              symbol.location.uri.fsPath !== document.uri.fsPath
            ) {
              continue;
            }

            // Skip if there are no usages (only for Rust → Frontend)
            if (!isFrontendSide && indexer.getUsages(name).length === 0) {
              continue;
            }

            let range: vscode.Range;

            if (isFrontendSide) {
              // Frontend: Find name in quotes - same for all
              const quotedName = `"${name}"`;
              const matchIndex = text.indexOf(quotedName);

              if (matchIndex === -1) {
                continue;
              }

              const lineNum =
                text.substring(0, matchIndex).split("\n").length - 1;
              const lineStart = text.lastIndexOf("\n", matchIndex) + 1;

              const nameStartCol = matchIndex - lineStart + 1;
              const nameEndCol = nameStartCol + name.length;

              range = new vscode.Range(
                lineNum,
                nameStartCol,
                lineNum,
                nameEndCol
              );
            } else {
              // Rust: Use symbol location
              range = symbol.location.range;
            }

            const title = isFrontendSide
              ? `Go to Rust: ${name}`
              : `Go to Frontend: ${name}`;

            const command = useReferences
              ? "editor.action.showReferences"
              : "vscode.open";

            const targetUsages = isFrontendSide
              ? [symbol.location]
              : indexer.getUsages(name);

            const targetLocation = isFrontendSide
              ? symbol.location
              : targetUsages[0];

            const args = useReferences
              ? [targetLocation.uri, targetLocation.range.start, targetUsages]
              : [targetLocation.uri, { selection: targetLocation.range }];

            lenses.push(
              new vscode.CodeLens(range, { title, command, arguments: args })
            );
          }

          return lenses;
        },
      }
    )
  );
}

export function deactivate() {}
