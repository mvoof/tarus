//! Go to Definition (F12) tests
//!
//! Tests navigation between Tauri command/event definitions and usage sites.

mod helpers;

use expect_test::expect;

// ===========================================================================
// Command: Call → Definition
// ===========================================================================

#[test]
fn definition_ts_call_to_rust() {
    helpers::check_definition(
        r#"
//- /backend.rs
#[tauri::command]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("gre$0et", { name: "World" });
"#,
        expect!["/backend.rs 1:3..1:8"],
    );
}

#[test]
fn definition_rust_definition_to_ts_calls() {
    helpers::check_definition(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et(name: String) -> String {
    format!("Hello, {}!", name)
}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet", { name: "Alice" });
"#,
        expect!["/frontend.ts 1:8..1:13"],
    );
}

#[test]
fn definition_js_call_to_rust() {
    helpers::check_definition(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /app.js
import { invoke } from "@tauri-apps/api/core";
invoke("gre$0et", { name: "Bob" });
"#,
        expect!["/backend.rs 1:3..1:8"],
    );
}

// ===========================================================================
// Event: Emit ↔ Listen
// ===========================================================================

#[test]
fn definition_event_emit_to_listen() {
    helpers::check_definition(
        r#"
//- /backend.rs
use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("user-notificatio$0n", "Hello").unwrap();
}

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("user-notification", (e) => console.log(e));
"#,
        expect!["/frontend.ts 1:8..1:25"],
    );
}

#[test]
fn definition_event_listen_to_emit() {
    helpers::check_definition(
        r#"
//- /backend.rs
use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("user-notification", "Hello").unwrap();
}

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("user-notificatio$0n", (e) => console.log(e));
"#,
        expect!["/backend.rs 3:14..3:31"],
    );
}

// ===========================================================================
// Multi-language
// ===========================================================================

#[test]
fn definition_angular_call_no_definition() {
    // Angular calls get_user, but only greet is defined
    helpers::check_definition(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /user.component.ts
import { Component } from '@angular/core';
import { invoke } from "@tauri-apps/api/core";

@Component({
  selector: 'app-user',
  template: '<button>Load</button>'
})
export class UserComponent {
  async loadUser() {
    const user = await invoke("get_use$0r", { id: 1 });
  }
}
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// No match
// ===========================================================================

#[test]
fn definition_no_match_returns_none() {
    helpers::check_definition(
        r#"
//- /backend.rs
// just a comment, no command
$0
"#,
        expect!["(none)"],
    );
}

#[test]
fn definition_unknown_file_returns_none() {
    // Cursor in a file that has no indexed content
    helpers::check_definition(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /empty.ts
$0
"#,
        expect!["(none)"],
    );
}
