use tauri::AppHandle;

// Test: Specta-style command with intermediate attribute
#[tauri::command]
#[cfg_attr(feature = "specta", specta::specta)]
fn get_user_profile(user_id: u32) -> Result<String, String> {
    Ok(format!("User {}", user_id))
}

// Test: Multiple intermediate attributes
#[tauri::command]
#[cfg_attr(feature = "specta", specta::specta)]
#[allow(dead_code)]
fn save_preferences(data: String) -> Result<(), String> {
    Ok(())
}

// Test: Regular command without intermediate attributes (should still work)
#[tauri::command]
fn delete_item(id: u32) -> Result<(), String> {
    Ok(())
}
