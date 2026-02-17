// Test fixture: User code calling Specta-generated bindings

import { commands } from './specta_bindings';

// Test: Direct call
async function testDirectCall() {
  const profile = await commands.getUserProfile(123);
  console.log(profile);
}

// Test: Without await
function testNoAwait() {
  commands.savePreferences("test");
}

// Test: Namespaced import pattern
import * as Specta from './specta_bindings';

async function testNamespaced() {
  const result = await Specta.commands.getUserProfile(456);
  return result;
}

// Test: Mixed with regular invoke
import { invoke } from '@tauri-apps/api/core';

async function testMixed() {
  // Regular invoke
  await invoke("legacy_command");

  // Specta call
  await commands.deleteItem(789);
}
