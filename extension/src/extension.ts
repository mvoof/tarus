import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { ExtensionContext } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';

// Import shared platform utilities
const { getTargetBinaryName } = require('../scripts/platform-utils');

const SUPPORTED_LANGUAGES = [
  'typescript',
  'typescriptreact',
  'javascript',
  'javascriptreact',
  'vue',
  'svelte',
  'rust',
] as const;

let client: LanguageClient;

const getServerCommand = (context: ExtensionContext): string => {
  const binaryName = getTargetBinaryName();
  return context.asAbsolutePath(path.join('bin', binaryName));
};

const activate = (context: ExtensionContext) => {
  console.log('[TARUS] Extension activating...');
  // This path specifies where the binary will be located AFTER compilation.
  // We assume you'll copy the binary to the 'bin' folder within the client.
  const serverCommand = getServerCommand(context);

  // Validate that LSP server binary exists
  if (!fs.existsSync(serverCommand)) {
    const errorMessage = `TARUS LSP Server binary not found at: ${serverCommand}\n\nPlease run "npm run vscode:prepublish" to build the extension.`;
    vscode.window.showErrorMessage(errorMessage);
    console.error('[TARUS] Binary not found:', serverCommand);
    return;
  }

  const serverOptions: ServerOptions = {
    run: { command: serverCommand, transport: TransportKind.stdio },
    debug: { command: serverCommand, transport: TransportKind.stdio },
  };

  const clientOptions: LanguageClientOptions = {
    // Specify for which languages ​​to activate LSP
    documentSelector: SUPPORTED_LANGUAGES.map((lang) => {
      return { scheme: 'file', language: lang };
    }),
  };

  client = new LanguageClient(
    'tarusLspServer',
    'Tarus LSP Server',
    serverOptions,
    clientOptions
  );

  // Start the client with error handling
  try {
    client.start();
  } catch (error) {
    const errorMessage = `Failed to start TARUS LSP Server: ${error instanceof Error ? error.message : String(error)}`;
    vscode.window.showErrorMessage(errorMessage);
    console.error('[TARUS] Start error:', error);
    return;
  }

  // Handle client initialization errors
  client.onDidChangeState((event) => {
    if (event.newState === 3) {
      // State.Stopped
      console.error('[TARUS] LSP Server stopped unexpectedly');
    }
  });

  context.subscriptions.push(
    vscode.commands.registerCommand(
      'tarus.show_references',
      async (uriStr: string, pos: any, locs: any[]) => {
        const uri = vscode.Uri.parse(uriStr);
        const position = new vscode.Position(pos.line, pos.character);

        const locations = locs.map((l) => {
          return new vscode.Location(
            vscode.Uri.parse(l.uri),
            new vscode.Range(
              l.range.start.line,
              l.range.start.character,
              l.range.end.line,
              l.range.end.character
            )
          );
        });

        if (locations.length === 1) {
          const loc = locations[0];

          await vscode.commands.executeCommand('vscode.open', loc.uri, {
            selection: loc.range,
          });
        } else {
          await vscode.commands.executeCommand(
            'editor.action.showReferences',
            uri,
            position,
            locations
          );
        }
      }
    )
  );
};

const deactivate = (): Thenable<void> | undefined => {
  if (!client) {
    return undefined;
  }

  return client.stop();
};

export { activate, deactivate };
