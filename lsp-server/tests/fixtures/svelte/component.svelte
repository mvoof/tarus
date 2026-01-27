<script>
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";

let message = "";

async function loadUser() {
    const user = await invoke("get_user", { id: 1 });
    message = user;
    emit("user-loaded", user);
}

listen("refresh-data", () => {
    loadUser();
});
</script>

<button on:click={loadUser}>Load User</button>
<p>{message}</p>
