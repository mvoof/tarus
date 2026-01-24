; TypeScript-specific queries (generics and type annotations)
; These patterns are combined with common-ecma.scm for full TypeScript support

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

; Override common patterns to exclude type_arguments (to avoid double-matching)
; Simple function calls without generics
(call_expression
  function: (identifier) @func_name
  !type_arguments
  arguments: (arguments
    .
    (string
      (string_fragment) @arg_value))
) @call_simple_no_generic

; Await expression with simple call (no generics): await invoke("cmd")
(call_expression
  function: (await_expression
    (identifier) @func_name)
  !type_arguments
  arguments: (arguments
    .
    (string
      (string_fragment) @arg_value))
) @call_await_simple

; Function calls with second string argument (no generics)
(call_expression
  function: (identifier) @func_name_second
  !type_arguments
  arguments: (arguments
    (_)
    .
    (string
      (string_fragment) @arg_value_second))
) @call_second_arg_no_generic

; Await expression with second string argument (no generics)
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
