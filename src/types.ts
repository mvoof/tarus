import { Range } from 'vscode';

export type SymbolType = 'command' | 'event';

export type LanguageId = string;

export type FilePath = string;

export interface RegistryEntry {
  location: FilePath;
  language: LanguageId; // vscode languageId
  offset: number; // Offset of the start of the symbol name in the file
  counterpart: {
    language: LanguageId; // vscode languageId
    type: SymbolType;
    name: string;
    offset: number;
  };
}

export type CodeLensSettings = 'new tab' | 'references';

export type DeveloperMode = boolean;

export interface SymbolInfo {
  name: string;
  type: SymbolType;
  offset: number;
  range: Range;
}
