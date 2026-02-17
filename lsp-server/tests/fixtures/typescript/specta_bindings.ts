// Test fixture: Specta-generated bindings file

export const commands = {
  async getUserProfile(userId: number): Promise<string> {
    return await TAURI_INVOKE("get_user_profile", { userId });
  },

  async savePreferences(data: string): Promise<void> {
    return await TAURI_INVOKE("save_preferences", { data });
  },

  async deleteItem(id: number): Promise<void> {
    return await TAURI_INVOKE("delete_item", { id });
  }
};

// Mock TAURI_INVOKE
declare function TAURI_INVOKE<T>(cmd: string, args?: Record<string, unknown>): Promise<T>;
