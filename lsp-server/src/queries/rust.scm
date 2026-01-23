; Rust queries for Tauri commands and events

; #[tauri::command] fn name() - attribute followed immediately by function
(
  (attribute_item
    (attribute
      (scoped_identifier
        path: (identifier) @_attr_path
        name: (identifier) @_attr_name)))
  .
  (function_item
    name: (identifier) @command_name)
  (#eq? @_attr_path "tauri")
  (#eq? @_attr_name "command")
)

; #[command] fn name() - simplified attribute
(
  (attribute_item
    (attribute
      (identifier) @_attr_simple))
  .
  (function_item
    name: (identifier) @command_name)
  (#eq? @_attr_simple "command")
)

; Method calls: .emit("event"), .listen("event"), etc.
; First argument is the event name
(call_expression
  function: (field_expression
    field: (field_identifier) @method_name)
  arguments: (arguments
    .
    (string_literal
      (string_content) @event_name))
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
      (string_content) @event_name))
  (#any-of? @method_name "emit_to" "emit_str_to")
)
