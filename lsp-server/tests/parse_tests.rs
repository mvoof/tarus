//! Parser tests — validates tree-sitter parsing of all supported languages
//!
//! Each test uses check_parse() which parses inline fixture files and
//! compares Finding output via expect-test snapshots.

mod helpers;

use expect_test::expect;
use std::path::Path;

// ===========================================================================
// Rust
// ===========================================================================

#[test]
fn parse_rust_single_command() {
    helpers::check_parse(
        r#"
//- /backend.rs
#[tauri::command]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}
"#,
        expect![[r#"
            /backend.rs:
              Command Definition "greet" 1:3..1:8"#]],
    );
}

#[test]
fn parse_rust_multiple_commands() {
    helpers::check_parse(
        r#"
//- /commands.rs
use tauri::AppHandle;

#[tauri::command]
fn get_user(id: u32) -> Result<String, String> {
    Ok(format!("User {}", id))
}

#[tauri::command]
fn save_data(data: String) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn process_item(item: String) -> String {
    item.to_uppercase()
}
"#,
        expect![[r#"
            /commands.rs:
              Command Definition "get_user" 3:3..3:11
              Command Definition "save_data" 8:3..8:12
              Command Definition "process_item" 13:3..13:15"#]],
    );
}

#[test]
fn parse_rust_multi_attr_commands() {
    helpers::check_parse(
        r#"
//- /multi.rs
#[tauri::command]
#[allow(dead_code)]
fn single_extra_attr() -> String {
    "hello".to_string()
}

#[tauri::command]
#[allow(dead_code)]
#[allow(unused_variables)]
pub fn multiple_extra_attrs(x: i32) -> i32 {
    x
}

#[tauri::command]
fn simple_command() -> String {
    "simple".to_string()
}
"#,
        expect![[r#"
            /multi.rs:
              Command Definition "single_extra_attr" 2:3..2:20
              Command Definition "multiple_extra_attrs" 9:7..9:27
              Command Definition "simple_command" 14:3..14:17"#]],
    );
}

#[test]
fn parse_rust_events() {
    helpers::check_parse(
        r#"
//- /events.rs
use tauri::{AppHandle, Manager};

fn notify_user(app: &AppHandle) {
    app.emit("user-notification", "Hello").unwrap();
}

fn handle_event(app: &AppHandle) {
    app.listen("button-clicked", |event| {
        println!("Button clicked!");
    });

    app.emit("status-update", "Ready").unwrap();
}
"#,
        expect![[r#"
            /events.rs:
              Event Emit "user-notification" 3:14..3:31
              Event Listen "button-clicked" 7:16..7:30
              Event Emit "status-update" 11:14..11:27"#]],
    );
}

// ===========================================================================
// TypeScript
// ===========================================================================

#[test]
fn parse_ts_invoke() {
    helpers::check_parse(
        r#"
//- /app.ts
import { invoke } from "@tauri-apps/api/core";

async function greetUser() {
    const result = await invoke("greet", { name: "Alice" });
    console.log(result);
}

async function fetchData() {
    const data = await invoke("get_user", { id: 42 });
    return data;
}
"#,
        expect![[r#"
            /app.ts:
              Command Call "greet" 3:33..3:38
              Command Call "get_user" 8:31..8:39"#]],
    );
}

#[test]
fn parse_ts_generic_invoke() {
    helpers::check_parse(
        r#"
//- /generic.tsx
import { invoke } from "@tauri-apps/api/core";

interface User {
    id: number;
    name: string;
}

async function getUser(): Promise<User> {
    const user = await invoke<User>("get_user", { id: 1 });
    return user;
}

async function saveData<T>(data: T): Promise<void> {
    await invoke<void>("save_data", { data });
}
"#,
        expect![[r#"
            /generic.tsx:
              Command Call "get_user" 8:37..8:45 return_type=User
              Command Call "save_data" 13:24..13:33 return_type=void"#]],
    );
}

#[test]
fn parse_ts_emit_listen() {
    helpers::check_parse(
        r#"
//- /events.ts
import { emit, listen } from "@tauri-apps/api/event";

function notifyStatusChange() {
    emit("status-changed", { status: "active" });
}

function setupListener() {
    listen("user-notification", (event) => {
        console.log("Received:", event.payload);
    });
}
"#,
        expect![[r#"
            /events.ts:
              Event Emit "status-changed" 3:10..3:24
              Event Listen "user-notification" 7:12..7:29"#]],
    );
}

#[test]
fn parse_ts_specta_calls() {
    helpers::check_parse(
        r#"
//- /specta.ts
import { commands } from './bindings';

const user = await commands.getUserProfile(42);
const u2 = await commands.createUser("Bob", 25, "extra");
await commands.ping();

import { invoke } from '@tauri-apps/api';
const result = await invoke('get_user', { id: 1 });
"#,
        expect![[r#"
            /specta.ts:
              Command SpectaCall "get_user_profile" 2:28..2:42 args=1
              Command SpectaCall "create_user" 3:26..3:36 args=3
              Command SpectaCall "ping" 4:15..4:19 args=0
              Command Call "get_user" 7:29..7:37"#]],
    );
}

#[test]
fn parse_ts_specta_events() {
    helpers::check_parse(
        r#"
//- /specta_events.ts
import { events } from '../bindings';

events.globalEvent.listen((e) => console.log(e));
events.globalEvent.emit({ message: "hello" });
events.globalEvent.once((e) => console.log(e));

events.myCustomEvent(appWindow).listen((e) => console.log(e));
events.myCustomEvent(appWindow).emit({ data: 42 });

events.userProfileUpdated.listen((e) => console.log(e));
"#,
        expect![[r#"
            /specta_events.ts:
              Event Listen "global-event" 2:7..2:18
              Event Emit "global-event" 3:7..3:18
              Event Listen "global-event" 4:7..4:18
              Event Listen "my-custom-event" 6:7..6:20
              Event Emit "my-custom-event" 7:7..7:20
              Event Listen "user-profile-updated" 9:7..9:25"#]],
    );
}

// ===========================================================================
// JavaScript
// ===========================================================================

#[test]
fn parse_js_invoke() {
    helpers::check_parse(
        r#"
//- /app.js
import { invoke } from "@tauri-apps/api/core";

function greetUser() {
    invoke("greet", { name: "Bob" }).then(result => {
        console.log(result);
    });
}

function processData() {
    invoke("process_item", { item: "test" });
}
"#,
        expect![[r#"
            /app.js:
              Command Call "greet" 3:12..3:17
              Command Call "process_item" 9:12..9:24"#]],
    );
}

#[test]
fn parse_jsx_emit() {
    helpers::check_parse(
        r#"
//- /component.jsx
import { emit } from "@tauri-apps/api/event";

export function MyComponent() {
    const handleClick = () => {
        emit("button-clicked", { timestamp: Date.now() });
    };

    return <button onClick={handleClick}>Click me</button>;
}
"#,
        expect![[r#"
            /component.jsx:
              Event Emit "button-clicked" 4:14..4:28"#]],
    );
}

#[test]
fn parse_js_specta_events() {
    helpers::check_parse(
        r#"
//- /specta_events.js
const { events } = require('../bindings');

events.globalEvent.listen((e) => console.log(e));
events.globalEvent.emit({ message: "hello" });
events.myCustomEvent(appWindow).listen((e) => console.log(e));
"#,
        expect![[r#"
            /specta_events.js:
              Event Listen "global-event" 2:7..2:18
              Event Emit "global-event" 3:7..3:18
              Event Listen "my-custom-event" 4:7..4:20"#]],
    );
}

// ===========================================================================
// Vue (uses fixture files — SFC needs <script> tags)
// ===========================================================================

#[test]
fn parse_vue_single_script() {
    let content = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vue/single_script.vue"),
    )
    .unwrap();
    let path = std::path::PathBuf::from("/test/component.vue");
    let result = lsp_server::tree_parser::parse(&path, &content).unwrap();

    let mut out = String::new();
    let mut findings = result.findings;
    findings.sort_by_key(|f| (f.range.start.line, f.range.start.character));
    for f in &findings {
        use std::fmt::Write;
        writeln!(
            out,
            "{} {} \"{}\" {}:{}..{}:{}",
            match f.entity {
                lsp_server::syntax::EntityType::Command => "Command",
                lsp_server::syntax::EntityType::Event => "Event",
            },
            match f.behavior {
                lsp_server::syntax::Behavior::Definition => "Definition",
                lsp_server::syntax::Behavior::Call => "Call",
                lsp_server::syntax::Behavior::SpectaCall => "SpectaCall",
                lsp_server::syntax::Behavior::Emit => "Emit",
                lsp_server::syntax::Behavior::Listen => "Listen",
            },
            f.key,
            f.range.start.line,
            f.range.start.character,
            f.range.end.line,
            f.range.end.character,
        )
        .unwrap();
    }

    let expect = expect![[r#"Command Call "greet" 12:35..12:40"#]];
    expect.assert_eq(out.trim_end());
}

#[test]
fn parse_vue_multiple_scripts() {
    let content = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vue/multiple_scripts.vue"),
    )
    .unwrap();
    let path = std::path::PathBuf::from("/test/multi.vue");
    let result = lsp_server::tree_parser::parse(&path, &content).unwrap();
    assert!(
        !result.findings.is_empty(),
        "Expected findings in Vue multi-script"
    );
}

// ===========================================================================
// Svelte (fixture file — SFC)
// ===========================================================================

#[test]
fn parse_svelte_component() {
    let content = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/svelte/component.svelte"),
    )
    .unwrap();
    let path = std::path::PathBuf::from("/test/component.svelte");
    let result = lsp_server::tree_parser::parse(&path, &content).unwrap();
    assert!(
        !result.findings.is_empty(),
        "Expected findings in Svelte component"
    );
}

// ===========================================================================
// Angular
// ===========================================================================

#[test]
fn parse_angular_component() {
    helpers::check_parse(
        r#"
//- /user.component.ts
import { Component } from '@angular/core';
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";

@Component({
  selector: 'app-user',
  template: '<button (click)="loadUser()">Load</button>'
})
export class UserComponent {
  async loadUser() {
    const user = await invoke("get_user", { id: 1 });
    emit("user-loaded", user);
  }
}
"#,
        expect![[r#"
            /user.component.ts:
              Command Call "get_user" 10:31..10:39
              Event Emit "user-loaded" 11:10..11:21"#]],
    );
}
