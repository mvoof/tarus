; `<T>x` arguments: TypeScript only. The TSX grammar has no `type_assertion` —
; `<T>` opens an element there — so these are appended for `.ts`, Vue, Svelte and
; Angular files, not for `.tsx`.


(call_expression
  function: (identifier) @func_name
  !type_arguments
  arguments: (arguments
    .
    (type_assertion) @arg_value)
) @call_simple

(call_expression
  function: (await_expression
    (identifier) @func_name)
  !type_arguments
  arguments: (arguments
    .
    (type_assertion) @arg_value)
) @call_await_simple

(call_expression
  function: (identifier) @func_name_second
  !type_arguments
  arguments: (arguments
    .
    (_)
    .
    (type_assertion) @arg_value_second)
) @call_second_arg

(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  !type_arguments
  arguments: (arguments
    .
    (_)
    .
    (type_assertion) @arg_value_second)
) @call_await_second_arg

(call_expression
  function: (identifier) @func_name
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (type_assertion) @arg_value)
) @call_generic

(call_expression
  function: (await_expression
    (identifier) @func_name)
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (type_assertion) @arg_value)
) @call_await_generic

(call_expression
  function: (identifier) @func_name_second
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (_)
    .
    (type_assertion) @arg_value_second)
) @call_generic_second_arg

(call_expression
  function: (await_expression
    (identifier) @func_name_second)
  type_arguments: (type_arguments)
  arguments: (arguments
    .
    (_)
    .
    (type_assertion) @arg_value_second)
) @call_await_generic_second_arg
