//! CodeLens tests
//!
//! Tests inline navigation buttons shown on command/event definitions and calls.

mod helpers;

use expect_test::expect;

// ===========================================================================
// Rust file with commands
// ===========================================================================

#[test]
fn code_lens_rust_single_command() {
    helpers::check_code_lens(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}
$0
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");
"#,
        expect![[r#"1:3 "Go to frontend.ts""#]],
    );
}

#[test]
fn code_lens_rust_events() {
    helpers::check_code_lens(
        r#"
//- /backend.rs
use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("user-notification", "Hello").unwrap();
}
$0
//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("user-notification", (e) => console.log(e));
"#,
        expect![[r#"3:14 "Go to frontend.ts""#]],
    );
}

// ===========================================================================
// Frontend files
// ===========================================================================

#[test]
fn code_lens_ts_call_sites() {
    helpers::check_code_lens(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");
$0
"#,
        expect![[r#"1:8 "Go to backend.rs""#]],
    );
}

#[test]
fn code_lens_js_file() {
    helpers::check_code_lens(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /app.js
import { invoke } from "@tauri-apps/api/core";
invoke("greet", { name: "Bob" });
$0
"#,
        expect![[r#"1:8 "Go to backend.rs""#]],
    );
}

// ===========================================================================
// Empty / no targets
// ===========================================================================

#[test]
fn code_lens_empty_file_returns_none() {
    helpers::check_code_lens(
        r#"
//- /empty.rs
$0
"#,
        expect!["(none)"],
    );
}

#[test]
fn code_lens_no_cross_file_targets() {
    // Only Rust file, no frontend files → no lenses (targets must be in other files)
    helpers::check_code_lens(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}
$0
"#,
        expect!["(none)"],
    );
}
