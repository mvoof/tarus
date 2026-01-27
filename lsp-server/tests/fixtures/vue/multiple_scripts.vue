<template>
  <div>{{ message }}</div>
</template>

<script setup>
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";

const loadData = async () => {
  const data = await invoke("get_user", { id: 1 });
  emit("data-loaded", data);
};
</script>
