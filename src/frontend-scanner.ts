import * as vscode from 'vscode';
import { registerPair, updateCounterpart } from './registry';
import { SymbolType, LanguageId, FilePath } from './types';

const FRONTEND_FUNCS_FIRST = ['invoke', 'listen', 'once', 'emit'];
const FRONTEND_FUNCS_SECOND = ['emitTo'];

export class FrontendScanner {
  async scanAll() {
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
    const rustLanguageId: LanguageId = 'rust';

    // invoke, listen, once, emit
    const regexFirst = new RegExp(
      `\\b(${FRONTEND_FUNCS_FIRST.join('|')})\\s*(?:<[^>]*>)?\\s*\\(\\s*['"]([^'"]+)['"]`,
      'g'
    );

    let match: RegExpExecArray | null;

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

      updateCounterpart(name, filePath, currentDocumentLanguage, type, offset);
    }

    // emitTo
    const regexSecond = new RegExp(
      `\\b(${FRONTEND_FUNCS_SECOND.join('|')})\\s*\\(\\s*['"][^'"]+['"]\\s*,\\s*['"]([^'"]+)['"]`,
      'g'
    );

    while ((match = regexSecond.exec(text)) !== null) {
      const name = match[2];

      const nameIndex = match[0].lastIndexOf(name);
      const offset = match.index + nameIndex;

      registerPair(
        name,
        filePath,
        currentDocumentLanguage,
        rustLanguageId,
        'event' as SymbolType,
        offset
      );

      updateCounterpart(
        name,
        filePath,
        currentDocumentLanguage,
        'event' as SymbolType,
        offset
      );
    }
  }
}
