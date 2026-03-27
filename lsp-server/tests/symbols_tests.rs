//! Document and Workspace Symbol tests

mod helpers;

use expect_test::expect;

// ===========================================================================
// Document symbols
// ===========================================================================

#[test]
fn document_symbols_rust_commands() {
    helpers::check_document_symbols(
        r#"
//- /backend.rs
$0
use tauri::AppHandle;

#[tauri::command]
fn get_user(id: u32) -> Result<String, String> { Ok("".into()) }

#[tauri::command]
fn save_data(data: String) -> Result<(), String> { Ok(()) }

#[tauri::command]
fn process_item(item: String) -> String { item }
"#,
        expect![[r#"
            Function "get_user (command)" 4:3..4:11
            Function "process_item (command)" 10:3..10:15
            Function "save_data (command)" 7:3..7:12"#]],
    );
}

#[test]
fn document_symbols_events() {
    helpers::check_document_symbols(
        r#"
//- /events.rs
$0
use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("user-notification", "Hello").unwrap();
}

fn handle(app: &AppHandle) {
    app.listen("button-clicked", |e| {});
}
"#,
        expect![[r#"
            Event "button-clicked (listen)" 8:16..8:30
            Event "user-notification (emit)" 4:14..4:31"#]],
    );
}

#[test]
fn document_symbols_empty_file_returns_none() {
    helpers::check_document_symbols(
        r#"
//- /empty.rs
$0
"#,
        expect!["(none)"],
    );
}

// ===========================================================================
// Workspace symbols
// ===========================================================================

#[test]
fn workspace_symbol_search_exact() {
    helpers::check_workspace_symbols(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

#[tauri::command]
fn get_user() {}
"#,
        "greet",
        expect![[r#"Function "greet (command)" /backend.rs"#]],
    );
}

#[test]
fn workspace_symbol_partial_match() {
    helpers::check_workspace_symbols(
        r#"
//- /backend.rs
#[tauri::command]
fn get_user() {}

#[tauri::command]
fn get_item() {}

#[tauri::command]
fn save_data() {}
"#,
        "get",
        expect![[r#"
            Function "get_item (command)" /backend.rs
            Function "get_user (command)" /backend.rs"#]],
    );
}

#[test]
fn workspace_symbol_empty_query_returns_all() {
    helpers::check_workspace_symbols(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}

//- /events.rs
use tauri::{AppHandle, Manager};

fn notify(app: &AppHandle) {
    app.emit("status-changed", "data").unwrap();
}
"#,
        "",
        expect![[r#"
            Event "status-changed (emit)" /events.rs
            Function "greet (command)" /backend.rs"#]],
    );
}

#[test]
fn workspace_symbol_no_match_returns_none() {
    helpers::check_workspace_symbols(
        r#"
//- /backend.rs
#[tauri::command]
fn greet() {}
"#,
        "nonexistent_xyz_abc",
        expect!["(none)"],
    );
}
