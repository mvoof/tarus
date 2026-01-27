import { invoke } from "@tauri-apps/api/core";

async function greetUser() {
    const result = await invoke("greet", { name: "Alice" });
    console.log(result);
}

async function fetchData() {
    const data = await invoke("get_user", { id: 42 });
    return data;
}
