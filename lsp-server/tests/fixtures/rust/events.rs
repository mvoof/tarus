use tauri::{AppHandle, Manager};

fn notify_user(app: &AppHandle) {
    app.emit("user-notification", "Hello").unwrap();
}

fn handle_event(app: &AppHandle) {
    app.listen("button-clicked", |event| {
        println!("Button clicked!");
    });
    
    app.emit("status-update", "Ready").unwrap();
}
