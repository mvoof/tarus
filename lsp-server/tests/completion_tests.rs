//! Completion tests
//!
//! Tests autocomplete inside invoke(")/emit(")/listen(" contexts.
//! NOTE: This is a NEW test file — the old completion_tests.rs only had UTF-16 utility tests.

mod helpers;

use expect_test::expect;

// ===========================================================================
// invoke() context
// ===========================================================================

#[test]
fn completion_inside_invoke_returns_commands() {
    helpers::check_completion(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

#[tauri::command]
fn get_user() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0");
"#,
        expect![[r#"
            get_user
            greet"#]],
    );
}

#[test]
fn completion_inside_invoke_with_generic() {
    helpers::check_completion(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /frontend.tsx
import { invoke } from "@tauri-apps/api/core";
const r = await invoke<string>("$0");
"#,
        expect!["greet"],
    );
}

// ===========================================================================
// emit/listen context
// ===========================================================================

#[test]
fn completion_inside_emit_returns_events() {
    helpers::check_completion(
        r#"
//- /backend.rs
use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("user-notification", "data").unwrap();
}

//- /frontend.ts
import { emit } from "@tauri-apps/api/event";
emit("$0");
"#,
        expect!["user-notification"],
    );
}

#[test]
fn completion_inside_listen_returns_events() {
    helpers::check_completion(
        r#"
//- /backend.rs
use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("status-changed", "data").unwrap();
}

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0", (e) => console.log(e));
"#,
        expect!["status-changed"],
    );
}

// ===========================================================================
// Dedup and negative cases
// ===========================================================================

#[test]
fn completion_no_duplicates() {
    // Same command called from two files — should appear once in completion
    helpers::check_completion(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /a.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");

//- /b.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0");
"#,
        expect!["greet"],
    );
}

#[test]
fn completion_outside_trigger_returns_none() {
    helpers::check_completion(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
const x = 42;$0
"#,
        expect!["(none)"],
    );
}

#[test]
fn completion_returns_both_commands_and_events() {
    helpers::check_completion(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("status-changed", "data").unwrap();
}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0");
"#,
        expect![[r#"
            greet
            status-changed"#]],
    );
}
