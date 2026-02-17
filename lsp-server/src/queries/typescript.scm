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

; === BINDINGS - EXPORTED TYPES (from ts-rs, tauri-specta, tauri-typegen) ===

; Exported type alias: export type Foo = { ... }; or export type Status = "a" | "b";
(export_statement
  declaration: (type_alias_declaration
    name: (type_identifier) @binding_type_name
    value: (_) @binding_type_value
  )
) @binding_type_def

; Exported interface: export interface Foo { ... }
(export_statement
  declaration: (interface_declaration
    name: (type_identifier) @binding_interface_name
    body: (interface_body) @binding_interface_body
  )
) @binding_interface_def

; === BINDINGS - EXPORTED FUNCTIONS (for tauri-specta and tauri-plugin-typegen) ===

; Exported async function with return type
; export async function greet(name: string): Promise<string> { ... }
(export_statement
  declaration: (function_declaration
    "async" ?
    name: (identifier) @binding_func_name
    parameters: (formal_parameters) @binding_func_params
    return_type: (type_annotation
      (_) ? @binding_return_type
    ) ?
  )
) @binding_func_def

; === BINDINGS - OBJECT METHODS (SPECTA & TYPEGEN) ===

; Method inside exported object: export const commands = { async methodName(...) { ... } }
; Used by both tauri-specta and tauri-plugin-typegen
; Specta: return await TAURI_INVOKE("command_name", { args })
; Typegen: return await invoke("command_name", { args })
(export_statement
  declaration: (lexical_declaration
    (variable_declarator
      name: (identifier) @specta_object_name
      value: (object
        (method_definition
          name: (property_identifier) @specta_method_name
          parameters: (formal_parameters) @specta_method_params
          return_type: (type_annotation (_) ? @specta_method_return) ?
          body: (statement_block) @specta_method_body
        )
      )
    )
  )
) @specta_binding_method

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
