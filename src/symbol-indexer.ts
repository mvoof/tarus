import * as vscode from "vscode";
import * as fs from "fs";
import * as path from "path";
import { TauriMapping, SymbolMap, UsageMap } from "./types";

interface RootPaths {
  rust: string | null;
  frontend: string | null;
}

export default class TauriIndexer {
  private readonly config: vscode.WorkspaceConfiguration;
  private readonly symbols: SymbolMap = new Map();
  private readonly usages: UsageMap = new Map();
  private mappings: TauriMapping[] = [];
  private debounceTimer: NodeJS.Timeout | null = null;
  private rootPaths: RootPaths | null = null;

  constructor(config: vscode.WorkspaceConfiguration) {
    this.config = config;
    this.loadMappings();
    this.scheduleInitialScan();
  }

  /** Combine default and user-defined mappings */
  private loadMappings(): void {
    const userMappings = this.config.get<TauriMapping[]>("mappings") ?? [];
    this.mappings = [...this.getDefaultMappings(), ...userMappings];
  }

  /** Built-in Tauri patterns */
  private getDefaultMappings(): TauriMapping[] {
    return [
      {
        rust: "app.emit",
        frontend: ["listen", "once"],
        eventArgIndex: 1,
        type: "event",
      },
      {
        rust: "app.listen",
        frontend: ["emit"],
        eventArgIndex: 1,
        type: "event",
      },
      {
        rust: "app.emit_to",
        frontend: ["listen", "once"],
        eventArgIndex: 2,
        type: "event",
      },
      {
        rust: "app.emit_filter",
        frontend: ["listen", "once"],
        eventArgIndex: 1,
        type: "event",
      },
      {
        rust: "#[tauri::command]",
        frontend: ["invoke"],
        eventArgIndex: 0,
        type: "command",
      },
    ];
  }

  /** Delay initial scan to allow workspace to load */
  private scheduleInitialScan(): void {
    setTimeout(() => this.trigger(), 1000);
  }

  /** Public: trigger re-indexing with debounce */
  public trigger(): void {
    if (this.debounceTimer) {
      clearTimeout(this.debounceTimer);
    }

    this.debounceTimer = setTimeout(() => this.index(), 300);
  }

  /** Main indexing pipeline */
  private async index(): Promise<void> {
    await vscode.window.withProgress(
      {
        location: vscode.ProgressLocation.Window,
        title: "Tarus: Indexing symbols...",
      },
      async () => {
        this.symbols.clear();
        this.usages.clear();
        this.rootPaths = this.resolveRootPaths();

        await this.indexRustFiles();
        await this.indexFrontendFiles();
      }
    );
  }

  /** Resolve and cache workspace roots */
  private resolveRootPaths(): RootPaths {
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;

    if (!workspaceRoot) {
      return { rust: null, frontend: null };
    }

    const rustRel = this.config.get<string>("rustRoot") ?? "src-tauri/src";
    const frontendRel = this.config.get<string>("frontendRoot") ?? "src";

    const rust = this.resolvePath(workspaceRoot, rustRel);
    const frontend = this.resolvePath(workspaceRoot, frontendRel);

    return { rust, frontend };
  }

  private resolvePath(root: string, rel: string): string | null {
    const full = path.join(root, rel);

    return fs.existsSync(full) ? full : null;
  }

  /** Scan all Rust files for symbols */
  private async indexRustFiles(): Promise<void> {
    const root = this.rootPaths?.rust;

    if (!root) {
      return;
    }

    const files = this.findFiles(root, [".rs"]);
    for (const file of files) {
      const content = fs.readFileSync(file, "utf8");

      this.extractRustSymbols(content, file);
    }
  }

  /** Extract commands and events from Rust source */
  private extractRustSymbols(content: string, filePath: string): void {
    const lines = content.split("\n");

    let isCommandContext = false;

    for (let lineIndex = 0; lineIndex < lines.length; lineIndex++) {
      const line = lines[lineIndex];

      // Handle #[tauri::command]
      if (line.includes("#[tauri::command]")) {
        isCommandContext = true;

        continue;
      }

      if (isCommandContext) {
        const fnPattern =
          /(?:pub\s+)?(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)/;
        const match = fnPattern.exec(line);

        if (match) {
          const name = match[1];
          const col = line.indexOf(name);
          const range = new vscode.Range(
            lineIndex,
            col,
            lineIndex,
            col + name.length
          );

          const location = new vscode.Location(
            vscode.Uri.file(filePath),
            range
          );

          this.symbols.set(name, { name, location, type: "command" });
        }

        isCommandContext = false;

        continue;
      }

      // Handle event emissions
      for (const mapping of this.mappings.filter((m) => m.type === "event")) {
        this.extractEventFromRust(content, filePath, mapping);
      }
    }
  }

