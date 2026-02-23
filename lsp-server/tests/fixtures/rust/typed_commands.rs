// Fixture: Rust commands with typed parameters and return values

use tauri::AppHandle;

#[tauri::command]
fn get_user(id: u32) -> Result<String, String> {
    Ok(format!("user-{}", id))
}

#[tauri::command]
fn create_user(name: String, age: u32) -> Result<String, String> {
    Ok(name)
}

#[tauri::command]
fn ping() {
    // no return
}

#[tauri::command]
fn get_items() -> Vec<String> {
    vec![]
}

#[tauri::command]
fn find_user(id: u32) -> Option<String> {
    None
}

// This function has NO #[tauri::command] attribute and should NOT be extracted
fn helper_fn(x: u32) -> u32 {
    x + 1
}
