; JavaScript queries for Tauri commands and events

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
