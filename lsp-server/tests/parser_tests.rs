//! Parser tests for all supported languages

mod common_fixtures;
mod common_paths;

use common_fixtures::load_fixture;
use common_paths::test_path;

#[cfg(test)]
mod rust_parser_tests {
    use super::*;
    use lsp_server::syntax::{Behavior, EntityType};
    use lsp_server::tree_parser;

    #[test]
    fn test_parse_rust_simple_command() {
        let content = load_fixture("rust/simple_command.rs");
        let path = test_path("simple_command.rs");

        let result = tree_parser::parse(&path, &content);
        assert!(
            result.is_ok(),
            "Failed to parse Rust file: {:?}",
            result.err()
        );

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

        let command_names: Vec<&str> = file_index.findings.iter().map(|f| f.key.as_str()).collect();

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
        let emits: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.behavior == Behavior::Emit)
            .collect();

        let listens: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.behavior == Behavior::Listen)
            .collect();

        assert!(!emits.is_empty(), "Expected at least one emit");
        assert!(!listens.is_empty(), "Expected at least one listen");
    }

    #[test]
    fn test_event_payload_scoped_identifier() {
        let content = load_fixture("rust/event_payloads.rs");
        let path = test_path("event_payloads.rs");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());

        let file_index = result.unwrap();
        let emit = file_index
            .findings
            .iter()
            .find(|f| f.key == "calc-status" && f.behavior == Behavior::Emit)
            .expect("Expected calc-status emit");

        assert_eq!(
            emit.return_type.as_deref(),
            Some("CalculationStatus"),
            "Scoped identifier payload should infer to CalculationStatus"
        );
    }

    #[test]
    fn test_event_payload_struct_expression() {
        let content = load_fixture("rust/event_payloads.rs");
        let path = test_path("event_payloads.rs");

        let result = tree_parser::parse(&path, &content).unwrap();
        let emit = result
            .findings
            .iter()
            .find(|f| f.key == "struct-event" && f.behavior == Behavior::Emit)
            .expect("Expected struct-event emit");

        assert_eq!(
            emit.return_type.as_deref(),
            Some("MyStruct"),
            "Struct expression payload should infer to MyStruct"
        );
    }

    #[test]
    fn test_event_payload_string_literal() {
        let content = load_fixture("rust/event_payloads.rs");
        let path = test_path("event_payloads.rs");

        let result = tree_parser::parse(&path, &content).unwrap();
        let emit = result
            .findings
            .iter()
            .find(|f| f.key == "string-event" && f.behavior == Behavior::Emit)
            .expect("Expected string-event emit");

        assert_eq!(
            emit.return_type.as_deref(),
            Some("String"),
            "String literal payload should infer to String"
        );
    }

    #[test]
    fn test_event_payload_variable_typed() {
        let content = load_fixture("rust/event_payloads.rs");
        let path = test_path("event_payloads.rs");

        let result = tree_parser::parse(&path, &content).unwrap();
        let emit = result
            .findings
            .iter()
            .find(|f| f.key == "typed-var-event" && f.behavior == Behavior::Emit)
            .expect("Expected typed-var-event emit");

        assert_eq!(
            emit.return_type.as_deref(),
            Some("CalculationStatus"),
            "Typed variable payload should resolve to CalculationStatus"
        );
    }

    #[test]
    fn test_event_payload_variable_inferred() {
        let content = load_fixture("rust/event_payloads.rs");
        let path = test_path("event_payloads.rs");

        let result = tree_parser::parse(&path, &content).unwrap();
        let emit = result
            .findings
            .iter()
            .find(|f| f.key == "inferred-var-event" && f.behavior == Behavior::Emit)
            .expect("Expected inferred-var-event emit");

        assert_eq!(
            emit.return_type.as_deref(),
            Some("CalculationStatus"),
            "Inferred variable payload should resolve to CalculationStatus"
        );
    }

    #[test]
    fn test_parse_rust_command_with_intermediate_attrs() {
        let content = load_fixture("rust/specta_commands.rs");
        let path = test_path("specta_commands.rs");

        let result = tree_parser::parse(&path, &content);
        assert!(
            result.is_ok(),
            "Failed to parse Rust file with intermediate attributes: {:?}",
            result.err()
        );

        let file_index = result.unwrap();
        assert_eq!(
            file_index.findings.len(),
            3,
            "Expected 3 commands (with and without intermediate attributes)"
        );

        let command_names: Vec<&str> = file_index.findings.iter().map(|f| f.key.as_str()).collect();

        // Should detect all commands despite intermediate #[cfg_attr] and #[allow] attributes
        assert!(
            command_names.contains(&"get_user_profile"),
            "Should detect command with one intermediate attribute"
        );
        assert!(
            command_names.contains(&"save_preferences"),
            "Should detect command with two intermediate attributes"
        );
        assert!(
            command_names.contains(&"delete_item"),
            "Should detect regular command without intermediate attributes"
        );
    }
}

