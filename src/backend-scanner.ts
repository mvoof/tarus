import * as vscode from 'vscode';
import { getFrontendLanguageId, registerPair } from './registry';
import { SymbolType, LanguageId, FilePath } from './types';

const RUST_LANGUAGE_ID: LanguageId = 'rust';
const GENERIC_FRONTEND_ID: LanguageId = 'frontend-generic'; // placeholder

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

    const regex =
      /#\[\s*(?:tauri::)?command(?:[^\]]*?)?\]\s*[\s\S]*?(?:pub\s+)?(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/g;

    let match: RegExpExecArray | null;
    const type: SymbolType = 'command';

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

    // any_word.(
    const regexFirst =
      /\b\w+\.(emit|emit_filter|emit_str|emit_str_filter|listen|listen_any|once|once_any)\s*\(\s*['"]([^'"]+)['"]/g;

    let match: RegExpExecArray | null;

    while ((match = regexFirst.exec(text)) !== null) {
      const name = match[2];
      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      const targetLang =
        getFrontendLanguageId(type, name) || GENERIC_FRONTEND_ID;

      registerPair(name, filePath, RUST_LANGUAGE_ID, targetLang, type, offset);
    }

    const regexSecond =
      /\b\w+\.(emit_to|emit_str_to)\s*\(\s*['"][^'"]+['"]\s*,\s*['"]([^'"]+)['"]/g;

    while ((match = regexSecond.exec(text)) !== null) {
      const name = match[2];
      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      const targetLang =
        getFrontendLanguageId(type, name) || GENERIC_FRONTEND_ID;

      registerPair(name, filePath, RUST_LANGUAGE_ID, targetLang, type, offset);
    }
  }
}
