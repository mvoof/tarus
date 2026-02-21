; TypeScript queries for Tauri commands and events
; Includes both simple and generic patterns

; === IMPORTS ===

; Import specifiers to track aliases
; import { invoke as myInvoke } from '@tauri-apps/api'
(import_specifier
  name: (identifier) @imported_name
  alias: (identifier) @local_alias
) @import_alias

; Simple imports without alias
; import { invoke } from '@tauri-apps/api'
(import_specifier
  name: (identifier) @imported_simple
  !alias
) @import_simple

; === SIMPLE CALLS (no generics) ===

; Simple function calls: invoke("cmd"), emit("event"), window.listen("event")
(call_expression
  function: [
    (identifier) @func_name
    (member_expression property: (property_identifier) @func_name)
  ]
  !type_arguments
  arguments: (arguments
    (string
      (string_fragment) @arg_value)
    (_) @invoke_args ?
  )
) @call_simple

; Await expression with simple call: await invoke("cmd")
(call_expression
  function: (await_expression
    [
      (identifier) @func_name
      (member_expression property: (property_identifier) @func_name)
    ])
  !type_arguments
  arguments: (arguments
    (string
      (string_fragment) @arg_value)
    (_) @invoke_args ?
  )
) @call_await_simple

; Function calls with second string argument: emitTo("target", "event")
(call_expression
  function: [
    (identifier) @func_name_second
    (member_expression property: (property_identifier) @func_name_second)
  ]
  !type_arguments
  arguments: (arguments
    (_)
    (string
      (string_fragment) @arg_value_second)
    (_) @invoke_args ?
  )
) @call_second_arg

; Await expression with second string argument: await emitTo("target", "event")
(call_expression
  function: (await_expression
    [
      (identifier) @func_name_second
      (member_expression property: (property_identifier) @func_name_second)
    ])
  !type_arguments
  arguments: (arguments
    (_)
    (string
      (string_fragment) @arg_value_second)
    (_) @invoke_args ?
  )
) @call_await_second_arg

; === GENERIC CALLS (with type arguments) ===

; Generic function calls: invoke<T>("cmd"), emit<T>("event")
(call_expression
  function: [
    (identifier) @func_name
    (member_expression property: (property_identifier) @func_name)
  ]
  type_arguments: (type_arguments) @type_args
  arguments: (arguments
    (string
      (string_fragment) @arg_value)
    (_) @invoke_args ?
  )
) @call_generic

; Await expression with generic call: await invoke<T>("cmd")
(call_expression
  function: (await_expression
    [
      (identifier) @func_name
      (member_expression property: (property_identifier) @func_name)
    ])
  type_arguments: (type_arguments) @type_args
  arguments: (arguments
    (string
      (string_fragment) @arg_value)
    (_) @invoke_args ?
  )
) @call_await_generic

; Generic calls with second string argument: emitTo<T>("target", "event")
(call_expression
  function: [
    (identifier) @func_name_second
    (member_expression property: (property_identifier) @func_name_second)
  ]
  type_arguments: (type_arguments) @type_args
  arguments: (arguments
    (_)
    (string
      (string_fragment) @arg_value_second)
    (_) @invoke_args ?
  )
) @call_generic_second_arg

; Await expression with second string argument and generics: await emitTo<T>("target", "event")
(call_expression
  function: (await_expression
    [
      (identifier) @func_name_second
      (member_expression property: (property_identifier) @func_name_second)
    ])
  type_arguments: (type_arguments) @type_args
  arguments: (arguments
    (_)
    (string
      (string_fragment) @arg_value_second)
    (_) @invoke_args ?
  )
) @call_await_generic_second_arg

; === INTERFACES ===

(interface_declaration
  name: (type_identifier) @interface_name
) @interface_def

; === USER CODE - CALL PATTERNS (SPECTA & TYPEGEN) ===

; commands.methodName(args)
; Works for both Specta and Typegen bindings
(call_expression
  function: (member_expression
    object: (identifier) @specta_call_object
    property: (property_identifier) @specta_call_method)
  arguments: (arguments) @specta_call_args
) @specta_call_direct

; await commands.methodName(args)
; Works for both Specta and Typegen bindings
(await_expression
  (call_expression
    function: (member_expression
      object: (identifier) @specta_await_object
      property: (property_identifier) @specta_await_method)
    arguments: (arguments) @specta_await_args
  )
) @specta_call_await

; Namespace.commands.methodName(args)
; Works for both Specta and Typegen namespaced imports
; e.g., import * as Specta from './bindings' or import * as Typegen from './bindings'
(call_expression
  function: (member_expression
    object: (member_expression
      object: (identifier) @specta_ns_object
      property: (property_identifier) @specta_ns_commands)
    property: (property_identifier) @specta_ns_method)
  arguments: (arguments) @specta_ns_args
) @specta_ns_call

; await Namespace.commands.methodName(args)
; Works for both Specta and Typegen namespaced imports
(await_expression
  (call_expression
    function: (member_expression
      object: (member_expression
        object: (identifier) @specta_ns_await_object
        property: (property_identifier) @specta_ns_await_commands)
      property: (property_identifier) @specta_ns_await_method)
    arguments: (arguments) @specta_ns_await_args
  )
) @specta_ns_call_await
