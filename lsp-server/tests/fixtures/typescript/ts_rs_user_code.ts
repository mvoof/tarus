// Test fixture: User code calling ts-rs generated bindings

import { commands, User } from './ts_rs_bindings';

// Test: Direct call
async function testDirectCall() {
  const user: User = await commands.getUserProfile(123);
  console.log(user.name);
}

// Test: Without await
function testNoAwait() {
  commands.deleteItem(456);
}

// Test: Namespaced import pattern
import * as TsRs from './ts_rs_bindings';

async function testNamespaced() {
  const users = await TsRs.commands.listUsers();
  return users;
}

// Test: Mixed with regular invoke
import { invoke } from '@tauri-apps/api/core';

async function testMixed() {
  // Regular invoke
  await invoke("legacy_command");

  // ts-rs call
  await commands.deleteItem(789);
}

// Test: Using types
async function testWithTypes() {
  const prefs: TsRs.Preferences = {
    theme: "dark",
    language: "en"
  };
  await commands.savePreferences(prefs);
}
