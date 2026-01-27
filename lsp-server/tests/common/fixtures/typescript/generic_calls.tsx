import { invoke } from "@tauri-apps/api/core";

interface User {
    id: number;
    name: string;
}

async function getUser(): Promise<User> {
    const user = await invoke<User>("get_user", { id: 1 });
    return user;
}

async function saveData<T>(data: T): Promise<void> {
    await invoke<void>("save_data", { data });
}
