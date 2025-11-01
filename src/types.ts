import * as vscode from "vscode";

export type SymbolType = "command" | "event";
export type CodeLensType = "open" | "references";

export interface TauriMapping {
  /** Pattern in Rust (e.g. "app.emit") */
  rust: string;
  /** Frontend function names (e.g. ["listen", "once"]) */
  frontend: string[];
  /** 1-based index of event name argument */
  eventArgIndex: number;
  type: SymbolType;
}

export interface TauriSymbol {
  name: string;
  location: vscode.Location;
  type: SymbolType;
}

export type SymbolMap = Map<string, TauriSymbol>;
export type UsageMap = Map<string, vscode.Location[]>;
