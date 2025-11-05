import { LanguageId } from './types';

/** Supported languages for activation and definition providers. */
export const SUPPORTED_LANGUAGES: LanguageId[] = [
  'typescript',
  'typescriptreact',
  'javascript',
  'javascriptreact',
  'vue',
  'rust',
  'frontend-generic',
];

export const RUST_LANGUAGE_ID: LanguageId = 'rust';
export const GENERIC_FRONTEND_ID: LanguageId = 'frontend-generic'; // placeholder for Rust targets when frontend not found

// --- Frontend Scanner Constants ---

/** Functions that take the command/event name as the first string argument (e.g., invoke, listen, once, emit) */
export const FRONTEND_FUNCS_FIRST = ['invoke', 'listen', 'once', 'emit'];
/** Functions that take the event name as the second string argument (e.g., emitTo) */
export const FRONTEND_FUNCS_SECOND = ['emitTo'];

/** Regex for frontend functions that take name as the FIRST argument (invoke, listen, once, emit) */
export const REGEX_FRONTEND_FIRST = new RegExp(
  `\\b(${FRONTEND_FUNCS_FIRST.join('|')})\\s*(?:<[^>]*>)?\\s*\\(\\s*['"]([^'"]+)['"]`,
  'g'
);

/** Regex for frontend functions that take name as the SECOND argument (emitTo) */
export const REGEX_FRONTEND_SECOND = new RegExp(
  `\\b(${FRONTEND_FUNCS_SECOND.join('|')})\\s*\\(\\s*['"][^'"]+['"]\\s*,\\s*['"]([^'"]+)['"]`,
  'g'
);

// --- Backend Scanner/CodeLens Constants ---

/** Regex for Rust commands (#[tauri::command] fn name) */
export const REGEX_RUST_COMMAND =
  /#\[\s*(?:tauri::)?command(?:[^\]]*?)?\]\s*[\s\S]*?(?:pub\s+)?(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/g;

/** Regex for Rust events (e.g., app_handle.emit, window.listen) - Single string argument */
export const REGEX_RUST_EVENT_SINGLE_ARG =
  /\b\w+\.(emit|emit_filter|emit_str|emit_str_filter|listen|listen_any|once|once_any)\s*\(\s*['"]([^'"]+)['"]/g;

/** Regex for Rust events with two arguments where the event name is the second string (emit_to, emit_str_to) */
export const REGEX_RUST_EVENT_TWO_ARGS =
  /\b\w+\.(emit_to|emit_str_to)\s*\(\s*['"][^'"]+['"]\s*,\s*['"]([^'"]+)['"]/g;

/** Regex for all frontend calls (invoke, emit, listen, once, emitTo) */
export const REGEX_FRONTEND_CALLS =
  /(invoke|emit|listen|once)\s*(?:<[^>]*>)?\s*\(\s*['"]([^'"]+)['"]|emitTo\s*\(\s*['"][^'"]+['"]\s*,\s*['"]([^'"]+)['"]/g;
// --- getSymbolAtPosition Constants ---

/** Regex for finding a function name in Rust (used for command check) */
export const REGEX_RUST_FN_NAME =
  /(?:pub\s+)?(?:async\s+)?fn\s+([a-zA-Z_][a-zA-Z0-9_]*)\s*\(/;

/** Regex for finding a quoted string ('name' or "name") */
export const REGEX_QUOTED_STRING = /(['"])([^'"]+)\1/g;

/** Regex for checking if a quoted string is part of a relevant Tauri call */
// Note: name will be inserted into this regex dynamically to ensure it matches the extracted symbol.
export const REGEX_FULL_CALL_CHECK = (name: string) =>
  new RegExp(
    `\\b(invoke|emit|listen|once|emitTo|\\b\\w+\\.(emit|emit_filter|emit_str|emit_str_filter|listen|listen_any|once|once_any|emit_to|emit_str_to))\\s*(?:<[^>]*>)?\\s*\\((?:\\s*['"][^'"]+['"]\\s*,)?\\s*['"]${name}['"]`
  );
