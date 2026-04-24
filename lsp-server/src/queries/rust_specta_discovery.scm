;; tauri-specta: builder.export(Typescript::default(), "path/to/bindings.ts")
(call_expression
  function: (field_expression
    field: (field_identifier) @method_name)
  arguments: (arguments
    (_)
    (string_literal (string_content) @path_arg)))

;; standalone specta-typescript: Typescript::default().export_to("path/to/bindings.ts", &types)
(call_expression
  function: (field_expression
    field: (field_identifier) @method_name)
  arguments: (arguments
    (string_literal (string_content) @path_arg)
    (_)))
