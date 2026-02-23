// Commands with additional attributes between #[tauri::command] and fn

#[tauri::command]
#[allow(dead_code)]
fn single_extra_attr() -> String {
    "hello".to_string()
}

#[tauri::command]
#[allow(dead_code)]
#[allow(unused_variables)]
pub fn multiple_extra_attrs(x: i32) -> i32 {
    x
}

// Simple command — no extra attributes (existing behavior must still work)
#[tauri::command]
fn simple_command() -> String {
    "simple".to_string()
}
