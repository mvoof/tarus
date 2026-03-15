; Rust queries for Tauri commands and events

; Function items — #[tauri::command] detection is done via sibling walk in Rust code.
; This handles any number of attributes between #[tauri::command] and fn.
(function_item
  name: (identifier) @fn_name) @fn_item

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

; Struct items — #[derive(Event)] detection is done via sibling walk in Rust code.
(struct_item
  name: (type_identifier) @struct_name) @struct_item

; Specta typed event emit: GlobalEvent(payload).emit_to(&app)
(call_expression
  function: (field_expression
    value: (call_expression
      function: (identifier) @specta_emit_struct)
    field: (field_identifier) @specta_emit_method)
  (#any-of? @specta_emit_method "emit" "emit_to" "emit_filter" "emit_str")
)
