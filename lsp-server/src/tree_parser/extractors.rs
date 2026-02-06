//! Parameter and field extraction utilities for various node types

use super::utils::NodeTextExt;
use crate::indexer::Parameter;
use tree_sitter::Node;

/// Extract Rust function parameters
pub fn extract_rust_params(node: Node, content: &str) -> Vec<Parameter> {
    let mut params = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "parameter" {
            let name_node = child.child_by_field_name("pattern");
            let type_node = child.child_by_field_name("type");

            if let (Some(n), Some(t)) = (name_node, type_node) {
                params.push(Parameter {
                    name: n.text_or_default(content),
                    type_name: t.text_or_default(content),
                });
            }
        }
    }
    params
}

/// Extract Rust struct fields
pub fn extract_rust_struct_fields(node: Node, content: &str) -> Vec<Parameter> {
    let mut fields = Vec::new();
    let mut cursor = node.walk();

    // Navigate to field_declaration_list
    for child in node.children(&mut cursor) {
        if child.kind() == "field_declaration_list" {
            let mut field_cursor = child.walk();

            for field in child.children(&mut field_cursor) {
                if field.kind() == "field_declaration" {
                    let name_node = field.child_by_field_name("name");
                    let type_node = field.child_by_field_name("type");

                    if let (Some(n), Some(t)) = (name_node, type_node) {
                        fields.push(Parameter {
                            name: n.text_or_default(content),
                            type_name: t.text_or_default(content),
                        });
                    }
                }
            }
        }
    }

    fields
}

/// Extract Rust enum variants
pub fn extract_rust_enum_variants(node: Node, content: &str) -> Vec<Parameter> {
    let mut variants = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "enum_variant_list" {
            let mut variant_cursor = child.walk();

            for variant in child.children(&mut variant_cursor) {
                if variant.kind() == "enum_variant" {
                    let name_node = variant.child_by_field_name("name");

                    if let Some(n) = name_node {
                        variants.push(Parameter {
                            name: n.text_or_default(content),
                            type_name: "enum_variant".to_string(),
                        });
                    }
                }
            }
        }
    }

    variants
}

/// Extract TypeScript interface fields
pub fn extract_ts_interface_fields(node: Node, content: &str) -> Vec<Parameter> {
    let mut fields = Vec::new();
    let mut cursor = node.walk();

    // Navigate to interface_body
    for child in node.children(&mut cursor) {
        if child.kind() == "interface_body" {
            let mut field_cursor = child.walk();

            for field in child.children(&mut field_cursor) {
                if field.kind() == "property_signature" {
                    let name_node = field.child_by_field_name("name");
                    let type_ann_node = field.child_by_field_name("type");

                    if let (Some(n), Some(ta)) = (name_node, type_ann_node) {
                        // type_annotation has a child which is the actual type
                        let mut ta_cursor = ta.walk();

                        let type_node = ta
                            .children(&mut ta_cursor)
                            .find(|c| c.kind() != ":" && c.kind() != "comment");

                        if let Some(tn) = type_node {
                            fields.push(Parameter {
                                name: n.text_or_default(content),
                                type_name: tn.text_or_default(content),
                            });
                        }
                    }
                }
            }
        }
    }
    fields
}

/// Extract TypeScript parameters from an object literal (invoke arguments)
pub fn extract_ts_params(node: Node, content: &str) -> Vec<Parameter> {
    let mut params = Vec::new();

    if node.kind() == "object" {
        let mut cursor = node.walk();

        for child in node.children(&mut cursor) {
            // Handle { key: value } syntax
            if child.kind() == "pair" {
                let key_node = child.child_by_field_name("key");
                let value_node = child.child_by_field_name("value");

                if let Some(k) = key_node {
                    let name = k.text_or_default(content);
                    let mut type_name = "any".to_string();

                    if let Some(v) = value_node {
                        // Very basic type inference from literal values
                        type_name = match v.kind() {
                            "string" => "string",
                            "number" => "number",
                            "true" | "false" => "boolean",
                            "array" => "any[]",
                            "object" => "object",
                            _ => "any",
                        }
                        .to_string();
                    }

                    params.push(Parameter { name, type_name });
                }
            }
            // Handle { name } shorthand syntax (shorthand_property_identifier)
            else if child.kind() == "shorthand_property_identifier"
                || child.kind() == "shorthand_property_identifier_pattern"
            {
                let name = child.text_or_default(content);

                // For shorthand, we can't infer type from literal - it's a variable reference
                params.push(Parameter {
                    name,
                    type_name: "any".to_string(),
                });
            }
        }
    }
    params
}
