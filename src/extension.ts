import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';
import { BackendScanner } from './backend-scanner';
import { FrontendScanner } from './frontend-scanner';
import {
  getCounterpartInfo,
  clearRegistry,
  registry,
  clearRegistryForFile,
} from './registry';
import {
  SymbolType,
  LanguageId,
  CodeLensSettings,
  DeveloperMode,
} from './types';

const output = vscode.window.createOutputChannel('Tarus');

const frontendScanner = new FrontendScanner();
const backendScanner = new BackendScanner();

// Create an EventEmitter to force a CodeLens update
const _onDidChangeCodeLenses: vscode.EventEmitter<void> =
  new vscode.EventEmitter<void>();

function debounce<T extends any[]>(
  func: (...args: T) => void,
  wait: number
): (...args: T) => void {
  let timeout: NodeJS.Timeout | null = null;

  return function (this: any, ...args: T) {
    const context = this;

    if (timeout) {
      clearTimeout(timeout);
    }

    timeout = setTimeout(() => {
      timeout = null;

      func.apply(context, args);
    }, wait);
  };
}

// Debug function only in dev mode
function saveRegistryForDebug(
  output: vscode.OutputChannel,
  isDevMode: DeveloperMode
) {
  if (!isDevMode) {
    return;
  }

  const workspaceFolders = vscode.workspace.workspaceFolders;

  if (!workspaceFolders || workspaceFolders.length === 0) {
    output.appendLine('Debug: Cannot save registry, no workspace folder open.');

    return;
  }

  const workspaceRoot = workspaceFolders[0].uri.fsPath;
  const filePath = path.join(workspaceRoot, 'tarus_registry_debug.json');

  const serializableRegistry: any = {};

  for (const [langId, maps] of registry.entries()) {
    serializableRegistry[langId] = {
      command: Array.from(maps.command.entries()).map(([name, entry]) => ({
        name,
        ...entry,
      })),
      event: Array.from(maps.event.entries()).map(([name, entry]) => ({
        name,
        ...entry,
      })),
    };
  }

  try {
    fs.writeFileSync(
      filePath,
      JSON.stringify(serializableRegistry, null, 2),
      'utf-8'
    );

    output.appendLine(`Debug: Registry successfully saved to ${filePath}`);

    vscode.window.showInformationMessage(
      `Tarus Debug: Registry savet to ${path.basename(filePath)}`
    );
  } catch (error) {
    output.appendLine(`Debug: Failed to save registry: ${error}`);

    vscode.window.showErrorMessage(
      `Tarus Debug: Failed to save registry. Check output channel.`
    );
  }
}

