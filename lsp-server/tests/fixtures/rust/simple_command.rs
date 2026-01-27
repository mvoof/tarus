// Simple Tauri command
#[tauri::command]
fn greet(name: String) -> String {
    format!("Hello, {}!", name)
}
