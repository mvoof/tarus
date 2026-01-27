use tauri::AppHandle;

#[tauri::command]
fn get_user(id: u32) -> Result<String, String> {
    Ok(format!("User {}", id))
}

#[tauri::command]
fn save_data(data: String) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
fn process_item(item: String) -> String {
    item.to_uppercase()
}
