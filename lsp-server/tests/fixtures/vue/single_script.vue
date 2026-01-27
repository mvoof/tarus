<template>
  <div>
    <button @click="handleClick">Greet</button>
  </div>
</template>

<script>
import { invoke } from "@tauri-apps/api/core";

export default {
  methods: {
    async handleClick() {
      const result = await invoke("greet", { name: "Vue User" });
      console.log(result);
    }
  }
}
</script>
