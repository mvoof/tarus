// Fixture: TypeScript file using Specta-generated commands object

import { commands } from './bindings';

// Correct usage: getUserProfile takes 1 arg
const user = await commands.getUserProfile(42);

// Wrong arg count: createUser takes 2 args (name + age), but gets 3
const u2 = await commands.createUser("Bob", 25, "extra");

// Correct usage: ping takes 0 args
await commands.ping();

// Regular invoke (not specta) for comparison
import { invoke } from '@tauri-apps/api';
const result = await invoke('get_user', { id: 1 });
