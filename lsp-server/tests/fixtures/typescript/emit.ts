import { emit, listen } from "@tauri-apps/api/event";

function notifyStatusChange() {
    emit("status-changed", { status: "active" });
}

function setupListener() {
    listen("user-notification", (event) => {
        console.log("Received:", event.payload);
    });
}
