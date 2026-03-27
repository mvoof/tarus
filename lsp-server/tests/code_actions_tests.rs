//! Code Actions tests
//!
//! Tests quick fixes: return type insert/replace, event payload fix, stub generation.

mod helpers;

use expect_test::expect;

// ===========================================================================
// Return type — missing
// ===========================================================================

#[test]
fn code_action_return_type_missing() {
    helpers::check_code_actions(
        r#"
$SCHEMA greet(): string

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet");

//- /backend.rs
#[tauri::command]
fn greet() -> String { String::new() }
"#,
        expect![[r#"
            "Add return type 'string'" [quickfix]
              edit /frontend.ts 1:6 insert "<string>""#]],
    );
}

// ===========================================================================
// Return type — mismatch
// ===========================================================================

#[test]
fn code_action_return_type_mismatch() {
    helpers::check_code_actions(
        r#"
$SCHEMA greet(): string

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
const r = await invoke<number>("$0greet");

//- /backend.rs
#[tauri::command]
fn greet() -> String { String::new() }
"#,
        expect![[r#"
            "Fix return type to 'string'" [quickfix]
              edit /frontend.ts 1:22..1:30 replace "<string>""#]],
    );
}

// ===========================================================================
// Return type — void/any → no action
// ===========================================================================

#[test]
fn code_action_return_type_void_no_action() {
    helpers::check_code_actions(
        r#"
$SCHEMA greet(): void

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet");

//- /backend.rs
#[tauri::command]
fn greet() {}
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Event payload — missing
// ===========================================================================

#[test]
fn code_action_event_payload_missing() {
    helpers::check_code_actions(
        r#"
$EVENT_SCHEMA my-event(UserPayload)

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0my-event", (e) => console.log(e));

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("my-event", "data").unwrap();
}
"#,
        expect![[r#"
            "Add payload type 'UserPayload'" [quickfix]
              edit /frontend.ts 1:6 insert "<UserPayload>""#]],
    );
}

// ===========================================================================
// No bindings → no type actions
// ===========================================================================

#[test]
fn code_action_no_bindings_no_type_fix() {
    // Without bindings, return type code actions should not appear
    helpers::check_code_actions(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet");
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// No match → none
// ===========================================================================

#[test]
fn code_action_on_non_entity_returns_none() {
    helpers::check_code_actions(
        r#"
//- /frontend.ts
const x = 42;
$0
"#,
        expect!["(none)"],
    );
}
