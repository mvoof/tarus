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
    // A lone definition has nothing to point at: its own location is never a target.
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

#[test]
fn code_lens_forwarded_emit_links_to_listener_file() {
    // The emit only exists because the helper's parameter was resolved from this
    // call site; the lens proves that finding reaches navigation like any other.
    helpers::check_code_lens(
        r#"
//- /sync.ts
import { emitTo } from "@tauri-apps/api/event";

const emitToOverlays = async (event: string, payload: unknown) => {
  await emitTo("overlay", event, payload);
};

export const emitUnitsChanged = (system: string) =>
  emitToOverlays("units-changed", system);
$0
//- /overlay.ts
import { listen } from "@tauri-apps/api/event";
listen<string>("units-changed", (e) => console.log(e.payload));
"#,
        expect![[r#"7:18 "Go to overlay.ts""#]],
    );
}

#[test]
fn code_lens_same_file_emit_and_listen() {
    // Both sides live in one file. The lens names the line rather than the file,
    // which would be tautological, and neither side points at itself.
    helpers::check_code_lens(
        r#"
//- /events.ts
import { emit, listen } from "@tauri-apps/api/event";

emit("units-changed");

listen("units-changed", () => {});
$0
"#,
        expect![[r#"
            2:6 "Go to line 5"
            4:8 "Go to line 3""#]],
    );
}

#[test]
fn code_lens_same_file_summarises_past_reference_limit() {
    // Four same-file references exceed the default limit of three, so the
    // per-line links collapse exactly as the cross-file ones do.
    helpers::check_code_lens(
        r#"
//- /events.ts
import { emit, listen } from "@tauri-apps/api/event";

emit("units-changed");
emit("units-changed");
emit("units-changed");
emit("units-changed");

listen("units-changed", () => {});
$0
"#,
        expect![[r#"
            2:6 "4 in this file"
            3:6 "4 in this file"
            4:6 "4 in this file"
            5:6 "4 in this file"
            7:8 "4 in this file""#]],
    );
}

#[test]
fn code_lens_same_file_lens_is_separate_from_cross_file() {
    // A cross-file target and a same-file one produce two distinct lenses.
    helpers::check_code_lens(
        r#"
//- /events.ts
import { emit, listen } from "@tauri-apps/api/event";

emit("units-changed");

listen("units-changed", () => {});
$0
//- /overlay.ts
import { listen } from "@tauri-apps/api/event";
listen("units-changed", () => {});
"#,
        expect![[r#"
            2:6 "Go to line 5"
            2:6 "Go to overlay.ts"
            4:8 "Go to line 3"
            4:8 "Go to overlay.ts""#]],
    );
}

#[test]
fn code_lens_same_file_setup_function_and_emitters() {
    // Shape of a real sync module: listeners registered in a setup function near
    // the top, emitters exported from the bottom of the same file.
    helpers::check_code_lens(
        r#"
//- /events.ts
import { emit, listen } from "@tauri-apps/api/event";

export const setupListeners = async () => {
  await listen<boolean>("interact-mode-changed", (e) => console.log(e.payload));
  await listen<number>("steering-lock-changed", (e) => console.log(e.payload));
};

export const emitInteractMode = (active: boolean) =>
  emit("interact-mode-changed", active);

export const resetInteractMode = () => emit("interact-mode-changed", false);

export const emitSteeringLock = (degrees: number) =>
  emit("steering-lock-changed", degrees);
$0
"#,
        expect![[r#"
            10:45 "Go to line 4"
            10:45 "Go to line 9"
            13:8 "Go to line 5"
            3:25 "Go to line 11"
            3:25 "Go to line 9"
            4:24 "Go to line 14"
            8:8 "Go to line 11"
            8:8 "Go to line 4""#]],
    );
}

#[test]
fn code_lens_frontend_summarises_past_reference_limit() {
    // Four listener files exceed the default limit of three, so the individual
    // "Go to <file>" links collapse into one summary.
    helpers::check_code_lens(
        r#"
//- /emitter.ts
import { emit } from "@tauri-apps/api/event";
emit("units-changed");
$0
//- /a.ts
import { listen } from "@tauri-apps/api/event";
listen("units-changed", () => {});

//- /b.ts
import { listen } from "@tauri-apps/api/event";
listen("units-changed", () => {});

//- /c.ts
import { listen } from "@tauri-apps/api/event";
listen("units-changed", () => {});

//- /d.ts
import { listen } from "@tauri-apps/api/event";
listen("units-changed", () => {});
"#,
        expect![[r#"1:6 "4 references""#]],
    );
}

#[test]
fn code_lens_frontend_lists_files_within_reference_limit() {
    helpers::check_code_lens(
        r#"
//- /emitter.ts
import { emit } from "@tauri-apps/api/event";
emit("units-changed");
$0
//- /a.ts
import { listen } from "@tauri-apps/api/event";
listen("units-changed", () => {});

//- /b.ts
import { listen } from "@tauri-apps/api/event";
listen("units-changed", () => {});
"#,
        expect![[r#"
            1:6 "Go to a.ts"
            1:6 "Go to b.ts""#]],
    );
}

#[test]
fn code_lens_limit_counts_links_across_categories() {
    // One Rust definition plus three frontend call sites is four links on a single
    // line. The limit is about how many links a lens shows, so it must count them
    // all, not restart per category.
    helpers::check_code_lens(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /caller.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");
$0
//- /a.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");

//- /b.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");

//- /c.ts
import { invoke } from "@tauri-apps/api/core";
invoke("greet");
"#,
        expect![[r#"
            1:8 "1 rust ref"
            1:8 "3 references""#]],
    );
}
