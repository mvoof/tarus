use std::path::Path;

use crate::{
    indexer::{FileIndex, Finding},
    syntax::{ArgSource, BackendSyntax},
};
use tower_lsp::lsp_types::{Position, Range};

use syn::visit::{self, Visit};
use syn::{Attribute, Expr, ExprLit, ExprMethodCall, ItemFn, Lit, Meta};

/// Visitor to AST Rust
struct BackendVisitor<'a> {
    config: &'a BackendSyntax,
    findings: Vec<Finding>,
}

impl<'a> BackendVisitor<'a> {
    fn new(config: &'a BackendSyntax) -> Self {
        Self {
            config,
            findings: Vec::new(),
        }
    }

    /// Convert Span from syn (Line/Col 1-based) to LSP Range (Line/Col 0-based).
    /// In Rust, lines are counted from 1 (the first line is 1).
    /// LSP always requires counting from 0 (the first line is 0)
    fn span_to_range(&self, span: proc_macro2::Span) -> Range {
        let start = span.start();
        let end = span.end();

        Range {
            start: Position {
                line: start.line.saturating_sub(1) as u32,
                character: start.column as u32,
            },
            end: Position {
                line: end.line.saturating_sub(1) as u32,
                character: end.column as u32,
            },
        }
    }

    /// Checks if a function is a Tauri command
    fn is_tauri_command(&self, attrs: &[Attribute]) -> bool {
        for attr in attrs {
            // #[tauri::command]
            if attr.path().is_ident("command") {
                return true;
            }

            // Check the full path of tauri::command
            if let Meta::Path(path) = &attr.meta {
                if path.segments.len() == 2
                    && path.segments[0].ident == "tauri"
                    && path.segments[1].ident == "command"
                {
                    return true;
                }
            }
        }
        false
    }

    /// Processing the rule for arguments (pulling the string out of the call)
    fn process_args(
        &self,
        args_source: &ArgSource,
        method_call: &ExprMethodCall,
    ) -> Option<(String, Range)> {
        match args_source {
            ArgSource::Index { index } => {
                // Get argument by index
                if let Some(arg_expr) = method_call.args.iter().nth(*index) {
                    // We only need string literals: "my-event"
                    // arg_expr can be a reference (&str), so we go inside
                    let expr = match arg_expr {
                        Expr::Reference(r) => &*r.expr,
                        e => e,
                    };

                    if let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = expr
                    {
                        return Some((s.value(), self.span_to_range(s.span())));
                    }
                }

                None
            }

            ArgSource::FunctionName => {
                // Ignore. We handle the case where the command is a function name elsewhere.
                // #[tauri::command] fn **my_command**() { ... }
                None
            }
        }
    }
}

impl<'a> Visit<'a> for BackendVisitor<'a> {
    // 1. Search for Command Definitions
    fn visit_item_fn(&mut self, node: &'a ItemFn) {
        // Check if the #[tauri::command] attribute exists
        if self.is_tauri_command(&node.attrs) {
            let fn_name = node.sig.ident.to_string();

            // Check if there is a rule for attributes in the config
            for rule in &self.config.attributes {
                if rule.name == "tauri::command" {
                    let range = self.span_to_range(node.sig.ident.span());

                    self.findings.push(Finding {
                        key: fn_name.clone(),
                        entity: rule.entity,     // Command
                        behavior: rule.behavior, // Definition
                        range,
                    });

                    break;
                }
            }
        }

        // Continue
        visit::visit_item_fn(self, node);
    }

    // 2. Call Search (Emit / Listen)
    fn visit_expr_method_call(&mut self, node: &'a ExprMethodCall) {
        let method_name = node.method.to_string();

        // Check the rule for this method in the config
        let rule = self.config.functions.iter().find(|r| r.name == method_name);

        if let Some(rule) = rule {
            // Get the event name from the arguments
            if let Some((key, range)) = self.process_args(&rule.args, node) {
                self.findings.push(Finding {
                    key,
                    entity: rule.entity,
                    behavior: rule.behavior,
                    range,
                });
            }
        }

        // Continue
        visit::visit_expr_method_call(self, node);
    }
}

pub fn parse(path: &Path, source_code: &str, config: &BackendSyntax) -> FileIndex {
    let Ok(ast) = syn::parse_file(source_code) else {
        eprintln!("Failed to parse Rust file: {:?}", path);

        return FileIndex {
            path: path.to_path_buf(),
            findings: Vec::new(),
        };
    };

    let mut visitor = BackendVisitor::new(config);
    visitor.visit_file(&ast);

    FileIndex {
        path: path.to_path_buf(),
        findings: visitor.findings,
    }
}
