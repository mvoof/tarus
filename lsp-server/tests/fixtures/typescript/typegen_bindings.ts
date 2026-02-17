// Test fixture: tauri-plugin-typegen generated bindings file

import { invoke } from '@tauri-apps/api/core';

export const commands = {
  async getUserProfile(userId: number): Promise<string> {
    return await invoke("get_user_profile", { userId });
  },

  async savePreferences(data: string): Promise<void> {
    return await invoke("save_preferences", { data });
  },

  async deleteItem(id: number): Promise<void> {
    return await invoke("delete_item", { id });
  },

  async fetchData(): Promise<string> {
    return await invoke("fetch_data");
  }
};
