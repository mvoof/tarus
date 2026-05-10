//! Smoke tests for the expect-test framework
//!
//! One test per capability to validate that parse_fixture + check_* work correctly.

mod helpers;

use expect_test::expect;

#[test]
fn smoke_parse_rust_command() {
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
fn smoke_parse_ts_invoke() {
    helpers::check_parse(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet", { name: "World" });
"#,
        expect![[r#"
            /frontend.ts:
              Command Call "greet" 1:8..1:13"#]],
    );
}

#[test]
fn smoke_definition_ts_to_rust() {
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
fn smoke_references() {
    helpers::check_references(
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
        expect![[r#"
            /backend.rs 1:3..1:8
            /frontend.ts 1:8..1:13"#]],
    );
}

#[test]
fn smoke_code_lens() {
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
fn smoke_hover() {
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

            **Returns:** `void`

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
fn smoke_completion() {
    helpers::check_completion(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0");
"#,
        expect!["greet"],
    );
}

#[test]
fn smoke_diagnostics_undefined() {
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0nonexistent");
"#,
        expect![[r#"WARNING 1:8..1:19 "Command 'nonexistent' is not defined in Rust backend""#]],
    );
}

#[test]
fn smoke_code_actions_no_definition() {
    helpers::check_code_actions(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0missing_cmd");
"#,
        expect!["(none)"],
    );
}

#[test]
fn smoke_document_symbols() {
    helpers::check_document_symbols(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}
$0
"#,
        expect![[r#"Function "greet (command)" 1:3..1:8"#]],
    );
}

#[test]
fn smoke_workspace_symbols() {
    helpers::check_workspace_symbols(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}
"#,
        "greet",
        expect![[r#"Function "greet (command)" /backend.rs"#]],
    );
}
