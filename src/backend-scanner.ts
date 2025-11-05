import * as vscode from 'vscode';
import { getFrontendLanguageId, registerPair } from './registry';
import { SymbolType, LanguageId, FilePath } from './types';
import {
  RUST_LANGUAGE_ID,
  GENERIC_FRONTEND_ID,
  REGEX_RUST_COMMAND,
  REGEX_RUST_EVENT_SINGLE_ARG, // <--- Использование новой константы
  REGEX_RUST_EVENT_TWO_ARGS, // <--- Использование новой константы
} from './constants';

export class BackendScanner {
  async scanAll() {
    const files = await vscode.workspace.findFiles(
      '**/*.rs',
      '{**/node_modules/**,**/.git/**,**/target/**}'
    );

    for (const file of files) {
      const doc = await vscode.workspace.openTextDocument(file);
      this.extractCommands(doc);
      this.extractEvents(doc);
    }
  }

  scanDocument(doc: vscode.TextDocument) {
    this.extractCommands(doc);
    this.extractEvents(doc);
  }

  private extractCommands(doc: vscode.TextDocument) {
    const text = doc.getText();
    const filePath: FilePath = doc.uri.fsPath;

    const regex = REGEX_RUST_COMMAND;

    let match: RegExpExecArray | null;
    const type: SymbolType = 'command';
    regex.lastIndex = 0; // Reset regex state

    while ((match = regex.exec(text)) !== null) {
      const name = match[1];

      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      const targetLang =
        getFrontendLanguageId(type, name) || GENERIC_FRONTEND_ID;

      registerPair(name, filePath, RUST_LANGUAGE_ID, targetLang, type, offset);
    }
  }

  private extractEvents(doc: vscode.TextDocument) {
    const text = doc.getText();
    const filePath: FilePath = doc.uri.fsPath;
    const type: SymbolType = 'event';

    let match: RegExpExecArray | null;

    // 1. Scan for single-argument events (emit, listen, once, etc.)
    const regexSingle = REGEX_RUST_EVENT_SINGLE_ARG;
    regexSingle.lastIndex = 0;

    while ((match = regexSingle.exec(text)) !== null) {
      const name = match[2]; // Event name is the second capture group
      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      const targetLang =
        getFrontendLanguageId(type, name) || GENERIC_FRONTEND_ID;

      registerPair(name, filePath, RUST_LANGUAGE_ID, targetLang, type, offset);
    }

    // 2. Scan for two-argument events (emit_to, emit_str_to)
    const regexTwo = REGEX_RUST_EVENT_TWO_ARGS;
    regexTwo.lastIndex = 0;

    while ((match = regexTwo.exec(text)) !== null) {
      const name = match[2]; // Event name is the second capture group
      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      const targetLang =
        getFrontendLanguageId(type, name) || GENERIC_FRONTEND_ID;

      registerPair(name, filePath, RUST_LANGUAGE_ID, targetLang, type, offset);
    }
  }
}
