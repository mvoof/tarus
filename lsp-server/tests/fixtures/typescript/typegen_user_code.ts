// Test fixture: User code calling tauri-plugin-typegen generated bindings

import { commands } from './typegen_bindings';

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
import * as Typegen from './typegen_bindings';

async function testNamespaced() {
  const result = await Typegen.commands.getUserProfile(456);
  return result;
}

// Test: Mixed with regular invoke
import { invoke } from '@tauri-apps/api/core';

async function testMixed() {
  // Regular invoke
  await invoke("legacy_command");

  // Typegen call
  await commands.deleteItem(789);
}

// Test: Multiple calls
async function testMultipleCalls() {
  const data = await commands.fetchData();
  await commands.savePreferences(data);
  await Typegen.commands.deleteItem(1);
}
