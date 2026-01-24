; Common ECMAScript queries for Tauri commands and events
; Used by both TypeScript and JavaScript parsers

; Import specifiers to track aliases
; import { invoke as myInvoke } from '@tauri-apps/api'
(import_specifier
  name: (identifier) @imported_name
  alias: (identifier) @local_alias
) @import_alias

; Simple imports without alias (for reference)
; import { invoke } from '@tauri-apps/api'
(import_specifier
  name: (identifier) @imported_simple
  !alias
) @import_simple

; Simple function calls: invoke("cmd"), emit("event"), listen("event"), once("event")
; Note: We don't filter by function name here to support import aliases.
; Filtering is done in Rust code after alias resolution.
(call_expression
  function: (identifier) @func_name
  arguments: (arguments
    .
    (string
      (string_fragment) @arg_value))
) @call_simple

; Function calls with second string argument: emitTo("target", "event")
(call_expression
  function: (identifier) @func_name_second
  arguments: (arguments
    (_)
    .
    (string
      (string_fragment) @arg_value_second))
) @call_second_arg
