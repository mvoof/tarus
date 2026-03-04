// Specta event patterns in JavaScript
const { events } = require('../bindings');

events.globalEvent.listen((e) => console.log(e));
events.globalEvent.emit({ message: "hello" });
events.myCustomEvent(appWindow).listen((e) => console.log(e));
