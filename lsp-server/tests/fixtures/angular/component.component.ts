import { Component } from '@angular/core';
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";

@Component({
  selector: 'app-user',
  template: '<button (click)="loadUser()">Load</button>'
})
export class UserComponent {
  async loadUser() {
    const user = await invoke("get_user", { id: 1 });
    emit("user-loaded", user);
  }
}