export function activate(context: vscode.ExtensionContext) {
  output.appendLine('Tarus: extension activated'); // Always show

  let config = vscode.workspace.getConfiguration('tarus');

  let useReferences =
    config.get<CodeLensSettings>('codeLensAction', 'new tab') === 'references';

  let showCodeLens = config.get<boolean>('showCodeLens', true);
  let isDevMode = config.get<DeveloperMode>('developerMode') || false;

  const triggerScan = async (savedDocument?: vscode.TextDocument) => {
    if (isDevMode) {
      output.appendLine('Scanning symbols...');
    }

    if (savedDocument) {
      const langId = savedDocument.languageId;

      if (supportedLanguages.includes(langId)) {
        const filePath = savedDocument.uri.fsPath;

        clearRegistryForFile(filePath);

        if (langId === 'rust') {
          backendScanner.scanDocument(savedDocument);
        } else {
          frontendScanner.scanDocument(savedDocument);
        }

        if (isDevMode) {
          output.appendLine(
            `Incremental scan complete for: ${savedDocument.fileName}`
          );
        }
      } else {
        if (isDevMode) {
          output.appendLine(
            `Ignoring save event for unsupported language: ${langId}`
          );
        }
      }
    } else {
      clearRegistry();

      await frontendScanner.scanAll();
      await backendScanner.scanAll();

      if (isDevMode) {
        output.appendLine('Full scan complete');
      }

      saveRegistryForDebug(output, isDevMode);
    }

    // Force a CodeLens update
    _onDidChangeCodeLenses.fire();
  };

  // Initial scan
  triggerScan();

  const debouncedScan = debounce(
    (doc: vscode.TextDocument) => triggerScan(doc),
    500
  );

  context.subscriptions.push(
    // When the configuration changes
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration('tarus')) {
        const config = vscode.workspace.getConfiguration('tarus');

        const newUseReferences =
          config.get<CodeLensSettings>('codeLensAction', 'new tab') ===
          'references';

        const newShowCodeLens = config.get<boolean>('showCodeLens', true);

        const newIsDevMode =
          config.get<DeveloperMode>('developerMode') || false;

        // Update settings
        if (
          showCodeLens !== newShowCodeLens ||
          useReferences !== newUseReferences
        ) {
          showCodeLens = newShowCodeLens;
          useReferences = newUseReferences;
          _onDidChangeCodeLenses.fire();
        }

        if (isDevMode !== newIsDevMode) {
          isDevMode = newIsDevMode;
        }
      }
    }),

    vscode.workspace.onDidSaveTextDocument(debouncedScan),
    vscode.commands.registerCommand('tarus.rescan', debouncedScan)
  );

  const supportedLanguages = [
    'typescript',
    'typescriptreact',
    'javascript',
    'javascriptreact',
    'vue',
    'rust',
    'frontend-generic',
  ];

  // Command for precise transition
  context.subscriptions.push(
    vscode.commands.registerCommand(
      'tarus.goToSymbol',
      (uri: vscode.Uri, offset: number) => {
        vscode.workspace.openTextDocument(uri).then((doc) => {
          const position = doc.positionAt(offset);
          const range = new vscode.Range(position, position);

          vscode.window.showTextDocument(doc, { selection: range });
        });
      }
    )
  );

  // Definition Provider (Ctrl+Click/F12)
  context.subscriptions.push(
    vscode.languages.registerDefinitionProvider(supportedLanguages, {
      async provideDefinition(
        document,
        position
      ): Promise<vscode.LocationLink[] | undefined> {
        const symbol = getSymbolAtPosition(document, position);

        if (!symbol) return undefined;

        const { name, type, range: sourceRange } = symbol;

        const languageId: LanguageId = document.languageId;
        const info = getCounterpartInfo(languageId, type, name);

        if (!info) return undefined;

        const uri = vscode.Uri.file(info.location);

        const targetDoc = await vscode.workspace.openTextDocument(uri);
        const targetPosition = targetDoc.positionAt(info.offset);

        const targetRange = new vscode.Range(targetPosition, targetPosition);

        return [
          {
            originSelectionRange: sourceRange,
            targetUri: uri,
            targetRange: targetRange,
            targetSelectionRange: targetRange,
          },
        ];
      },
    })
  );

  // Hover
  context.subscriptions.push(
    vscode.languages.registerHoverProvider(supportedLanguages, {
      async provideHover(document, position) {
        const symbol = getSymbolAtPosition(document, position);

        if (!symbol || !symbol.name || !symbol.type) return;

        const { name, type, range } = symbol;

        const language: LanguageId = document.languageId;
        const info = getCounterpartInfo(language, type, name);

        if (!info) return;

        const md = new vscode.MarkdownString(
          `**${type}** \`${name}\` → \`${info.location}\``
        );
        md.isTrusted = true;

        return new vscode.Hover(md, range);
      },
    })
  );

  // CodeLens
  context.subscriptions.push(
    vscode.languages.registerCodeLensProvider(supportedLanguages, {
      onDidChangeCodeLenses: _onDidChangeCodeLenses.event,

      provideCodeLenses(document) {
        if (!showCodeLens) {
          return [];
        }

        const lenses: vscode.CodeLens[] = [];
        const text = document.getText();

        const language: LanguageId = document.languageId;
        const isRust = language === 'rust';

        // (#[tauri::command] fn name)
        const rustCommandRegex =
          /#\[\s*(?:tauri::)?command(?:[^\]]*?)?\]\s*[\s\S]*?(?:pub\s+)?(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/g;

        // Rust events
        const rustEventRegex =
          /\b\w+\.(emit|emit_to|emit_filter|emit_str|emit_str_to|emit_str_filter|listen|listen_any|once|once_any)\s*\(\s*['"]([^'"]+)['"]/g;

        // Frontend events
        const frontendRegex =
          /(invoke|emit|listen|once)\s*(?:<[^>]*>)?\s*\(\s*['"]([^'"]+)['"]|emitTo\s*\(\s*['"][^'"]+['"]\s*,\s*['"]([^'"]+)['"]/g;

        if (isRust) {
          let match: RegExpExecArray | null;

          while ((match = rustCommandRegex.exec(text)) !== null) {
            const name = match[1];
            const type: SymbolType = 'command';

            const nameIndex = match[0].lastIndexOf(name);
            const startOffset = match.index + nameIndex;

            const info = getCounterpartInfo(language, type, name);

            if (!info) continue;

            const start = document.positionAt(startOffset);
            const end = document.positionAt(startOffset + name.length);
            const range = new vscode.Range(start, end);

            const title = `Go to Frontend: ${name}`;

            let command: string;
            let args: any[];

            if (useReferences) {
              command = 'editor.action.peekDefinition';
              args = [];
            } else {
              command = 'tarus.goToSymbol';

              const targetUri = vscode.Uri.file(info.location);
              const targetOffset = info.offset;

              args = [targetUri, targetOffset];
            }

            lenses.push(
              new vscode.CodeLens(range, {
                title,
                command: command,
                arguments: args,
              })
            );
          }
        }

        // Handling Rust/Frontend events/commands (calls)
        const currentRegex = isRust ? rustEventRegex : frontendRegex;

        let match: RegExpExecArray | null;

        while ((match = currentRegex.exec(text)) !== null) {
          const name = isRust ? match[2] : match[2] || match[4];
          const funcName = isRust ? match[1] : match[1] || match[3];

          if (!name) continue;

          const type: SymbolType = funcName === 'invoke' ? 'command' : 'event';

          const nameIndex = match[0].lastIndexOf(name);
          const startOffset = match.index + nameIndex;

          const info = getCounterpartInfo(language, type, name);

          if (!info) continue;

          const start = document.positionAt(startOffset);
          const end = document.positionAt(startOffset + name.length);
          const range = new vscode.Range(start, end);

          const title = isRust
            ? `Go to Frontend: ${name}`
            : `Go to Rust: ${name}`;

          let command: string;
          let args: any[];

          if (useReferences) {
            command = 'editor.action.peekDefinition';
            args = [];
          } else {
            command = 'tarus.goToSymbol';

            const targetUri = vscode.Uri.file(info.location);
            const targetOffset = info.offset;

            args = [targetUri, targetOffset];
          }

          lenses.push(
            new vscode.CodeLens(range, {
              title,
              command: command,
              arguments: args,
            })
          );
        }

        return lenses;
      },
      resolveCodeLens(codeLens) {
        return codeLens;
      },
    })
  );
}

/**
 * Gets the name, type, offset, and RANGE of the symbol under the cursor.
 */
function getSymbolAtPosition(
  document: vscode.TextDocument,
  position: vscode.Position
): {
  name: string;
  type: SymbolType;
  offset: number;
  range: vscode.Range;
} | null {
  const line = document.lineAt(position.line);
  const lineText = line.text;
  const isRust = document.languageId === 'rust';

  let name = '';
  let offset = -1;
  let symbolRange: vscode.Range | null = null;
  let type: SymbolType | null = null;

  if (isRust) {
    // Check if the cursor is on a line that contains a function name
    const fnPattern =
      /(?:pub\s+)?(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/;
    const fnMatch = lineText.match(fnPattern);

    if (fnMatch) {
      const nameCandidate = fnMatch[1]; // Function name (example: start_process)

      // The index of the start of the name relative to the start of the string
      const nameStartIndex = lineText.indexOf(nameCandidate, fnMatch.index);

      if (nameStartIndex !== -1) {
        const nameEndIndex = nameStartIndex + nameCandidate.length;

        // Check if the cursor is inside the function name
        if (
          position.character >= nameStartIndex &&
          position.character <= nameEndIndex
        ) {
          // Check the previous lines (up to 3) for the presence of the #[tauri::command] attribute
          let isCommand = false;
          for (let j = position.line; j >= 0 && position.line - j < 3; j--) {
            if (document.lineAt(j).text.includes('#[tauri::command]')) {
              isCommand = true;

              break;
            }
          }

          if (isCommand) {
            name = nameCandidate;
            type = 'command' as SymbolType;

            // Самый важный шаг: вычисляем точный offset
            offset = document.offsetAt(
              new vscode.Position(position.line, nameStartIndex)
            );

            symbolRange = new vscode.Range(
              new vscode.Position(position.line, nameStartIndex),
              new vscode.Position(position.line, nameEndIndex)
            );

            return { name, type, offset, range: symbolRange };
          }
        }
      }
    }
  }

  // Regex for 'name' or "name"
  const quotedStringRegex = /(['"])([^'"]+)\1/g;
  let match: RegExpExecArray | null;

  // Search for a string in quotation marks that contains the cursor position
  while ((match = quotedStringRegex.exec(lineText)) !== null) {
    const startChar = match.index;
    const endChar = match.index + match[0].length;

    // Check if the cursor is inside quotation marks
    if (position.character > startChar && position.character < endChar - 1) {
      name = match[2];

      const nameStartChar = startChar + 1;
      const nameEndChar = endChar - 1;

      offset = document.offsetAt(
        new vscode.Position(position.line, nameStartChar)
      );

      symbolRange = new vscode.Range(
        new vscode.Position(position.line, nameStartChar),
        new vscode.Position(position.line, nameEndChar)
      );

      break;
    }
  }

  if (!name || !symbolRange) return null;

  // Regex for finding a full call (invoke, emit, app.listen etc...)
  // This ensures that we don't hit a random word in quotation marks
  const fullRegex = new RegExp(
    `\\b(invoke|emit|listen|once|emitTo|\\b\\w+\\.(emit|emit_filter|emit_str|emit_str_filter|listen|listen_any|once|once_any|emit_to|emit_str_to))\\s*(?:<[^>]*>)?\\s*\\((?:\\s*['"][^'"]+['"]\\s*,)?\\s*['"]${name}['"]`
  );

  const fullLineMatch = lineText.match(fullRegex);

  if (!fullLineMatch) return null;

  const fullFuncName = fullLineMatch[1];
  const funcName = fullFuncName.split('.').pop() || fullFuncName;

  if (!type) {
    type = funcName === 'invoke' ? 'command' : 'event';
  }

  // Additional check for emitTo/emit_to where event is the SECOND argument
  if (
    (funcName === 'emitTo' || funcName.includes('emit_to')) &&
    type === 'event'
  ) {
    const stringInQuotesIndex =
      lineText.indexOf(`'${name}'`) !== -1
        ? lineText.indexOf(`'${name}'`)
        : lineText.indexOf(`"${name}"`);

    // If the symbol is not the second argument, exit
    if (
      stringInQuotesIndex !== -1 &&
      !lineText.slice(0, stringInQuotesIndex).includes(',')
    ) {
      return null;
    }
  }

  return { name, type, offset, range: symbolRange };
}

export function deactivate() {}