#[cfg(test)]
mod typescript_parser_tests {
    use super::*;
    use lsp_server::syntax::{Behavior, EntityType};
    use lsp_server::tree_parser;

    #[test]
    fn test_parse_ts_invoke() {
        let content = load_fixture("typescript/invoke.ts");
        let path = test_path("invoke.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let invokes: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert!(!invokes.is_empty(), "Expected at least one invoke call");

        let command_names: Vec<&str> = invokes.iter().map(|f| f.key.as_str()).collect();
        assert!(command_names.contains(&"greet"));
        assert!(command_names.contains(&"get_user"));
    }

    #[test]
    fn test_parse_ts_invoke_no_args() {
        let content = "
            import { invoke } from '@tauri-apps/api/core';
            async function test() {
                await invoke('short_command');
            }
        ";
        let path = test_path("short.ts");

        let result = tree_parser::parse(&path, content);
        assert!(result.is_ok());

        let file_index = result.unwrap();
        let invokes: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert_eq!(invokes.len(), 1, "Expected 1 invoke call");
        assert_eq!(invokes[0].key, "short_command");
    }

    #[test]
    fn test_parse_ts_generic_invoke() {
        let content = load_fixture("typescript/generic_calls.tsx");
        let path = test_path("generic_calls.tsx");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let invokes: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();

        assert!(!invokes.is_empty(), "Expected invoke calls with generics");
    }

    #[test]
    fn test_parse_ts_interface() {
        let content = "
            interface GreetType {
                message: string;
                count: number;
            }
        ";
        let path = test_path("interface.ts");

        let result = tree_parser::parse(&path, content);
        if let Err(e) = &result {
            panic!("Failed to parse TS interface: {:?}", e);
        }
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let interfaces: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Interface)
            .collect();

        assert_eq!(interfaces.len(), 1, "Expected 1 interface");
        let iface = interfaces[0];
        assert_eq!(iface.key, "GreetType");

        let fields = iface.fields.as_ref().expect("Expected fields");
        assert_eq!(fields.len(), 2);

        assert_eq!(fields[0].name, "message");
        assert_eq!(fields[0].type_name, "string");

        assert_eq!(fields[1].name, "count");
        assert_eq!(fields[1].type_name, "number");
    }

    #[test]
    fn test_parse_specta_direct_call() {
        let content = load_fixture("typescript/specta_user_code.ts");
        let path = test_path("specta_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(
            result.is_ok(),
            "Failed to parse Specta user code: {:?}",
            result.err()
        );

        let file_index = result.unwrap();

        // Should find Specta method calls converted to snake_case command names
        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert!(
            !command_calls.is_empty(),
            "Expected Specta command calls in user code"
        );

        let command_names: Vec<&str> = command_calls.iter().map(|f| f.key.as_str()).collect();

        // getUserProfile -> get_user_profile
        assert!(
            command_names.contains(&"get_user_profile"),
            "Should convert camelCase getUserProfile to snake_case get_user_profile"
        );

        // savePreferences -> save_preferences
        assert!(
            command_names.contains(&"save_preferences"),
            "Should convert camelCase savePreferences to snake_case save_preferences"
        );

        // deleteItem -> delete_item
        assert!(
            command_names.contains(&"delete_item"),
            "Should convert camelCase deleteItem to snake_case delete_item"
        );
    }

