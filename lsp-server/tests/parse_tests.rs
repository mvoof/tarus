//! Parser tests — validates tree-sitter parsing of all supported languages
//!
//! Each test uses check_parse() which parses inline fixture files and
//! compares Finding output via expect-test snapshots.

mod helpers;

use expect_test::expect;
use std::collections::HashMap;
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
fn parse_ts_no_tauri_import() {
    helpers::check_parse(
        r#"
//- /app.ts
function invoke(cmd: string, args?: any) {}
function emit(event: string, payload?: any) {}
function listen(event: string, handler: any) {}
function once(event: string, handler: any) {}
function emitTo(target: string, event: string, payload?: any) {}

async function test_local() {
    await invoke("greet", { name: "Alice" });
    emit("status-changed", { status: "active" });
    listen("user-notification", (e) => {});
    once("single-event", (e) => {});
    emitTo("window", "custom-event", { data: 123 });
}

import { invoke as i, emit as e, listen as l, once as o, emitTo as et } from "./unrelated-lib";

async function test_imported() {
    await i("greet", { name: "Alice" });
    e("status-changed", { status: "active" });
    l("user-notification", (e) => {});
    o("single-event", (e) => {});
    et("window", "custom-event", { data: 123 });
}
"#,
        expect![[r#""#]],
    );
}

#[test]
fn parse_ts_all_tauri_imports() {
    helpers::check_parse(
        r#"
//- /app.ts
import { invoke } from "@tauri-apps/api/core";
import { emit, listen, once, emitTo } from "@tauri-apps/api/event";

async function test() {
    await invoke("greet", { name: "Alice" });
    emit("status-changed", { status: "active" });
    listen("user-notification", (e) => {});
    once("single-event", (e) => {});
    emitTo("window", "custom-event", { data: 123 });
}
"#,
        expect![[r#"
            /app.ts:
              Command Call "greet" 4:18..4:23
              Event Emit "status-changed" 5:10..5:24
              Event Listen "user-notification" 6:12..6:29
              Event Listen "single-event" 7:10..7:22
              Event Emit "custom-event" 8:22..8:34"#]],
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
    let result =
        lsp_server::tree_parser::parse(&path, &content, &std::collections::HashMap::new()).unwrap();

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
    let result =
        lsp_server::tree_parser::parse(&path, &content, &std::collections::HashMap::new()).unwrap();
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
    let result =
        lsp_server::tree_parser::parse(&path, &content, &std::collections::HashMap::new()).unwrap();
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

#[test]
fn parse_rust_event_constant() {
    helpers::check_parse(
        r#"
//- /events.rs
const MY_EVENT: &str = "my-event-name";
const OTHER_EVENT: &str = "other-event-name";

fn notify_user(app: &tauri::AppHandle) {
    app.emit(MY_EVENT, "Hello").unwrap();
    app.emit(OTHER_EVENT, "World").unwrap();
}
"#,
        expect![[r#"
            /events.rs:
              Event Emit "my-event-name" 4:13..4:21
              Event Emit "other-event-name" 5:13..5:24"#]],
    );
}

#[test]
fn parse_ts_event_constant() {
    helpers::check_parse(
        r#"
//- /app.ts
import { emit, listen } from "@tauri-apps/api/event";

const MY_EVENT = "my-event-name";
const OTHER_EVENT = "other-event-name";

emit(MY_EVENT, { data: 123 });
listen(OTHER_EVENT, (e) => {});
"#,
        expect![[r#"
            /app.ts:
              Event Emit "my-event-name" 5:5..5:13
              Event Listen "other-event-name" 6:7..6:18"#]],
    );
}

#[test]
fn parse_ts_invoke_constant_ignored() {
    helpers::check_parse(
        r#"
//- /app.ts
import { invoke } from "@tauri-apps/api/core";

const MY_CMD = "greet";

invoke(MY_CMD);
"#,
        expect![[r#""#]],
    );
}

// Cross-file constant resolution (simulates the two-pass indexing done in the LSP server)
#[test]
fn parse_rust_event_constant_cross_file() {
    let lib_path = std::path::PathBuf::from("/src/lib.rs");

    let emitter_content = r#"
pub const EVENT_STATUS: &str = "sim://status";
pub const EVENT_DISCONNECTED: &str = "sim://disconnected";
"#;

    let lib_content = r#"
use tauri::{AppHandle, Manager};

fn on_connected(app: &AppHandle) {
    app.emit(EVENT_STATUS, "connected").unwrap();
    app.emit(EVENT_DISCONNECTED, ()).unwrap();
}
"#;

    // Pass 1: collect constants from the definitions file
    let rust_constants = lsp_server::utils::extract_rust_constants_from_content(emitter_content);

    // Pass 2: parse the consumer file with global constants available
    let result = lsp_server::tree_parser::parse_rust_full(lib_content, &lib_path, &rust_constants)
        .expect("parse should succeed");

    let mut findings = result.file_index.findings;
    findings.sort_by_key(|f| f.range.start.line);

    let names: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(names, vec!["sim://status", "sim://disconnected"]);
}

#[test]
fn parse_rust_once_constant_cross_file() {
    let lib_path = std::path::PathBuf::from("/src/lib.rs");

    let constants_content = r#"pub const ONE_TIME_EVENT: &str = "one-time-event";"#;

    let lib_content = r#"
use constants::{ONE_TIME_EVENT};
const SECRET_EVENT: &str = "secret-event";

pub fn run() {
    app.listen("frontend-event", |event: Event| {
        println!("{:?}", event.payload());
    });

    app.once(ONE_TIME_EVENT, |event: Event| {
        println!("{:?}", event.payload());
    });
}
"#;

    let rust_constants = lsp_server::utils::extract_rust_constants_from_content(constants_content);

    let result = lsp_server::tree_parser::parse_rust_full(lib_content, &lib_path, &rust_constants)
        .expect("parse should succeed");

    let mut findings = result.file_index.findings;
    findings.sort_by_key(|f| f.range.start.line);

    let names: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert!(
        names.contains(&"one-time-event"),
        "expected 'one-time-event' in findings, got: {names:?}"
    );
}

#[test]
fn parse_ts_event_constant_cross_file() {
    let component_path = std::path::PathBuf::from("/components/telemetry.ts");

    let constants_content = r#"
export const SIM_STATUS = 'sim://status';
export const SIM_DISCONNECTED = 'sim://disconnected';
"#;

    let component_content = r#"
import { listen } from "@tauri-apps/api/event";

listen(SIM_STATUS, (event) => {});
listen(SIM_DISCONNECTED, (event) => {});
"#;

    // Pass 1: collect constants from the constants file
    let js_constants =
        lsp_server::utils::extract_js_constants_from_content(constants_content, false);

    // Pass 2: parse the consumer file with global constants
    let result = lsp_server::tree_parser::parse(&component_path, component_content, &js_constants)
        .expect("parse should succeed");

    let mut findings = result.findings;
    findings.sort_by_key(|f| f.range.start.line);

    let names: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(names, vec!["sim://status", "sim://disconnected"]);
}

#[test]
fn parse_rust_event_constant_multiline_struct_payload() {
    // Reproduces the exact Marble-Trace pattern:
    //   app.emit(EVENT_STATUS, &SimStatus { status: "connected".into(), sim: Some(x) })
    let lib_path = std::path::PathBuf::from("/src/bridge.rs");

    let emitter_content = r#"
pub const EVENT_STATUS: &str = "sim://status";
pub const EVENT_DISCONNECTED: &str = "sim://disconnected";
"#;

    let lib_content = r#"
use tauri::{AppHandle, Manager};

fn on_connected(app: &AppHandle, source: &Source) {
    app.emit(
        EVENT_STATUS,
        &SimStatus {
            status: "connected".into(),
            sim: Some(source.sim_type()),
        },
    ).unwrap();
    app.emit(EVENT_DISCONNECTED, ()).unwrap();
}
"#;

    let rust_constants = lsp_server::utils::extract_rust_constants_from_content(emitter_content);
    let result = lsp_server::tree_parser::parse_rust_full(lib_content, &lib_path, &rust_constants)
        .expect("parse should succeed");

    let mut findings = result.file_index.findings;
    findings.sort_by_key(|f| f.range.start.line);
    let keys: Vec<&str> = findings.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, vec!["sim://status", "sim://disconnected"]);
}

#[test]
fn parse_rust_demo_specta_librs() {
    use std::collections::HashMap;
    let lib_path = std::path::PathBuf::from("/demo-specta/src-tauri/src/lib.rs");

    let constants_rs = r#"pub const ONE_TIME_EVENT: &str = "one-time-event";"#;

    let lib_content = r#"
use constants::{ONE_TIME_EVENT};

const SECRET_EVENT: &str = "secret-event";

#[tauri::command]
#[specta::specta]
fn trigger_event(app: AppHandle) {
    app.emit_to(EventTarget::app(), SECRET_EVENT, EventPayload { message: "Hello!".into() })
        .unwrap();
}

pub fn run() {
    tauri::Builder::default()
        .setup(move |app| {
            app.listen("frontend-event", |event:Event| {
                println!("{:?}", event.payload());
            });

            app.once(ONE_TIME_EVENT, |event:Event| {
                println!("{:?}", event.payload());
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error");
}
"#;

    let rust_constants = lsp_server::utils::extract_rust_constants_from_content(constants_rs);
    eprintln!("rust_constants: {rust_constants:?}");

    let result = lsp_server::tree_parser::parse_rust_full(lib_content, &lib_path, &rust_constants)
        .expect("parse should succeed");

    let findings = &result.file_index.findings;
    eprintln!("findings ({}):", findings.len());
    for f in findings {
        eprintln!(
            "  key={} behavior={:?} range={:?}",
            f.key, f.behavior, f.range
        );
    }

    assert!(
        findings.iter().any(|f| f.key == "one-time-event"),
        "expected 'one-time-event' in findings"
    );
}

#[test]
fn parse_rust_exact_demo_specta_librs() {
    let lib_path = std::path::PathBuf::from("/src-tauri/src/lib.rs");
    let constants_content = r#"pub const ONE_TIME_EVENT: &str = "one-time-event";"#;
    let lib_content = include_str!(
        "/home/voof/projects/learning/tauri-tutorials/demo-specta/src-tauri/src/lib.rs"
    );

    let rust_constants = lsp_server::utils::extract_rust_constants_from_content(constants_content);
    eprintln!("rust_constants: {rust_constants:?}");

    let result = lsp_server::tree_parser::parse_rust_full(lib_content, &lib_path, &rust_constants)
        .expect("parse should succeed");

    let findings = &result.file_index.findings;
    eprintln!("findings ({}):", findings.len());
    for f in findings {
        eprintln!(
            "  key={} behavior={:?} line={}",
            f.key, f.behavior, f.range.start.line
        );
    }

    assert!(
        findings.iter().any(|f| f.key == "one-time-event"),
        "expected 'one-time-event' in findings, got: {:?}",
        findings.iter().map(|f| &f.key).collect::<Vec<_>>()
    );
}
