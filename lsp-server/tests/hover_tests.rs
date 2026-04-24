//! Hover tests
//!
//! Tests tooltip content for commands and events (usage stats, return types, diagnostic tips).

mod helpers;

use expect_test::expect;

// ===========================================================================
// Command hover
// ===========================================================================

#[test]
fn hover_on_command_definition() {
    helpers::check_hover(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");
"#,
        expect![[r#"
            ### ⚙️ Command: `greet`

            **Definition:**
            - 🦀 `backend.rs:2`

            **References (2 total)**
            - 🦀 1 definition(s)
            - ⚡ 1 call(s)

            **Sample References:**
            - ⚡️ `[CALL] frontend.ts:2`"#]],
    );
}

#[test]
fn hover_on_command_call() {
    helpers::check_hover(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("gre$0et");
"#,
        expect![[r#"
            ### ⚙️ Command: `greet`

            **Definition:**
            - 🦀 `backend.rs:2`

            **References (2 total)**
            - 🦀 1 definition(s)
            - ⚡ 1 call(s)

            **Sample References:**
            - ⚡️ `[CALL] frontend.ts:2`"#]],
    );
}

// ===========================================================================
// Event hover
// ===========================================================================

#[test]
fn hover_on_event_emit() {
    helpers::check_hover(
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
            ### 📡 Event: `user-notification`

            **Definition:**
            - ⚡️ `frontend.ts:2`

            **References (2 total)**
            - 📤 1 emit(s)
            - 👂 1 listener(s)

            **Sample References:**
            - 🦀 `[EMIT] backend.rs:4`"#]],
    );
}

// ===========================================================================
// Diagnostic tips
// ===========================================================================

#[test]
fn hover_undefined_command_shows_warning() {
    helpers::check_hover(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("gre$0et");
"#,
        expect![[r#"
            ### ⚙️ Command: `greet`

            **References (1 total)**
            - ⚡ 1 call(s)

            **Sample References:**
            - ⚡️ `[CALL] frontend.ts:2`

            ⚠️ *No backend implementation found*"#]],
    );
}

#[test]
fn hover_unused_command_shows_tip() {
    helpers::check_hover(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et() {}
"#,
        expect![[r#"
            ### ⚙️ Command: `greet`

            **Definition:**
            - 🦀 `backend.rs:2`

            **References (1 total)**
            - 🦀 1 definition(s)

            💡 *Defined but never called in frontend*"#]],
    );
}

// ===========================================================================
// No match
// ===========================================================================

#[test]
fn hover_no_entity_returns_none() {
    helpers::check_hover(
        r#"
//- /backend.rs
// just a comment
$0
"#,
        expect!["(none)"],
    );
}
