use crate::{
    indexer::{FileIndex, Finding},
    syntax::{ArgSource, FrontendSyntax, Rule},
};
use oxc::{
    allocator::Allocator,
    ast::ast::{
        Argument, CallExpression, Expression, ImportDeclaration, ImportDeclarationSpecifier,
    },
    ast_visit::{walk, Visit},
    parser::{Parser, ParserReturn},
    span::SourceType,
};
use std::collections::HashMap;
use std::path::Path;
use tower_lsp::lsp_types::{Position, Range};

/// Visitor to AST ts/tsx
struct FrontendVisitor<'a> {
    source_code: &'a str,
    config: &'a FrontendSyntax,
    findings: Vec<Finding>,
    // "myInvoke" -> "invoke"
    aliases: HashMap<&'a str, &'a str>,
}

impl<'a> FrontendVisitor<'a> {
    fn new(source_code: &'a str, config: &'a FrontendSyntax) -> Self {
        Self {
            source_code,
            config,
            findings: Vec::new(),
            aliases: HashMap::new(),
        }
    }

    /// Converts the byte offset of the Oxc to LSP coordinates.
    /// Oxc simply returns a number (for example, 150), but we need (Line: 5, Char: 10)
    fn offset_to_position(&self, source: &str, offset: usize) -> Position {
        // Trim the string to the required byte
        let safe_offset = offset.min(source.len());
        let split_str = &source[..safe_offset];

        // Count the number of line breaks - this will be the line number
        let line = split_str.lines().count().saturating_sub(1) as u32;

        // Find the last line break to find the indent (symbol)
        let last_line_start = split_str.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let character = (safe_offset - last_line_start) as u32;

        Position { line, character }
    }

    fn get_callee_name(&self, expr: &Expression<'a>) -> Option<&'a str> {
        match expr {
            Expression::Identifier(ident) => Some(ident.name.as_str()),
            _ => None,
        }
    }

    fn get_range(&self, start: u32, end: u32) -> Range {
        Range {
            start: self.offset_to_position(self.source_code, start as usize),
            end: self.offset_to_position(self.source_code, end as usize),
        }
    }

    fn process_rule(&mut self, rule: &Rule, expression: &CallExpression<'a>) {
        let found_key: Option<(String, Range)> = match &rule.args {
            ArgSource::Index { index } => {
                if let Some(arg) = expression.arguments.get(*index) {
                    match arg {
                        Argument::StringLiteral(s) => {
                            // Take the range of the argument string itself
                            let range = self.get_range(s.span.start, s.span.end);
                            Some((s.value.to_string(), range))
                        }
                        _ => None,
                    }
                } else {
                    None
                }
            }

            ArgSource::FunctionName => {
                // Ignore. Never use in frontend
                None
            }
        };

        if let Some((key_name, range)) = found_key {
            self.findings.push(Finding {
                key: key_name,
                entity: rule.entity,     // Command or Event
                behavior: rule.behavior, // Call, Emit or Listen
                range,
            });
        }
    }
}

impl<'a> Visit<'a> for FrontendVisitor<'a> {
    // Collect imports
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if let Some(specifiers) = &decl.specifiers {
            for specifier in specifiers {
                if let ImportDeclarationSpecifier::ImportSpecifier(import_spec) = specifier {
                    let local_name = import_spec.local.name.as_str();
                    let imported_name = import_spec.imported.name().as_str();

                    self.aliases.insert(local_name, imported_name);
                }
            }
        }

        // Continue
        walk::walk_import_declaration(self, decl);
    }

    // Check calls with aliases
    fn visit_call_expression(&mut self, expression: &CallExpression<'a>) {
        if let Some(fn_name) = self.get_callee_name(&expression.callee) {
            // Check if the name is an alias or use it as is
            let original_name = self.aliases.get(fn_name).copied().unwrap_or(fn_name);

            let matching_rule = self
                .config
                .functions
                .iter()
                .find(|rule| rule.name == original_name);

            if let Some(rule) = matching_rule {
                self.process_rule(rule, expression);
            }
        }

        // Continue
        walk::walk_call_expression(self, expression);
    }
}

pub fn parse(path: &Path, source_code: &str, config: &FrontendSyntax) -> FileIndex {
    let allocator = Allocator::default();

    // Try to guess the dialect (TS/JS/JSX)
    let source_type = SourceType::from_path(path)
        .unwrap_or_default()
        .with_typescript(true)
        .with_jsx(true);

    let ParserReturn { program, .. } = Parser::new(&allocator, source_code, source_type).parse();

    let mut visitor = FrontendVisitor::new(source_code, config);
    visitor.visit_program(&program);

    FileIndex {
        path: path.to_path_buf(),
        findings: visitor.findings,
    }
}