  /** Extract event names from Rust using mapping */
  private extractEventFromRust(
    content: string,
    filePath: string,
    mapping: TauriMapping
  ): void {
    const escaped = mapping.rust.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    const regex = new RegExp(`${escaped}\\s*\\(`, "g");

    let match: RegExpExecArray | null;

    while ((match = regex.exec(content))) {
      const args = this.parseCallArguments(
        content,
        match.index + match[0].length - 1
      );

      const eventName = args[mapping.eventArgIndex - 1];

      if (!eventName) {
        continue;
      }

      const quotePos = content.indexOf(`"${eventName}"`, match.index);

      if (quotePos === -1) {
        continue;
      }

      const pos = quotePos + 1;
      const lineNum = content.substring(0, pos).split("\n").length - 1;
      const lineStart = content.lastIndexOf("\n", pos) + 1;
      const col = pos - lineStart;

      const range = new vscode.Range(
        lineNum,
        col,
        lineNum,
        col + eventName.length
      );

      const location = new vscode.Location(vscode.Uri.file(filePath), range);

      this.symbols.set(eventName, { name: eventName, location, type: "event" });
    }
  }

  /** Scan frontend for usage of events/commands */
  private async indexFrontendFiles(): Promise<void> {
    const root = this.rootPaths?.frontend;

    if (!root) {
      return;
    }

    const files = this.findFiles(root, [".ts", ".tsx", ".js", ".jsx", ".vue"]);

    for (const file of files) {
      const content = fs.readFileSync(file, "utf8");
      this.extractFrontendUsages(content, file);
    }
  }

  /** Extract event listeners and invoke calls */
  private extractFrontendUsages(content: string, filePath: string): void {
    for (const mapping of this.mappings) {
      for (const fnName of mapping.frontend) {
        const regex = new RegExp(
          `${fnName.replace(
            /[.*+?^${}()|[\]\\]/g,
            "\\$&"
          )}\\s*(?:<[^>]*>)?\\s*\\(`,
          "g"
        );
        let match: RegExpExecArray | null;

        while ((match = regex.exec(content))) {
          const args = this.parseCallArguments(
            content,
            match.index + match[0].length - 1
          );

          const eventName = args[0];

          if (!eventName) {
            continue;
          }

          const quotePos = content.indexOf(`"${eventName}"`, match.index);

          if (quotePos === -1) {
            continue;
          }

          const pos = quotePos + 1;
          const lineNum = content.substring(0, pos).split("\n").length - 1;
          const lineStart = content.lastIndexOf("\n", pos) + 1;
          const col = pos - lineStart;

          const range = new vscode.Range(
            lineNum,
            col,
            lineNum,
            col + eventName.length
          );

          const location = new vscode.Location(
            vscode.Uri.file(filePath),
            range
          );

          const list = this.usages.get(eventName) ?? [];
          list.push(location);

          this.usages.set(eventName, list);
        }
      }
    }
  }

  /** Parse string arguments from function call */
  private parseCallArguments(content: string, startIndex: number): string[] {
    const args: string[] = [];
    let i = startIndex;
    let inString = false;
    let quoteChar = "";
    let currentArg = "";

    while (i < content.length) {
      const char = content[i];

      if (!inString && /["'``]/.test(char)) {
        inString = true;
        quoteChar = char;
        currentArg = "";
      } else if (inString && char === quoteChar && content[i - 1] !== "\\") {
        args.push(currentArg);
        inString = false;

        // Skip comma
        const rest = content.substring(i + 1).trimStart();

        if (rest.startsWith(",")) {
          i = content.indexOf(",", i) + 1;
        } else {
          break;
        }
      } else if (inString) {
        currentArg += char;
      } else if (char === ")") {
        break;
      }

      i++;
    }

    return args;
  }

  /** Recursively find files with given extensions */
  private findFiles(dir: string, extensions: string[]): string[] {
    if (!fs.existsSync(dir)) {
      return [];
    }

    const results: string[] = [];
    const entries = fs.readdirSync(dir, { withFileTypes: true });

    for (const entry of entries) {
      const fullPath = path.join(dir, entry.name);

      if (entry.isDirectory()) {
        results.push(...this.findFiles(fullPath, extensions));
      } else if (extensions.some((ext) => entry.name.endsWith(ext))) {
        results.push(fullPath);
      }
    }

    return results;
  }

  public getDefinition(name: string): vscode.Location | undefined {
    return this.symbols.get(name)?.location;
  }

  public getUsages(name: string): vscode.Location[] {
    return this.usages.get(name) ?? [];
  }

  public getSymbols(): SymbolMap {
    return this.symbols;
  }
}
