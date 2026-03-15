(call_expression
  function: (field_expression
    field: (field_identifier) @method_name)
  arguments: (arguments
    (string_literal (string_content) @event_name)
    . (_) @payload_arg)
  (#any-of? @method_name "emit" "emit_filter" "emit_to")
)
