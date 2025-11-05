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
  SymbolInfo,
} from './types';
import {
  SUPPORTED_LANGUAGES,
  RUST_LANGUAGE_ID,
  REGEX_RUST_COMMAND,
  REGEX_FRONTEND_CALLS,
  REGEX_RUST_FN_NAME,
  REGEX_QUOTED_STRING,
  REGEX_FULL_CALL_CHECK,
  REGEX_RUST_EVENT_SINGLE_ARG,
  REGEX_RUST_EVENT_TWO_ARGS,
} from './constants';

const output = vscode.window.createOutputChannel('Tarus');

const frontendScanner = new FrontendScanner();
const backendScanner = new BackendScanner();

// Create an EventEmitter to force a CodeLens update
const _onDidChangeCodeLenses: vscode.EventEmitter<void> =
  new vscode.EventEmitter<void>();

/** Debounces a function call */
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

/** Debug function only in dev mode: saves the registry to a JSON file */
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
      `Tarus Debug: Registry saved to ${path.basename(filePath)}`
    );
  } catch (error) {
    output.appendLine(`Debug: Failed to save registry: ${error}`);

    vscode.window.showErrorMessage(
      `Tarus Debug: Failed to save registry. Check output channel.`
    );
  }
}

/** Creates a CodeLens for transition */
function createCodeLens(
  document: vscode.TextDocument,
  name: string,
  startOffset: number,
  info: { location: string; offset: number },
  useReferences: boolean
): vscode.CodeLens {
  const start = document.positionAt(startOffset);
  const end = document.positionAt(startOffset + name.length);
  const range = new vscode.Range(start, end);

  const isRust = document.languageId === RUST_LANGUAGE_ID;

  const title = isRust ? `Go to Frontend: ${name}` : `Go to Rust: ${name}`;

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

  return new vscode.CodeLens(range, {
    title,
    command: command,
    arguments: args,
  });
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

      if (SUPPORTED_LANGUAGES.includes(langId)) {
        const filePath = savedDocument.uri.fsPath;

        clearRegistryForFile(filePath);

        if (langId === RUST_LANGUAGE_ID) {
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
    vscode.languages.registerDefinitionProvider(SUPPORTED_LANGUAGES, {
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

        // Open the target document to get its positions correctly
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
    vscode.languages.registerHoverProvider(SUPPORTED_LANGUAGES, {
      async provideHover(document, position) {
        const symbol = getSymbolAtPosition(document, position);

        if (!symbol) return;

        const { name, type, range } = symbol;

        const language: LanguageId = document.languageId;
        const info = getCounterpartInfo(language, type, name);

        if (!info) return;

        const md = new vscode.MarkdownString(
          `**${type}** \`${name}\` → \`${path.basename(info.location)}\``
        ); // Display file name instead of full path
        md.isTrusted = true;

        return new vscode.Hover(md, range);
      },
    })
  );

  // CodeLens
  context.subscriptions.push(
    vscode.languages.registerCodeLensProvider(SUPPORTED_LANGUAGES, {
      onDidChangeCodeLenses: _onDidChangeCodeLenses.event,

      provideCodeLenses(document) {
        if (!showCodeLens) {
          return [];
        }

        const lenses: vscode.CodeLens[] = [];
        const text = document.getText();

        const language: LanguageId = document.languageId;
        const isRust = language === RUST_LANGUAGE_ID;

        let match: RegExpExecArray | null;

        if (isRust) {
          const rustCommandRegex = REGEX_RUST_COMMAND;
          rustCommandRegex.lastIndex = 0; // Reset regex state

          while ((match = rustCommandRegex.exec(text)) !== null) {
            const name = match[1];
            const type: SymbolType = 'command';

            const nameIndex = match[0].lastIndexOf(name);
            const startOffset = match.index + nameIndex;

            const info = getCounterpartInfo(language, type, name);

            if (!info) continue;

            lenses.push(
              createCodeLens(document, name, startOffset, info, useReferences)
            );
          }

          // 2. Rust events (Single-argument)
          const singleEventRegex = REGEX_RUST_EVENT_SINGLE_ARG;
          singleEventRegex.lastIndex = 0;

          while ((match = singleEventRegex.exec(text)) !== null) {
            const name = match[2];
            const type: SymbolType = 'event';

            if (!name) continue;

            const nameIndex = match[0].lastIndexOf(name);
            const startOffset = match.index + nameIndex;

            const info = getCounterpartInfo(language, type, name);

            if (!info) continue;

            lenses.push(
              createCodeLens(document, name, startOffset, info, useReferences)
            );
          }

          // 3. Rust events (Two-argument - emit_to)
          const twoArgEventRegex = REGEX_RUST_EVENT_TWO_ARGS;
          twoArgEventRegex.lastIndex = 0;

          while ((match = twoArgEventRegex.exec(text)) !== null) {
            const name = match[2];
            const type: SymbolType = 'event';

            if (!name) continue;

            const nameIndex = match[0].lastIndexOf(name);
            const startOffset = match.index + nameIndex;

            const info = getCounterpartInfo(language, type, name);

            if (!info) continue;

            lenses.push(
              createCodeLens(document, name, startOffset, info, useReferences)
            );
          }
        }

        // 4. Frontend calls (Commands and Events)
        if (!isRust) {
          const frontendRegex = REGEX_FRONTEND_CALLS;
          frontendRegex.lastIndex = 0;

          while ((match = frontendRegex.exec(text)) !== null) {
            const name = match[2] || match[4];
            const funcName = match[1] || match[3];

            if (!name) continue;

            const type: SymbolType =
              funcName === 'invoke' ? 'command' : 'event';

            const nameIndex = match[0].lastIndexOf(name);
            const startOffset = match.index + nameIndex;

            const info = getCounterpartInfo(language, type, name);

            if (!info) continue;

            lenses.push(
              createCodeLens(document, name, startOffset, info, useReferences)
            );
          }
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
 * Checks if the cursor is on a Rust command function name.
 */
function getRustSymbolAtPosition(
  document: vscode.TextDocument,
  position: vscode.Position
): SymbolInfo | null {
  const line = document.lineAt(position.line);
  const lineText = line.text;

  const fnMatch = lineText.match(REGEX_RUST_FN_NAME);

  if (!fnMatch) return null;

  const nameCandidate = fnMatch[1];
  const nameStartIndex = lineText.indexOf(nameCandidate, fnMatch.index);

  if (nameStartIndex === -1) return null;

  const nameEndIndex = nameStartIndex + nameCandidate.length;

  // Check if the cursor is inside the function name
  if (
    position.character < nameStartIndex ||
    position.character > nameEndIndex
  ) {
    return null;
  }

  // Check the previous lines (up to 3) for the presence of the #[tauri::command] attribute
  let isCommand = false;

  for (let j = position.line; j >= 0 && position.line - j < 3; j--) {
    if (document.lineAt(j).text.includes('#[tauri::command]')) {
      isCommand = true;

      break;
    }
  }

  if (isCommand) {
    const name = nameCandidate;
    const type: SymbolType = 'command';

    const offset = document.offsetAt(
      new vscode.Position(position.line, nameStartIndex)
    );

    const range = new vscode.Range(
      new vscode.Position(position.line, nameStartIndex),
      new vscode.Position(position.line, nameEndIndex)
    );

    return { name, type, offset, range };
  }

  return null;
}

/**
 * Checks if the cursor is on a string argument of a Tauri call (frontend or Rust event/command call).
 */
function getFrontendSymbolAtPosition(
  document: vscode.TextDocument,
  position: vscode.Position
): SymbolInfo | null {
  const line = document.lineAt(position.line);
  const lineText = line.text;

  let name = '';
  let offset = -1;
  let symbolRange: vscode.Range | null = null;
  let type: SymbolType | null = null;

  // Search for a string in quotation marks that contains the cursor position
  const quotedStringRegex = REGEX_QUOTED_STRING;
  let match: RegExpExecArray | null;
  quotedStringRegex.lastIndex = 0; // Reset regex state

  while ((match = quotedStringRegex.exec(lineText)) !== null) {
    const startChar = match.index;
    const endChar = match.index + match[0].length;

    // Check if the cursor is inside quotation marks (not including the quotes themselves)
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
  const fullRegex = REGEX_FULL_CALL_CHECK(name);
  const fullLineMatch = lineText.match(fullRegex);

  if (!fullLineMatch) return null;

  // Determine the function name and symbol type
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
    // Check if the symbol is the FIRST argument in an emitTo/emit_to call.
    // In emitTo/emit_to calls, the first argument is the window label (not the event name).
    // The symbol must be the SECOND argument (the event name).

    // Find the index of the event string in the line
    const stringInQuotesIndex =
      lineText.indexOf(`'${name}'`) !== -1
        ? lineText.indexOf(`'${name}'`)
        : lineText.indexOf(`"${name}"`);

    if (stringInQuotesIndex === -1) return null; // Should not happen if fullRegex matched

    // If the symbol is not preceded by a comma before the function call ends, it's the first argument.
    // This simple check maintains the original logic: if the substring before the symbol
    // contains no comma, it's the first argument and should be ignored for emitTo.
    const preSymbolSubstring = lineText.slice(0, stringInQuotesIndex);

    if (!preSymbolSubstring.includes(',')) {
      // It's the first argument (window label), which we don't track.
      return null;
    }
  }

  return { name, type, offset, range: symbolRange };
}

/**
 * Gets the name, type, offset, and RANGE of the symbol under the cursor.
 */
function getSymbolAtPosition(
  document: vscode.TextDocument,
  position: vscode.Position
): SymbolInfo | null {
  if (document.languageId === RUST_LANGUAGE_ID) {
    // 1. Check for Rust command definition
    const rustCommandSymbol = getRustSymbolAtPosition(document, position);

    if (rustCommandSymbol) {
      return rustCommandSymbol;
    }
  }

  // 2. Check for string argument in any Tauri call (Rust event call, Frontend command/event call)
  return getFrontendSymbolAtPosition(document, position);
}

export function deactivate() {}
