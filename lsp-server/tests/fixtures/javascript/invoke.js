import { invoke } from "@tauri-apps/api/core";

function greetUser() {
    invoke("greet", { name: "Bob" }).then(result => {
        console.log(result);
    });
}

function processData() {
    invoke("process_item", { item: "test" });
}
