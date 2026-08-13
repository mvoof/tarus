; TypeScript queries for Tauri commands and events
; Includes both simple and generic patterns

; === IMPORTS ===

; Import specifiers with alias and source
(import_statement
  (import_clause
    (named_imports
      (import_specifier
        name: (identifier) @imported_name
        alias: (identifier) @local_alias
      )
    )
  )
  source: (string (string_fragment) @import_source)
)

; Simple imports without alias and with source
(import_statement
  (import_clause
    (named_imports
      (import_specifier
        name: (identifier) @imported_name
        !alias
      )
    )
  )
  source: (string (string_fragment) @import_source)
)

; === SIMPLE CALLS (no generics) ===

; Simple function calls: invoke("cmd"), emit("event")
(call_expression
  function: (identifier) @func_name
  !type_arguments
  arguments: (arguments
    .
    [(string) (template_string)] @arg_value)
) @call_simple

(call_expression
  function: (identifier) @func_name
  !type_arguments
  arguments: (arguments
    .
    (identifier) @arg_value)
) @call_simple

(call_expression
  function: (identifier) @func_name
  !type_arguments
  arguments: (arguments
    .
    (member_expression) @arg_value)
) @call_simple

; Await expression with simple call: await invoke("cmd")
(call_expression
  function: (await_expression
    (identifier) @func_name)
  !type_arguments
  arguments: (arguments
    .
    [(string) (template_string)] @arg_value)
) @call_await_simple

(call_expression
  function: (await_expression
    (identifier) @func_name)
  !type_arguments
  arguments: (arguments
    .
    (identifier) @arg_value)
) @call_await_simple

(call_expression
  function: (await_expression
    (identifier) @func_name)
  !type_arguments
  arguments: (arguments
    .
    (member_expression) @arg_value)
) @call_await_simple

; Function calls with second string argument: emitTo("target", "event")
(call_expression
  function: (identifier) @func_name_second
  !type_arguments
  arguments: (arguments
    (_)
    .
    [(string) (template_string)] @arg_value_second)
) @call_second_arg

(call_expression
  function: (identifier) @func_name_second
  !type_arguments
  arguments: (arguments
    (_)
    .
    (identifier) @arg_value_second)
) @call_second_arg

(call_expression
  function: (identifier) @func_name_second
  !type_arguments
  arguments: (arguments
    (_)
    .
    (member_expression) @arg_value_second)
) @call_second_arg

; Await expression with second string argument: await emitTo("target", "event")
(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  !type_arguments
  arguments: (arguments
    (_)
    .
    [(string) (template_string)] @arg_value_second)
) @call_await_second_arg

(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  !type_arguments
  arguments: (arguments
    (_)
    .
    (identifier) @arg_value_second)
) @call_await_second_arg

(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  !type_arguments
  arguments: (arguments
    (_)
    .
    (member_expression) @arg_value_second)
) @call_await_second_arg

; === GENERIC CALLS (with type arguments) ===

; Generic function calls: invoke<T>("cmd"), emit<T>("event")
(call_expression
  function: (identifier) @func_name
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    [(string) (template_string)] @arg_value)
) @call_generic

(call_expression
  function: (identifier) @func_name
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (identifier) @arg_value)
) @call_generic

(call_expression
  function: (identifier) @func_name
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (member_expression) @arg_value)
) @call_generic

; Await expression with generic call: await invoke<T>("cmd")
(call_expression
  function: (await_expression
    (identifier) @func_name)
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    [(string) (template_string)] @arg_value)
) @call_await_generic

(call_expression
  function: (await_expression
    (identifier) @func_name)
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (identifier) @arg_value)
) @call_await_generic

(call_expression
  function: (await_expression
    (identifier) @func_name)
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (member_expression) @arg_value)
) @call_await_generic

; Generic calls with second string argument: emitTo<T>("target", "event")
(call_expression
  function: (identifier) @func_name_second
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    [(string) (template_string)] @arg_value_second)
) @call_generic_second_arg

(call_expression
  function: (identifier) @func_name_second
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    (identifier) @arg_value_second)
) @call_generic_second_arg

(call_expression
  function: (identifier) @func_name_second
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    (member_expression) @arg_value_second)
) @call_generic_second_arg

; Await expression with second string argument and generics: await emitTo<T>("target", "event")
(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    [(string) (template_string)] @arg_value_second)
) @call_await_generic_second_arg

(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    (identifier) @arg_value_second)
) @call_await_generic_second_arg

(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  type_arguments: (type_arguments)
  arguments: (arguments
    (_)
    .
    (member_expression) @arg_value_second)
) @call_await_generic_second_arg

; === SPECTA CALLS (commands.methodName(...)) ===

; commands.getUserProfile(...)
(call_expression
  function: (member_expression
    object: (identifier) @_specta_obj
    property: (property_identifier) @specta_method_name)
  (#eq? @_specta_obj "commands")
) @specta_call

; === SPECTA EVENTS (events.eventName.listen/emit/once(...)) ===

; Global: events.globalEvent.listen(handler)
(call_expression
  function: (member_expression
    object: (member_expression
      object: (identifier) @_specta_events_obj
      property: (property_identifier) @specta_event_name)
    property: (property_identifier) @specta_event_method)
  (#eq? @_specta_events_obj "events")
) @specta_event_call

; Window-targeted: events.globalEvent(appWindow).listen(handler)
(call_expression
  function: (member_expression
    object: (call_expression
      function: (member_expression
        object: (identifier) @_specta_events_obj
        property: (property_identifier) @specta_event_name)
      arguments: (arguments))
    property: (property_identifier) @specta_event_method)
  (#eq? @_specta_events_obj "events")
) @specta_event_call

; === PLAIN CALLS (candidate forwarding helpers) ===
; Any bare call; only those naming a known forwarder are acted upon.
(call_expression
  function: (identifier) @plain_fn
  arguments: (arguments) @plain_args
)

; `await helper<T>(...)` parses with the await *inside* the callee:
;   (call_expression function: (await_expression (identifier)) type_arguments: ...)
; The plain `await helper(...)` does not — it nests the other way round. Without
; this pattern an awaited generic call never reaches the forwarder resolver, and
; every event the helper subscribes to looks like it has no listener.
(call_expression
  function: (await_expression
    (identifier) @plain_fn)
  arguments: (arguments) @plain_args
)