    #[test]
    fn test_parse_specta_namespaced_call() {
        // Content has namespaced pattern: Specta.commands.getUserProfile()
        let content = load_fixture("typescript/specta_user_code.ts");
        let path = test_path("specta_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        // Should recognize Specta.commands.methodName() pattern
        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert!(
            !command_calls.is_empty(),
            "Expected namespaced Specta calls to be recognized"
        );
    }

    #[test]
    fn test_parse_specta_alongside_invoke() {
        // Verify both invoke("cmd") and commands.method() work in the same file
        let content = load_fixture("typescript/specta_user_code.ts");
        let path = test_path("specta_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        let command_names: Vec<&str> = command_calls.iter().map(|f| f.key.as_str()).collect();

        // Should find both regular invoke calls
        assert!(
            command_names.contains(&"legacy_command"),
            "Should find regular invoke() calls"
        );

        // And Specta method calls
        assert!(
            command_names.contains(&"get_user_profile"),
            "Should find Specta method calls"
        );
    }

    #[test]
    fn test_parse_typegen_direct_call() {
        let content = load_fixture("typescript/typegen_user_code.ts");
        let path = test_path("typegen_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(
            result.is_ok(),
            "Failed to parse Typegen user code: {:?}",
            result.err()
        );

        let file_index = result.unwrap();

        // Should find Typegen method calls converted to snake_case command names
        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert!(
            !command_calls.is_empty(),
            "Expected Typegen command calls in user code"
        );

        let command_names: Vec<&str> = command_calls.iter().map(|f| f.key.as_str()).collect();

        // getUserProfile -> get_user_profile
        assert!(
            command_names.contains(&"get_user_profile"),
            "Should convert camelCase getUserProfile to snake_case get_user_profile"
        );

        // savePreferences -> save_preferences
        assert!(
            command_names.contains(&"save_preferences"),
            "Should convert camelCase savePreferences to snake_case save_preferences"
        );

        // deleteItem -> delete_item
        assert!(
            command_names.contains(&"delete_item"),
            "Should convert camelCase deleteItem to snake_case delete_item"
        );

        // fetchData -> fetch_data
        assert!(
            command_names.contains(&"fetch_data"),
            "Should convert camelCase fetchData to snake_case fetch_data"
        );
    }

    #[test]
    fn test_parse_typegen_namespaced_call() {
        // Content has namespaced pattern: Typegen.commands.getUserProfile()
        let content = load_fixture("typescript/typegen_user_code.ts");
        let path = test_path("typegen_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        // Should recognize Typegen.commands.methodName() pattern
        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert!(
            !command_calls.is_empty(),
            "Expected namespaced Typegen calls to be recognized"
        );
    }

    #[test]
    fn test_parse_typegen_alongside_invoke() {
        // Verify both invoke("cmd") and commands.method() work in the same file
        let content = load_fixture("typescript/typegen_user_code.ts");
        let path = test_path("typegen_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        let command_names: Vec<&str> = command_calls.iter().map(|f| f.key.as_str()).collect();

        // Should find both regular invoke calls
        assert!(
            command_names.contains(&"legacy_command"),
            "Should find regular invoke() calls"
        );

        // And Typegen method calls
        assert!(
            command_names.contains(&"get_user_profile"),
            "Should find Typegen method calls"
        );
    }

    #[test]
    fn test_parse_ts_rs_direct_call() {
        let content = load_fixture("typescript/ts_rs_user_code.ts");
        let path = test_path("ts_rs_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(
            result.is_ok(),
            "Failed to parse ts-rs user code: {:?}",
            result.err()
        );

        let file_index = result.unwrap();

        // Should find ts-rs method calls converted to snake_case command names
        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert!(
            !command_calls.is_empty(),
            "Expected ts-rs command calls in user code"
        );

        let command_names: Vec<&str> = command_calls.iter().map(|f| f.key.as_str()).collect();

        // getUserProfile -> get_user_profile
        assert!(
            command_names.contains(&"get_user_profile"),
            "Should convert camelCase getUserProfile to snake_case get_user_profile"
        );

        // savePreferences -> save_preferences
        assert!(
            command_names.contains(&"save_preferences"),
            "Should convert camelCase savePreferences to snake_case save_preferences"
        );

        // deleteItem -> delete_item
        assert!(
            command_names.contains(&"delete_item"),
            "Should convert camelCase deleteItem to snake_case delete_item"
        );

        // listUsers -> list_users
        assert!(
            command_names.contains(&"list_users"),
            "Should convert camelCase listUsers to snake_case list_users"
        );
    }

    #[test]
    fn test_parse_ts_rs_namespaced_call() {
        // Content has namespaced pattern: TsRs.commands.getUserProfile()
        let content = load_fixture("typescript/ts_rs_user_code.ts");
        let path = test_path("ts_rs_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        // Should recognize TsRs.commands.methodName() pattern
        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        assert!(
            !command_calls.is_empty(),
            "Expected namespaced ts-rs calls to be recognized"
        );
    }

    #[test]
    fn test_parse_ts_rs_alongside_invoke() {
        // Verify both invoke("cmd") and commands.method() work in the same file
        let content = load_fixture("typescript/ts_rs_user_code.ts");
        let path = test_path("ts_rs_user_code.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let command_calls: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command && f.behavior == Behavior::Call)
            .collect();

        let command_names: Vec<&str> = command_calls.iter().map(|f| f.key.as_str()).collect();

        // Should find both regular invoke calls
        assert!(
            command_names.contains(&"legacy_command"),
            "Should find regular invoke() calls"
        );

        // And ts-rs method calls
        assert!(
            command_names.contains(&"get_user_profile"),
            "Should find ts-rs method calls"
        );
    }
}

#[cfg(test)]
mod javascript_parser_tests {
    use super::*;
    use lsp_server::syntax::{Behavior, EntityType};
    use lsp_server::tree_parser;

    #[test]
    fn test_parse_js_invoke() {
        let content = load_fixture("javascript/invoke.js");
        let path = test_path("invoke.js");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let invokes: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();

        assert!(!invokes.is_empty(), "Expected invoke calls in JavaScript");
    }

    #[test]
    fn test_parse_js_invoke_no_args() {
        let content = "invoke('js_short_command');";
        let path = test_path("short.js");

        let result = tree_parser::parse(&path, content);
        assert!(result.is_ok());

        let file_index = result.unwrap();
        let invokes: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();

        assert_eq!(invokes.len(), 1, "Expected 1 invoke call in JS");
        assert_eq!(invokes[0].key, "js_short_command");
    }

    #[test]
    fn test_parse_jsx_emit() {
        let content = load_fixture("javascript/emit.jsx");
        let path = test_path("emit.jsx");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let emits: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.behavior == Behavior::Emit)
            .collect();

        assert!(!emits.is_empty(), "Expected emit calls in JSX");
    }
}

#[cfg(test)]
mod vue_parser_tests {
    use super::*;
    use lsp_server::syntax::EntityType;
    use lsp_server::tree_parser;

    #[test]
    fn test_parse_vue_single_script() {
        let content = load_fixture("vue/single_script.vue");
        let path = test_path("single_script.vue");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let invokes: Vec<_> = file_index
            .findings
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
        assert!(
            !file_index.findings.is_empty(),
            "Expected findings in Vue setup script"
        );
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
        assert!(
            !file_index.findings.is_empty(),
            "Expected findings in Svelte component"
        );
    }
}

#[cfg(test)]
mod angular_parser_tests {
    use super::*;
    use lsp_server::syntax::EntityType;
    use lsp_server::tree_parser;

    #[test]
    fn test_parse_angular_component() {
        let content = load_fixture("angular/component.component.ts");
        let path = test_path("component.component.ts");

        let result = tree_parser::parse(&path, &content);
        assert!(result.is_ok());

        let file_index = result.unwrap();

        let invokes: Vec<_> = file_index
            .findings
            .iter()
            .filter(|f| f.entity == EntityType::Command)
            .collect();

        assert!(!invokes.is_empty(), "Expected invoke in Angular component");
    }
}
