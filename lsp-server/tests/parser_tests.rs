//! Parser tests for all supported languages

mod common;

use common::{load_fixture, test_path};

#[cfg(test)]
mod rust_parser_tests {
    use super::*;
    use lsp_server::tree_parser;
    use lsp_server::syntax::{Behavior, EntityType};

    #[test]
    fn test_parse_rust_simple_command() {
        let content = load_fixture("rust/simple_command.rs");
        let path = test_path("simple_command.rs");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok(), "Failed to parse Rust file: {:?}", result.err());
        
        let file_index = result.unwrap();
        assert_eq!(file_index.findings.len(), 1, "Expected 1 command");
        
        let finding = &file_index.findings[0];
        assert_eq!(finding.key, "greet");
        assert_eq!(finding.entity, EntityType::Command);
        assert_eq!(finding.behavior, Behavior::Definition);
    }

    #[test]
    fn test_parse_rust_multiple_commands() {
        let content = load_fixture("rust/multiple_commands.rs");
        let path = test_path("multiple_commands.rs");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        assert_eq!(file_index.findings.len(), 3, "Expected 3 commands");
        
        let command_names: Vec<&str> = file_index.findings
            .iter()
            .map(|f| f.key.as_str())
            .collect();
        
        assert!(command_names.contains(&"get_user"));
        assert!(command_names.contains(&"save_data"));
        assert!(command_names.contains(&"process_item"));
    }

    #[test]
    fn test_parse_rust_events() {
        let content = load_fixture("rust/events.rs");
        let path = test_path("events.rs");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        // Should find emit and listen calls
        let emits: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.behavior == Behavior::Emit)
            .collect();
        
        let listens: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.behavior == Behavior::Listen)
            .collect();
        
        assert!(!emits.is_empty(), "Expected at least one emit");
        assert!(!listens.is_empty(), "Expected at least one listen");
    }
}

#[cfg(test)]
mod typescript_parser_tests {
    use super::*;
    use lsp_server::tree_parser;
    use lsp_server::syntax::{Behavior, EntityType};

    #[test]
    fn test_parse_ts_invoke() {
        let content = load_fixture("typescript/invoke.ts");
        let path = test_path("invoke.ts");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        let invokes: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();
        
        assert!(!invokes.is_empty(), "Expected at least one invoke call");
        
        let command_names: Vec<&str> = invokes.iter().map(|f| f.key.as_str()).collect();
        assert!(command_names.contains(&"greet"));
        assert!(command_names.contains(&"get_user"));
    }

    #[test]
    fn test_parse_ts_generic_invoke() {
        let content = load_fixture("typescript/generic_calls.tsx");
        let path = test_path("generic_calls.tsx");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        let invokes: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();
        
        assert!(!invokes.is_empty(), "Expected invoke calls with generics");
    }

    #[test]
    fn test_parse_ts_emit_listen() {
        let content = load_fixture("typescript/emit.ts");
        let path = test_path("emit.ts");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        let emits: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.behavior == Behavior::Emit)
            .collect();
        
        let listens: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.behavior == Behavior::Listen)
            .collect();
        
        assert!(!emits.is_empty(), "Expected emit calls");
        assert!(!listens.is_empty(), "Expected listen calls");
    }
}

#[cfg(test)]
mod javascript_parser_tests {
    use super::*;
    use lsp_server::tree_parser;
    use lsp_server::syntax::{Behavior, EntityType};

    #[test]
    fn test_parse_js_invoke() {
        let content = load_fixture("javascript/invoke.js");
        let path = test_path("invoke.js");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        let invokes: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();
        
        assert!(!invokes.is_empty(), "Expected invoke calls in JavaScript");
    }

    #[test]
    fn test_parse_jsx_emit() {
        let content = load_fixture("javascript/emit.jsx");
        let path = test_path("emit.jsx");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        let emits: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.behavior == Behavior::Emit)
            .collect();
        
        assert!(!emits.is_empty(), "Expected emit calls in JSX");
    }
}

#[cfg(test)]
mod vue_parser_tests {
    use super::*;
    use lsp_server::tree_parser;
    use lsp_server::syntax::EntityType;

    #[test]
    fn test_parse_vue_single_script() {
        let content = load_fixture("vue/single_script.vue");
        let path = test_path("single_script.vue");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        let invokes: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();
        
        assert!(!invokes.is_empty(), "Expected invoke in Vue single script");
    }

    #[test]
    fn test_parse_vue_setup() {
        let content = load_fixture("vue/multiple_scripts.vue");
        let path = test_path("multiple_scripts.vue");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        assert!(!file_index.findings.is_empty(), "Expected findings in Vue setup script");
    }
}

#[cfg(test)]
mod svelte_parser_tests {
    use super::*;
    use lsp_server::tree_parser;

    #[test]
    fn test_parse_svelte_component() {
        let content = load_fixture("svelte/component.svelte");
        let path = test_path("component.svelte");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        assert!(!file_index.findings.is_empty(), "Expected findings in Svelte component");
    }
}

#[cfg(test)]
mod angular_parser_tests {
    use super::*;
    use lsp_server::tree_parser;
    use lsp_server::syntax::EntityType;

    #[test]
    fn test_parse_angular_component() {
        let content = load_fixture("angular/component.component.ts");
        let path = test_path("component.component.ts");
        
        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());
        
        let file_index = result.unwrap();
        
        let invokes: Vec<_> = file_index.findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();
        
        assert!(!invokes.is_empty(), "Expected invoke in Angular component");
    }
}
