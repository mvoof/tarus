//! Find References (Shift+F12) tests
//!
//! Tests finding all references for commands and events across languages.

mod helpers;

use expect_test::expect;

// ===========================================================================
// Command references
// ===========================================================================

#[test]
fn references_from_rust_definition() {
    helpers::check_references(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et(name: String) -> String {
    format!("Hello, {}!", name)
}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet", { name: "Alice" });

//- /app.js
import { invoke } from "@tauri-apps/api/core";
invoke("greet", { name: "Bob" });
"#,
        expect![[r#"
            /app.js 1:8..1:13
            /backend.rs 1:3..1:8
            /frontend.ts 1:8..1:13"#]],
    );
}

#[test]
fn references_from_ts_call_site() {
    helpers::check_references(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("gre$0et");
"#,
        expect![[r#"
            /backend.rs 1:3..1:8
            /frontend.ts 1:8..1:13"#]],
    );
}

// ===========================================================================
// Event references
// ===========================================================================

#[test]
fn references_event_emit_and_listen() {
    helpers::check_references(
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
        expect![[r#"
            /backend.rs 3:14..3:31
            /frontend.ts 1:8..1:25"#]],
    );
}

// ===========================================================================
// No match
// ===========================================================================

#[test]
fn references_unknown_position_returns_none() {
    helpers::check_references(
        r#"
//- /backend.rs
// comment $0
"#,
        expect!["(none)"],
    );
}

#[test]
fn references_unknown_file_returns_none() {
    helpers::check_references(
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
