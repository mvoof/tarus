import * as path from 'path';
import * as vscode from 'vscode';
import { ExtensionContext } from 'vscode';
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
  TransportKind,
} from 'vscode-languageclient/node';

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
  const serverName = 'lsp-server'; // bin file name
  let platform: string = process.platform;
  let arch: string = process.arch;

  // We collect the file name depending on the OS
  let binaryName: string;

  if (platform === 'win32') {
    // Windows
    binaryName = `${serverName}-win-x64.exe`;
  } else if (platform === 'darwin') {
    // macOS
    if (arch === 'arm64') {
      // Apple Silicon M1/M2/M3
      binaryName = `${serverName}-macos-arm64`;
    } else {
      // Intel Mac
      binaryName = `${serverName}-macos-x64`;
    }
  } else {
    // Linux
    binaryName = `${serverName}-linux-x64`;
  }

  return context.asAbsolutePath(path.join('bin', binaryName));
};

const activate = (context: ExtensionContext) => {
  // This path specifies where the binary will be located AFTER compilation.
  // We assume you'll copy the binary to the 'bin' folder within the client.
  const serverCommand = getServerCommand(context);

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

  client.start();

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
