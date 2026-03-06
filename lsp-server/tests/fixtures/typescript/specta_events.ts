// Specta event patterns
import { events } from '../bindings';

// Global events
events.globalEvent.listen((e) => console.log(e));
events.globalEvent.emit({ message: "hello" });
events.globalEvent.once((e) => console.log(e));

// Window-targeted events
events.myCustomEvent(appWindow).listen((e) => console.log(e));
events.myCustomEvent(appWindow).emit({ data: 42 });

// Multi-word event
events.userProfileUpdated.listen((e) => console.log(e));
