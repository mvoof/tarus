//! Parameter and field extraction utilities for various node types

use super::utils::NodeTextExt;
use crate::indexer::Parameter;
use tree_sitter::Node;

/// Extract Rust function parameters
#[must_use]
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
#[must_use]
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

/// Extract Rust enum variants (legacy - returns Parameters)
#[must_use]
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

/// Extract detailed Rust enum variants with type information
#[must_use]
#[allow(dead_code)] // Infrastructure for future full enum variant support
pub fn extract_enum_variants_detailed(node: Node, content: &str) -> Vec<crate::indexer::EnumVariant> {
    let mut variants = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "enum_variant_list" {
            let mut variant_cursor = child.walk();

            for variant in child.children(&mut variant_cursor) {
                if variant.kind() == "enum_variant" {
                    let name_node = variant.child_by_field_name("name");

                    if let Some(name_n) = name_node {
                        let name = name_n.text_or_default(content);

                        // Check for struct-style fields: Name { field: Type, ... }
                        let mut has_struct_fields = false;
                        let mut has_tuple_fields = false;
                        let mut fields = Vec::new();

                        let mut variant_child_cursor = variant.walk();
                        for v_child in variant.children(&mut variant_child_cursor) {
                            match v_child.kind() {
                                "field_declaration_list" => {
                                    // Struct variant: Name { field: Type }
                                    has_struct_fields = true;
                                    let mut field_cursor = v_child.walk();

                                    for field in v_child.children(&mut field_cursor) {
                                        if field.kind() == "field_declaration" {
                                            let field_name = field.child_by_field_name("name");
                                            let field_type = field.child_by_field_name("type");

                                            if let (Some(fn_node), Some(ft_node)) = (field_name, field_type) {
                                                fields.push(Parameter {
                                                    name: fn_node.text_or_default(content),
                                                    type_name: ft_node.text_or_default(content),
                                                });
                                            }
                                        }
                                    }
                                }
                                "tuple_type" | "ordered_field_declaration_list" => {
                                    // Tuple variant: Name(Type1, Type2)
                                    has_tuple_fields = true;
                                    let mut tuple_cursor = v_child.walk();
                                    let mut idx = 0;

                                    for tuple_field in v_child.children(&mut tuple_cursor) {
                                        // For tuple types, each child that's a type is a field
                                        if tuple_field.kind() != "(" && tuple_field.kind() != ")" && tuple_field.kind() != "," {
                                            fields.push(Parameter {
                                                name: format!("_{idx}"),
                                                type_name: tuple_field.text_or_default(content),
                                            });
                                            idx += 1;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }

                        let variant_type = if has_struct_fields {
                            crate::indexer::EnumVariantType::Struct
                        } else if has_tuple_fields {
                            crate::indexer::EnumVariantType::Tuple
                        } else {
                            crate::indexer::EnumVariantType::Unit
                        };

                        variants.push(crate::indexer::EnumVariant {
                            name,
                            variant_type,
                            fields: if fields.is_empty() { None } else { Some(fields) },
                        });
                    }
                }
            }
        }
    }

    variants
}

/// Extract TypeScript interface fields
#[must_use]
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
#[must_use]
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
                        type_name = infer_ts_type(v, content);
                    }

                    params.push(Parameter { name, type_name });
                }
            }
            // Handle { name } shorthand syntax
            else if child.kind() == "shorthand_property_identifier"
                || child.kind() == "shorthand_property_identifier_pattern"
            {
                let name = child.text_or_default(content);
                params.push(Parameter {
                    name,
                    type_name: "any".to_string(), // Variable reference, treat as any
                });
            }
        }
    }
    params
}

fn infer_ts_type(node: Node, content: &str) -> String {
    match node.kind() {
        "string" => "string".to_string(),
        "number" => "number".to_string(),
        "true" | "false" => "boolean".to_string(),
        "array" => {
            // Try to infer array inner type if all elements are same
            // Simplification: just use any[] for now, or maybe check first element?
            "any[]".to_string()
        }
        "object" => {
            // Recursively build { key: type, ... }
            let mut fields = Vec::new();
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "pair" {
                    if let (Some(k), Some(v)) = (
                        child.child_by_field_name("key"),
                        child.child_by_field_name("value"),
                    ) {
                        let k_name = k.text_or_default(content);
                        let v_type = infer_ts_type(v, content);
                        fields.push(format!("{k_name}: {v_type}"));
                    }
                } else if child.kind() == "shorthand_property_identifier" {
                    let name = child.text_or_default(content);
                    fields.push(format!("{name}: any"));
                }
            }
            if fields.is_empty() {
                "{}".to_string() // Empty object or treat as 'object'
            } else {
                format!("{{ {} }}", fields.join(", "))
            }
        }
        // Variable reference
        _ => "any".to_string(),
    }
}

/// Builder for constructing Finding objects with optional fields
///
/// This builder provides a fluent interface for creating Finding objects,
/// automatically handling the conversion of empty collections to None.
pub struct FindingBuilder {
    key: String,
    entity: crate::syntax::EntityType,
    behavior: crate::syntax::Behavior,
    range: tower_lsp_server::ls_types::Range,
    parameters: Option<Vec<Parameter>>,
    return_type: Option<String>,
    fields: Option<Vec<Parameter>>,
    attributes: Option<Vec<String>>,
    variants: Option<Vec<crate::indexer::EnumVariant>>,
}

impl FindingBuilder {
    /// Create a new `FindingBuilder` with required fields
    #[must_use]
    pub fn new(
        key: String,
        entity: crate::syntax::EntityType,
        behavior: crate::syntax::Behavior,
        range: tower_lsp_server::ls_types::Range,
    ) -> Self {
        Self {
            key,
            entity,
            behavior,
            range,
            parameters: None,
            return_type: None,
            fields: None,
            attributes: None,
            variants: None,
        }
    }

    /// Set parameters as Option (automatically filters empty vecs)
    #[must_use]
    pub fn with_parameters_opt(mut self, params: Option<Vec<Parameter>>) -> Self {
        self.parameters = params.filter(|p| !p.is_empty());
        self
    }

    /// Set return type as Option
    #[must_use]
    pub fn with_return_type_opt(mut self, return_type: Option<String>) -> Self {
        self.return_type = return_type;
        self
    }

    /// Set fields (automatically converts empty Vec to None)
    #[must_use]
    pub fn with_fields(mut self, fields: Vec<Parameter>) -> Self {
        self.fields = if fields.is_empty() {
            None
        } else {
            Some(fields)
        };
        self
    }

    /// Set attributes (automatically converts empty Vec to None)
    #[must_use]
    pub fn with_attributes(mut self, attrs: Vec<String>) -> Self {
        self.attributes = if attrs.is_empty() { None } else { Some(attrs) };
        self
    }

    /// Set enum variants (automatically converts empty Vec to None)
    #[must_use]
    #[allow(dead_code)] // Infrastructure for future full enum variant support
    pub fn with_variants(mut self, variants: Vec<crate::indexer::EnumVariant>) -> Self {
        self.variants = if variants.is_empty() {
            None
        } else {
            Some(variants)
        };
        self
    }

    /// Build the Finding object
    #[must_use]
    pub fn build(self) -> crate::indexer::Finding {
        crate::indexer::Finding {
            key: self.key,
            entity: self.entity,
            behavior: self.behavior,
            range: self.range,
            parameters: self.parameters,
            return_type: self.return_type,
            fields: self.fields,
            attributes: self.attributes,
            variants: self.variants,
        }
    }
}
