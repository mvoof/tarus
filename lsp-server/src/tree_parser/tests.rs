//! Tests for tree_parser module

use super::*;

#[test]
fn test_rust_command_parsing() {
    let content = r#"
#[tauri::command]
fn save_file(path: String) -> Result<(), String> {
    Ok(())
}

#[command]
fn load_file() -> String {
    String::new()
}
"#;

    let findings = parse_rust(content);
    assert!(!findings.is_empty(), "Expected findings but got none");

    let cmd_names: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert!(
        cmd_names.contains(&"save_file"),
        "Expected save_file in {:?}",
        cmd_names
    );
    assert!(
        cmd_names.contains(&"load_file"),
        "Expected load_file in {:?}",
        cmd_names
    );
}

#[test]
fn test_typescript_parsing() {
    let content = r#"
import { invoke, emit, listen } from '@tauri-apps/api';

async function test() {
    await invoke("save_file");
    emit("file_saved");
    listen("progress", (event) => {});
}
"#;

    let findings = parse_frontend(content, LangType::TypeScript, 0);
    assert!(!findings.is_empty());
}

#[test]
fn test_vue_script_extraction() {
    let content = r#"
<template>
  <div>Hello</div>
</template>

<script lang="ts">
import { invoke } from '@tauri-apps/api';

export default {
    async mounted() {
        await invoke("get_data");
    }
}
</script>
"#;

    let result = extract_vue_script(content);
    assert!(result.is_some());

    let (script, offset) = result.unwrap();
    assert!(script.contains("invoke"));
    assert!(offset > 0);
}

#[test]
fn test_rust_event_parsing() {
    let content = r#"
fn main() {
    app.emit("file_saved", payload);
    app.listen("progress", |event| {});
}
"#;

    let findings = parse_rust(content);
    assert!(!findings.is_empty(), "Expected findings but got none");

    let event_names: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert!(
        event_names.contains(&"file_saved"),
        "Expected file_saved in {:?}",
        event_names
    );
    assert!(
        event_names.contains(&"progress"),
        "Expected progress in {:?}",
        event_names
    );
}

#[test]
fn test_svelte_script_extraction() {
    let content = r#"
<script lang="ts">
import { invoke } from '@tauri-apps/api';

async function load() {
    await invoke("load_data");
}
</script>

<main>
  <h1>Hello</h1>
</main>
"#;

    let result = extract_svelte_script(content);
    assert!(result.is_some());

    let (script, _offset) = result.unwrap();
    assert!(script.contains("invoke"));
}

#[test]
fn test_import_alias() {
    let content = r#"
import { invoke as my_invoke, emit as sendEvent } from '@tauri-apps/api/core';
import { listen as onEvent } from '@tauri-apps/api/event';

async function test() {
    await my_invoke("aliased_command");
    sendEvent("aliased_event");
    onEvent("another_event", (e) => {});
}
"#;

    let findings = parse_frontend(content, LangType::TypeScript, 0);
    assert!(!findings.is_empty(), "Expected findings but got none");

    let keys: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert!(
        keys.contains(&"aliased_command"),
        "Expected aliased_command in {:?}",
        keys
    );
    assert!(
        keys.contains(&"aliased_event"),
        "Expected aliased_event in {:?}",
        keys
    );
    assert!(
        keys.contains(&"another_event"),
        "Expected another_event in {:?}",
        keys
    );
}

#[test]
fn test_generic_type_calls() {
    let content = r#"
import { invoke, emit } from '@tauri-apps/api';

async function test() {
    await invoke<number>("generic_command", { value: 1 });
    await invoke<Session>("session_command");
    emit<void>("generic_event");
}
"#;

    let findings = parse_frontend(content, LangType::TypeScript, 0);
    assert!(!findings.is_empty(), "Expected findings but got none");

    let keys: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert!(
        keys.contains(&"generic_command"),
        "Expected generic_command in {:?}",
        keys
    );
    assert!(
        keys.contains(&"session_command"),
        "Expected session_command in {:?}",
        keys
    );
    assert!(
        keys.contains(&"generic_event"),
        "Expected generic_event in {:?}",
        keys
    );
}

#[test]
fn test_emit_to_second_arg() {
    let content = r#"
import { emitTo } from '@tauri-apps/api/event';

async function test() {
    emitTo("window-1", "target_event", { data: 123 });
}
"#;

    let findings = parse_frontend(content, LangType::TypeScript, 0);
    assert!(!findings.is_empty(), "Expected findings but got none");

    let keys: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert!(
        keys.contains(&"target_event"),
        "Expected target_event in {:?}",
        keys
    );
}

#[test]
fn test_no_duplicate_findings_for_generic_calls() {
    let content = r#"
import { invoke } from '@tauri-apps/api';

async function test() {
    await invoke<number>("test_cmd", { value: 1 });
}
"#;

    let findings = parse_frontend(content, LangType::TypeScript, 0);

    // Should find exactly one instance of "test_cmd"
    let test_cmd_count = findings.iter().filter(|f| f.key == "test_cmd").count();
    assert_eq!(
        test_cmd_count,
        1,
        "Expected exactly 1 finding for 'test_cmd', got {}. Findings: {:?}",
        test_cmd_count,
        findings.iter().map(|f| &f.key).collect::<Vec<_>>()
    );
}
