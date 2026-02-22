; Rust queries for Tauri commands and events

; #[tauri::command] fn name() - immediate sibling (most common case)
(
  (attribute_item
    (attribute
      (scoped_identifier
        path: (identifier) @_attr_path
        name: (identifier) @_attr_name))) .
  (function_item
    name: (identifier) @command_name
    parameters: (parameters) @command_params
    return_type: (_) @command_return_type
  )
  (#eq? @_attr_path "tauri")
  (#eq? @_attr_name "command")
)

; #[tauri::command] with ONE intermediate attribute (e.g., #[cfg_attr])
(
  (attribute_item
    (attribute
      (scoped_identifier
        path: (identifier) @_attr_path
        name: (identifier) @_attr_name))) .
  (attribute_item) .
  (function_item
    name: (identifier) @command_name
    parameters: (parameters) @command_params
    return_type: (_) @command_return_type
  )
  (#eq? @_attr_path "tauri")
  (#eq? @_attr_name "command")
)

; #[tauri::command] with TWO intermediate attributes (rare but possible)
(
  (attribute_item
    (attribute
      (scoped_identifier
        path: (identifier) @_attr_path
        name: (identifier) @_attr_name))) .
  (attribute_item) .
  (attribute_item) .
  (function_item
    name: (identifier) @command_name
    parameters: (parameters) @command_params
    return_type: (_) @command_return_type
  )
  (#eq? @_attr_path "tauri")
  (#eq? @_attr_name "command")
)

; #[command] fn name() - simplified attribute, immediate sibling
(
  (attribute_item
    (attribute
      (identifier) @_attr_simple)) .
  (function_item
    name: (identifier) @command_name
    parameters: (parameters) @command_params
    return_type: (_) @command_return_type
  )
  (#eq? @_attr_simple "command")
)

; #[command] with ONE intermediate attribute
(
  (attribute_item
    (attribute
      (identifier) @_attr_simple)) .
  (attribute_item) .
  (function_item
    name: (identifier) @command_name
    parameters: (parameters) @command_params
    return_type: (_) @command_return_type
  )
  (#eq? @_attr_simple "command")
)

; #[command] with TWO intermediate attributes
(
  (attribute_item
    (attribute
      (identifier) @_attr_simple)) .
  (attribute_item) .
  (attribute_item) .
  (function_item
    name: (identifier) @command_name
    parameters: (parameters) @command_params
    return_type: (_) @command_return_type
  )
  (#eq? @_attr_simple "command")
)

; Struct definitions with attributes
(
  (attribute_item)* @struct_attr
  (struct_item
    name: (type_identifier) @struct_name
    body: (field_declaration_list
      (field_declaration
        name: (field_identifier) @field_name
        type: (_) @field_type
      )*
    )
  ) @struct_def
)

; Enum definitions with attributes (captures all variant types)
(
  (attribute_item)* @enum_attr
  (enum_item
    name: (type_identifier) @enum_name
    body: (enum_variant_list
      (enum_variant
        name: (identifier) @variant_name
        ; Struct variant fields: Name { field: Type }
        (field_declaration_list
          (field_declaration
            name: (field_identifier) @variant_field_name
            type: (_) @variant_field_type
          )*
        ) ?
        ; Tuple variant fields: Name(Type1, Type2)
        (ordered_field_declaration_list) ? @variant_tuple_fields
        ; Alternative: (tuple_type) for inline tuple types
        (tuple_type) ? @variant_tuple_type
      )*
    )
  ) @enum_def
)

; Method calls: .emit("event"), .listen("event"), etc.
; First argument is the event name
(call_expression
  function: (field_expression
    field: (field_identifier) @method_name)
  arguments: (arguments
    .
    (string_literal
      (string_content) @event_name)
    (_) @event_args ? )
  (#any-of? @method_name "emit" "emit_str" "emit_filter" "emit_str_filter" "listen" "listen_any" "once" "once_any")
)

; Method calls with event as second argument: .emit_to(target, "event")
(call_expression
  function: (field_expression
    field: (field_identifier) @method_name)
  arguments: (arguments
    (_)
    .
    (string_literal
      (string_content) @event_name)
    (_) @event_args ? )
  (#any-of? @method_name "emit_to" "emit_str_to")
)
