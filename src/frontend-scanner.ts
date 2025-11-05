import * as vscode from 'vscode';
import { registerPair, updateCounterpart } from './registry';
import { SymbolType, LanguageId, FilePath } from './types';
import {
  RUST_LANGUAGE_ID,
  REGEX_FRONTEND_FIRST,
  REGEX_FRONTEND_SECOND,
} from './constants';

export class FrontendScanner {
  async scanAll() {
    // Removed FRONTEND_FUNCS_FIRST and FRONTEND_FUNCS_SECOND from here
    const files = await vscode.workspace.findFiles(
      '**/*.{ts,tsx,js,jsx,vue}',
      '{**/node_modules/**,**/.git/**,**/target/**,**/dist/**,**/build/**,**/gen/**,**/vite.config.ts}'
    );

    for (const file of files) {
      const doc = await vscode.workspace.openTextDocument(file);
      const lang: LanguageId = doc.languageId;

      this.extractEvents(doc, lang);
    }
  }

  scanDocument(doc: vscode.TextDocument) {
    const lang = doc.languageId;
    this.extractEvents(doc, lang);
  }

  private extractEvents(
    doc: vscode.TextDocument,
    currentDocumentLanguage: LanguageId
  ) {
    const text = doc.getText();
    const filePath: FilePath = doc.uri.fsPath;
    const rustLanguageId: LanguageId = RUST_LANGUAGE_ID; // Use constant

    // invoke, listen, once, emit
    const regexFirst = REGEX_FRONTEND_FIRST;

    let match: RegExpExecArray | null;
    regexFirst.lastIndex = 0; // Reset regex state before loop

    while ((match = regexFirst.exec(text)) !== null) {
      const func = match[1];
      const name = match[2];
      const type: SymbolType = func === 'invoke' ? 'command' : 'event';

      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      registerPair(
        name,
        filePath,
        currentDocumentLanguage,
        rustLanguageId,
        type,
        offset
      );

      // This is necessary to update the current document's registry entry with its own offset
      updateCounterpart(name, filePath, currentDocumentLanguage, type, offset);
    }

    // emitTo
    const regexSecond = REGEX_FRONTEND_SECOND;
    regexSecond.lastIndex = 0; // Reset regex state before loop

    while ((match = regexSecond.exec(text)) !== null) {
      const name = match[2];

      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      const type: SymbolType = 'event'; // explicitly set for emitTo

      registerPair(
        name,
        filePath,
        currentDocumentLanguage,
        rustLanguageId,
        type,
        offset
      );

      // This is necessary to update the current document's registry entry with its own offset
      updateCounterpart(name, filePath, currentDocumentLanguage, type, offset);
    }
  }
}
