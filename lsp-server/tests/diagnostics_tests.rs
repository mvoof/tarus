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
fn diag_no_false_positive_when_file_has_syntax_errors() {
    // A file broken mid-edit (here: an unterminated string in the listener) parses
    // into a tree with ERROR nodes. Indexing it would drop the half-typed `listen`
    // while still capturing the valid `emit` below, yielding a spurious
    // "emitted but no listeners" warning. The parser must reject such trees so the
    // pipeline keeps the last valid index and suppresses diagnostics for the file.
    helpers::check_diagnostics(
        r#"
//- /events.ts
import { emit, listen } from "@tauri-apps/api/event";
const setup = async () => {
  await listen("my-event, (e) => {});
};
const emitIt = () => {
  emit("$0my-event");
};
"#,
        expect!["(none)"],
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
        expect![[
            r#"HINT 1:8..1:13 "invoke('greet') is missing return type, expected 'string'" [tarus/return-type-missing]"#
        ]],
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
        expect![[
            r#"WARNING 1:32..1:37 "invoke<number>('greet') return type mismatch: expected 'string'" [tarus/return-type-mismatch]"#
        ]],
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
        expect![[
            r#"HINT 1:8..1:16 "listen('my-event') is missing payload type, expected 'UserPayload'" [tarus/event-payload-missing]"#
        ]],
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
        expect![[
            r#"WARNING 1:19..1:27 "listen<WrongType>('my-event') payload type mismatch: expected 'UserPayload'" [tarus/event-payload-mismatch]"#
        ]],
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
        expect![[
            r#"HINT 1:8..1:13 "invoke('greet') is missing return type, expected 'string'" [tarus/return-type-missing]"#
        ]],
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
        expect![[
            r#"WARNING 1:15..1:25 "commands.create_user() expected 2 arguments but got 3" [tarus/arg-count-mismatch]"#
        ]],
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
        expect![[
            r#"WARNING 1:15..1:25 "commands.create_user() expected 2 arguments but got 1" [tarus/arg-count-mismatch]"#
        ]],
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
        expect![[
            r#"WARNING 1:32..1:40 "invoke<string>('get_user') return type mismatch: expected 'User'" [tarus/return-type-mismatch]"#
        ]],
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

#[test]
fn diag_event_payload_vec_alias_passed_by_ref() {
    helpers::check_diagnostics(
        r#"
//- /backend.rs
fn run(app: AppHandle) {
    let forecast: Vec<WeatherForecastEntry> = vec![];
    app.emit("weather", &forecast);
}

//- /bindings.ts [specta]
export interface WeatherForecastEntry { time: number; }

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0weather", (e) => {});
"#,
        expect![[
            r#"HINT 1:8..1:15 "listen('weather') is missing payload type, expected 'WeatherForecastEntry[]'" [tarus/event-payload-missing]"#
        ]],
    );
}

// ===========================================================================
// Dynamic (unresolved) names — see `indexer::DynamicUsages`
// ===========================================================================

#[test]
fn diag_local_binding_does_not_resolve_to_foreign_constant() {
    // `event` is a parameter, so it must not pick up the same-named object key
    // declared in an unrelated file — that would index a "click" event nobody wrote.
    helpers::check_diagnostics(
        r#"
//- /widget.ts
const defaults = { event: "click" };

//- /sync.ts
import { emitTo } from "@tauri-apps/api/event";

const emitToOverlays = async (event: string, payload: unknown) => {
  await emitTo("overlay", ev$0ent, payload);
};
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_forwarded_emit_reaches_the_listener() {
    // The literal lives at the helper call site; resolving the forwarder links it
    // to the listener, so there is nothing to report and navigation works.
    helpers::check_diagnostics(
        r#"
//- /sync.ts
import { emitTo, listen } from "@tauri-apps/api/event";

const emitToOverlays = async (event: string, payload: unknown) => {
  await emitTo("overlay", event, payload);
};

export const emitUnits = (value: string) => emitToOverlays("units-changed", value);

listen("$0units-changed", () => {});
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_forwarder_called_with_a_variable_stays_unprovable() {
    // Here even the call site has no literal, so the emit is genuinely unknown and
    // "never emitted" cannot be proven.
    helpers::check_diagnostics(
        r#"
//- /sync.ts
import { emitTo, listen } from "@tauri-apps/api/event";

const emitToOverlays = async (event: string, payload: unknown) => {
  await emitTo("overlay", event, payload);
};

export const relay = (name: string, value: unknown) => emitToOverlays(name, value);

listen("$0units-changed", () => {});
"#,
        expect![[
            r#"HINT 6:70..6:74 "Tarus cannot read the event name `name` as a string: this emit is not linked to any listener, and "never emitted" warnings are off for the whole workspace" [tarus/unresolved-name]"#
        ]],
    );
}

#[test]
fn diag_forwarded_listen_reaches_the_emitter() {
    helpers::check_diagnostics(
        r#"
//- /sync.ts
import { emit, listen } from "@tauri-apps/api/event";

const subscribe = (event: string) => listen(event, () => {});

export const watchUnits = () => subscribe("units-changed");

emit("$0units-changed");
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_generic_forwarded_listen_reaches_the_emitter() {
    // Same helper as above, called with an explicit type argument. The type
    // argument changes nothing about which parameter carries the event name, so
    // the call site has to resolve exactly as the plain one does.
    helpers::check_diagnostics(
        r#"
//- /sync.ts
import { emit, listen } from "@tauri-apps/api/event";

const subscribe = <T>(event: string) => listen(event, (_e: T) => {});

export const watchUnits = () => subscribe<boolean>("units-changed");

emit("$0units-changed");
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_awaited_generic_forwarded_listen_reaches_the_emitter() {
    // `await helper<T>(...)` puts the await inside the callee, so the helper
    // name is not directly under `function:`. The helper is declared in another
    // module here because that is the shape this was reported against.
    helpers::check_diagnostics(
        r#"
//- /services/events.service.ts
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

export const listenTo = <PayloadType>(
  event: string,
  handler: EventCallback<PayloadType>
): Promise<UnlistenFn> => listen(event, handler);

//- /store/sync/events.ts
import { emit } from "@tauri-apps/api/event";
import { listenTo } from "../../services/events.service";

export const setupOverlayListeners = async () => {
  const unlistens: UnlistenFn[] = [];

  unlistens.push(
    await listenTo<boolean>("units-changed", () => {})
  );

  return unlistens;
};

export const emitUnitsChanged = (value: boolean) => emit("$0units-changed", value);
"#,
        expect!["(none)"],
    );
}

// The four ways a real app subscribes through a helper. Every one of them was
// reported as "emitted but no listeners found" while the helper call was both
// awaited and given a type argument.

#[test]
fn diag_awaited_forwarder_in_a_class_method() {
    // `this.unlistens.push(await listenTo<T>(CONST, cb))` inside a private
    // method — the shape a telemetry store uses.
    helpers::check_diagnostics(
        r#"
//- /src-tauri/src/telemetry/emitter.rs
pub const EVENT_TRACK_SHAPE: &str = "sim://track-shape";

pub fn emit_shape(app: &tauri::AppHandle, payload: &TrackShapePayload) {
    let _ = app.emit($0EVENT_TRACK_SHAPE, payload);
}

//- /src/services/events.service.ts
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

export const listenTo = <PayloadType>(
  event: string,
  handler: EventCallback<PayloadType>
): Promise<UnlistenFn> => listen(event, handler);
//- /src/store/sync/sim-events.ts
export const SIM_TRACK_SHAPE = "sim://track-shape";

//- /src/store/sim/sim.store.ts
import { listenTo, type UnlistenFn } from "../../services/events.service";
import { SIM_TRACK_SHAPE } from "../sync/sim-events";

export class SimStore {
  private unlistens: UnlistenFn[] = [];

  private async subscribeAllEvents() {
    this.unlistens.push(
      await listenTo<TrackShapePayload>(SIM_TRACK_SHAPE, (event) => {
        void event;
      })
    );
  }
}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_awaited_forwarder_assigned_to_a_const() {
    // `const handle = await listenTo<T>(CONST, cb)` — a chat store keeps the
    // unlisten handles in locals before pushing them together.
    helpers::check_diagnostics(
        r#"
//- /src-tauri/src/chat/twitch.rs
pub const EVENT_CHAT_MESSAGE: &str = "chat://message";

pub fn forward(app: &tauri::AppHandle, msg: &ChatMessage) {
    let _ = app.emit($0EVENT_CHAT_MESSAGE, msg);
}

//- /src/services/events.service.ts
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

export const listenTo = <PayloadType>(
  event: string,
  handler: EventCallback<PayloadType>
): Promise<UnlistenFn> => listen(event, handler);
//- /src/store/sync/sim-events.ts
export const CHAT_MESSAGE = "chat://message";

//- /src/store/data/chat.store.ts
import { listenTo, type UnlistenFn } from "../../services/events.service";
import { CHAT_MESSAGE } from "../sync/sim-events";

export class ChatStore {
  private unlisteners: UnlistenFn[] = [];

  async init() {
    const message = await listenTo<ChatMessage>(CHAT_MESSAGE, (event) => {
      void event;
    });

    this.unlisteners.push(message);
  }
}
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_awaited_forwarder_in_an_exported_async_arrow() {
    // `unlistens.push(await listenTo<T>(CONST, cb))` at module level.
    helpers::check_diagnostics(
        r#"
//- /src-tauri/src/input/runtime.rs
pub const INPUT_BUTTON_EVENT: &str = "input://button";

pub fn publish(app: &tauri::AppHandle, event: &InputButtonEvent) {
    let _ = app.emit($0INPUT_BUTTON_EVENT, event);
}

//- /src/services/events.service.ts
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

export const listenTo = <PayloadType>(
  event: string,
  handler: EventCallback<PayloadType>
): Promise<UnlistenFn> => listen(event, handler);
//- /src/store/sync/sim-events.ts
export const INPUT_BUTTON_EVENT = "input://button";

//- /src/store/hotkeys/bindings-sync.ts
import { listenTo, type UnlistenFn } from "../../services/events.service";
import { INPUT_BUTTON_EVENT } from "../sync/sim-events";

export const setupDeviceBindings = async (): Promise<UnlistenFn[]> => {
  const unlistens: UnlistenFn[] = [];

  unlistens.push(
    await listenTo<InputButtonEvent>(INPUT_BUTTON_EVENT, (event) => {
      void event;
    })
  );

  return unlistens;
};
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_awaited_forwarder_paired_with_a_frontend_emitter() {
    // Both sides in TypeScript: a named emit helper in the service, the
    // subscriber awaiting the forwarder with a literal name.
    helpers::check_diagnostics(
        r#"
//- /src/services/events.service.ts
import { emit, listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";

export const listenTo = <PayloadType>(
  event: string,
  handler: EventCallback<PayloadType>
): Promise<UnlistenFn> => listen(event, handler);

export const emitDragMode = (val: boolean) => emit("$0drag-mode-changed", val);

//- /src/store/sync/listeners.ts
import { listenTo, type UnlistenFn } from "../../services/events.service";

export const setupOverlayListeners = async (): Promise<UnlistenFn[]> => {
  const unlistens: UnlistenFn[] = [];

  unlistens.push(
    await listenTo<boolean>("drag-mode-changed", (e) => {
      void e;
    })
  );

  return unlistens;
};
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_dynamic_listen_suppresses_no_listeners() {
    helpers::check_diagnostics(
        r#"
//- /sync.ts
import { emit, listen } from "@tauri-apps/api/event";

const subscribe = (event: string) => listen(event, () => {});

export const watchAny = (name: string) => subscribe(name);

emit("$0units-changed");
"#,
        expect![[
            r#"HINT 4:52..4:56 "Tarus cannot read the event name `name` as a string: this listener is not linked to any emitter, and "no listeners" warnings are off for the whole workspace" [tarus/unresolved-name]"#
        ]],
    );
}

#[test]
fn diag_forwarded_invoke_reaches_the_command() {
    helpers::check_diagnostics(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";

const call = (name: string, args: unknown) => invoke(name, args);

export const greetSomeone = (who: string) => call("greet", { name: who });
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_dynamic_invoke_suppresses_unused_command() {
    helpers::check_diagnostics(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";

const call = (name: string) => invoke(name);

export const callAny = (name: string) => call(name);
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_dynamic_rust_emit_suppresses_never_emitted() {
    helpers::check_diagnostics(
        r#"
//- /backend.rs
fn broadcast(app: &AppHandle, name: &str) {
    app.emit(name, ()).unwrap();
}

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0units-changed", () => {});
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_static_emit_still_warns_alongside_unrelated_dynamic_invoke() {
    // A dynamic *invoke* says nothing about listeners, so event diagnostics stay on.
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";

const call = (name: string) => invoke(name);

emit("$0my-event");
"#,
        expect![[r#"WARNING 5:6..5:14 "Event 'my-event' is emitted but no listeners found""#]],
    );
}

#[test]
fn diag_empty_literal_is_not_a_dynamic_name() {
    // `invoke("")` names nothing, but it is fully known — it must not suppress
    // the unused-command warning the way an unresolvable name does.
    helpers::check_diagnostics(
        r#"
//- /backend.rs
#[tauri::command]
fn gre$0et() {}

//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";
invoke("");
"#,
        expect![[r#"WARNING 1:3..1:8 "Command 'greet' is defined but never invoked in frontend""#]],
    );
}

#[test]
fn diag_empty_event_literal_is_not_a_dynamic_name() {
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { emit, listen } from "@tauri-apps/api/event";
listen("", () => {});
emit("$0my-event");
"#,
        expect![[r#"WARNING 2:6..2:14 "Event 'my-event' is emitted but no listeners found""#]],
    );
}

#[test]
fn diag_empty_rust_event_literal_is_not_a_dynamic_name() {
    helpers::check_diagnostics(
        r#"
//- /backend.rs
fn noop(app: &AppHandle) {
    app.emit("", ()).unwrap();
}

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";
listen("$0units-changed", () => {});
"#,
        expect![[r#"WARNING 1:8..1:21 "Event 'units-changed' is listened for but never emitted""#]],
    );
}

// ===========================================================================
// Constants declared inside a macro body
// ===========================================================================

/// The shape of a project that declares its event names once, in a macro that
/// also exports them to TypeScript: the body is a token tree, not items. The
/// emit sits in the same file because the fixture resolves constants per file.
const MACRO_DECLARED_EVENTS: &str = r#"
//- /src-tauri/src/events.rs
ts_values! {
    export_event_names => GENERATED_HEADER;

    /// Sent when a perf run starts.
    pub const EVENT_PERF_BEGIN: &str = "perf://begin" => PERF_BEGIN;
}

fn start(app: &AppHandle) {
    app.emit(EVENT_PERF_BEGIN, ()).ok();
}
"#;

#[test]
fn diag_macro_declared_constant_reaches_the_listener() {
    helpers::check_diagnostics(
        &format!(
            r#"{MACRO_DECLARED_EVENTS}
//- /src/perf-run.ts
import {{ listen }} from "@tauri-apps/api/event";

listen("$0perf://begin", () => {{}});
"#
        ),
        expect!["(none)"],
    );
}

#[test]
fn diag_macro_declared_constant_keeps_other_events_checked() {
    // Before macro bodies were read, the emit above was an unknown name, and an
    // unknown emit switches "never emitted" off for the whole workspace.
    helpers::check_diagnostics(
        &format!(
            r#"{MACRO_DECLARED_EVENTS}
//- /src/perf-run.ts
import {{ listen }} from "@tauri-apps/api/event";

listen("perf://begin", () => {{}});
listen("$0perf://typo", () => {{}});
"#
        ),
        expect![[r#"WARNING 3:8..3:19 "Event 'perf://typo' is listened for but never emitted""#]],
    );
}

#[test]
fn diag_macro_declared_constant_emit_without_listener() {
    helpers::check_diagnostics(
        &format!(
            r#"{MACRO_DECLARED_EVENTS}
//- /src/other.ts
import {{ listen }} from "@tauri-apps/api/event";

listen("other", () => {{}});
"#
        )
        .replace("app.emit(EVENT_PERF_BEGIN", "app.emit($0EVENT_PERF_BEGIN"),
        expect![[r#"WARNING 8:13..8:29 "Event 'perf://begin' is emitted but no listeners found""#]],
    );
}

// ===========================================================================
// Raw strings and constants that name other constants
// ===========================================================================

#[test]
fn diag_raw_string_and_aliased_constants_reach_their_listeners() {
    // Before raw strings and references were read, both emits were unknown names,
    // and an unknown emit switches "never emitted" off for the whole workspace — so
    // the typo below went unreported.
    helpers::check_diagnostics(
        r###"
//- /src-tauri/src/events.rs
pub const EVENT_RAW: &str = r#"run://raw"#;
pub const EVENT_ALIAS: &str = EVENT_RAW;

names! {
    pub const EVENT_MACRO_ALIAS: &str = crate::events::EVENT_RAW => MACRO_ALIAS;
}

fn start(app: &AppHandle) {
    app.emit(EVENT_ALIAS, ()).ok();
    app.emit(EVENT_MACRO_ALIAS, ()).ok();
}

//- /src/runs.ts
import { listen } from "@tauri-apps/api/event";

listen("run://raw", () => {});
listen("$0run://typo", () => {});
"###,
        expect![[r#"WARNING 3:8..3:18 "Event 'run://typo' is listened for but never emitted""#]],
    );
}

// ===========================================================================
// Hint on a name Tarus cannot read
// ===========================================================================

#[test]
fn diag_hint_on_rust_emit_with_unreadable_constant() {
    // `concat!` is only evaluated by the compiler, so the emit has no name; the hint
    // says so where it is written, together with what it switches off.
    helpers::check_diagnostics(
        r#"
//- /src-tauri/src/run.rs
const EVENT_X: &str = concat!("run", "://x");

fn start(app: &AppHandle) {
    app.emit($0EVENT_X, ()).ok();
}
"#,
        expect![[
            r#"HINT 3:13..3:20 "Tarus cannot read the event name `EVENT_X` as a string: this emit is not linked to any listener, and "never emitted" warnings are off for the whole workspace" [tarus/unresolved-name]"#
        ]],
    );
}

#[test]
fn diag_hint_on_invoke_with_an_unresolvable_name() {
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { invoke } from "@tauri-apps/api/core";

// A parameter alone would make this a forwarding helper; a field of one is not.
export const run = (commands: Record<string, string>) => invoke(comm$0ands.run);
"#,
        expect![[
            r#"HINT 3:64..3:76 "Tarus cannot read the command name `commands.run` as a string: this call is not linked to its command, and "never invoked" warnings are off for the whole workspace" [tarus/unresolved-name]"#
        ]],
    );
}

#[test]
fn diag_hint_does_not_quote_a_long_expression() {
    helpers::check_diagnostics(
        r#"
//- /frontend.ts
import { listen } from "@tauri-apps/api/event";

export const watch = (prefix: string, id: string) =>
  listen($0`${prefix}://some-rather-long-event-suffix/${id}`, () => {});
"#,
        expect![[
            r#"HINT 3:9..3:58 "Tarus cannot read the event name as a string: this listener is not linked to any emitter, and "no listeners" warnings are off for the whole workspace" [tarus/unresolved-name]"#
        ]],
    );
}

#[test]
fn diag_no_hint_when_every_name_resolves() {
    helpers::check_diagnostics(
        r#"
//- /src-tauri/src/run.rs
const EVENT_X: &str = "run://x";

fn start(app: &AppHandle) {
    app.emit($0EVENT_X, ()).ok();
}

//- /frontend.ts
import { listen } from "@tauri-apps/api/event";

listen("run://x", () => {});
"#,
        expect!["(none)"],
    );
}

#[test]
fn diag_hint_on_forwarder_call_without_a_name_argument() {
    // The name is not unreadable here, it is missing: the hint must not quote `()`.
    helpers::check_diagnostics(
        r#"
//- /sync.ts
import { emitTo } from "@tauri-apps/api/event";

const emitToOverlays = async (event: string, payload: unknown) => {
  await emitTo("overlay", event, payload);
};

export const broken = () => emitToOverlays$0();
"#,
        expect![[
            r#"HINT 6:42..6:44 "This call passes no event name: this emit is not linked to any listener, and "never emitted" warnings are off for the whole workspace" [tarus/unresolved-name]"#
        ]],
    );
}
