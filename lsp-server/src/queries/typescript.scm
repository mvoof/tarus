; TypeScript queries for Tauri commands and events

; Simple function calls: invoke("cmd"), emit("event"), listen("event"), once("event")
; Note: We don't filter by function name here to support import aliases.
; Filtering is done in Rust code after alias resolution.
; Use !type_arguments to avoid matching generic calls (which are handled separately)
(call_expression
  function: (identifier) @func_name
  !type_arguments
  arguments: (arguments
    .
    (string
      (string_fragment) @arg_value))
) @call_simple

; Generic function calls: invoke<T>("cmd"), emit<T>("event")
; In TypeScript, type_arguments is a direct child of call_expression
(call_expression
  function: (identifier) @func_name
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (string
      (string_fragment) @arg_value))
) @call_generic

; Await expression with simple call: await invoke("cmd")
; Use !type_arguments to avoid matching generic calls
(call_expression
  function: (await_expression
    (identifier) @func_name)
  !type_arguments
  arguments: (arguments
    .
    (string
      (string_fragment) @arg_value))
) @call_await_simple

; Await expression with generic call: await invoke<T>("cmd")
(call_expression
  function: (await_expression
    (identifier) @func_name)
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (string
      (string_fragment) @arg_value))
) @call_await_generic

; Function calls with second string argument: emitTo("target", "event")
; Use !type_arguments to avoid matching generic calls
(call_expression
  function: (identifier) @func_name_second
  !type_arguments
  arguments: (arguments
    (_)
    .
    (string
      (string_fragment) @arg_value_second))
) @call_second_arg

; Generic calls with second string argument: emitTo<T>("target", "event")
(call_expression
  function: (identifier) @func_name_second
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    (string
      (string_fragment) @arg_value_second))
) @call_generic_second_arg

; Await expression with second string argument: await emitTo("target", "event")
; Use !type_arguments to avoid matching generic calls
(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  !type_arguments
  arguments: (arguments
    (_)
    .
    (string
      (string_fragment) @arg_value_second))
) @call_await_second_arg

; Await expression with second string argument and generics: await emitTo<T>("target", "event")
(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    (string
      (string_fragment) @arg_value_second))
) @call_await_generic_second_arg

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
