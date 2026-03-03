(export_statement
  (lexical_declaration
    (variable_declarator
      name: (identifier) @var_name
      value: (object
        (method_definition
          name: (property_identifier) @method_name
          parameters: (formal_parameters) @params
          return_type: (type_annotation (_) @return_type)
          body: (statement_block))))))
