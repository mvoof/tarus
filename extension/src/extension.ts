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

import { getTargetBinaryName } from './utils/platform-utils';

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

// Canonical list of Tauri configuration file names used to detect a Tauri
// workspace, aligned with the documented config file formats:
// https://v2.tauri.app/reference/config/ — base `tauri.conf.json`,
// `tauri.conf.json5`, `Tauri.toml`, plus the `tauri.<platform>.conf.json` and
// `Tauri.<platform>.toml` overrides (no platform-specific JSON5 exists).
//
// The same set is maintained by hand in two other places that must be updated
// together whenever this list changes:
//   - `activationEvents` in package.json (as `workspaceContains:**/<name>` entries)
//   - `TAURI_CONFIG_FILES` in lsp-server/src/discovery/scanner.rs
const TAURI_CONFIG_FILES = [
  'tauri.conf.json',
  'tauri.conf.json5',
  'Tauri.toml',
  'tauri.*.conf.json',
  'Tauri.*.toml',
] as const;

const TAURI_CONFIG_GLOB = `**/{${TAURI_CONFIG_FILES.join(',')}}`;

// Directories ignored during the search (mirrors EXCLUDED_DIRS in the Rust scanner),
// so a config buried in target/, dist/, gen/, etc. does not spuriously start the server.
const EXCLUDED_DIRS_GLOB =
  '**/{node_modules,.git,.vscode,.github,docs,target,dist,build,gen}/**';

// Returns true if the workspace contains at least one Tauri configuration file.
// The server is not started otherwise, so no binary process is spawned.
const hasTauriConfig = async (): Promise<boolean> => {
  const matches = await vscode.workspace.findFiles(
    TAURI_CONFIG_GLOB,
    EXCLUDED_DIRS_GLOB,
    1
  );

  return matches.length > 0;
};

const activate = async (context: ExtensionContext) => {
  console.log('[TARUS] Extension activating...');

  // Register the restart command unconditionally so it exists even when the
  // server was not started (e.g. a non-Tauri workspace). The handler reports a
  // proper message instead of VS Code's "command not found".
  context.subscriptions.push(
    vscode.commands.registerCommand('tarus.restartServer', async () => {
      if (!client) {
        vscode.window.showErrorMessage('TARUS: server is not initialized.');
        return;
      }
      try {
        await client.stop();
        await client.start();
        vscode.window.showInformationMessage('TARUS: server restarted.');
      } catch (error) {
        vscode.window.showErrorMessage(
          `TARUS: failed to restart server — ${error instanceof Error ? error.message : String(error)}`
        );
      }
    })
  );

  // Do not spawn the LSP server binary unless this is a Tauri project.
  if (!(await hasTauriConfig())) {
    console.log('[TARUS] No Tauri config found — server not started.');

    return;
  }

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
    await client.start();
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
