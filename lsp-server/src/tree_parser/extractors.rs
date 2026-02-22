//! Parameter and field extraction utilities for various node types

use super::utils::NodeTextExt;
use crate::syntax::{parse_serde_attributes, EnumVariant, Parameter, VariantKind};
use std::collections::HashMap;
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

/// Extract Rust enum variants with full detail (kind, fields, serde attributes)
#[must_use]
pub fn extract_rust_enum_variants_full(node: Node, content: &str) -> Vec<EnumVariant> {
    let mut variants = Vec::new();
    let mut cursor = node.walk();

    for child in node.children(&mut cursor) {
        if child.kind() == "enum_variant_list" {
            let mut variant_cursor = child.walk();

            for variant in child.children(&mut variant_cursor) {
                if variant.kind() != "enum_variant" {
                    continue;
                }

                let Some(name_node) = variant.child_by_field_name("name") else {
                    continue;
                };

                let name = name_node.text_or_default(content);

                // Collect variant-level serde attributes
                let mut variant_attrs: Vec<String> = Vec::new();
                let mut vc = variant.walk();

                for vc_child in variant.children(&mut vc) {
                    if vc_child.kind() == "attribute_item" {
                        variant_attrs.push(vc_child.text_or_default(content));
                    }
                }

                let variant_serde = parse_serde_attributes(Some(&variant_attrs));

                // Determine variant kind and extract payload info
                let mut kind = VariantKind::Unit;
                let mut struct_fields = Vec::new();
                let mut tuple_types = Vec::new();

                let mut vc2 = variant.walk();
                for vc_child in variant.children(&mut vc2) {
                    match vc_child.kind() {
                        "field_declaration_list" => {
                            kind = VariantKind::Struct;
                            let mut fc = vc_child.walk();
                            for field in vc_child.children(&mut fc) {
                                if field.kind() == "field_declaration" {
                                    let fname = field.child_by_field_name("name");
                                    let ftype = field.child_by_field_name("type");
                                    if let (Some(n), Some(t)) = (fname, ftype) {
                                        struct_fields.push(Parameter {
                                            name: n.text_or_default(content),
                                            type_name: t.text_or_default(content),
                                        });
                                    }
                                }
                            }
                        }
                        "ordered_field_declaration_list" => {
                            kind = VariantKind::Tuple;
                            let mut tc = vc_child.walk();
                            for field in vc_child.children(&mut tc) {
                                if field.kind() == "ordered_field_declaration" {
                                    if let Some(t) = field.child_by_field_name("type") {
                                        tuple_types.push(t.text_or_default(content));
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }

                variants.push(EnumVariant {
                    name,
                    kind,
                    struct_fields,
                    tuple_types,
                    serde_rename: variant_serde.rename,
                    serde_skip: variant_serde.skip,
                });
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

/// Walk up the tree from `node` and collect parameter types from the first enclosing function.
///
/// Returns a map from parameter name to its TypeScript type annotation text.
/// Returns an empty map if there is no enclosing function or if it has no typed parameters.
#[must_use]
pub fn collect_enclosing_fn_params(node: Node, content: &str) -> HashMap<String, String> {
    let mut ctx = HashMap::new();
    let mut current = node.parent();

    while let Some(n) = current {
        match n.kind() {
            "function_declaration"
            | "function"
            | "arrow_function"
            | "method_definition"
            | "function_expression" => {
                if let Some(params_node) = n.child_by_field_name("parameters") {
                    let mut cursor = params_node.walk();
                    for param in params_node.children(&mut cursor) {
                        if param.kind() != "required_parameter"
                            && param.kind() != "optional_parameter"
                        {
                            continue;
                        }
                        let name_node = param.child_by_field_name("pattern");
                        let type_ann_node = param.child_by_field_name("type");

                        if let (Some(n_node), Some(t_ann)) = (name_node, type_ann_node) {
                            let name = n_node.text_or_default(content);
                            // type_annotation wraps `: type`; find the actual type child
                            let mut tc = t_ann.walk();
                            let type_str = t_ann
                                .children(&mut tc)
                                .find(|c| c.kind() != ":" && !c.is_extra())
                                .map_or_else(
                                    || t_ann.text_or_default(content),
                                    |c| c.text_or_default(content),
                                );
                            ctx.insert(name, type_str);
                        }
                    }
                }
                break; // Stop at the first enclosing function
            }
            _ => {}
        }
        current = n.parent();
    }

    ctx
}

/// Like `infer_ts_type` but resolves identifier names via `ctx` (enclosing fn params).
fn infer_ts_type_with_ctx<S: std::hash::BuildHasher>(
    node: Node,
    content: &str,
    ctx: &HashMap<String, String, S>,
) -> String {
    if node.kind() == "identifier" {
        return ctx
            .get(&node.text_or_default(content))
            .cloned()
            .unwrap_or_else(|| "any".to_string());
    }
    infer_ts_type(node, content)
}

/// Extract TypeScript parameters from an object literal (invoke arguments).
///
/// `ctx` maps variable names to their TypeScript types from the enclosing function,
/// enabling resolution of identifier references (e.g., `{ count: n }` where `n: number`).
#[must_use]
pub fn extract_ts_params<S: std::hash::BuildHasher>(
    node: Node,
    content: &str,
    ctx: &HashMap<String, String, S>,
) -> Vec<Parameter> {
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
                    let type_name = value_node
                        .map_or_else(|| "any".to_string(), |v| infer_ts_type_with_ctx(v, content, ctx));
                    params.push(Parameter { name, type_name });
                }
            }
            // Handle { name } shorthand syntax — look up name in context
            else if child.kind() == "shorthand_property_identifier"
                || child.kind() == "shorthand_property_identifier_pattern"
            {
                let name = child.text_or_default(content);
                let type_name = ctx
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(|| "any".to_string());
                params.push(Parameter { name, type_name });
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
        }
    }
}
