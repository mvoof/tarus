//! Diagnostics tests
//!
//! Layer 1: Structural — undefined/unused commands and events
//! Layer 2: Type — param-key, return-type, event-payload checks (requires bindings)

mod helpers;

use expect_test::expect;

// ===========================================================================
// Layer 1: Structural diagnostics
// ===========================================================================

#[test]
fn diag_undefined_command() {
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
fn diag_unused_command() {
    helpers::check_diagnostics(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et() {}
"#,
        expect![[r#"WARNING 1:3..1:8 "Command 'greet' is defined but never invoked in frontend""#]],
    );
}

#[test]
fn diag_defined_and_called_no_warning() {
    helpers::check_diagnostics(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet");
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_event_emitted_but_no_listeners() {
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { emit } from "@tauri-apps/api/event";
emit("$0my-event");
"#,
        expect![[r#"WARNING 1:6..1:14 "Event 'my-event' is emitted but no listeners found""#]],
    );
}

#[test]
fn diag_event_listened_but_no_emitters() {
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0my-event", (e) => console.log(e));
"#,
        expect![[r#"WARNING 1:8..1:16 "Event 'my-event' is listened for but never emitted""#]],
    );
}

#[test]
fn diag_first_call_only_for_undefined() {
    // Only the first call should get the "undefined" warning, not subsequent ones
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0missing");
invoke("missing");
"#,
        expect![[r#"WARNING 1:8..1:15 "Command 'missing' is not defined in Rust backend""#]],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — return type
// ===========================================================================

#[test]
fn diag_return_type_missing() {
    helpers::check_diagnostics(
        r#"
$SCHEMA greet(): string

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet");

//- /backend.rs
#[tauri::command]
fn greet() -> String { String::new() }
"#,
        expect![[r#"HINT 1:8..1:13 "invoke('greet') is missing return type, expected 'string'" [tarus/return-type-missing]"#]],
    );
}

#[test]
fn diag_return_type_mismatch() {
    helpers::check_diagnostics(
        r#"
$SCHEMA greet(): string

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
const r = await invoke<number>("$0greet");

//- /backend.rs
#[tauri::command]
fn greet() -> String { String::new() }
"#,
        expect![[r#"WARNING 1:32..1:37 "invoke<number>('greet') return type mismatch: expected 'string'" [tarus/return-type-mismatch]"#]],
    );
}

#[test]
fn diag_return_type_void_skipped() {
    helpers::check_diagnostics(
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

#[test]
fn diag_return_type_any_skipped() {
    helpers::check_diagnostics(
        r#"
$SCHEMA greet(): string

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
const r = await invoke<any>("$0greet");

//- /backend.rs
#[tauri::command]
fn greet() -> String { String::new() }
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — param keys
// ===========================================================================

#[test]
fn diag_param_key_missing() {
    helpers::check_diagnostics(
        r#"
$SCHEMA greet(name: string, age: number): void

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet", { name: "Alice" });

//- /backend.rs
#[tauri::command]
fn greet() {}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_param_key_extra() {
    helpers::check_diagnostics(
        r#"
$SCHEMA greet(name: string): void

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet", { name: "Alice", extra: 42 });

//- /backend.rs
#[tauri::command]
fn greet() {}
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — event payload
// ===========================================================================

#[test]
fn diag_event_payload_missing() {
    helpers::check_diagnostics(
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
        expect![[r#"HINT 1:8..1:16 "listen('my-event') is missing payload type, expected 'UserPayload'" [tarus/event-payload-missing]"#]],
    );
}

#[test]
fn diag_event_payload_mismatch() {
    helpers::check_diagnostics(
        r#"
$EVENT_SCHEMA my-event(UserPayload)

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen<WrongType>("$0my-event", (e) => console.log(e));

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("my-event", "data").unwrap();
}
"#,
        expect![[r#"WARNING 1:19..1:27 "listen<WrongType>('my-event') payload type mismatch: expected 'UserPayload'" [tarus/event-payload-mismatch]"#]],
    );
}

#[test]
fn diag_event_payload_null_skipped() {
    helpers::check_diagnostics(
        r#"
$EVENT_SCHEMA my-event(null)

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0my-event", (e) => console.log(e));

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("my-event", "data").unwrap();
}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_event_payload_void_skipped() {
    helpers::check_diagnostics(
        r#"
$EVENT_SCHEMA my-event(void)

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0my-event", (e) => console.log(e));

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("my-event", "data").unwrap();
}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_event_payload_rust_file_skipped() {
    // Rust files don't use generic type params on emit/listen — no payload check
    helpers::check_diagnostics(
        r#"
$EVENT_SCHEMA my-event(UserPayload)

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("$0my-event", "data").unwrap();
}
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — codegen_origin skip
// ===========================================================================

#[test]
fn diag_specta_event_no_payload_check() {
    // Specta typed events (codegen_origin set) skip payload checking
    helpers::check_diagnostics(
        r#"
$EVENT_SCHEMA global-event(MyPayload)

//- /frontend.ts
import { events } from '../bindings';
events.globalEvent.listen$0((e) => console.log(e));

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("global-event", "data").unwrap();
}
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — no bindings guard
// ===========================================================================

#[test]
fn diag_no_type_diagnostic_without_bindings() {
    // Without bindings files, even wrong param keys should not trigger type diagnostics
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet", { wrong_key: 42 });

//- /backend.rs
#[tauri::command]
fn greet() {}
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — param keys (additional)
// ===========================================================================

#[test]
fn diag_param_keys_correct_no_warning() {
    helpers::check_diagnostics(
        r#"
$SCHEMA create_user(name: string, email: string): void

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0create_user", { name: "Alice", email: "a@b.c" });

//- /backend.rs
#[tauri::command]
fn create_user() {}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_param_keys_rust_source_skipped() {
    // RustSource schemas should not trigger param-key diagnostics
    helpers::check_diagnostics(
        r#"
$RUST_SCHEMA greet(name: string): string
$TYPE_ALIAS UserProfile = { id: number }

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("$0greet", { bad_key: 42 });

//- /backend.rs
#[tauri::command]
fn greet(name: String) -> String { name }
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — SpectaCall argument count
// ===========================================================================

#[test]
fn diag_specta_call_too_many_args() {
    helpers::check_diagnostics(
        r#"
$SCHEMA create_user(name: string, age: number): void

//- /frontend.ts
import { commands } from './bindings';
await commands.createUser$0("Bob", 25, "extra");

//- /backend.rs
#[tauri::command]
fn create_user() {}
"#,
        expect![[r#"WARNING 1:15..1:25 "commands.create_user() expected 2 arguments but got 3" [tarus/arg-count-mismatch]"#]],
    );
}

#[test]
fn diag_specta_call_too_few_args() {
    helpers::check_diagnostics(
        r#"
$SCHEMA create_user(name: string, age: number): void

//- /frontend.ts
import { commands } from './bindings';
await commands.createUser$0("Bob");

//- /backend.rs
#[tauri::command]
fn create_user() {}
"#,
        expect![[r#"WARNING 1:15..1:25 "commands.create_user() expected 2 arguments but got 1" [tarus/arg-count-mismatch]"#]],
    );
}

#[test]
fn diag_specta_call_correct_args_no_warning() {
    helpers::check_diagnostics(
        r#"
$SCHEMA get_user(id: number): void

//- /frontend.ts
import { commands } from './bindings';
await commands.getUser$0(42);

//- /backend.rs
#[tauri::command]
fn get_user() {}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_specta_call_zero_args_no_warning() {
    helpers::check_diagnostics(
        r#"
$SCHEMA ping(): void

//- /frontend.ts
import { commands } from './bindings';
await commands.ping$0();

//- /backend.rs
#[tauri::command]
fn ping() {}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_specta_call_rust_source_skipped() {
    // RustSource schemas should not trigger arg count diagnostics
    helpers::check_diagnostics(
        r#"
$RUST_SCHEMA greet(name: string): string
$TYPE_ALIAS SomeType = { x: number }

//- /frontend.ts
import { commands } from './bindings';
await commands.greet$0("a", "b", "c", "d", "e");

//- /backend.rs
#[tauri::command]
fn greet(name: String) -> String { name }
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — return type (additional)
// ===========================================================================

#[test]
fn diag_return_type_match_no_warning() {
    helpers::check_diagnostics(
        r#"
$SCHEMA get_user(): User

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
const u = await invoke<User>("$0get_user");

//- /backend.rs
#[tauri::command]
fn get_user() -> User { todo!() }
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_return_type_rust_source_skipped_without_alias() {
    // RustSource schema with return type NOT in type_aliases → skip
    helpers::check_diagnostics(
        r#"
$RUST_SCHEMA get_user(): User
$TYPE_ALIAS OtherType = { x: number }

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
const u = await invoke<string>("$0get_user");

//- /backend.rs
#[tauri::command]
fn get_user() -> User { todo!() }
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_return_type_rust_source_used_with_alias() {
    // RustSource schema with return type IN type_aliases → diagnose
    helpers::check_diagnostics(
        r#"
$RUST_SCHEMA get_user(): User
$TYPE_ALIAS User = { id: number }

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
const u = await invoke<string>("$0get_user");

//- /backend.rs
#[tauri::command]
fn get_user() -> User { todo!() }
"#,
        expect![[r#"WARNING 1:32..1:40 "invoke<string>('get_user') return type mismatch: expected 'User'" [tarus/return-type-mismatch]"#]],
    );
}

// ===========================================================================
// Layer 2: Type diagnostics — event payload (additional)
// ===========================================================================

#[test]
fn diag_event_no_type_diagnostic_without_bindings() {
    // Without bindings, no event payload diagnostics
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0my-event", (e) => console.log(e));

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("my-event", "data").unwrap();
}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_event_correct_payload_no_warning() {
    helpers::check_diagnostics(
        r#"
$EVENT_SCHEMA user-updated(UserProfile)

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen<UserProfile>("$0user-updated", (e) => console.log(e));

//- /backend.rs
use tauri::{AppHandle, Manager};

fn emit_event(app: &AppHandle) {
    app.emit("user-updated", "data").unwrap();
}
"#,
        expect!["(none)"],
    );
}
